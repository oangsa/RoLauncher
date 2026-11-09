# RoLauncher 1.9.0

Upcoming Free feature release based on published stable 1.8.0. Local packages carry a unique development suffix.

## Added

- Observe Windows clients independently of WMI and retain verified tracker evidence on exact process handles.
- Prepare up to four unrelated launches concurrently while preserving five-second protocol spacing.

## Changed

- Show Stopping until owned clients and cancelled launches are resolved.

## Fixed

- Reject trackers hidden inside quoted payloads and client images outside the verified installation root.
- Keep explicit Stop pending through temporary verification outages and report partial Stop All failures.
- Rediscover Windows log ownership after file replacement.

The API adds `stopping`; HTTP 202 still accepts an operation without confirming exit.
The standard current-user Roblox installation remains required; ambiguous clients stay open.
No new launch Event, client modifications, or Premium functionality is included.

[Lifecycle findings and validation](INSTANCE-LIFECYCLE-AUDIT.md) · [Diagnostic logs](DIAGNOSTICS.md)
