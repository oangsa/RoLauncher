# RoLauncher 1.7.7

Bug-fix release based on published stable 1.7.6.

## Changes

- Close persistent disconnects gracefully, then escalate after five seconds if the same verified client remains.
- Cancel automatic escalation if connection returns during the graceful-close window.

- Resume automatic rejoin after a failed client close once the managed process has exited.
- Recover saved accounts paused by this failure when RoLauncher restarts and no client remains.

## Behavior and limits

Recovery allows 30 seconds for reconnection before graceful closure and preserves retry count and backoff.
Replacement waits for confirmed process exit; unverified clients remain protected.
Ownership, policy, manual-close, authentication and retry-limit pauses still require attention.

## Validation

- Passed 160 Windows Rust tests, 98 desktop checks, formatting, Clippy, WinUI smoke and authenticated Rust/WinUI bootstrap.
- Four regressions cover exact-exit recovery, saved paused accounts, reconnect cancellation and force ownership checks; six unrelated tests remain explicitly ignored.
- No authenticated Roblox launch or live disconnect was performed.

[Validation history](VALIDATION.md)
