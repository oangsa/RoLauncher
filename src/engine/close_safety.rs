use super::*;

#[derive(Default)]
pub(super) struct CloseAttempts(Vec<CloseAttempt>);
struct CloseAttempt {
    account_id: String,
    intent: CloseIntent,
    identity: ProcessIdentity,
    started: Instant,
    denied: bool,
}
impl CloseAttempts {
    pub(super) fn remove(&mut self, identity: &ProcessIdentity) {
        self.0.retain(|a| a.identity != *identity);
        #[cfg(windows)]
        platform::unpin_close(identity);
    }
    fn force(
        &self,
        id: &str,
        intent: &CloseIntent,
        identity: &ProcessIdentity,
        now: Instant,
        grace: Duration,
    ) -> bool {
        self.0.iter().any(|a| {
            !a.denied
                && a.account_id == id
                && a.intent == *intent
                && a.identity == *identity
                && now.saturating_duration_since(a.started) >= grace
        })
    }
    fn record(&mut self, id: &str, intent: &CloseIntent, identity: &ProcessIdentity, now: Instant) {
        if !self
            .0
            .iter()
            .any(|a| a.account_id == id && a.intent == *intent && a.identity == *identity)
        {
            self.0.retain(|a| a.identity != *identity);
            self.0.push(CloseAttempt {
                account_id: id.into(),
                intent: intent.clone(),
                identity: identity.clone(),
                started: now,
                denied: false,
            });
        }
    }
    fn blocked(&self, id: &str, intent: &CloseIntent, identity: &ProcessIdentity) -> bool {
        self.0.iter().any(|a| {
            a.denied
                && a.account_id == id
                && a.identity == *identity
                // An unsaved classification can be recreated with a fresh UUID.
                // Only explicit commands may restore authority after denial.
                && (a.intent == *intent || intent.reason.automatic())
        })
    }
    fn deny(&mut self, id: &str, intent: &CloseIntent, identity: &ProcessIdentity, now: Instant) {
        self.record(id, intent, identity, now);
        #[cfg(windows)]
        platform::unpin_close(identity);
        for a in &mut self.0 {
            if a.account_id == id && a.intent == *intent && a.identity == *identity {
                a.denied = true;
            }
        }
    }
}

fn verified_unique(
    state: &State,
    id: &str,
    identity: &ProcessIdentity,
    players: &[platform::Player],
    retired: bool,
) -> bool {
    if identity.tracker.is_empty() {
        return false;
    }
    let matches: Vec<_> = players
        .iter()
        .filter(|p| p.tracker == identity.tracker)
        .collect();
    matches.len() == 1
        && matches[0].pid == identity.pid
        && matches[0].creation_time == identity.creation_time
        && players.iter().filter(|p| p.pid == identity.pid).count() == 1
        && uniquely_claimed(state, id, identity, retired)
}

fn uniquely_claimed(state: &State, id: &str, identity: &ProcessIdentity, retired: bool) -> bool {
    !state.database.accounts.iter().any(|s| {
        (retired || s.account.id != id)
            && (s.account.tracker.as_ref() == Some(&identity.tracker)
                || s.account
                    .process
                    .as_ref()
                    .is_some_and(|p| p.pid == identity.pid || p.tracker == identity.tracker))
    }) && !state.database.retired_launches.iter().any(|r| {
        r.tracker != identity.tracker && r.process.as_ref().is_some_and(|p| p.pid == identity.pid)
    }) && state
        .database
        .retired_launches
        .iter()
        .filter(|r| r.tracker == identity.tracker)
        .count()
        == usize::from(retired)
}

fn eligible(
    a: &Account,
    intent: &CloseIntent,
    network_suspended: bool,
    launch_allowed: bool,
) -> bool {
    if intent.denied
        || intent.id.is_nil()
        || intent.operation_id.is_none()
        || intent.generation != a.generation
        || intent.operation_id != a.operation_id
    {
        return false;
    }
    match intent.reason {
        CloseReason::UserStop | CloseReason::SessionEnded => !a.desired_running,
        CloseReason::ManualRestart | CloseReason::ManualStartReplacement => {
            a.desired_running && a.status == Status::Queued && a.target.is_some() && launch_allowed
        }
        CloseReason::AutomaticRecovery | CloseReason::TargetReplacement => {
            a.desired_running
                && a.auto_recovery
                && a.target.is_some()
                && a.failures < 5
                && !network_suspended
                && launch_allowed
                && (recovery_due(a, Utc::now())
                    || a.status == Status::Backoff && a.next_retry.is_some())
        }
        CloseReason::ObsoleteLaunch => false,
    }
}

