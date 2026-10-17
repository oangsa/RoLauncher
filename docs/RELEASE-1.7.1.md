# RoLauncher 1.7.1

Upcoming hotfix from stable 1.7.0. Local packages use unique dev suffixes.
Patch version explicitly requested; no GitHub release published.

## Recovery and startup

- Pause for bans, access restrictions and update errors, including 267, 268, 280 and 600; preserve duplicate-session codes.
- Ignore connection tags inside oversized game-output lines, including lines split across reads.
- Recognize join HTTP failures with spaced status fields; a failed request alone does not authorize closing a client.
- Find error 273/disconnects in busy logs; keep confirmed disconnects when log reads fail.
- Keep Windows exit checks running during slow client/log discovery; reuse verified handles and stop repeating blocked close attempts.
- Read recent RAM log messages when Windows file logs stall. RAM-only hints cannot close clients; memory and client files stay unchanged.
- Startup cleanup defaults Off. Enable it in Settings to close hidden external Roblox clients only after a verified launch conflict.
- Keep visible games, saved managed clients and unresolved launches protected; failed inspection gives manual cleanup guidance.
- Multiple instances optional, default On for new/existing users. Restart app to apply changes.

## Immediate recovery policy

Confirmed disconnects start controlled recovery immediately, including Connection Lost, recoverable Session Lost and error 279. No reconnection grace timer applies.
Verified exits schedule recovery immediately; retry backoff still controls the next launch.

1. Check Auto Recovery, desired account state, account generation and exact managed process.
2. Begin controlled close immediately; recheck identity and cancellation under the command lock.
3. Verify the old process exited before launching a replacement with saved settings.

- Known process-owned disk events can confirm disconnects; unknown codes, transport changes and isolated HTTP failures need independent evidence.
- RAM strings, silence, missing logs and inaccessible memory never authorize automatic termination.
- A fresh owned Connected event cancels pending close; the monitor never waits for one after a confirmed disconnect.
- Stop, disabled Auto Recovery and shutdown prevent replacement. Duplicate signals count as one incident; retry limits remain.
- Bans, access restrictions, unexpected client behavior and required updates stay in Needs Attention.
- Publisher activation remains disabled. Independent re-review is required before merge or release.

Event codes follow [Roblox's error guidance](https://en.help.roblox.com/hc/en-us/articles/36639955031060-Roblox-Error-Codes).
Unknown codes remain unconfirmed; log format alone does not establish terminality.

## Desktop and imports

- Report malformed JSON, null roots and invalid error fields without leaving refresh locked.
- Desktop imports use batches up to 50, validating four sessions at once and saving in submitted order.
- Report failed entry positions, retain partial successes, clear cookie arrays and restore controls after errors.
- Large imports get ten minutes; other requests keep 30 seconds.

## Server selection

- Smart checks pages beyond 500 servers, team space, reported ping, player count.
- Keep one candidate per player count. Lowest/Highest stop at first sorted page with room.
- Repeated page cursor, API error, rate limit, or two-minute timeout stops search safely. Timeout schedules delayed retry.
- Region/local latency unavailable; nearest region not guaranteed.

## Recovery policy measurements

Three optimized Windows fixture runs used 100 events per case, fake managed identities and a stub close callback.
Times are medians of each run's median; detection starts after a log append and excludes the monitor's normal one-second polling wait.
Scheduling includes accepting and saving evidence; close timing also includes saving retry state and checking cancellation.

| Log fixture | Detection | Recovery scheduling | Controlled-close callback | Worst callback |
| --- | ---: | ---: | ---: | ---: |
| Small log | 1.10 ms | 14.17 ms | 31.32 ms | 205.63 ms |
| 3.5 MB backlog | 16.66 ms | 3.42 ms | 16.32 ms | 139.06 ms |

The old policy required 30,000 ms of grace; the new policy adds zero grace (100% of that fixed delay removed).
This is a policy comparison, not a measured whole-restart speedup; process shutdown, retry backoff and launch time remain separate.
No live Roblox or low-end PC recovery claim is made.

The isolated test process averaged 11.06-16.94 MiB working set, 3.02-3.09 MiB private memory and 1.05-1.51% machine CPU across runs (16 logical processors).
Resource samples include fixture setup/file writes, exclude Roblox/desktop and use the last live 50 ms CPU sample.
These observations establish no CPU or memory improvement percentage.

Reproduce with `cargo test --locked --release --lib recovery_policy_latency_fixture -- --ignored --nocapture --test-threads=1`.
Runtime evidence is not persisted or exported; tests cover stale identities, RAM-only hints, duplicates, Stop, access errors, exit gating and backoff.

## Validation and performance

- [Check results](https://github.com/oangsa/RoLauncher/blob/codex/hotfix-supporting-files/docs/VALIDATION.md#upcoming-171-hotfix--2026-10-17).
- Three optimized runs: 93.1% less log-reading time; 99.3% less exit-check time in stated tests.
- 500-server test kept 48 entries instead of 480: 90% fewer entries, not measured RAM savings.
- Final idle build: 21.74 MiB working set, 2.05 MiB private memory, 0.019% machine CPU. Desktop/Roblox excluded; no idle saving proven.
- Live Roblox, low-end PCs with many clients, native Linux untested. RAM detection not guaranteed.

[Measurements and methodology](https://github.com/oangsa/RoLauncher/blob/codex/hotfix-supporting-files/docs/PERFORMANCE.md#171-hotfix-measurements) · [Experiment ledger](https://github.com/oangsa/RoLauncher/blob/codex/hotfix-supporting-files/docs/HOTFIX-1.7.1-EXPERIMENTS.md) · [Usage](USAGE.md).
