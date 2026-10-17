# Linux/Sober beta

**Matching Windows controls and live Sober support still unverified.** Windows keeps native WinUI.
Builds/simulated tests do not prove native GNOME/KDE support.

## UI architecture

- Windows app: `desktop/RoLauncher.Desktop.csproj`.
- Linux: `desktop.linux/RoLauncher.Linux.csproj`, Uno 6.7.30/.NET 10; shares XAML, models, control handlers.
- Linux-specific code handles tray/windows, portals, sign-in, encryption, processes, updates.
- Skia uses X11/XWayland; native Wayland not claimed. Window borders differ.
- Test fractional/mixed DPI. Uno offers `UNO_DISPLAY_SCALE_OVERRIDE` for XWayland.
- Linux bundles Inter; Windows uses system fonts. Shared layouts do not guarantee matching text sizes.
- Never redistribute Windows fonts without rights. Segoe UI Variable not licensed for non-Windows use.

## Runtime prerequisites

- Ubuntu 24.04+ or current Arch, x86-64.
- Flatpak 1.15.6+ for input access; installed, unmodified Sober with first-run setup finished.
- Unlocked GNOME Keyring/KWallet with Secret Service, even without UI. No plaintext fallback.
- Matching `xdg-desktop-portal-gnome` or `xdg-desktop-portal-kde`.
- GNOME needs AppIndicator/StatusNotifier extension for tray; KDE normally provides one.

Ubuntu development packages:

```sh
sudo apt install build-essential pkg-config libdbus-1-dev python3 python3-gi gir1.2-gtk-3.0 gir1.2-webkit2-4.1 libayatana-appindicator3-1 libfontconfig1 libx11-6 libxrandr2 libxi6 libgl1 xwayland xdg-desktop-portal
```

Arch development packages:

```sh
sudo pacman -S --needed base-devel pkgconf dbus python python-gobject gtk3 webkit2gtk-4.1 libayatana-appindicator fontconfig libx11 libxrandr libxi mesa xorg-xwayland xdg-desktop-portal
```

Follow official Flatpak/Sober setup, then:

```sh
flatpak install flathub org.vinegarhq.Sober
```

Without tray support, close minimizes; lost tray support restores window.
Sign-in uses a temporary RoLauncher WebKit cookie store, never another browser profile.

## Install the Linux preview

