# RoLauncher 1.7.0

2026-10-15 · Changes since stable 1.6.0. Linux remains a beta.

## Added

Under **Settings → General → Game joining**, preferences save for future starts/rejoins:

- **Smart:** fewest players within 25 ms of the best positive reported ping; population fallback when all ping values are unavailable.
- **Lowest / Highest:** fewest/most players with room. Scan at most 500 servers within 30 seconds; no listed servers uses normal matchmaking.
- **Friend:** join managed accounts with the same assigned saved game/destination. The first account seeks room for the team; followers wait for its instance.
- Hidden presence can use the peer's saved session; the place must match. Missing presence, access failures, or full servers use retry/attention behavior.
- **Random:** independent selection; default.

Job IDs/private links take priority; running clients stay open.
Assignments survive individual/bulk edits, presets, backups, and restarts.
Existing accounts must select their saved game again; copied destinations alone do not group them.
Deleting a game clears assignments but retains destinations.

## Fixed

- Failed HTTP join requests, including timeout/Join Error 529 without connection or `GameJoinFailed` events, now trigger recovery from Unknown.
- Detection requires a verified owned log; unrelated HTTP errors are ignored.
- Existing 30-second grace, retry limits, Stop, and automatic-rejoin settings still apply.

## Limits

- Reported ping/population cannot guarantee region or local latency; bounded scans can miss better servers.
- Roblox enforces access/capacity; selection and presence reserve no slots.
- No process-memory access, injection, executors, or client-file changes are used.

[Validation](https://github.com/oangsa/RoLauncher/blob/v1.7.0/docs/VALIDATION.md) · [Public-server API](https://create.roblox.com/docs/cloud/reference/domains/games) · [Presence API](https://create.roblox.com/docs/cloud/reference/domains/presence)
