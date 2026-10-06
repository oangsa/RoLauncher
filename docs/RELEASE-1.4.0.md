# RoLauncher 1.4.0

Release date: 2026-10-07.

Changes since published stable 1.3.0.

## Fixed

- Starting RoLauncher while a Roblox client is open no longer permanently disables launching. Close existing clients yourself, then retry Start without restarting RoLauncher.

## Added

- System, Light, and Dark themes with a saved appearance preference.
- Game names in the account table, with Place ID shown when a name is unavailable.
- Start, Stop, and Restart buttons for each account, independent of the table selection.
- Paste multiple session cookies at once, one per line; blank lines and duplicate cookies are ignored.

## Changed

- Settings are organized into dedicated sidebar pages for general preferences, account presets, Discord notifications, backups, updates, and Support and integrations.
- Dashboard highlights account status distribution, automatic rejoin coverage, network and destination status, and accounts needing attention.
- Account presets have separate use and creation sections, a single Choose accounts button that opens a searchable account table in a modal, with group, status and rejoin filters, and a preview of the saved accounts. Choices persist across filters; confirm to use them or cancel to discard changes.
- Automatic rejoin is managed in individual or bulk account settings; the separate control in Settings is removed.
- The account table expands to use wide windows and scrolls horizontally in smaller windows. Clicking empty workspace space clears the selection.
- Cookie import is visible alongside browser sign-in instead of hidden in an expander.
- Choosing a saved game locks its destination fields; choose Custom destination to edit them manually.

Existing accounts, destinations, rejoin preferences, and presets are preserved. Automatic recovery behavior is unchanged.

## Validation

Local validation covers Rust tests, clippy and formatting; desktop model/editor/update tests; native Windows UI interactions and every sidebar page at the minimum window size; cookie paste import and modal preset lookup selection across filters, confirmation, cancellation and reopening; and installer/update handoff. The Linux shell builds and its native-input smoke passes under WSL, including account Save/Cancel and bulk Cancel. The release pipeline runs Windows and Ubuntu checks and verifies packaged artifacts before publication. Live Sober validation remains pending.