Get archive/checksums from [1.7.0 release](https://github.com/oangsa/RoLauncher/releases/tag/v1.7.0).
Install dependencies and unlock keyring first. .NET included.

```sh
sha256sum --ignore-missing -c rolauncher-v1.7.0-SHA256SUMS.txt
tar -xzf rolauncher-v1.7.0-linux-x64.tar.gz
cd rolauncher-v1.7.0
./RoLauncher
```

- Keep extracted folder in a stable writable location. Optional menu entry: `python3 install-linux.py`.
- Sign in again; Windows/Linux credentials and backups work only for their OS/user.
- Updates wait for both processes, then replace app folder. Keep data/update cache elsewhere; no root needed.
- Reports: distro, desktop, session type, failed action. Never include credentials, authenticated URLs, raw command lines/player logs.

## Build, test and package

Build on Linux with Rust 1.99.0 and .NET 10 SDK.
Scripts create fresh dev versions; replace `VERSION` with full result.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
bash scripts/build-linux.sh
python3 scripts/package-linux.py --directory target/linux/rolauncher-vVERSION
python3 scripts/verify-linux-package.py --directory target/linux/rolauncher-vVERSION --smoke-shell
```

- Existing build folders/archives never overwritten.
- Packages include monitor, desktop runtime, instructions, licenses. Windows source ZIP includes Linux project.
- Before publishing, combine Linux and Windows/source checksum files; require every artifact.

UI tests need X11/Xvfb and same full `VERSION`:

```sh
cd desktop.linux
dotnet restore --locked-mode -p:Configuration=Release --configfile NuGet.Config
dotnet publish --no-restore -c Release -p:Version=VERSION -p:UiSmoke=true -o ../target/linux-ui-smoke
cd ..
python3 scripts/ui-smoke-linux.py --shell target/linux-ui-smoke/RoLauncher.Desktop
```

- Native pointer tests need XTest: Ubuntu `libxtst6`, Arch `libxtst`; normal app does not.
- Simulated-account tests cover pages/dialogs, filters, selection, Save/Cancel, invalid edits, bulk Cancel.
- Package tests start normal shell against simulated API; check cleanup when monitor exits.
- Never ship test output. Tests exclude Secret Service, WebKit login, Sober, full visual matching.

## Sober adapter and ownership

- Launch through authenticated Roblox requests and installed Sober `roblox-player` protocol.
- Per-account Flatpak HOME separates saved state; host XDG paths find user-installed runtimes.
- Each home may need Sober setup. Never change Sober/Roblox binaries or configuration.

### Experimental concurrent launch isolation

**Sober does not officially support concurrent instances; real-game stability unverified.**
[Support declined upstream](https://github.com/vinegarhq/sober/issues/390).

- Since 1.2.0, Start/retries use built-in Flatpak `--sandbox`, private account access, explicit XDG directories.
- Temporary/runtime files, SysV IPC, PID namespaces separate; GPU/display/audio/host networking shared.
- Since 1.3.0, every launch needs `--device=input`; host overrides alone insufficient in sandbox mode.
- Reserve account during startup to block duplicates. Normal Stop/Restart ownership checks remain.
- Shared networking may cause socket conflicts. Sandbox may affect portals, bus names, X11 shared-memory rendering.
- Before rejoin, test setup, display/audio, two accounts in games, independent Stop/Restart, status.
- Removing frozen-instance warning does not prove isolation works.

Old `scripts/sober-isolation` adapter targets published 1.1.0 beta; current builds need no adapter/environment flag.
`dbus-run-session -- python3 scripts/test-sober-isolation.py` runs a disposable Flatpak/C test.
It proves namespace separation, not Sober games or native desktop rendering.

### Process and log ownership

- Accept only Sober executables inside `org.vinegarhq.Sober` with exact launch tracker.
- Boot ID/start ticks detect reused PIDs. Stop/Restart use pinned pidfd; never app-wide `flatpak kill` or guessed PIDs.
- Require one uniquely owned, open Sober log in verified PID namespace.
- Helpers may share its descriptor. Shared/competing logs or conflicting trackers remain uncertain.
- Search on attachment; verified descriptors work across mount namespaces. Reused descriptor requires fresh search.
- Missing tracker/log or changed format leaves **Unknown**. **Running** needs recognized connection message.

With a client open, run `./RoLauncher --diagnose`.
`~/.local/share/RoLauncher/compatibility.json` reports available process details and verified player/log counts, never identities/raw logs.

## Required acceptance matrix

All pending. WSL does not replace native desktop tests.

| Distribution | Desktop | X11 | Wayland/XWayland | UI/UX parity | Live Sober |
| --- | --- | --- | --- | --- | --- |
| Ubuntu 24.04+ | GNOME | Pending | Pending | Pending | Pending |
| Ubuntu 24.04+ | KDE Plasma | Pending | Pending | Pending | Pending |
| Arch rolling | GNOME | Pending | Pending | Pending | Pending |
| Arch rolling | KDE Plasma | Pending | Pending | Pending | Pending |

- Compare same Windows/Linux data/window sizes at 100%, 125%, 150%, 175%, 200% scale.
- Test fonts, controls, keyboard/accessibility, selection, rejoin, filters, editors, validation, presets, histories, feedback.
- Test file portals, sign-in/repair, tray/Exit, monitor death, update progress/cancel/relaunch/rollback.
- Test two real accounts; public/private/Job ID targets; exact process/log matching; redirects/disconnects; grace/retry delays; late launches.
- Stop/Restart, access errors, exits must never affect unrelated clients.
- Failed/untested matching behavior blocks support claims.

## Upcoming 1.5.0: Arch/KDE follow-up

Historical follow-up: use build with explicit sandbox HOME, cross-namespace log descriptors, descriptor-reuse checks.
Native concurrent games still unverified.

- Discord screenshots need identifiable composited X11/XWayland window, XRes/XComposite (`libxres`, `libxcomposite` on Arch).
- No native Wayland capture. Missing capture gives text-only alerts; never whole-desktop fallback.
- On KDE, test separate account controls, redirects/disconnects, correct screenshot/account matching.

References: [Uno architecture](https://platform.uno/docs/articles/intro.html), [Linux host/DPI/portals](https://platform.uno/docs/articles/features/using-skia-desktop.html), [fonts](https://platform.uno/docs/articles/features/custom-fonts.html), [Microsoft font rights](https://learn.microsoft.com/en-us/typography/fonts/font-faq), [Flatpak device grants](https://docs.flatpak.org/en/latest/flatpak-command-reference.html#flatpak-metadata), [Flatpak runtime sharing](https://github.com/flatpak/flatpak/blob/main/common/flatpak-run.c), [Sober logs](https://vinegarhq.org/Sober/Troubleshooting.html), [Sober](https://sober.vinegarhq.org/), [Roblox OS support](https://en.help.roblox.com/hc/en-us/articles/203312800-Computer-Hardware-Operating-System-Requirements).
