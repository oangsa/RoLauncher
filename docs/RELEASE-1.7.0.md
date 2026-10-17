# RoLauncher 1.7.0

2026-10-15 · Changes since stable 1.6.0. Linux remains a beta.

## Added

Under **Settings → General → Game joining**, preferences save for future starts/rejoins:

- **Smart:** fewest players within 25 ms of the best positive reported ping; population fallback when all ping values are unavailable.
- **Lowest / Highest:** fewest/most players with room. Scan at most 500 servers within 30 seconds; no listed servers uses normal matchmaking.
- **Friend:** join managed accounts with the same assigned saved game/destination. The first account seeks room for the team; followers wait for its instance.
- Hidden presence can use the peer's saved session; the place must match. Missing presence, access failures, or full servers use retry/attention behavior.
- **Random:** independent selection; default.

Job IDs/private links override; running clients stay open.
Assignments survive single/bulk edits, presets, backups, restarts.
Existing accounts must reselect saved game; copied destinations alone cannot group them.
Deleting game clears assignments, keeps destinations.

## Fixed

- Failed HTTP join requests, including timeout/Join Error 529 without connection or `GameJoinFailed` events, now trigger recovery from Unknown.
- Require verified owned log; ignore unrelated HTTP errors.
- Keep 30-second wait, retry limits, Stop, automatic-rejoin settings.

## Limits

- Reported ping/player count cannot confirm region/local latency; limited search may miss better servers.
- Roblox checks access/capacity; selection/friend status reserve no slots.
- No process-memory access, injection, executors, or client-file changes are used.

[Validation](https://github.com/oangsa/RoLauncher/blob/v1.7.0/docs/VALIDATION.md) · [Public-server API](https://create.roblox.com/docs/cloud/reference/domains/games) · [Presence API](https://create.roblox.com/docs/cloud/reference/domains/presence)
