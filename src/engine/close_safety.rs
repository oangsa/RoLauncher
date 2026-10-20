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
    }
    fn force(
        &self,
        id: &str,
        intent: &CloseIntent,
        identity: &ProcessIdentity,
        now: Instant,
    ) -> bool {
        self.0.iter().any(|a| {
            !a.denied
                && a.account_id == id
                && a.intent == *intent
                && a.identity == *identity
                && now.saturating_duration_since(a.started) >= Duration::from_secs(5)
        })
    }
    fn record(&mut self, id: &str, intent: &CloseIntent, identity: &ProcessIdentity, now: Instant) {
        if !self
            .0
            .iter()
            .any(|a| a.account_id == id && a.intent == *intent && a.identity == *identity)
        {
            self.remove(identity);
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
    let matches: Vec<_> = players
        .iter()
        .filter(|p| p.tracker == identity.tracker)
        .collect();
    matches.len() == 1
        && matches[0].pid == identity.pid
        && matches[0].creation_time == identity.creation_time
        && !state.database.accounts.iter().any(|s| {
            (retired || s.account.id != id)
                && (s.account.tracker.as_ref() == Some(&identity.tracker)
                    || s.account
                        .process
                        .as_ref()
                        .is_some_and(|p| p.pid == identity.pid || p.tracker == identity.tracker))
        })
        && !state.database.retired_launches.iter().any(|r| {
            r.tracker != identity.tracker
                && r.process.as_ref().is_some_and(|p| p.pid == identity.pid)
        })
        && state
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
        CloseReason::UserStop => !a.desired_running,
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

impl Engine {
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

    // Same durable-before-visible ordering as Engine::transaction; callers already
    // hold the command mutex, including through the subsequent synchronous effect.
    fn commit_close_state(&self, state: &mut State, mut next: State) -> Result<(), String> {
        normalize_accounts(&mut next);
        management::record_activity(state, &mut next);
        self.0.store.save(&next.database)?;
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
            discover()
                .map(|players| verified_unique(&next, &expected.id, identity, &players, false))
        } else {
            Ok(false)
        };
        if !policy || !matches!(verification, Ok(true)) {
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
        let force = attempts.force(&expected.id, &intent, identity, now + started.elapsed());
        match effect(identity, force) {
            Ok(()) => {
                attempts.record(&expected.id, &intent, identity, now + started.elapsed());
                Ok(true)
            }
            Err(_) => {
                attempts.deny(&expected.id, &intent, identity, now);
                let mut next = state.clone();
                let a = account_mut(&mut next, &expected.id)?;
                a.close_intent.as_mut().unwrap().denied = true;
                a.status = Status::NeedsAttention;
                a.last_error = Some(format!(
                    "{CLOSE_BLOCKED_PREFIX}verified process close failed; inspect the client and use Stop to retry"
                ));
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
        let mut binding_expired = false;
        if matches.len() == 1 && next.database.retired_launches[index].process.is_none() {
            next.database.retired_launches[index].process =
                Some(matches[0].identity(expected.generation));
            binding_expired = Utc::now() > expected.deadline;
        }
        let record = &next.database.retired_launches[index];
        let intent = record.close_intent.clone();
        let identity = record.process.clone();
        let allowed = identity
            .as_ref()
            .zip(intent.as_ref())
            .is_some_and(|(identity, intent)| {
                !binding_expired
                    && !attempts.blocked(&expected.account_id, intent, identity)
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
                    && discover().is_ok_and(|players| {
                        verified_unique(&next, &expected.account_id, identity, &players, true)
                    })
            });
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
        );
        if effect(&identity, force).is_err() {
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