pub(super) fn stop_ended_session(a: &mut Account) {
    a.desired_running = false;
    a.status = Status::Stopping;
    a.next_retry = None;
    a.last_error = None;
    a.close_intent = Some(CloseIntent {
        id: Uuid::new_v4(),
        generation: a.generation,
        operation_id: a.operation_id,
        reason: CloseReason::SessionEnded,
        denied: false,
    });
}

pub(super) fn resume_verified_handoff(a: &mut Account) {
    if a.status == Status::NeedsAttention && a.last_error.as_deref() == Some(HANDOFF_UNRESOLVED) {
        a.last_error = None;
        a.status = if !a.desired_running {
            Status::Stopping
        } else if a.close_intent.as_ref().is_some_and(|i| {
            matches!(
                i.reason,
                CloseReason::ManualRestart | CloseReason::ManualStartReplacement
            )
        }) {
            Status::Queued
        } else if a.next_retry.is_some() {
            Status::Backoff
        } else {
            Status::Unknown
        };
    }
}

fn attach_surviving_player(
    a: &mut Account,
    successor: &ProcessIdentity,
    clean: bool,
    direct_child: bool,
) {
    let ended_session = direct_child
        && a.desired_running
        && a.status != Status::NeedsAttention
        && a.close_intent
            .as_ref()
            .is_none_or(|i| i.reason.automatic() && !i.denied)
        && (matches!(
            a.recovery_evidence,
            RecoveryEvidence::ConfirmedTerminalDisconnect | RecoveryEvidence::RecoveryStarted
        ) || clean && a.connected_since.is_some());
    let retry_reserved = a.next_retry.is_some() && a.status == Status::Backoff;
    let first_clean_handoff = ended_session
        && clean
        && a.failures == 0
        && a.recovery_reason != "RateLimit"
        && a.next_retry.is_none();
    let explicit_replacement = a.close_intent.as_ref().is_some_and(|i| {
        matches!(
            i.reason,
            CloseReason::ManualRestart | CloseReason::ManualStartReplacement
        )
    });
    // An old client's disconnect/close permission cannot authorize closing its successor.
    if a.close_intent
        .as_ref()
        .is_some_and(|i| i.reason.automatic() && !i.denied)
    {
        a.close_intent = None;
    }
    a.connected_since = None;
    a.disconnected_since = None;
    attach_verified_player(a, successor.clone());
    if direct_child {
        a.ownership_deadline = None;
    }
    if ended_session {
        if a.auto_recovery {
            if !retry_reserved {
                reserve_recovery(a);
            } else {
                a.status = Status::Backoff;
            }
            if a.status == Status::Backoff {
                if first_clean_handoff {
                    a.next_retry = Some(Utc::now());
                }
                a.recovery_process = Some(successor.clone());
                a.recovery_evidence = RecoveryEvidence::RecoveryStarted;
                a.close_intent = Some(CloseIntent {
                    id: Uuid::new_v4(),
                    reason: CloseReason::AutomaticRecovery,
                    generation: a.generation,
                    operation_id: a.operation_id,
                    denied: false,
                });
            }
        } else {
            stop_ended_session(a);
        }
    }
    if explicit_replacement && a.status != Status::NeedsAttention {
        a.status = Status::Queued;
    }
}

