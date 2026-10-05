# Changelog

## [1.0.0] — 2026-10-06

- Renamed the product and Windows app to RoLauncher, retaining existing accounts and encrypted backups.
- Added the launch-arrow app, tray and installer icon selected by the owner.
- Added a per-user Windows `.exe` installer with shortcuts and uninstall support.
- Check the official GitHub repository automatically at startup and every six hours. Download a verified installer, then press Relaunch to back up data, install and reopen automatically. Roblox clients stay open.
- Added groups, atomic bulk editing, saved launch profiles, recovery policies, persistent history, support reports and encrypted backups.
- Added reviewed `feature/* -> dev -> main` delivery, Windows CI, versioned release notes, corresponding source, license notices and SHA256 checksums.

The first public GitHub release remains 1.0.0 at the owner's explicit request. Earlier local packages are retained. See `docs/VALIDATION.md` for test coverage and live-integration limitations.
