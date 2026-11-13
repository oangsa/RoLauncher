# RoLauncher 2.0.0-dev.1c5f27a17cc4

Manage Roblox accounts, save destinations, and rejoin disconnected sessions.
RoLauncher is a native WinUI 3 desktop application for Windows.

## Download

[Download RoLauncher](https://github.com/oangsa/RoLauncher/releases/latest).
Choose **setup-x64.exe**, install, and open it from Start. A portable ZIP is available.

- Windows 10 (1809+) or Windows 11, x64.
- Browser sign-in requires [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).
- Linux support is discontinued and unsupported; [historical 1.7.0 preview notes](docs/LINUX.md) are retained for archival reference only.

## Get started

1. Add accounts through browser sign-in or cookie import.
2. Choose destinations and select accounts.
3. Press **Start**.

Keep cookies private; never include them in bug reports.

## Features

- Saved games, account groups, presets, and bulk edits.
- Automatic rejoin, fallback policies, activity Dashboard, and read-only Resource Monitor.
- Encrypted backups, optional Discord alerts, and bot commands (`/action`, `/status-register`, `/resources`, `/resources-register`).
- System/light/dark themes, tray controls, and **Download → Relaunch** updates.
- Optional beta updates under **Settings → Updates**.

Release 1.7.6 restores 30-second reconnect grace so redirects can finish before recovery closes a client.
Persistent confirmed disconnects recover after grace; replacements wait for verified exit and retry backoff.
RAM-only hints cannot close clients. Smart cannot guarantee region or local latency. See [release notes](docs/RELEASE-1.7.6.md).

## Documentation

Unexpected launches or paused rejoin: **Support → Export report** includes recent decision logs.
Full rotating logs and troubleshooting steps: [Diagnostic logs](docs/DIAGNOSTICS.md).

[User guide](docs/USAGE.md) · [API](docs/API.md) · [Development](docs/DEVELOPMENT.md) · [Changelog](CHANGELOG.md)

[Report bugs](https://github.com/oangsa/RoLauncher/issues) · [Validation and limits](docs/VALIDATION.md)

## License

[GPL-3.0-only](LICENSE) · [Third-party notices](THIRD_PARTY_NOTICES.md)
