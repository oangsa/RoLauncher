# RoLauncher 1.2.0

Native Windows release with a Linux preview.

- Linux Start/retries automatically isolate per-account Flatpak temporary/runtime filesystems and IPC; no adapter or environment flag required.
- Find logs immediately, including shared handles inside one verified sandbox. Uncertain matches stay Unknown.
- Align group dropdown height with buttons.
- Add **Include beta updates**, off by default, for startup/six-hour/manual checks; retain beta labels, publisher verification, and SHA256 checks.

## Download and upgrade

- Windows: `rolauncher-v1.2.0-setup-x64.exe` or `rolauncher-v1.2.0-windows-x64.zip`.
- From 1.0.0/1.1.0: **Check updates → Download → Relaunch**; data kept/backed up.
- Linux: verify `rolauncher-v1.2.0-linux-x64.tar.gz` against `rolauncher-v1.2.0-SHA256SUMS.txt`, extract into a stable writable folder, and run `./RoLauncher`.
- Linux needs Ubuntu 24.04+/current Arch x64, Sober/dependencies, unlocked Secret Service.
- Checksums cover Windows/Linux/source. Packages include AGENTS.md/notices; published files unchanged.

## Linux preview validation

- Local check results: [Validation](VALIDATION.md#120-linux-isolation-update-channel-and-dropdown-fixes--2026-10-06). Hosted checks gate publication.
- Disposable two-instance Flatpak test confirmed separate files/IPC/PID namespaces, not concurrent games.
- Live setup, display/audio, ownership, Stop/Restart/recovery, matching native desktops pending. Shared networking may conflict.

[Linux setup](https://github.com/oangsa/RoLauncher/blob/v1.2.0/docs/LINUX.md#runtime-prerequisites) · [Validation](https://github.com/oangsa/RoLauncher/blob/v1.2.0/docs/VALIDATION.md)
