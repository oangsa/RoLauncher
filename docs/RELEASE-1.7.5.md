# RoLauncher 1.7.5

Upcoming bug-fix release. Based on published stable 1.7.4. Local packages carry a unique development suffix.

## Changes

- Fully terminate verified disconnected clients before rejoining, including clients still running in tray; wait for confirmed exit and retry backoff.

## Behavior and limits

Confirmed disconnects now terminate the verified managed process directly, avoiding background clients left alive after window closure.
Replacement waits for process exit and retry backoff; other accounts stay open.
Stop, disabled rejoin, reconnection, retry limits, access restrictions, and process ownership checks still apply.

For manual cleanup on Windows, use Accounts → More actions → Stop all Roblox.
Automatic rejoin stops for every saved account; start wanted accounts after cleanup finishes.
This existing action includes external, hidden, and tray clients in the current Windows session.

Automated recovery and disposable Windows process fixtures cover the change.
A live Roblox tray/rejoin cycle and native Linux recovery remain unverified.

[Validation](VALIDATION.md)
