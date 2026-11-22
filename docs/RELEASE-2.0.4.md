# RoLauncher 2.0.4

Upcoming patch release (development builds carry a `-dev.<id>` suffix).

## Changes

- Reconcile verified Roblox process exits in 1–5 seconds instead of waiting up to 90 seconds for an expired startup ownership deadline.
- Complete manual Restart from `Unknown`, `Backoff`, `Reconnecting`, recoverable `NeedsAttention`, and pending-exit states without requiring `Stop` then `Start`.
- Record Windows process exit codes on exit observation and expose truthful `exit_pending`, `Waiting for handoff`, and `Restart pending` states in the UI and Support Export.
- Deduplicate PR CI runs, add safe PR cancellation, and cache Rust build artifacts to reduce Windows CI and release delivery latency.

## Delivery baseline

- v2.0.3 multi-PR promotion sequence spanned 127m58s wall-clock across PR #77, `dev` push/PR, PR #78, `main` CI (`1262s`), and `Protected release promotion` (`62s`).
- Cold-cache `Windows checks` on PR #79 completed in `1223s` (test step `535s`, down from `964–1009s`).

