# RoLauncher 1.2.0

RoLauncher 1.2.0 adds integrated Linux launch isolation, optional beta updates, and consistent account filter sizing. Windows remains a native WinUI application. The included Linux package is a preview.

## Changes

- Linux launches and automatic retries use private per-account Flatpak temporary/runtime directories and IPC namespaces automatically. Start accounts normally; no adapter installation or environment variable is needed. Sober and Roblox client files are not modified.
- Linux status discovery accepts a log inherited by helpers in the same verified sandbox and checks for logs immediately after attaching a client. Ambiguous ownership remains Unknown.
- The account group dropdown now matches the adjacent buttons' height on Windows and Linux.
- Settings adds **Include beta updates**, off by default. The saved preference applies to startup, six-hour and manual checks. Beta banners are labeled, and downloads retain official-publisher and SHA256 verification.

## Download and upgrade

For Windows, download `rolauncher-v1.2.0-setup-x64.exe`, or use **Check updates → Download → Relaunch** in 1.0.0/1.1.0. This is the new stable release, so existing stable-only updaters can discover it. Accounts/settings are retained and backed up during the update. The portable package is `rolauncher-v1.2.0-windows-x64.zip`.

For Linux, download `rolauncher-v1.2.0-linux-x64.tar.gz`, verify its entry in `rolauncher-v1.2.0-SHA256SUMS.txt`, extract it into a stable writable folder and run `./RoLauncher`. Existing 1.1.0 beta installations can use their stable update check to discover 1.2.0. Ubuntu 24.04+ or current Arch, x86-64, Sober and an unlocked Secret Service keyring are required. See [Linux prerequisites](https://github.com/oangsa/RoLauncher/blob/v1.2.0/docs/LINUX.md#runtime-prerequisites).

The checksum file covers the Windows installer, portable ZIP, Linux archive and corresponding source ZIP. Packages include AGENTS.md and license notices. Published 1.0.0 and 1.1.0 artifacts are preserved.

## Linux preview validation

Windows/Uno UI fixtures verified dropdown sizing and shared interactions. A disposable two-instance Flatpak fixture verified independent temporary/runtime filesystems and IPC/PID namespaces using the app's built-in launch code. Windows and Linux Rust, desktop and updater checks passed locally; hosted release checks gate packaging and publication.

This does not establish concurrent Roblox-game support. Native Arch/KDE and Ubuntu/GNOME/KDE testing of onboarding, rendering, audio, per-account connection status, Stop/Restart and recovery remains pending. Host networking is shared and socket conflicts remain possible. Linux UI parity is also incomplete. See [validation details](https://github.com/oangsa/RoLauncher/blob/v1.2.0/docs/VALIDATION.md).
