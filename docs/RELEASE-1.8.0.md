# RoLauncher 1.8.0

Upcoming feature release based on published stable 1.7.8. Local packages carry a unique development suffix.

## Added

- Save detailed local launch and recovery logs, including pause reasons, process ownership, retry decisions, and recent events in support exports.

Logs start automatically and rotate within the existing application data folder.
Use **Support → Export report** after an unexpected launch or pause; see [diagnostic logs](DIAGNOSTICS.md).
Cookies, authentication tickets, private links, launch URLs, and raw Roblox log lines are excluded.

## Fixed

- Check for a surviving client before rejoining after a process exits, preventing replacement during a verified launch handoff.
- Retry temporary process-verification failures without permanently pausing automatic recovery.
- Keep brief redirects from repeating online alerts while an earlier retry is still counted.

Exit confirmation requires a fresh process scan and at least five seconds for handoff.
An early exit also retains the existing 90-second launch-discovery deadline; a unique surviving client with the same tracker resumes monitoring immediately.
Access restrictions, updates, retry limits, genuine ownership conflicts, and failed native closes retain their safeguards.

[Recovery comparison and evidence](RECOVERY-AUDIT.md)

[Validation and limits](VALIDATION.md)
