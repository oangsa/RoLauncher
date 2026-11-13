# RoLauncher 2.0.0

Upcoming Free release (Phase 1 Resource Monitor preview on `dev`; not yet released).

## Resource Monitor

- Added a centralized, read-only Rust resource sampler using Windows process and memory APIs tied to verified PID and creation identity.
- Reports host CPU utilization, physical memory totals/availability, running managed instance counts, and aggregated managed CPU and RAM usage.
- Reports per-account CPU (normalized across logical processors), working-set and private memory, session uptime, sample timestamps, and explicit exited, inaccessible, stale, or mismatched states.
- Includes optional best-effort PDH GPU Engine utilization that reports `Unavailable` cleanly when unsupported.

## Local API and temporary WinUI preview

- Added authenticated loopback routes `GET /v1/resources`, `GET /v1/resources/accounts/{account_id}`, and SSE `GET /v1/resources/events` while keeping existing `/v1` compatibility unchanged.
- Active 1-second sampling runs while consumers stream or view **Resources** and suspends automatically when idle.
- Added a temporary **Workspace → Resources** WinUI 3 page with 60-second CPU/RAM graphs and a sortable per-instance table.

## Discord bot integration

- Added `/resources` (with optional `account` lookup and authorized autocomplete) and `/resources-register` to post a paginated, privacy-warned live monitor refreshed about every 30 seconds independently of `/status-register`.
