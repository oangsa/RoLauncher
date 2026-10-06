# RoLauncher 1.1.0

A Roblox account launcher. Manage your accounts, save launch profiles, and reconnect automatically when a session disconnects. Windows uses native WinUI; the Linux development port reuses the same views through Uno and targets Sober on Ubuntu and Arch with GNOME and KDE.

Linux is a **beta**, with live Sober account/recovery behavior still awaiting testing on GNOME and KDE. See [Linux setup and acceptance criteria](docs/LINUX.md). Windows keeps its native WinUI interface.

## Download

[**Download RoLauncher**](https://github.com/oangsa/RoLauncher/releases/latest)

Choose the **setup-x64.exe** installer, run it, then open RoLauncher from the Start menu. A portable ZIP is also available.

Requires Windows 10 (1809+) or Windows 11, 64-bit. Browser sign-in needs the [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).

[**Download the 1.1.0 Linux/Sober beta**](https://github.com/oangsa/RoLauncher/releases/tag/beta-v1.1.0)

For Linux, choose **linux-x64.tar.gz** and follow the [setup instructions](docs/LINUX.md#install-the-beta). The beta is separate from the stable Windows release.

## Features

- Organize accounts into groups and launch them with saved profiles.
- Edit several accounts at once and manage automatic rejoin.
- View recovery status and activity history.
- Keep encrypted backups and receive optional Discord alerts.
- Update with **Download → Relaunch** when a new version is available.

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
