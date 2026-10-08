# Changelog

## [1.5.0] — 2026-10-08

- Add Discord Stop/Restart slash commands, username autocomplete and /status-register for a saved status message that updates automatically, restricted to configured server/users. Separate Save, Start, Stop and Restart controls retain bot credentials, with a colored connection status. The bot resumes on app launch if it was previously started; Stop keeps it off until started again.
- Show current running time and saved longest online streak on each account row, plus the longest streak and account on Dashboard. Disconnects and automatic rejoins count toward a run; Stop or manual Restart ends it.
- Open account and game editors by clicking rows/cards; select bulk accounts with a dedicated checkbox column and highlight game cards on hover.
- Suppress transient redirect and clean-exit alerts; include game names, detected errors and available instance screenshots.
- Explicitly isolate Sober HOME inside Flatpak and read owned logs through process descriptors. Native Arch/KDE validation remains pending.

## [1.4.0] — 2026-10-07

### Fixed

- Starting RoLauncher while a Roblox client is open no longer permanently disables launching. Close existing clients yourself, then retry Start without restarting RoLauncher.

### Added

- System, Light, and Dark themes with a saved appearance preference.
- Game names in the account table, with Place ID shown when a name is unavailable.
- Start, Stop, and Restart buttons for each account, independent of the table selection.
- Paste multiple session cookies at once, one per line; blank lines and duplicate cookies are ignored.

### Changed

- Settings are organized into dedicated sidebar pages for general preferences, account presets, Discord notifications, backups, updates, and Support and integrations.
- Dashboard highlights account status distribution, automatic rejoin coverage, network and destination status, and accounts needing attention.
- Account presets have separate use and creation sections, a single Choose accounts button that opens a searchable account table in a modal, with group, status and rejoin filters, and a preview of the saved accounts. Choices persist across filters; confirm to use them or cancel to discard changes.
- Automatic rejoin is managed in individual or bulk account settings; the separate control in Settings is removed.
- The account table expands to use wide windows and scrolls horizontally in smaller windows. Clicking empty workspace space clears the selection.
- Cookie import is visible alongside browser sign-in instead of hidden in an expander.
- Choosing a saved game locks its destination fields; choose Custom destination to edit them manually.

## [1.3.0] — 2026-10-06

- Sidebar navigation for Accounts, Games, Dashboard, and Settings; secondary actions move into menus and expanders.
- Saved game profiles include a profile name, Roblox game name, thumbnail, and public, specific, or private-server destination. Create, edit, delete, and choose a saved game in single or bulk account settings.
- Read-only Dashboard with account counts, 24-hour event trends, persistent history, and recent activity moved from Settings. Recovery actions live in Accounts.
- Changelog opens from the icon beside the version, shows the latest two versions, and links to the full GitHub changelog.
- Settings renames Recovery & Access to Auto Rejoin and saves the close-to-tray or close-completely preference. The header Exit button is removed.
- Page changes reset scrolling; account row actions resolve the clicked account explicitly; Ctrl+A uses the shared keyboard accelerator without overriding text selection.
- Every isolated Sober launch grants input-device access, including retries, so a host Flatpak override is no longer required for that permission.

## [1.2.1] — 2026-10-06

- Center the update banner's Download, Cancel download and Relaunch button vertically with balanced spacing.

## [1.2.0] — 2026-10-06

- Match the account group dropdown height to its adjacent action buttons.
- Add a saved "Include beta updates" setting, beta release discovery and validated beta downloads; stable updates remain the default.
- Integrate private Flatpak runtime/IPC isolation into every Linux launch and retry, with separate saved state per account; no adapter installation or environment variable is needed. Simultaneous games remain unverified.
- Accept a Sober log inherited by helpers in the same verified sandbox; continue rejecting logs shared across sandboxes and ambiguous ownership.
- Discover logs immediately after verifying a launched PID and include safe log-availability counts in compatibility diagnostics.
- Include the Linux preview package alongside the stable Windows release; concurrent Roblox games and complete Linux parity remain unverified.

## [1.1.0] — 2026-10-06

- Add a Linux development port for Sober with shared WinUI views rendered by Uno.
- Keep native WinUI on Windows; add Linux credential, sign-in, process and desktop adapters.
- Linux support remains preview until the Ubuntu/Arch, GNOME/KDE parity matrix passes.

## [1.0.0] — 2026-10-06

- Renamed the product and Windows app to RoLauncher, retaining existing accounts and encrypted backups.
- Added the launch-arrow app, tray and installer icon selected by the owner.
- Added a per-user Windows `.exe` installer with shortcuts and uninstall support.
- Check the official GitHub repository automatically at startup and every six hours. Download a verified installer, then press Relaunch to back up data, install and reopen automatically. Roblox clients stay open.
- Added groups, atomic bulk editing, saved launch profiles, recovery policies, persistent history, support reports and encrypted backups.
- Kept legacy accounts available when an update download creates the new data folder before migration.
- Simplified the README and separated the user guide, API reference and build instructions.
- Added reviewed `feature/* -> dev -> main` delivery, Windows CI, versioned release notes, corresponding source, license notices and SHA256 checksums.

The first public GitHub release remains 1.0.0 at the owner's explicit request. Earlier local packages are retained. See `docs/VALIDATION.md` for test coverage and live-integration limitations.
