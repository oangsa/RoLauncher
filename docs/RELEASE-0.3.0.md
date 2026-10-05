# RoLauncher 0.3.0

Desktop feature release, 2026-10-05, Windows x64.

- Adds native Accounts, Settings and Changelog tabs.
- Removes Save. Valid alias/target edits save after 600 ms without further typing and flush when switching tabs, selecting accounts or performing actions. Incomplete targets retain the previous persisted destination; Start/Restart are blocked for the affected account until the draft is valid. Drafts are tied to account IDs and stay available during the current session.
- Renames the user-facing automatic recovery switch to Automatic rejoin. Moves it and the API token action into Settings.
- The switch applies to selected accounts, indicates mixed values, and saves immediately. Turning it off cancels backoff and prevents an automatic launch still awaiting its authentication request. Turning it on resumes only a retry paused by this switch. Manual Start remains available.
- Adds multi-selection with Ctrl+A in the account table, Ctrl-click and Shift-click. Start, Stop, Restart, Remove and rejoin changes apply to all selected accounts. Valid per-account actions continue when another account reports an error. Editing individual targets requires a single selection.
- Adds a Rejoin column to show each account's setting. Selection persists across status refreshes.

The local API retains the auto_recovery field for compatibility; desktop labels use rejoin. No credential export, process-memory access or client modification is introduced.

Validation includes native Win32 controls with three simulated accounts, tab visibility, automatic saving, invalid drafts, mixed settings, multi-selection, and bulk Start/Stop/Restart/Remove. Backend coverage checks cancelling/re-enabling backoff while preserving authentication pauses. Full live Roblox rejoin compatibility remains unverified.

Upgrade: Exit the previous app, extract the entire ZIP, and run RoLauncher.exe with WebView2Loader.dll beside it. Saved accounts and launch records use the existing data folder.
