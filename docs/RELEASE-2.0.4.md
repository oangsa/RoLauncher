# RoLauncher 2.0.4

Upcoming release fixing process-exit reconciliation delays, manual Restart reliability, and lifecycle status reporting after Roblox exits before connection.

## Highlights

- **Faster, account-scoped exit reconciliation:** Settle confirmed process exits within 1–5 seconds instead of waiting for the 90-second launch ownership deadline, without unrelated bootstrappers or metadata failures blocking another account.
- **Reliable manual Restart:** Restart recovers from pending-exit, unresolved-tracker, pre-dispatch, paused, and automatic-recovery states and survives launcher reopens without Stop → Start.
- **Exit telemetry:** Record read-only OS exit codes, process lifetimes, and final pre-exit log events in diagnostics without exposing secrets.
- **Accurate status and support exports:** Hide exited PIDs, report `exit_pending` accurately, and replace misleading late-launch labels in Accounts.
- **Faster verified delivery:** Deduplicate PR CI runs, add PR cancellation, cache Rust builds safely, unify workspace builds, and verify both Release binaries before publishing.

## Upgrade notes

- Local builds use `2.0.4-dev.<hex>` development identities until `2.0.4` is published.
- Saved account lists remain compatible with 2.0.3 without manual migration.
