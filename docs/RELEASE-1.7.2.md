# RoLauncher 1.7.2

Hotfix release based on published stable 1.7.1.

## Changes

- Add Close all Roblox to stop rejoin and close external, hidden, and tray clients in the current Windows session.
- Rejoin after confirmed game kicks (267), with bounded retries and verified process exit before replacement.
- Prevent duplicate launches while an earlier launch remains unresolved; require discovery before releasing expired launch tracking.
- Honor Stop for verified clients discovered late, allowing cleanup after the launch discovery deadline.

## Usage and limits

1. Open Settings → Game joining → Close all Roblox to clean up every client in your Windows session.
2. Confirm the action; automatic rejoin stops for all saved accounts.
3. Start the accounts you want after cleanup finishes.

The action verifies Roblox executable paths and process creation times before termination, including clients without windows or launch trackers. Access failures are reported; they do not enable automatic relaunch. Windows only; Linux remains a beta.

Error 267 is a game kick, so confirmed owned-log evidence now allows bounded recovery. Explicit access restrictions (524/533/600/773), client rejection (268), and required updates (280) still pause recovery.

## Validation

Windows regression tests, 98 desktop checks, WinUI smoke, backend-to-desktop connection, formatting, Clippy, and development package verification passed. The isolated stable branch is rebuilt and tested by release CI. Live Roblox sessions and native Linux remain unverified.
