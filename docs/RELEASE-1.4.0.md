# RoLauncher 1.4.0

2026-10-07 · Changes since stable 1.3.0.

## Added

- Saved System/Light/Dark theme, game names with Place ID fallback, and per-row Start/Stop/Restart.
- Multi-cookie import beside sign-in; blanks/duplicates ignored.

## Changed

- Dedicated sidebar settings pages; richer Dashboard status, rejoin, connectivity, and attention summaries.
- Separate preset use/creation with searchable, filtered account selection and confirm/cancel.
- Rejoin controls move to individual/bulk editors; saved-game fields lock until **Custom destination** is chosen.
- Account table fills wide windows and scrolls in small ones; empty-space clicks clear selection.

## Fixed

- After startup with Roblox already open, close those clients and retry Start without restarting RoLauncher.

Accounts, destinations, rejoin preferences, presets, and recovery behavior are preserved.

Validation: Rust/Clippy/formatting, desktop checks, WinUI pages/interactions, imports/preset dialogs, and installer handoff passed.
Linux built and passed native-input/Save/Cancel smoke under WSL; native live Sober remains pending.
Hosted Windows/Ubuntu checks and package verification gate publication.
