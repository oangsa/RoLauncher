# RoLauncher 2.0.2

Upcoming Windows release.

## Fixed

- Recheck startup launch conflicts after empty cleanup scans and clear resolved coordination warnings automatically.
- Probe Roblox service reachability independently, defer temporary coordination or inspection outages, and keep shared outages from spending account retries.
- Classify idle (278) and duplicate-session (264/273) disconnects, protect active cooldowns on restart, and avoid duplicate force-close errors during process teardown.
- Show explicit waiting reasons, coordination banners, and queued-restart confirmation in the desktop interface.

## Upgrade

Install over 2.0.1 to keep your existing accounts and settings.
See [reliability validation](INSTANCE-RELIABILITY.md) for evidence and live-test limits.
