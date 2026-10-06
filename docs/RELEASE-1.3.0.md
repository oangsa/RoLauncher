# RoLauncher 1.3.0

- Sidebar navigation for Accounts, Games, Dashboard, and Settings; secondary actions move into menus and expanders.
- Saved game profiles include a profile name, Roblox game name, thumbnail, and public, specific, or private-server destination. Create, edit, delete, and choose a saved game in single or bulk account settings.
- Read-only Dashboard with account counts, 24-hour event trends, persistent history, and recent activity moved from Settings. Recovery actions live in Accounts.
- Changelog opens from the icon beside the version, shows the latest two versions, and links to the full GitHub changelog.
- Settings renames Recovery & Access to Auto Rejoin and saves the close-to-tray or close-completely preference. The header Exit button is removed.
- Page changes reset scrolling; account row actions resolve the clicked account explicitly; Ctrl+A uses the shared keyboard accelerator without overriding text selection.
- Every isolated Sober launch grants input-device access, including retries, so a host Flatpak override is no longer required for that permission.

Existing account destinations and legacy account presets remain available. New game profiles are independent of accounts and are included in encrypted backups. Deleting a game does not erase account destinations already chosen from it. Close behavior retains minimize-to-tray for existing installations; Linux falls back to taskbar minimization when a tray host is unavailable.

Linux input grants require Flatpak 1.15.6 or newer.

Validation results and Linux live-player limitations are documented in docs/VALIDATION.md.
