# RoLauncher 1.1.0

Linux/Sober beta; `beta-v1.1.0` did not replace stable Windows 1.0.0.

- Uno/Skia shares Windows layouts/handlers; Windows retains native WinUI.
- Linux targets Ubuntu/Arch with GNOME/KDE through X11/XWayland.
- Adds per-account Sober homes, verified processes/logs, Secret Service encryption, WebKit sign-in, portals, tray, and verified archive updates.
- Uses installed protocols/metadata/logs; no Roblox memory reads or client-file changes.
- Full visual/interaction parity and live recovery remain unverified; credentials/backups are OS/user specific.

## Install the Linux beta

1. Download `rolauncher-v1.1.0-linux-x64.tar.gz` and `rolauncher-v1.1.0-SHA256SUMS.txt`; verify the archive entry.
2. On Ubuntu 24.04+ or current Arch x64, install Sober/dependencies and unlock Secret Service.
3. Extract into a stable writable folder, enter `rolauncher-v1.1.0`, and run `./RoLauncher`.
4. Sign in again; optionally run `python3 install-linux.py`. .NET is included.

Windows setup/portable packages retain WinUI. Source, AGENTS.md, notices, and checksums accompany packages; published 1.0.0 artifacts stay unchanged.
Report distro/desktop/session and failed action without cookies, tickets, launch URLs, or raw logs.

[Linux prerequisites](https://github.com/oangsa/RoLauncher/blob/beta-v1.1.0/docs/LINUX.md#runtime-prerequisites)
