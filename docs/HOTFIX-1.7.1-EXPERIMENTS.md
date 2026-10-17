# 1.7.1 experiment ledger

October 17, 2026: verified published `v1.7.0`; working base `dc7b161` includes release rules.
No live Roblox clients/logs available.

| Test | Result / finding |
| --- | --- |
| Error 273 after 3.5 MB of output | Old reader misses first check; old output delays current state. |
| Match on page six | Old query fails; new local HTTP test passes. Five-page limit hides matches. |
| Slow client/log discovery | Old monitor waits before checking exits; metadata delays affect every account. |
| Exit checks | Old code reopens/rechecks clients each second; verified handles can be reused. |
| Region data | API reports ping, not verified region/local latency. No nearest-region claim. |
| Log error after 273 | 50 accounts keep Reconnecting; read failure no longer erases disconnect. |
| Deliberately blocked discovery | Monitor returns immediately; pending work does not stop other checks. |
| 50 harmless processes exit | All retained handles detect exit without repeated path checks. |
| RAM log tests | Fresh messages pass; old/fake-channel messages fail. Source bytes unchanged; no raw-memory reports. |
| Hidden startup process beside protected peer | Only unmanaged process closes; installation/session/visibility/saved identity limit cleanup. |
| Save multiple instances Off | Saved choice changes now; active mode changes after app restart. |
| Three optimized comparisons | Median log reading: 93.1% less time; exit checks: 99.3% less time. Test parts only. |
| Separate 20-second no-UI runs | CPU about 0.019% each; working set 20.11 to 21.05 MiB. No idle resource saving claimed. |

Simulated tests do not prove live recovery rates or low-end PC performance.

## Error audit — October 17, 2026

Reviewed logs, recovery, server/friend queries, imports, backups, Discord, updates and desktop refresh.
Existing tests stay unchanged. Replay tools/data use isolated temporary files and simulated accounts.

| Replay | Before | After |
| --- | --- | --- |
| `GameJoinFailed` code 600 | Retries a creator ban | Pauses for access check |
| Code 280, join/network channels | Repeated recovery | Pauses; asks for Roblox update |
| Network code 264 | Generic disconnect | Keeps duplicate-session error code |
| Join HTTP `status: 503` | Missed | Join failure found |
| Oversized output, three appends, fake network tag | False Connected | Ignore until original line ends; accept next real line |
| API broken JSON / null / numeric error / array error | Escaped exceptions or null | Handled API error; refresh lock always released |
| Import 50 sessions | Sequential checks; desktop waits 30 seconds | Four checks at once; ordered saves; ten-minute deadline |
| Import invalid/failed/duplicate sessions | Ordered partial results | Same order, indexed failures, original strings cleared, at most four requests |

Early log rejection only skips unrelated channels. Real network/error channels keep existing handling.
Five comparisons: [scope and results](PERFORMANCE.md#additional-error-audit-comparisons).

- Real-event timing exposed repeated channel checks in the first optimization. Classifying once removed that extra work; genuine events also compared.
- Final compilation hit a full disk. Removed only workspace Rust incremental caches, then reran checks; source/tests/packages preserved.

### Error coverage and limits

- 264/273 duplicate sessions, 266/277 timeouts/lost connection, 267/268 kicks and 279 failures use existing grace and bounded recovery.
- 280 now pauses for update; 524/533/600/773 pause for access. 528/772 use configured destination fallback policy.
- Other authoritative `GameJoinFailed` codes keep their number and use bounded recovery; HTTP join failures keep sanitized summaries.
- Studio/Team Create error 270 is outside player supervision. Library-load dialogs need a verified exit/log signal; visual-only dialogs are not guaranteed detected.
- Bans, missing permissions, client repairs, server outages and geographic server data require Roblox/user action. No bypass or client-memory changes.
- Server paging follows Roblox's returned cursor. A missing cursor cannot prove every server was listed; nearest region remains unverified.

Sources: Roblox [error list](https://en.help.roblox.com/hc/en-us/sections/360007755831-Reporting-Bugs), [264](https://en.help.roblox.com/hc/en-us/articles/36665660855700-Error-Code-264-Same-account-launched-experience-from-different-devices), [280](https://en.help.roblox.com/hc/en-us/articles/36792120219284-Error-Code-280-Your-version-of-Roblox-may-be-out-of-date-Please-update-Roblox-and-try-again), [600](https://en.help.roblox.com/hc/en-us/articles/39673241220116-Error-Code-600-Banned-from-an-Experience), [game API](https://create.roblox.com/docs/cloud/reference/domains/games).
