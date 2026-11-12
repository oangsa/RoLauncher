# Historical Linux/Sober preview (discontinued and unsupported)

> **Status: Discontinued and unsupported.** RoLauncher is now Windows-only. The notes below are preserved strictly as a historical archive of the 1.7.0 experimental preview and must not be used as current installation or support instructions.

**Linux UI/UX parity and live Sober support remain unverified.** Windows retains native WinUI.
Builds and simulated smoke tests do not certify native GNOME/KDE support.

## UI architecture

- `desktop/RoLauncher.Desktop.csproj`: native Windows application.
- `desktop.linux/RoLauncher.Linux.csproj`: Uno 6.7.30/.NET 10; links the same XAML, models, and interaction handlers.
- Linux adapters handle tray/windows, portals, sign-in, credentials, processes, and updates.
- Skia controls use X11 or XWayland; native Wayland is not claimed. Compositor decorations differ.
- Test fractional/mixed DPI; Uno supports `UNO_DISPLAY_SCALE_OVERRIDE` for XWayland.
- Linux bundles Inter; Windows uses native typography. Shared layouts alone do not prove matching font metrics.
- Never redistribute Windows system fonts without rights; Segoe UI Variable is unavailable for non-Windows licensing/use.

## Runtime prerequisites

- Ubuntu 24.04+ or current Arch, x86-64.
- Flatpak 1.15.6+ for input-device grants; installed, unmodified Sober with first-run setup complete.
- Unlocked GNOME Keyring or KWallet with Secret Service, including headless mode. No plaintext vault fallback.
- Matching `xdg-desktop-portal-gnome` or `xdg-desktop-portal-kde` backend.
- GNOME needs an AppIndicator/StatusNotifier extension for tray parity; KDE normally supplies a host.

Ubuntu development dependencies:

```sh
sudo apt install build-essential pkg-config libdbus-1-dev python3 python3-gi gir1.2-gtk-3.0 gir1.2-webkit2-4.1 libayatana-appindicator3-1 libfontconfig1 libx11-6 libxrandr2 libxi6 libgl1 xwayland xdg-desktop-portal
```

Arch development dependencies:

```sh
sudo pacman -S --needed base-devel pkgconf dbus python python-gobject gtk3 webkit2gtk-4.1 libayatana-appindicator fontconfig libx11 libxrandr libxi mesa xorg-xwayland xdg-desktop-portal
```

Install Flatpak/Sober using their official instructions, then:

```sh
flatpak install flathub org.vinegarhq.Sober
```

Without a tray host, closing minimizes; loss of the host restores the window.
Browser sign-in uses RoLauncher's ephemeral WebKit cookie store, never another browser profile.

## Install the Linux preview

