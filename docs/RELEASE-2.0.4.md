# RoLauncher 2.0.4

Upcoming patch release (development builds carry a `-dev.<id>` suffix).

## Changes

- Reconcile verified Roblox process exits in 1–5 seconds instead of waiting up to 90 seconds for an expired startup ownership deadline.
- Complete manual Restart from `Unknown`, `Backoff`, `Reconnecting`, recoverable `NeedsAttention`, and pending-exit states without requiring `Stop` then `Start`.
- Record Windows process exit codes on exit observation and expose truthful `exit_pending`, `Waiting for handoff`, and `Restart pending` states in the UI and Support Export.
- Deduplicate PR CI runs, add safe PR cancellation, and cache Rust build artifacts to reduce Windows CI and release delivery latency.
