# RoLauncher 1.2.0

Native Windows release with a Linux preview.

- Linux Start/retries automatically isolate per-account Flatpak temporary/runtime filesystems and IPC; no adapter or environment flag required.
- Recognize logs inherited within one verified sandbox; discover logs immediately. Ambiguity remains Unknown.
- Match account group dropdown height to adjacent buttons.
- Add **Include beta updates**, off by default, for startup/six-hour/manual checks; retain beta labels, publisher verification, and SHA256 checks.

## Download and upgrade

- Windows: `rolauncher-v1.2.0-setup-x64.exe` or `rolauncher-v1.2.0-windows-x64.zip`.
- Existing 1.0.0/1.1.0: **Check updates → Download → Relaunch**; data is retained and backed up.
- Linux: verify `rolauncher-v1.2.0-linux-x64.tar.gz` against `rolauncher-v1.2.0-SHA256SUMS.txt`, extract into a stable writable folder, and run `./RoLauncher`.
- Linux requires Ubuntu 24.04+/current Arch x64, Sober, dependencies, and unlocked Secret Service.
- Checksums cover Windows, Linux, and corresponding source; packages include AGENTS.md/notices. Published artifacts stay unchanged.

## Linux preview validation

- Windows/Linux Rust, desktop, updater, and UI fixtures passed locally; hosted checks gate publication.
- A disposable two-instance Flatpak probe verified private filesystems and IPC/PID namespaces, not concurrent games.
- Live onboarding, rendering/audio, ownership, Stop/Restart/recovery, and full native desktop parity remain pending. Shared networking can still conflict.

[Linux setup](https://github.com/oangsa/RoLauncher/blob/v1.2.0/docs/LINUX.md#runtime-prerequisites) · [Validation](https://github.com/oangsa/RoLauncher/blob/v1.2.0/docs/VALIDATION.md)
