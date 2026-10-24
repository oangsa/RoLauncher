# RoLauncher 1.8.0

Upcoming feature release based on published stable 1.7.8. Local packages carry a unique development suffix.

## Added

- Save detailed local launch and recovery logs, including pause reasons, process ownership, retry decisions, and recent events in support exports.

Logs start automatically and rotate within the existing application data folder.
Use **Support → Export report** after an unexpected launch or pause; see [diagnostic logs](DIAGNOSTICS.md).
Cookies, authentication tickets, private links, launch URLs, and raw Roblox log lines are excluded.

[Validation and limits](VALIDATION.md)