impl Engine {
    pub(super) fn pause_unresolved_exit(&self, a: &Account, p: &ProcessIdentity) {
        if a.status == Status::NeedsAttention {
            return;
        }
        let _ = self.update(&a.id, a.generation, |current| {
            if current.process.as_ref() == Some(p) && current.status != Status::NeedsAttention {
                current.status = Status::NeedsAttention;
                current.last_error = Some(HANDOFF_UNRESOLVED.into());
            }
        });
    }
    pub(super) fn attach_player(
        &self,
        expected: &Account,
        identity: ProcessIdentity,
        players: &[platform::Player],
    ) -> Result<(), String> {
        let stage_recording = self.transaction(|state| {
            let current = account_mut(state, &expected.id)?;
            if current.generation != expected.generation
                || current.process.is_some()
                || current.tracker.as_ref() != Some(&identity.tracker)
            {
                return Err("Player attachment superseded".into());
            }
            if !verified_unique(state, &expected.id, &identity, players, false) {
                let current = account_mut(state, &expected.id)?;
                current.status = Status::NeedsAttention;
                current.last_error = Some("Launch ownership is ambiguous; client preserved".into());
                let opid = current.operation_id;
                let error = current.last_error.clone();
                if let Some(op) = opid.and_then(|id| state.operations.get_mut(&id)) {
                    op.state = "failed".into();
                    op.error = error;
                }
                return Ok((expected.generation, LaunchReadinessStage::StartupFailed));
            }
            let current = account_mut(state, &expected.id)?;
            let late_after_unresolved =
                current.last_error.as_deref() == Some(STARTUP_UNRESOLVED_ERROR);
            let opid = current.operation_id;
            attach_verified_player(current, identity.clone());
            let current_snapshot = crate::diagnostics::account(current);
            let current_generation = current.generation;
            if late_after_unresolved
                && let Some(op) = opid.and_then(|id| state.operations.get_mut(&id))
                && op.error.as_deref() == Some(STARTUP_UNRESOLVED_ERROR)
            {
                op.state = "queued".into();
                op.error = None;
            }
            self.0.journal.record(
                "launch_ownership_verified",
                serde_json::json!({
                    "account": current_snapshot,
                    "operation_id": opid,
                    "generation": current_generation,
                    "tracker": identity.tracker.parse::<u64>().ok(),
                    "process": crate::diagnostics::process(&identity),
                    "stage": "ownership_verified",
                    "late_after_readiness_timeout": late_after_unresolved,
                }),
            );
            Ok((current_generation, LaunchReadinessStage::OwnershipVerified))
        })?;
        self.note_launch_stage(
            &expected.id,
            stage_recording.0,
            stage_recording.1,
            Some(identity.pid),
        );
        Ok(())
    }
    #[cfg(any(windows, test))]
    pub(super) fn cleanup_launch_conflict(
        &self,
        state: &State,
        cleanup: impl FnOnce(&[ProcessIdentity]) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.0.launch_allowed.load(Ordering::Acquire)
            || !state.database.external_background_cleanup_enabled
        {
            return Ok(());
        }
        // Unknown late launches must not be mistaken for external tray leftovers.
        if state
            .database
            .accounts
            .iter()
            .any(|s| s.account.tracker.is_some() && s.account.process.is_none())
            || state
                .database
                .retired_launches
                .iter()
                .any(|r| r.process.is_none())
        {
            return Err("A saved launch still needs inspection; use Stop and wait for process discovery before starting again".into());
        }
        let protected: Vec<_> = state
            .database
            .accounts
            .iter()
            .filter_map(|s| s.account.process.clone())
            .chain(
                state
                    .database
                    .retired_launches
                    .iter()
                    .filter_map(|r| r.process.clone()),
            )
            .collect();
        // The command mutex remains held through cleanup, so new launches cannot race the scan.
        cleanup(&protected)
    }

