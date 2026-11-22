use super::*;

impl Engine {
    // Called only with a completed, successful scan, no unavailable metadata
    // and no bootstrapper. Scan start (not completion) must follow the deadline.
    pub(super) fn reconcile_completed_inspection(
        &self,
        players: &[platform::Player],
        scan_started: chrono::DateTime<Utc>,
    ) {
        for expected in self.snapshot().accounts {
            let waiting = expected.status == Status::NeedsAttention
                && matches!(
                    expected.last_error.as_deref(),
                    Some(
                        OWNERSHIP_TIMEOUT_ERROR
                            | STARTUP_UNRESOLVED_ERROR
                            | STARTUP_AMBIGUOUS_TIMEOUT_ERROR
                    )
                );
            if !waiting
                || !expected.desired_running
                || expected.process.is_some()
                || expected.close_intent.as_ref().is_some_and(|i| i.denied)
            {
                continue;
            }
            let absent = expected.tracker.is_some()
                && players.iter().all(|p| {
                    !p.tracker.is_empty() && Some(&p.tracker) != expected.tracker.as_ref()
                })
                && expected
                    .ownership_deadline
                    .is_some_and(|deadline| deadline <= scan_started);
            let inspection_resolved = expected.tracker.is_none()
                && expected.last_error.as_deref() == Some(STARTUP_AMBIGUOUS_TIMEOUT_ERROR);
            if !absent && !inspection_resolved {
                continue;
            }
            let _ = self.transaction(|state| {
                let suspended = state.network_suspended;
                if retired_blocks_launch(&state.database, &expected.id) { return Ok(()); }
                let current = account_mut(state, &expected.id)?;
                if current.generation != expected.generation || current.operation_id != expected.operation_id
                    || current.process.is_some() || current.tracker != expected.tracker
                    || current.status != expected.status || current.last_error != expected.last_error
                    || current.close_intent != expected.close_intent || !current.desired_running
                { return Ok(()); }
                if absent {
                    current.tracker = None;
                    current.ownership_deadline = None;
                    let cooldown = active_cooldown(current)
                        .then_some(current.next_retry)
                        .flatten();
                    apply_failure(current, &Failure::new(
                        if suspended { FailureKind::Network } else { FailureKind::Other },
                        "Launch ended without a client; completed inspection confirms absence",
                    ));
                    if suspended {
                        current.transient_failures = 0;
                        current.next_retry = Some(cooldown.unwrap_or_else(Utc::now));
                    }
                } else {
                    current.status = Status::Queued;
                    current.last_error = None;
                }
                let operation = current.operation_id;
                let pending = current.status != Status::NeedsAttention;
                if pending && let Some(op) = operation.and_then(|id| state.operations.get_mut(&id)) {
                    op.state = "queued".into();
                    op.error = None;
                }
                self.0.journal.record("launch_inspection_resolved", serde_json::json!({
                    "account": crate::diagnostics::account(&expected),
                    "reason": if absent { "verified_absence_after_deadline" } else { "metadata_available" },
                }));
                Ok(())
            });
        }
    }
}
