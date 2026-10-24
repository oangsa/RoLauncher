# RoLauncher 1.7.3

Hotfix release based on published stable 1.7.2.

## Changes

- Clean up tray helper clients and tolerate exited processes while preserving visible games and verified managed clients.
- Retry opted-in background cleanup when Start is blocked by a lingering Roblox process.
- Move the all-client stop action to Accounts and remove Integration status from Dashboard.
- Give Game joining its own settings tab with separate joining preferences and Roblox instance controls.
- Show 10 history and recent activity entries per page, with pagination below each list.

## Usage and limits

1. Use Accounts → More actions → Stop all Roblox to close clients and stop automatic rejoin.
2. Open Game joining to configure joining preferences and Roblox instances.
3. Enable background cleanup to retry hidden-client cleanup when Start is blocked; the default remains Off.

Background cleanup preserves visible game windows, saved managed processes, and unresolved launches. The explicit Stop all action closes every verified Roblox client in the current Windows session. Windows process identity and executable checks still apply; no Roblox memory or client files are modified.

[Validation](VALIDATION.md). Native Linux and live Roblox sessions require separate validation.

## Validation

Windows: 151 Rust tests, 98 desktop checks, native WinUI interactions, Rust-to-desktop bootstrap, formatting, Clippy, and isolated installer/update checks passed. The tray helper fixture failed before the fix and passed afterward; fixtures also cover visible-window protection, consent, unresolved launches, and activity pagination. Release CI rebuilds and verifies Windows and Linux packages from the exact committed source.
