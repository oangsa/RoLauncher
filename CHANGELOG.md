# Changelog

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
