# Linux/Sober beta

**Acceptance requirement: match the Windows application UI and UX, while retaining native WinUI on Windows. This requirement has not yet been met or certified.** Linux is a beta for desktop and live Sober testing; successful compilation and fixture smoke tests do not establish complete support.

## UI architecture

`desktop/RoLauncher.Desktop.csproj` remains the native WinUI application. `desktop.linux/RoLauncher.Linux.csproj` uses Uno 6.7.30 and .NET 10, linking the exact same App.xaml, MainWindow.xaml, models, filtering, selection, editors, bulk edits, profiles, recovery, settings, feedback and update handlers. There is no second Linux copy of those layouts. Linux platform adapters handle window/tray integration, desktop portals, browser sign-in, credentials, player processes and update installation.

Uno renders application controls through Skia, so GNOME/KDE widget themes cannot restyle the workspace. The currently selected Uno host uses X11, including XWayland in Wayland sessions. Native Wayland is not claimed. Uno documents `UNO_DISPLAY_SCALE_OVERRIDE` for XWayland DPI overrides; testing must cover mixed DPI and fractional scaling. Window decorations are supplied by the compositor and are not identical to Windows.

Typography is a known parity gap. [Uno uses Open Sans by default outside Windows](https://platform.uno/docs/articles/features/custom-fonts.html), and the Linux preview screenshots differ from Windows' typography. [Microsoft's font FAQ](https://learn.microsoft.com/en-us/typography/fonts/font-faq) requires separate rights to redistribute Windows fonts; Segoe UI can be licensed separately, while Segoe UI Variable is unavailable for licensing/use on non-Windows platforms. Do not copy Windows system fonts into the package. A permitted font and verified matching metrics are still required; shared XAML alone does not satisfy the exact visual requirement.

## Runtime prerequisites

Install Sober yourself from Flathub, complete its first-run setup, and use its unmodified runtime. RoLauncher does not install, patch or change Roblox or Sober binaries or client configuration.

Ubuntu 24.04+ development prerequisites:

```sh
sudo apt install build-essential pkg-config libdbus-1-dev python3 python3-gi gir1.2-gtk-3.0 gir1.2-webkit2-4.1 libayatana-appindicator3-1 libfontconfig1 libx11-6 libxrandr2 libxi6 libgl1 xwayland xdg-desktop-portal
```

Arch development prerequisites:

```sh
sudo pacman -S --needed base-devel pkgconf dbus python python-gobject gtk3 webkit2gtk-4.1 libayatana-appindicator fontconfig libx11 libxrandr libxi mesa xorg-xwayland xdg-desktop-portal
```

Use the matching portal backend (`xdg-desktop-portal-gnome` or `xdg-desktop-portal-kde`). Unlock GNOME Keyring or a KWallet providing the Secret Service API before opening RoLauncher. The vault key lives in that service; no plaintext fallback is used. Headless mode also needs an unlocked Secret Service session. Browser sign-in uses only RoLauncher's own ephemeral WebKit cookie store, never another browser profile.

GNOME requires an AppIndicator/StatusNotifier extension for tray behavior equivalent to Windows. KDE normally supplies a tray host. If no host acknowledges the tray, closing minimizes instead of hiding the only window. If the tray host disappears, the window is restored. This preserves access but is an explicit UX difference that needs resolution before exact parity can be accepted.

Install Flatpak/Sober following their official documentation, then:

```sh
flatpak install flathub org.vinegarhq.Sober
```

## Install the beta

Download `rolauncher-v1.1.0-linux-x64.tar.gz` and the checksum file from [1.1.0 Beta](https://github.com/oangsa/RoLauncher/releases/tag/beta-v1.1.0). The binary targets Ubuntu 24.04+ and current Arch, x86-64. Install the runtime dependencies above and unlock your keyring first. The self-contained desktop does not require a .NET installation.

In the folder containing the download, check the Linux archive's checksum entry, then extract and start it:

```sh
sha256sum --ignore-missing -c rolauncher-v1.1.0-SHA256SUMS.txt
tar -xzf rolauncher-v1.1.0-linux-x64.tar.gz
cd rolauncher-v1.1.0
./RoLauncher
```

Keep this folder in a stable writable location. Optionally run `python3 install-linux.py` there to add RoLauncher to the application menu. Sign in again on Linux; Windows credentials use a different encryption system. Report your distribution, desktop, session type and the action that failed, without including credentials or raw player logs.

## Build, test and package

Install Rust 1.99.0 and the .NET 10 SDK. Release publishes a self-contained Linux x64 desktop; end users do not need .NET installed. Build from Linux:

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
bash scripts/build-linux.sh
python3 scripts/package-linux.py --directory target/linux/rolauncher-v1.1.0
python3 scripts/verify-linux-package.py --directory target/linux/rolauncher-v1.1.0 --smoke-shell
```

An existing build directory or release archive is never overwritten. The tar.gz includes the supervisor, complete desktop runtime, instructions and license notices. The Windows packaging script includes the Linux project in the corresponding source ZIP. The Linux checksum fragment is combined with the Windows/source checksum file for a combined release; all files must be present before publication.

Run an isolated UI fixture under an X11 display or Xvfb:

```sh
cd desktop.linux
dotnet restore --locked-mode -p:Configuration=Release --configfile NuGet.Config
dotnet publish --no-restore -c Release -p:Version=1.1.0 -p:UiSmoke=true -o ../target/linux-ui-smoke
cd ..
python3 scripts/ui-smoke-linux.py --shell target/linux-ui-smoke/RoLauncher.Desktop
```

The fixture captures Accounts, Settings, Recovery, Changelog, account and bulk dialogs, and feedback without using real accounts. It checks filtering, selection, account Save/Cancel, invalid edit rejection and bulk Cancel. Smoke output must not be shipped. The package verifier separately starts the normal Uno shell against a simulated authenticated API and checks supervisor-exit cleanup. These tests do not exercise Secret Service, WebKit sign-in or Sober. Screenshot success proves rendering and selected interactions only; it is not a visual parity assertion.

After extracting a normal package into a stable writable folder, run `python3 install-linux.py` there to register it in the application menu. Keep the folder in place. Updates swap that application folder after both processes exit. The data directory and update cache must be outside the application folder, and no root access is needed.

## Sober adapter and ownership

Launches use the authenticated Roblox request flow and Sober's `roblox-player` protocol. A stable per-account Flatpak HOME keeps Sober's own state and lock separate; the real XDG directories still locate user-installed Flatpak runtimes. Sober can show its own onboarding once in each isolated home. That extra onboarding is another platform difference to validate.

The supervisor identifies only Sober executables inside the `org.vinegarhq.Sober` sandbox with an exact outer browser tracker in command-line metadata. It records boot ID and process start ticks to prevent PID reuse and cross-reboot mistakes. Stop/Restart target a pinned pidfd, never an application-wide `flatpak kill` or guessed PID. Log monitoring requires a unique existing open Sober log owned by the identified process. If Sober discards the launch tracker, closes its log descriptor, changes formats, or exposes ambiguous ownership, monitoring pauses conservatively instead of guessing. This needs testing against an installed current Sober before live recovery can be claimed.

Never put session cookies, tickets, authenticated protocol URLs, raw command lines or raw player logs in diagnostics. Windows DPAPI credentials and Linux Secret Service envelopes are not portable across operating systems/users; export profiles if needed, then sign in again.

## Required acceptance matrix

Every row remains pending until tested on that desktop; WSL is not a substitute for GNOME/KDE acceptance.

| Distribution | Desktop | X11 | Wayland/XWayland | Visual and UX parity | Live Sober |
| --- | --- | --- | --- | --- | --- |
| Ubuntu 24.04+ | GNOME | Pending | Pending | Pending | Pending |
| Ubuntu 24.04+ | KDE Plasma | Pending | Pending | Pending | Pending |
| Arch rolling | GNOME | Pending | Pending | Pending | Pending |
| Arch rolling | KDE Plasma | Pending | Pending | Pending | Pending |

Compare the same simulated data, font and content dimensions against Windows at 100%, 125%, 150%, 175% and 200% scaling. Check screenshots, hover/pressed/disabled/focus states, keyboard navigation, Ctrl/Shift selection, tri-state rejoin, search/filter retention, account and bulk editors, cancel/save validation, profile review/import/export, recovery histories, feedback timeout/placement, file portals, browser login and repair, tray close/restore/Exit, supervisor death, screen-reader labels and update progress/cancel/relaunch/rollback. Verify two distinct Sober accounts, private/job/public destinations, exact process/log ownership, disconnect grace, backoff, restart, late launches, permission failures and process exit without touching unrelated clients. Do not mark a platform supported with failed or untested parity items.

Sources: [Uno architecture](https://platform.uno/docs/articles/intro.html), [Uno Linux host/DPI/portals](https://platform.uno/docs/articles/features/using-skia-desktop.html), [Sober protocol and logs](https://vinegarhq.org/Sober/Troubleshooting.html), [Sober](https://sober.vinegarhq.org/), [Roblox OS support](https://en.help.roblox.com/hc/en-us/articles/203312800-Computer-Hardware-Operating-System-Requirements).
