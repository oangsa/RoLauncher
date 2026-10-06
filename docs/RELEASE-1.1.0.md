# RoLauncher 1.1.0

## Linux/Sober beta

Add an Uno/Skia desktop project that links the Windows App.xaml, MainWindow.xaml, models and interaction handlers directly. Windows continues to use its existing native WinUI project. Linux targets Ubuntu and Arch, with GNOME and KDE, through X11 or XWayland.

Linux adapters use the installed Sober Flatpak protocol, isolated per-account homes, /proc process metadata, pidfds, existing Sober logs, Secret Service backed authenticated encryption, an ephemeral WebKitGTK sign-in browser, desktop portals and an AppIndicator tray. The Linux updater uses verified archives and the same Download → Relaunch flow, with backups before handoff. Roblox process memory is never read; executables and client files are never patched or modified.

The Linux port is preview: shared source and compilation do not establish 100% visual or interaction parity. Validate fonts, sizing, DPI, dialog behavior, accessibility, tray hosts and live authenticated Sober launches/recovery against the matrix in LINUX.md before declaring Linux supported. Sober's first-run setup and limitations remain its own; credentials and encrypted backups are OS/user specific, so sign in again on Linux.

Release artifacts must use the new 1.1.0 version, include corresponding source/AGENTS.md/license notices and SHA256 sums, and never replace published 1.0.0 artifacts. The `beta-v1.1.0` release is a prerelease and does not replace 1.0.0 as the latest stable Windows release.

## Install the Linux beta

Download `rolauncher-v1.1.0-linux-x64.tar.gz` and `rolauncher-v1.1.0-SHA256SUMS.txt` from this release. Linux x64 requires Ubuntu 24.04+ or current Arch, Sober, an unlocked Secret Service keyring, and the desktop/browser dependencies in [Linux setup](https://github.com/oangsa/RoLauncher/blob/beta-v1.1.0/docs/LINUX.md#runtime-prerequisites).

Check the archive against its SHA256 entry, extract it into a stable writable folder, open `rolauncher-v1.1.0`, and run `./RoLauncher`. Optionally run `python3 install-linux.py` to add it to the application menu. Sign in again on Linux; Windows-encrypted credentials cannot be imported. The package includes its .NET runtime.

The Windows setup and portable ZIP retain native WinUI. The source ZIP corresponds to this beta, and the checksum file covers all four packages.

Please report your distribution/version, GNOME or KDE version, X11 or Wayland session, and whether opening the app, sign-in, launching through Sober, and tray behavior work. Never include cookies, tickets, authenticated launch URLs or raw player logs.