    /// Explicit global stop: serialize launch cancellation and process termination.
    pub fn close_all_instances(&self) -> Result<usize, String> {
        #[cfg(windows)]
        {
            self.close_all_observed_with(
                || platform::observe_all_players().map(|scan| (scan.players, scan.inaccessible)),
                |identity| {
                    platform::close(identity, true)?;
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while platform::alive(identity)? {
                        if Instant::now() >= deadline {
                            return Err("Roblox process has not exited".into());
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Ok(())
                },
            )
        }
        #[cfg(not(windows))]
        {
            Err("Close all Roblox instances is available on Windows".into())
        }
    }

    #[cfg(any(windows, test))]
    #[cfg(test)]
    fn close_all_with(
        &self,
        discover: impl FnOnce() -> Result<Vec<platform::Player>, String>,
        close: impl FnMut(&ProcessIdentity) -> Result<(), String>,
    ) -> Result<usize, String> {
        self.close_all_observed_with(|| discover().map(|players| (players, 0)), close)
    }
    #[cfg(any(windows, test))]
    fn close_all_observed_with(
        &self,
        discover: impl FnOnce() -> Result<(Vec<platform::Player>, usize), String>,
        mut close: impl FnMut(&ProcessIdentity) -> Result<(), String>,
    ) -> Result<usize, String> {
        let mut state = self.0.state.lock().unwrap();
        let mut next = state.clone();
        for saved in &mut next.database.accounts {
            let a = &mut saved.account;
            a.finish_run(Utc::now());
            a.desired_running = false;
            a.status = Status::Stopped;
            a.next_retry = None;
            a.recovery_evidence = RecoveryEvidence::Unavailable;
            a.recovery_process = None;
            a.generation = Uuid::new_v4();
            let operation = Uuid::new_v4();
            if let Some(old) = a.operation_id.and_then(|id| next.operations.get_mut(&id))
                && old.state == "queued"
            {
                old.state = "cancelled".into();
            }
            a.operation_id = Some(operation);
            a.close_intent = Some(CloseIntent {
                id: Uuid::new_v4(),
                reason: CloseReason::UserStop,
                generation: a.generation,
                operation_id: Some(operation),
                denied: false,
            });
            if a.process.is_none()
                && let Some(tracker) = a.tracker.take()
            {
                next.database.retired_launches.push(RetiredLaunch {
                    tracker,
                    generation: a.generation,
                    account_id: a.id.clone(),
                    deadline: a
                        .ownership_deadline
                        .take()
                        .unwrap_or_else(|| Utc::now() + ChronoDuration::seconds(90)),
                    close_intent: a.close_intent.clone(),
                    process: None,
                });
            }
            next.operations.insert(
                operation,
                Operation {
                    id: operation,
                    account_id: a.id.clone(),
                    state: if a.process.is_some() {
                        "queued"
                    } else {
                        "completed"
                    }
                    .into(),
                    error: None,
                },
            );
            for retired in next
                .database
                .retired_launches
                .iter_mut()
                .filter(|r| r.account_id == a.id)
            {
                retired.close_intent = a.close_intent.clone();
            }
        }
        // A failed discovery/close still leaves automatic relaunch durably stopped.
        self.commit_close_state(&mut state, next)?;
        let (players, inaccessible) = discover()?;
        let mut closed = 0;
        let mut failed = 0;
        for player in players {
            let identity = player.identity(Uuid::nil());
            let result = close(&identity);
            self.0.journal.record("stop_all_process_result", serde_json::json!({"process": crate::diagnostics::process(&identity), "exit_confirmed": result.is_ok(), "error_code": result.as_ref().err().and_then(|error| crate::diagnostics::error_code(error))}));
            if result.is_ok() {
                closed += 1;
            } else {
                failed += 1;
            }
        }
        self.0.journal.record(
            "stop_all_result",
            serde_json::json!({"closed": closed, "failed": failed, "inaccessible": inaccessible}),
        );
        if failed > 0 || inaccessible > 0 {
            Err(format!(
                "Closed {closed} Roblox instances; {failed} could not be closed; {inaccessible} could not be inspected. Automatic rejoin is stopped; retry Close all Roblox."
            ))
        } else {
            Ok(closed)
        }
    }

    /// Non-destructive observation: stale exit reports cannot erase current ownership.
    pub(super) fn owned_exit_observed(
        &self,
        account: &Account,
        identity: &ProcessIdentity,
        clean: bool,
    ) {
        self.confirmed_exit(&account.id, account.generation, identity, clean);
        if !account.desired_running {
            self.finish_operation(&account.id, account.generation, None);
        }
    }

    // Exit reconciliation is separate from the exact-process exit transition so
    // monitor discovery can establish whether the launch has a surviving client.
    pub(super) fn reconcile_exit(
        &self,
        account: &Account,
        identity: &ProcessIdentity,
        clean: bool,
        players: &[platform::Player],
        settled: bool,
    ) -> bool {
        {
            let mut state = self.0.state.lock().unwrap();
            let Some(current) = state
                .database
                .accounts
                .iter()
                .map(|s| &s.account)
                .find(|a| a.id == account.id)
            else {
                return false;
            };
            if current.generation != account.generation
                || current.process.as_ref() != Some(identity)
            {
                return false;
            }
            let matches: Vec<_> = players
                .iter()
                .filter(|p| p.tracker == identity.tracker)
                .collect();
            self.0.journal.record("exit_reconciliation_checked", serde_json::json!({
                "account": crate::diagnostics::account(current), "settled": settled,
                "uniquely_claimed": uniquely_claimed(&state, &current.id, identity, false),
                "candidates": matches.iter().map(|p| crate::diagnostics::process(&p.identity(current.generation))).collect::<Vec<_>>(),
            }));
            if matches.len() == 1 {
                let successor = matches[0].identity(current.generation);
                if successor.pid == identity.pid
                    && successor.creation_time == identity.creation_time
                {
                    return false;
                }
                if !verified_unique(&state, &current.id, &successor, players, false) {
                    drop(state);
                    if settled {
                        self.pause_unresolved_exit(account, identity);
                    }
                    return false;
                }
                let mut next = state.clone();
                let a = account_mut(&mut next, &account.id).unwrap();
                resume_verified_handoff(a);
                #[cfg(windows)]
                let direct_child = platform::direct_successor(identity, &successor);
                #[cfg(not(windows))]
                let direct_child = false;
                attach_surviving_player(a, &successor, clean, direct_child);
                self.0.journal.record("process_handoff_verified", serde_json::json!({
                    "account_id": a.id.parse::<u64>().ok(), "old_process": crate::diagnostics::process(identity),
                    "successor": crate::diagnostics::process(&successor), "reason": if direct_child { "verified_direct_child" } else { "same_tracker_unique_successor" },
                }));
                return self.commit_close_state(&mut state, next).is_ok();
            }
            if !matches.is_empty()
                || !settled
                || !uniquely_claimed(&state, &current.id, identity, false)
            {
                drop(state);
                if settled {
                    self.pause_unresolved_exit(account, identity);
                }
                return false;
            }
        }
        self.owned_exit_observed(account, identity, clean);
        true
    }

    // Same durable-before-visible ordering as Engine::transaction; callers already
    // hold the command mutex, including through the subsequent synchronous effect.
    pub(super) fn commit_close_state(
        &self,
        state: &mut State,
        mut next: State,
    ) -> Result<(), String> {
        normalize_accounts(&mut next);
        management::record_activity(state, &mut next);
        if let Err(error) = self.0.store.save(&next.database) {
            self.0.journal.record(
                "state_save_failed",
                serde_json::json!({"effect": "close_cancelled"}),
            );
            return Err(error);
        }
        self.journal_changes(state, &next);
        for saved in &next.database.accounts {
            if let Some(old) = state
                .database
                .accounts
                .iter()
                .find(|a| a.account.id == saved.account.id)
                && let Some(mut notice) =
                    crate::discord::account_notice(&old.account, &saved.account)
            {
                notice.game_name = next
                    .database
                    .game_profiles
                    .iter()
                    .find(|p| Some(p.target.place_id) == notice.place_id)
                    .map(|p| p.game_name.clone());
                let _ = self.0.discord.record(notice, false);
            }
        }
        *state = next;
        let _ = self.0.events.send(snapshot(state));
        self.0.wake.notify_one();
        Ok(())
    }

    /// The mutex is held through discovery, persistence and the synchronous effect. No permit
    /// escapes this boundary; a command linearizes wholly before or after an OS close request.
    pub(super) fn close_owned(
        &self,
        expected: &Account,
        now: Instant,
        attempts: &mut CloseAttempts,
        discover: impl FnOnce() -> Result<Vec<platform::Player>, String>,
        effect: impl FnOnce(&ProcessIdentity, bool) -> Result<(), String>,
    ) -> Result<bool, String> {
        let started = Instant::now();
        let Some(identity) = expected.process.as_ref() else {
            return Ok(false);
        };
        let mut state = self.0.state.lock().unwrap();
        let mut next = state.clone();
        let network_suspended = next.network_suspended;
        let a = account_mut(&mut next, &expected.id)?;
        if a.generation != expected.generation
            || a.operation_id != expected.operation_id
            || a.process.as_ref() != Some(identity)
            || a.close_intent != expected.close_intent
        {
            return Err("Close superseded; observing the current client".into());
        }
        if a.close_intent.is_none()
            && !a.auto_recovery
            && a.desired_running
            && a.status == Status::Stopping
            && a.recovery_evidence == RecoveryEvidence::ConfirmedTerminalDisconnect
            && a.recovery_process.as_ref() == Some(identity)
            && a.disconnected_since
                .is_some_and(|t| Utc::now() - t >= ChronoDuration::seconds(30))
        {
            stop_ended_session(a);
        }
        if a.close_intent.is_none()
            && recovery_due(a, Utc::now())
            && a.recovery_process.as_ref() == Some(identity)
        {
            a.close_intent = Some(CloseIntent {
                id: Uuid::new_v4(),
                reason: CloseReason::AutomaticRecovery,
                generation: a.generation,
                operation_id: a.operation_id,
                denied: false,
            });
        }
        let Some(intent) = a.close_intent.clone() else {
            if a.status != Status::NeedsAttention
                && (!a.desired_running || a.status == Status::Queued)
            {
                a.status = Status::NeedsAttention;
                a.last_error = Some("Client preserved: no explicit close intent; inspect it and use Stop or Restart".into());
                self.commit_close_state(&mut state, next)?;
            }
            return Ok(false);
        };
        if intent.denied && a.status == Status::NeedsAttention {
            return Ok(false);
        }
        let was_recovery =
            intent.reason == CloseReason::AutomaticRecovery && recovery_due(a, Utc::now());
        let policy = !attempts.blocked(&expected.id, &intent, identity)
            && eligible(
                a,
                &intent,
                network_suspended,
                self.0.launch_allowed.load(Ordering::Acquire),
            );
        let verification = if policy {
            discover().map(|players| {
                self.0.journal.record("close_verification", serde_json::json!({
                    "account_id": expected.id.parse::<u64>().ok(), "intent": intent,
                    "expected_process": crate::diagnostics::process(identity),
                    "uniquely_claimed": uniquely_claimed(&next, &expected.id, identity, false),
                    "candidates": players.iter().filter(|p| p.pid == identity.pid || p.tracker == identity.tracker)
                        .map(|p| crate::diagnostics::process(&p.identity(expected.generation))).collect::<Vec<_>>(),
                }));
                // The process can exit after the monitor's alive check, including after
                // graceful closure. Absence is not proof of conflicting ownership.
                if intent.reason == CloseReason::AutomaticRecovery
                    && uniquely_claimed(&next, &expected.id, identity, false)
                    && !players
                        .iter()
                        .any(|p| p.pid == identity.pid || p.tracker == identity.tracker)
                {
                    return None;
                }
                Some(verified_unique(
                    &next,
                    &expected.id,
                    identity,
                    &players,
                    false,
                ))
            })
        } else {
            Ok(Some(false))
        };
        if let Err(error) = &verification {
            self.0.journal.record("close_deferred", serde_json::json!({
                "account": crate::diagnostics::account(expected), "reason": "process_discovery_unavailable",
                "error_code": crate::diagnostics::error_code(error),
            }));
            return Ok(false);
        }
        if matches!(verification, Ok(None)) {
            self.0.journal.record("close_deferred", serde_json::json!({"account": crate::diagnostics::account(expected), "reason": "absent_from_discovery_waiting_for_exact_exit"}));
            // Retain the process and retry reservation until the exact exit watch
            // confirms exit; do not submit an effect or latch a permanent denial.
            return Ok(false);
        }
        if !policy || !matches!(verification, Ok(Some(true))) {
            self.0.journal.record("close_denied", serde_json::json!({"account": crate::diagnostics::account(expected),
                "policy_eligible": policy, "verification": if !policy { "not_checked_policy_denied" } else { match &verification { Ok(Some(true)) => "unique_owner", Ok(_) => "ownership_mismatch", Err(_) => "discovery_failed" } },
                "verification_error_code": verification.as_ref().err().and_then(|e| crate::diagnostics::error_code(e)),
                "network_suspended": network_suspended, "launch_allowed": self.0.launch_allowed.load(Ordering::Acquire)}));
            attempts.deny(&expected.id, &intent, identity, now);
            let a = account_mut(&mut next, &expected.id)?;
            a.close_intent.as_mut().unwrap().denied = true;
            a.status = Status::NeedsAttention;
            a.last_error = Some(format!(
                "{CLOSE_BLOCKED_PREFIX}intent or unique process ownership is no longer valid; inspect the client, then use Stop or Restart"
            ));
            self.commit_close_state(&mut state, next)?;
            return Err("Close denied; client preserved, inspect it before retrying".into());
        }
        if was_recovery {
            let a = account_mut(&mut next, &expected.id)?;
            reserve_recovery(a);
            a.close_intent = Some(intent.clone());
            if a.status == Status::NeedsAttention {
                let operation = a.operation_id;
                let error = a.last_error.clone();
                if let Some(op) = operation.and_then(|id| next.operations.get_mut(&id)) {
                    op.state = "failed".into();
                    op.error = error;
                }
                self.commit_close_state(&mut state, next)?;
                return Ok(false);
            }
        }
        // Persist newly classified recovery before any destructive effect.
        self.commit_close_state(&mut state, next)?;
        // Coordination is an independently updated atomic. Discovery/disk work may
        // change it even while account commands are serialized by this mutex.
        let a = account_mut(&mut state, &expected.id)?;
        if !eligible(
            a,
            &intent,
            network_suspended,
            self.0.launch_allowed.load(Ordering::Acquire),
        ) {
            attempts.deny(&expected.id, &intent, identity, now);
            let mut next = state.clone();
            let a = account_mut(&mut next, &expected.id)?;
            a.close_intent.as_mut().unwrap().denied = true;
            a.status = Status::NeedsAttention;
            a.last_error = Some(format!(
                "{CLOSE_BLOCKED_PREFIX}replacement readiness changed; inspect the client and retry explicitly"
            ));
            self.commit_close_state(&mut state, next)?;
            return Err("Replacement readiness changed before close; client preserved".into());
        }
        #[cfg(windows)]
        let grace = platform::successor_close_grace(identity);
        #[cfg(not(windows))]
        let grace = Duration::from_secs(5);
        let force = attempts.force(
            &expected.id,
            &intent,
            identity,
            now + started.elapsed(),
            grace,
        );
        self.0.journal.record("close_submitted", serde_json::json!({"account": crate::diagnostics::account(a), "force": force, "intent": intent}));
        match effect(identity, force) {
            Ok(()) => {
                self.0.journal.record("close_result", serde_json::json!({"account_id": expected.id.parse::<u64>().ok(), "process": crate::diagnostics::process(identity), "intent": intent, "force": force, "success": true}));
                attempts.record(&expected.id, &intent, identity, now + started.elapsed());
                Ok(true)
            }
            Err(error) => {
                self.0.journal.record("close_result", serde_json::json!({"account_id": expected.id.parse::<u64>().ok(), "process": crate::diagnostics::process(identity), "intent": intent, "force": force, "success": false, "error_code": crate::diagnostics::error_code(&error)}));
                attempts.deny(&expected.id, &intent, identity, now);
                let mut next = state.clone();
                let a = account_mut(&mut next, &expected.id)?;
                a.close_intent.as_mut().unwrap().denied = true;
                a.status = Status::NeedsAttention;
                a.last_error = Some(CLOSE_FAILED_ERROR.into());
                self.commit_close_state(&mut state, next)?;
                Err("Verified process close failed; force escalation cancelled".into())
            }
        }
    }
    pub(super) fn close_retired(
        &self,
        expected: &RetiredLaunch,
        observed: &[platform::Player],
        now: Instant,
        attempts: &mut CloseAttempts,
        discover: impl FnOnce() -> Result<Vec<platform::Player>, String>,
        effect: impl FnOnce(&ProcessIdentity, bool) -> Result<(), String>,
    ) -> Result<bool, String> {
        let started = Instant::now();
        let mut state = self.0.state.lock().unwrap();
        let mut next = state.clone();
        let Some(index) = next.database.retired_launches.iter().position(|r| {
            r.account_id == expected.account_id
                && r.generation == expected.generation
                && r.tracker == expected.tracker
                && r.close_intent == expected.close_intent
                && r.process == expected.process
        }) else {
            return Err("Retired close superseded; observation retained".into());
        };
        // The monitor's shared scan can bind or reject observation-only records.
        // It never grants close authority; actionable candidates get a fresh scan below.
        let matches: Vec<_> = observed
            .iter()
            .filter(|p| p.tracker == expected.tracker)
            .collect();
        if matches.is_empty() && expected.process.is_none() {
            return Ok(false);
        }
        if matches.len() == 1 && next.database.retired_launches[index].process.is_none() {
            next.database.retired_launches[index].process =
                Some(matches[0].identity(expected.generation));
        }
        let record = &next.database.retired_launches[index];
        let intent = record.close_intent.clone();
        let identity = record.process.clone();
        let policy = identity
            .as_ref()
            .zip(intent.as_ref())
            .is_some_and(|(identity, intent)| {
                !attempts.blocked(&expected.account_id, intent, identity)
                    && intent.reason == CloseReason::UserStop
                    && next
                        .database
                        .accounts
                        .iter()
                        .find(|a| a.account.id == expected.account_id)
                        .is_some_and(|a| {
                            a.account.close_intent.as_ref() == Some(intent)
                                && eligible(&a.account, intent, next.network_suspended, false)
                        })
                    && verified_unique(&next, &expected.account_id, identity, observed, true)
            });
        let allowed = if policy {
            match discover() {
                Ok(players) => verified_unique(
                    &next,
                    &expected.account_id,
                    identity.as_ref().unwrap(),
                    &players,
                    true,
                ),
                Err(_) => {
                    self.0.journal.record("close_deferred", serde_json::json!({"account_id": expected.account_id.parse::<u64>().ok(), "retired": true, "reason": "process_discovery_unavailable"}));
                    // Observation may freeze identity; an outage grants no effect and
                    // must not revoke a valid Stop or start its escalation clock.
                    if next.database.retired_launches[index].process != expected.process {
                        self.commit_close_state(&mut state, next)?;
                    }
                    return Ok(false);
                }
            }
        } else {
            false
        };
        if !allowed {
            let mut changed = next.database.retired_launches[index].process != expected.process;
            if let Some(intent) = &mut next.database.retired_launches[index].close_intent {
                changed |= !intent.denied;
                intent.denied = true;
            }
            if let Some((identity, intent)) = identity.as_ref().zip(intent.as_ref()) {
                attempts.deny(&expected.account_id, intent, identity, now);
            }
            if let Ok(a) = account_mut(&mut next, &expected.account_id)
                && (changed || a.status != Status::NeedsAttention || a.last_error.is_none())
            {
                changed = true;
                a.status = Status::NeedsAttention;
                a.last_error = Some("Late client preserved: close intent or unique ownership is unverified; inspect the client, then use Stop after discovery resolves".into());
            }
            if changed {
                self.commit_close_state(&mut state, next)?;
            }
            return Ok(false);
        }
        let identity = identity.unwrap();
        let intent = intent.unwrap();
        // Freeze the unique process identity durably before closing. A later PID/tracker
        // reuse can never replace this binding, including after supervisor restart.
        self.commit_close_state(&mut state, next)?;
        let force = attempts.force(
            &expected.account_id,
            &intent,
            &identity,
            now + started.elapsed(),
            Duration::from_secs(5),
        );
        self.0.journal.record("close_submitted", serde_json::json!({"account_id": expected.account_id.parse::<u64>().ok(), "process": crate::diagnostics::process(&identity), "intent": intent, "force": force, "retired": true}));
        let result = effect(&identity, force);
        self.0.journal.record("close_result", serde_json::json!({"account_id": expected.account_id.parse::<u64>().ok(), "process": crate::diagnostics::process(&identity), "intent": intent, "force": force, "retired": true, "success": result.is_ok(), "error_code": result.as_ref().err().and_then(|e| crate::diagnostics::error_code(e))}));
        if result.is_err() {
            attempts.deny(&expected.account_id, &intent, &identity, now);
            let mut next = state.clone();
            next.database.retired_launches[index]
                .close_intent
                .as_mut()
                .unwrap()
                .denied = true;
            if let Ok(a) = account_mut(&mut next, &expected.account_id) {
                a.status = Status::NeedsAttention;
                a.last_error =
                    Some("Late client close failed; inspect it and use Stop to retry".into());
            }
            self.commit_close_state(&mut state, next)?;
            return Err("Late client close failed; force escalation cancelled".into());
        }
        attempts.record(
            &expected.account_id,
            &intent,
            &identity,
            now + started.elapsed(),
        );
        Ok(true)
    }
}

#[cfg(test)]
#[path = "close_tests.rs"]
mod tests;
