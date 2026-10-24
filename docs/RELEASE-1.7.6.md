# RoLauncher 1.7.6

Bug-fix release based on published stable 1.7.5.

## Changes

- Restore the 30-second reconnect grace used in 1.7.0 and earlier, allowing redirects to finish before closing a client.
- Suppress Discord disconnect alerts for brief generic connection losses; known errors still notify promptly.

## Behavior and limits

Reconnection cancels pending recovery without closing the client or consuming a retry.
Confirmed disconnects persisting for 30 seconds still terminate the verified client, including its tray lifetime.
Replacement waits for confirmed process exit and retry backoff.

Stop, disabled rejoin, retry limits, access restrictions, and process ownership checks still apply.
Ambiguous log events and RAM-only hints never authorize automatic closure.

## Validation

- Passed 156 Windows Rust tests, formatting, Clippy, 98 desktop checks, WinUI interaction smoke, authenticated Rust/WinUI bootstrap, and isolated installer/update checks.
- Versioning, beta publication, release promotion, hotfix policy, and owner-evidence regression checks passed.
- Replay covers reconnect grace, retry preservation, persistent disconnects, verified exit, Stop, disabled recovery, and other accounts; six Rust tests remain explicitly ignored.
- Live Roblox redirect/rejoin and native Linux recovery remain unverified locally; hosted Windows and Linux checks gate publication.