Download the Linux archive and checksum file from the [1.7.0 release](https://github.com/oangsa/RoLauncher/releases/tag/v1.7.0).
Install dependencies and unlock the keyring first; the package includes .NET.

```sh
sha256sum --ignore-missing -c rolauncher-v1.7.0-SHA256SUMS.txt
tar -xzf rolauncher-v1.7.0-linux-x64.tar.gz
cd rolauncher-v1.7.0
./RoLauncher
```

- Keep the extracted folder in a stable writable location; optionally run `python3 install-linux.py` for a menu entry.
- Sign in again: Windows/Linux credentials and backups are OS/user specific.
- Updates replace the app folder after both processes exit. Keep data/update cache outside it; no root is needed.
- Report distro, desktop, session type, and failed action. Omit credentials, authenticated URLs, raw command lines, and raw player logs.

## Build, test and package

Install Rust 1.99.0 and .NET 10 SDK; build on Linux.
Build scripts stamp a fresh dev identity; replace `VERSION` below with the resulting full version.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
bash scripts/build-linux.sh
python3 scripts/package-linux.py --directory target/linux/rolauncher-vVERSION
python3 scripts/verify-linux-package.py --directory target/linux/rolauncher-vVERSION --smoke-shell
```

- Existing build folders/archives are never overwritten.
- Archives include supervisor, desktop runtime, instructions, and licenses; Windows source ZIP includes the Linux project.
- Combine Linux and Windows/source checksum fragments before publication; all artifacts must exist.

Run UI fixtures under X11 or Xvfb, substituting the same full `VERSION`:

```sh
cd desktop.linux
dotnet restore --locked-mode -p:Configuration=Release --configfile NuGet.Config
dotnet publish --no-restore -c Release -p:Version=VERSION -p:UiSmoke=true -o ../target/linux-ui-smoke
cd ..
python3 scripts/ui-smoke-linux.py --shell target/linux-ui-smoke/RoLauncher.Desktop
```

- Native pointer checks need XTest: Ubuntu `libxtst6`, Arch `libxtst`; production does not.
- Fixtures cover pages/dialogs, filtering, selection, Save/Cancel, invalid edits, and bulk Cancel using simulated accounts.
- Package checks start the normal shell against a simulated API and verify supervisor-exit cleanup.
- Never ship smoke output. These checks do not exercise Secret Service, WebKit login, Sober, or complete visual parity.

## Sober adapter and ownership

- Launches use authenticated Roblox requests and Sober's installed `roblox-player` protocol.
- Stable per-account Flatpak HOME separates saved state; host XDG paths still locate user-installed runtimes.
- Each home may require Sober onboarding. RoLauncher does not modify Sober/Roblox binaries or configuration.

### Experimental concurrent launch isolation

**Concurrent Sober instances have no upstream support; real-game stability remains unverified.**
[Sober declined multi-session support](https://github.com/vinegarhq/sober/issues/390).

- Since 1.2.0, Start/retries use built-in Flatpak `--sandbox`, private account grants, and explicit XDG directories.
- Temporary/runtime filesystems, SysV IPC, and PID namespaces are private; GPU, display, audio, and host networking remain available.
- Since 1.3.0, each launch grants `--device=input`; host overrides alone are insufficient in sandbox mode.
- Duplicate account launches are reserved during startup; normal Stop/Restart ownership checks still apply.
- Shared host networking can cause socket conflicts. Sandbox restrictions can affect portals, bus names, and X11 shared-memory rendering.
- Validate onboarding, display/audio, two accounts in games, independent Stop/Restart, and status before enabling rejoin.
- Removing the frozen-instance warning alone does not prove isolation works.

The old `scripts/sober-isolation` adapter targets the published 1.1.0 beta; current builds need no adapter or environment flag.
Run `dbus-run-session -- python3 scripts/test-sober-isolation.py` for a disposable Flatpak/C probe.
It proves namespace behavior, not Sober games or native desktop rendering.

### Process and log ownership

- Only Sober executables in `org.vinegarhq.Sober` with an exact launch tracker are eligible.
- Boot ID/start ticks prevent PID reuse; Stop/Restart use a pinned pidfd, never app-wide `flatpak kill` or guessed PIDs.
- Monitoring needs one uniquely owned, open Sober log in the verified PID namespace.
- Helpers may inherit the same descriptor; shared logs, distinct competing logs, or conflicting trackers remain ambiguous.
- Discovery starts at attachment. Verified descriptors work across mount namespaces; descriptor reuse triggers rediscovery.
- Missing trackers/logs or changed formats leave status **Unknown**. **Running** requires a recognized connection event.

Run `./RoLauncher --diagnose` while a client is open.
`~/.local/share/RoLauncher/compatibility.json` reports metadata availability and verified player/log counts without identities or raw logs.

## Required acceptance matrix

All rows remain pending. WSL does not replace native desktop testing.

| Distribution | Desktop | X11 | Wayland/XWayland | UI/UX parity | Live Sober |
| --- | --- | --- | --- | --- | --- |
| Ubuntu 24.04+ | GNOME | Pending | Pending | Pending | Pending |
| Ubuntu 24.04+ | KDE Plasma | Pending | Pending | Pending | Pending |
| Arch rolling | GNOME | Pending | Pending | Pending | Pending |
| Arch rolling | KDE Plasma | Pending | Pending | Pending | Pending |

- Compare identical Windows/Linux data and dimensions at 100%, 125%, 150%, 175%, and 200% scaling.
- Check fonts, control states, keyboard/accessibility, selection, rejoin, filters, editors, validation, presets, histories, and feedback.
- Check file portals, sign-in/repair, tray/Exit, supervisor death, and update progress/cancel/relaunch/rollback.
- Test two real accounts, public/private/Job ID targets, exact process/log ownership, redirects/disconnects, grace/backoff, and late launches.
- Verify Stop/Restart, permission errors, and exits never affect unrelated clients.
- Failed or untested parity items prevent declaring support.

## Upcoming 1.5.0: Arch/KDE follow-up

Historical follow-up: explicit sandbox HOME, cross-namespace log descriptors, and descriptor-reuse checks require a build containing those changes.
Native concurrent games remain unverified.

- Discord screenshots require an identifiable composited X11/XWayland window plus XRes/XComposite (`libxres`, `libxcomposite` on Arch).
- Native Wayland capture is unavailable. Missing capture yields text-only alerts; there is no full-desktop fallback.
- On KDE, verify independent account controls, redirects/disconnects, and screenshot/account correspondence.

References: [Uno architecture](https://platform.uno/docs/articles/intro.html), [Linux host/DPI/portals](https://platform.uno/docs/articles/features/using-skia-desktop.html), [fonts](https://platform.uno/docs/articles/features/custom-fonts.html), [Microsoft font rights](https://learn.microsoft.com/en-us/typography/fonts/font-faq), [Flatpak device grants](https://docs.flatpak.org/en/latest/flatpak-command-reference.html#flatpak-metadata), [Flatpak runtime sharing](https://github.com/flatpak/flatpak/blob/main/common/flatpak-run.c), [Sober logs](https://vinegarhq.org/Sober/Troubleshooting.html), [Sober](https://sober.vinegarhq.org/), [Roblox OS support](https://en.help.roblox.com/hc/en-us/articles/203312800-Computer-Hardware-Operating-System-Requirements).
