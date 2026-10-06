# Linux/Sober beta

**Acceptance requirement: match the Windows application UI and UX, while retaining native WinUI on Windows. This requirement has not yet been met or certified.** Linux is a beta for desktop and live Sober testing; successful compilation and fixture smoke tests do not establish complete support.

## UI architecture

`desktop/RoLauncher.Desktop.csproj` remains the native WinUI application. `desktop.linux/RoLauncher.Linux.csproj` uses Uno 6.7.30 and .NET 10, linking the exact same App.xaml, MainWindow.xaml, models, filtering, selection, editors, bulk edits, profiles, recovery, settings, feedback and update handlers. There is no second Linux copy of those layouts. Linux platform adapters handle window/tray integration, desktop portals, browser sign-in, credentials, player processes and update installation.

Uno renders application controls through Skia, so GNOME/KDE widget themes cannot restyle the workspace. The currently selected Uno host uses X11, including XWayland in Wayland sessions. Native Wayland is not claimed. Uno documents `UNO_DISPLAY_SCALE_OVERRIDE` for XWayland DPI overrides; testing must cover mixed DPI and fractional scaling. Window decorations are supplied by the compositor and are not identical to Windows.

Typography is a known parity gap. [Uno uses Open Sans by default outside Windows](https://platform.uno/docs/articles/features/custom-fonts.html), and the Linux preview screenshots differ from Windows' typography. [Microsoft's font FAQ](https://learn.microsoft.com/en-us/typography/fonts/font-faq) requires separate rights to redistribute Windows fonts; Segoe UI can be licensed separately, while Segoe UI Variable is unavailable for licensing/use on non-Windows platforms. Do not copy Windows system fonts into the package. A permitted font and verified matching metrics are still required; shared XAML alone does not satisfy the exact visual requirement.

## Runtime prerequisites

Flatpak 1.15.6 or newer is required for the input-device grant. Older distro packages must be updated before using the launcher. See the [Flatpak device reference](https://docs.flatpak.org/en/latest/flatpak-command-reference.html#flatpak-metadata).

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

## Install the Linux preview

Download `rolauncher-v1.2.0-linux-x64.tar.gz` and the checksum file from [1.2.0](https://github.com/oangsa/RoLauncher/releases/tag/v1.2.0). Linux remains a preview; Windows 1.2.0 is stable. The binary targets Ubuntu 24.04+ and current Arch, x86-64. Install the runtime dependencies above and unlock your keyring first. The self-contained desktop does not require a .NET installation.

In the folder containing the download, check the Linux archive's checksum entry, then extract and start it:

```sh
sha256sum --ignore-missing -c rolauncher-v1.2.0-SHA256SUMS.txt
tar -xzf rolauncher-v1.2.0-linux-x64.tar.gz
cd rolauncher-v1.2.0
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
python3 scripts/package-linux.py --directory target/linux/rolauncher-v1.2.0
python3 scripts/verify-linux-package.py --directory target/linux/rolauncher-v1.2.0 --smoke-shell
```

An existing build directory or release archive is never overwritten. The tar.gz includes the supervisor, complete desktop runtime, instructions and license notices. The Windows packaging script includes the Linux project in the corresponding source ZIP. The Linux checksum fragment is combined with the Windows/source checksum file for a combined release; all files must be present before publication.

Run an isolated UI fixture under an X11 display or Xvfb:

```sh
cd desktop.linux
dotnet restore --locked-mode -p:Configuration=Release --configfile NuGet.Config
dotnet publish --no-restore -c Release -p:Version=1.2.0 -p:UiSmoke=true -o ../target/linux-ui-smoke
cd ..
python3 scripts/ui-smoke-linux.py --shell target/linux-ui-smoke/RoLauncher.Desktop
```

Linux UI development tests also require XTest (`libxtst6` on Ubuntu, `libxtst` on Arch) for native pointer-event checks. This is a smoke-test dependency only.

The fixture captures Accounts, Settings, Dashboard, changelog modal, account and bulk dialogs, and feedback without using real accounts. It checks filtering, selection, account Save/Cancel, invalid edit rejection and bulk Cancel. Smoke output must not be shipped. The package verifier separately starts the normal Uno shell against a simulated authenticated API and checks supervisor-exit cleanup. These tests do not exercise Secret Service, WebKit sign-in or Sober. Screenshot success proves rendering and selected interactions only; it is not a visual parity assertion.

After extracting a normal package into a stable writable folder, run `python3 install-linux.py` there to register it in the application menu. Keep the folder in place. Updates swap that application folder after both processes exit. The data directory and update cache must be outside the application folder, and no root access is needed.

## Sober adapter and ownership

Launches use the authenticated Roblox request flow and Sober's `roblox-player` protocol. A stable per-account Flatpak HOME keeps saved state separate; the real XDG directories still locate user-installed Flatpak runtimes. Sober can show its own onboarding once in each isolated home. That extra onboarding is another platform difference to validate.

**Concurrent Sober instances have no upstream support.** [Sober's maintainer declined multi-session support](https://github.com/vinegarhq/sober/issues/390). Private homes do not isolate all runtime resources: [Flatpak shares temporary and runtime directories between instances of the same application](https://github.com/flatpak/flatpak/blob/main/common/flatpak-run.c). The 1.1.0 beta attempted concurrent launches without validating this behavior; an Arch/KDE report shows later clients displaying the frozen-instance warning. Version 1.2.0 integrates OS isolation into every launch and automatic retry. Users start accounts normally; there is no separate adapter to install or environment variable to set. Duplicate launches for an account are reserved during startup. These changes are included in 1.2.0 and are absent from the earlier 1.1.0 beta.

Version 1.3.0 also grants `--device=input` on every isolated launch and retry. Sandbox mode strips manifest/host device grants, so running a host `flatpak override` alone was insufficient. This uses the documented per-launch device grant; no system-wide override or Sober/client file modification is performed.

### Experimental concurrent launch isolation

In 1.2.0, start `./RoLauncher` and use Start as usual. Each launch automatically uses Flatpak's documented `--sandbox` mode, a private per-account filesystem grant and explicit XDG directories. The isolation options are compiled into the app. Temporary files, runtime directories and SysV IPC are private; GPU, Wayland/X11, audio and host networking remain available. Sober and Roblox executables and configuration are not edited. The regular Stop/Restart process ownership checks remain in use.

This is an experiment, not verified multi-client support. Host networking is shared, so fixed TCP ports or abstract Unix sockets may still conflict. Sandbox mode also omits the document portal, restricts application bus names, and private IPC can affect X11 shared-memory rendering. Test onboarding, display, audio, two distinct accounts staying in games, independent Stop/Restart and per-account connection status on your native desktop before accepting it. Do not enable automatic rejoin until those tests pass. Never assume that removing the warning proves isolation is complete.

The original `scripts/sober-isolation` adapter remains a developer experiment for the already-published 1.1.0 binary. New application builds invoke Flatpak directly with built-in isolation and include the log-ownership fix below. End users should install 1.2.0; they do not need that experiment script.

Developers can check OS isolation with `dbus-run-session -- python3 scripts/test-sober-isolation.py`. This builds a disposable local Flatpak app/runtime with a static C probe, reproduces a shared temporary lock in normal mode, and compares two experimental sandboxes. It never launches Sober or uses Roblox accounts. Passing proves filesystem/namespace behavior only; it does not prove concurrent games or native KDE rendering.

### Process and log ownership

The supervisor identifies only Sober executables inside the `org.vinegarhq.Sober` sandbox with an exact outer browser tracker in command-line metadata. It records boot ID and process start ticks to prevent PID reuse and cross-reboot mistakes. Stop/Restart target a pinned pidfd, never an application-wide `flatpak kill` or guessed PID. Log monitoring requires one unique existing open Sober log in the verified client's kernel PID namespace. Sober helpers can inherit that same descriptor without making ownership ambiguous; files shared across sandboxes, multiple distinct logs or conflicting trackers remain ambiguous. Discovery runs immediately after a PID is attached. If Sober discards the launch tracker, closes its log descriptor, changes formats, or exposes ambiguous ownership, monitoring stays Unknown instead of guessing. This needs testing against an installed current Sober before live recovery can be claimed.

To investigate Unknown without sharing secrets, run `./RoLauncher --diagnose` while the client is open. The saved `compatibility.json` in the RoLauncher data directory (normally `~/.local/share/RoLauncher`) reports whether process/log metadata is available and how many players/logs were verified. It contains no account identities, command lines, protocol URLs or raw logs. A verified PID alone is not a connected session: Running requires a recognized connection event in its verified log.

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
