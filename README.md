# RoLauncher 1.6.0-dev.f5d4ce6294f5

A Roblox account launcher. Manage your accounts, save launch profiles, and reconnect automatically when a session disconnects. Windows uses native WinUI; the Linux development port reuses the same views through Uno and targets Sober on Ubuntu and Arch with GNOME and KDE.

Linux is a **beta**, with live Sober account/recovery behavior still awaiting testing on GNOME and KDE. See [Linux setup and acceptance criteria](docs/LINUX.md). Windows keeps its native WinUI interface.

This development build targets the upcoming 1.6.0 navigation and Windows materials update. See [upcoming release notes](docs/RELEASE-1.6.0.md), [stable 1.5.0 notes](docs/RELEASE-1.5.0.md), and [development versioning](docs/DEVELOPMENT.md#releases).

## Download

[**Download RoLauncher**](https://github.com/oangsa/RoLauncher/releases/latest)

Choose the **setup-x64.exe** installer, run it, then open RoLauncher from the Start menu. A portable ZIP is also available.

Requires Windows 10 (1809+) or Windows 11, 64-bit. Browser sign-in needs the [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).

[**Download the 1.5.0 Linux preview**](https://github.com/oangsa/RoLauncher/releases/tag/v1.5.0)

For Linux, choose **linux-x64.tar.gz** and follow the [setup instructions](docs/LINUX.md#install-the-linux-preview). Linux remains a preview even though the Windows release is stable.

## Features

- Use bundled Open Sans typography, matching the Windows and Linux interfaces.
- Choose dark, light, or system appearance with a saved theme preference, a neutral Mica workspace, consistent dialogs, an animated active-page highlight and gentle page transitions.
- Manage automatic rejoin in account settings, or search/filter and edit multiple accounts together. Create or launch presets through guided sections.
- Save games with thumbnails and private-server destinations, then choose them in account settings.
- Organize accounts into groups and launch them together.
- Edit several accounts at once and manage automatic rejoin.
- Monitor live Dashboard summary cards, account status distribution, rejoin coverage, network status, accounts needing attention, event trends, and activity history.
- Navigate grouped sidebar tabs for accounts, settings, and tools; choose whether closing minimizes to tray or exits.
- Keep encrypted backups and receive optional Discord alerts.
- Update with **Download → Relaunch** when a new version is available.
- Use **Settings → Updates → Include beta updates** to opt into beta releases. Windows 1.0.0 and 1.1.0 can update to this stable version through their existing update check.

## Get started

1. Add an account using browser sign-in or cookie import.
2. Set its destination and select the accounts you want to use.
3. Press **Start**.

Keep session cookies private. Never include them in bug reports.

## Documentation

[User guide](docs/USAGE.md) · [API reference](docs/API.md) · [Build and contribute](docs/DEVELOPMENT.md) · [Changelog](CHANGELOG.md)

For bugs, [open an issue](https://github.com/oangsa/RoLauncher/issues). See [validation notes](docs/VALIDATION.md) for tested behavior and compatibility limits.

## License

[GPL-3.0-only](LICENSE). Protocol references and dependency notices are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
