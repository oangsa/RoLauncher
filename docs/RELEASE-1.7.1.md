# RoLauncher 1.7.1

Upcoming hotfix from stable 1.7.0. Local packages use unique dev suffixes.
Patch version explicitly requested; no GitHub release published.

## Recovery and startup

- Pause on Roblox update error 280 and experience ban 600; preserve duplicate-session code 264.
- Ignore connection tags inside oversized game-output lines, including lines split across reads.
- Detect join HTTP failures when status/error numbers have leading spaces.
- Find error 273/disconnects in busy logs; keep confirmed disconnects when log reads fail.
- Keep Windows exit checks running during slow client/log discovery; reuse verified handles and stop repeating blocked close attempts.
- Read recent RAM log messages when Windows file logs stall. Never modify client memory/files.
- Close hidden startup leftovers from current user's Roblox installation/session when startup is blocked. Keep visible games and saved managed clients.
- Multiple instances optional, default On for new/existing users. Restart app to apply changes.

## Desktop and imports

- Report malformed JSON, null roots and invalid error fields without leaving refresh locked.
- Validate four sessions at once; save accounts/results in submitted order. Large imports get ten minutes; other requests keep 30 seconds.

## Server selection

- Smart checks pages beyond 500 servers, team space, reported ping, player count.
- Keep one candidate per player count. Lowest/Highest stop at first sorted page with room.
- Repeated page cursor, API error, rate limit, or two-minute timeout stops search safely. Timeout schedules delayed retry.
- Region/local latency unavailable; nearest region not guaranteed.

## Validation and performance

- [Check results](VALIDATION.md#upcoming-171-hotfix--2026-10-17).
- Three optimized runs: 93.1% less log-reading time; 99.3% less exit-check time in stated tests.
- 500-server test kept 48 entries instead of 480: 90% fewer entries, not measured RAM savings.
- Final idle build: 21.74 MiB working set, 2.05 MiB private memory, 0.019% machine CPU. Desktop/Roblox excluded; no idle saving proven.
- Live Roblox, low-end PCs with many clients, native Linux untested. RAM detection not guaranteed.

[Measurements and methodology](PERFORMANCE.md#171-hotfix-measurements) · [Experiment ledger](HOTFIX-1.7.1-EXPERIMENTS.md) · [Usage](USAGE.md).
