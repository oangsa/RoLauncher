# RoLauncher 1.7.1

Upcoming patch based on stable 1.7.0. Linux remains a beta.

## Recovery

- Begin controlled recovery as soon as an owned log confirms a terminal disconnect; verify the process identity and exit before replacement.
- Keep retry backoff, Stop, account state and process-generation checks in force.
- RAM-only hints, ambiguous events, isolated HTTP failures, and missing logs cannot close a client.
- Pause for bans, access restrictions, required updates, and duplicate-session errors that need attention.
- Startup cleanup defaults Off. When enabled, it closes only hidden external clients after a verified launch conflict; visible games and managed clients stay open.
- Multiple instances remain optional and default On; restart RoLauncher to apply the setting.

## Server selection

- Smart searches beyond the first 500 listed servers and considers team capacity, reported ping and player count.
- Lowest and Highest stop at the first sorted page with enough room. Repeated cursors and API failures stop the search safely.
- Reported ping cannot guarantee region or local latency.

## Account imports

- Import multiple session cookies in batches of up to 50, with up to four validations running at once.
- Preserve submission order, report failed entry positions, and keep successful accounts when another entry fails.
- Large imports have a longer request deadline; other API requests retain their existing timeout.

## Limits

- Live Roblox behavior, low-end Windows systems with many clients, and native Linux/Sober recovery remain unverified.
- Recovery depends on Roblox logs and process signals; unknown events do not authorize automatic termination.

[Usage](USAGE.md) · [Validation](VALIDATION.md)
