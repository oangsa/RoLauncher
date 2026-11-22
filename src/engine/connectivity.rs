use super::*;

impl Engine {
    // Independent of preparation and launch-slot waits. Notifications wake the
    // idle timer, never cancel an in-flight probe and postpone it another cycle.
    pub(super) async fn connectivity_loop(&self) {
        let mut next_probe = Instant::now();
        let mut failures = 0usize;
        while !self.is_shutdown() {
            let probe_requested = self.0.probe_requested.swap(false, Ordering::AcqRel);
            let snapshot = self.snapshot();
            if probe_requested && !snapshot.network_suspended {
                next_probe = Instant::now();
            }
            let needed = probe_requested
                || snapshot.network_suspended
                || snapshot.accounts.iter().any(|a| {
                    a.desired_running
                        && ((a.status == Status::Backoff && a.recovery_reason == "Network")
                            || (a.status == Status::Reconnecting
                                && a.recovery_evidence
                                    == RecoveryEvidence::ConfirmedTerminalDisconnect
                                && matches!(
                                    a.recovery_reason.as_str(),
                                    "ConnectionFailed"
                                        | "NetworkDisconnect"
                                        | "ConfirmedTerminalDisconnect"
                                        | "Network"
                                )))
                });
            if needed && Instant::now() >= next_probe {
                let reachable = self.0.roblox.reachable().await;
                if self.is_shutdown() {
                    break;
                }
                self.0.journal.record(
                    "connectivity_probe",
                    serde_json::json!({
                        "reachable": reachable,
                        // Endpoint reachability cannot distinguish host internet loss
                        // from a Roblox service outage, nor prove a game is broken.
                        "scope": "roblox_service",
                    }),
                );
                let _ = self.transaction_inner(
                    |s| {
                        let was_suspended = s.network_suspended;
                        let mut changed = was_suspended != !reachable;
                        s.network_suspended = !reachable;
                        if !reachable {
                            for saved in &mut s.database.accounts {
                                let a = &mut saved.account;
                                if a.desired_running
                                    && ((a.status == Status::Reconnecting
                                        && matches!(
                                            a.recovery_reason.as_str(),
                                            "ConnectionFailed"
                                                | "NetworkDisconnect"
                                                | "ConfirmedTerminalDisconnect"
                                        ))
                                        || (a.status == Status::Backoff
                                            && a.recovery_reason == "Network"))
                                {
                                    let message = "Waiting for internet / Roblox service";
                                    if a.recovery_reason != "Network"
                                        || a.last_error.as_deref() != Some(message)
                                    {
                                        changed = true;
                                    }
                                    a.recovery_reason = "Network".into();
                                    a.last_error = Some(message.into());
                                }
                            }
                        } else if was_suspended {
                            let now = Utc::now();
                            for saved in &mut s.database.accounts {
                                let a = &mut saved.account;
                                if a.desired_running
                                    && a.status == Status::Backoff
                                    && a.recovery_reason == "Network"
                                    && !active_cooldown(a)
                                {
                                    a.transient_failures = 0;
                                    a.next_retry = Some(now);
                                    changed = true;
                                }
                            }
                        }
                        Ok(changed)
                    },
                    false,
                    |changed| *changed,
                );
                let seconds = if reachable {
                    failures = 0;
                    15
                } else {
                    failures = failures.saturating_add(1);
                    [2, 5, 10, 20, 30][failures.saturating_sub(1).min(4)]
                };
                next_probe = Instant::now()
                    + Duration::from_secs(seconds)
                    + Duration::from_millis(rand::random_range(0..=500));
            } else if !needed {
                failures = 0;
                next_probe = Instant::now();
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(250)) => {},
                _ = self.0.wake.notified() => {},
            }
        }
    }
}
