use super::*;

impl Engine {
    // Independent of preparation and launch-slot waits. Notifications wake the
    // idle timer, never cancel an in-flight probe and postpone it another cycle.
    pub(super) async fn connectivity_loop(&self) {
        let mut next_probe = Instant::now();
        let mut failures = 0usize;
        while !self.is_shutdown() {
            let snapshot = self.snapshot();
            let needed = snapshot.network_suspended
                || snapshot.accounts.iter().any(|a| {
                    a.desired_running
                        && a.status == Status::Reconnecting
                        && a.recovery_evidence == RecoveryEvidence::ConfirmedTerminalDisconnect
                        && matches!(
                            a.recovery_reason.as_str(),
                            "ConnectionFailed"
                                | "NetworkDisconnect"
                                | "ConfirmedTerminalDisconnect"
                                | "Network"
                        )
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
                let _ = self.transaction(|s| {
                    s.network_suspended = !reachable;
                    if !reachable {
                        for saved in &mut s.database.accounts {
                            let a = &mut saved.account;
                            if a.desired_running
                                && a.status == Status::Reconnecting
                                && matches!(
                                    a.recovery_reason.as_str(),
                                    "ConnectionFailed"
                                        | "NetworkDisconnect"
                                        | "ConfirmedTerminalDisconnect"
                                )
                            {
                                a.recovery_reason = "Network".into();
                                a.last_error = Some("Waiting for internet / Roblox service".into());
                            }
                        }
                    }
                    Ok(())
                });
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
