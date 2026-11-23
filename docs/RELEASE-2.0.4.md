# RoLauncher 2.0.4

Upcoming release fixing process-exit reconciliation delays, manual Restart reliability, and lifecycle status reporting after Roblox exits before connection.

## Highlights

- **Faster exit reconciliation:** Settle confirmed process exits within 1–5 seconds instead of waiting for the 90-second launch ownership deadline.
- **Reliable manual Restart:** Restart recovers from pending-exit, pre-dispatch, paused, and automatic-recovery states and survives launcher reopens without Stop → Start.
- **Exit telemetry:** Record read-only OS exit codes, process lifetimes, and final pre-exit log events in diagnostics without exposing secrets.
- **Accurate status and support exports:** Hide exited PIDs, report `exit_pending` accurately, and replace misleading late-launch labels in Accounts.

## Upgrade notes

- Local builds use `2.0.4-dev.<hex>` development identities until `2.0.4` is published.
- Saved account lists remain compatible with 2.0.3 without manual migration.
