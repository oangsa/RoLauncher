# RoLauncher 1.7.8

Bug-fix release based on published stable 1.7.7.

## Changes

- Avoid permanently pausing automatic rejoin when a closing client disappears before fresh process verification.

Missing process discovery now waits for exact process-exit confirmation before rejoin; retry count and backoff remain intact.
Conflicting ownership, unavailable discovery, policy denials and manual-close pauses remain protected.

Existing ownership-denial pauses from older versions require an explicit Stop then Start after inspecting the account; their saved cause is ambiguous.

No live Roblox disconnect cycle has been validated.

## Validation

- Regression failed against 1.7.7, then passed with missing-process deferral.
- Passed 162 Windows Rust tests, 98 desktop checks, formatting, Clippy, fresh WinUI smoke and authenticated Rust/WinUI bootstrap.
- Negative checks preserve manual-close, launch-policy and shared-ownership denials; no live account state was changed.
