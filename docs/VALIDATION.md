# Validation

Historical test results, not guarantees of live Roblox support.
Tests use separate data/simulated accounts unless stated otherwise.

## Upcoming 1.7.1 hotfix — 2026-10-17

- Passed: 87 Rust tests, authenticated Rust/WinUI connection test, 74 desktop checks, UI/rendering, formatting, Clippy, Python release/version checks.
- Installer/update: checksum rejection, cancel, exit waiting, install/restart, retained data passed.
- Old code failed error-273 backlog/page-six tests before fixes.
- Tested: 50-account recovery, 50 harmless process exits, slow discovery, log failure, read-only RAM, old/fake-channel messages, setting migration/saving, protected startup clients.
- Three optimized runs plus separate no-UI CPU/memory measurements: [Performance](PERFORMANCE.md#171-hotfix-measurements).
- UI: default On, saved Off, restart notice, existing account/preset/update flows passed.
- Additional audit replays passed: 264/280/600, spaced HTTP fields, oversized-line spoof, malformed/null/error JSON and ordered bounded imports.
- Existing 41 test files/blocks remained byte-identical to this audit's start; final Rust tests, formatting and Clippy rerun after parser refinement.
- Five parser/import comparisons and final idle CPU/RAM measurements: [results and limits](PERFORMANCE.md#additional-error-audit-comparisons).
- No live Roblox clients/logs, real launches, loaded low-end-PC tests, or native Linux tests.

## 1.7.0 HTTP join recovery — 2026-10-15

- Windows passed: 75 Rust tests, authenticated Rust/WinUI connection test, 74 desktop checks, UI, formatting, Clippy, release consistency.
- Installer/update: checksum rejection, cancel, exit waiting, install/restart, retained data passed.
- Test failed before fix; read-only log replay then found one HTTP join failure and zero connection messages.
- Tested: public/instance/private HTTP errors, code 529, Unknown-to-Reconnecting, 30-second wait, repeated failure/reconnection, Stop, rejoin Off, destinations, retry limits, verified closure.
- Ignore unrelated HTTP errors/fake script messages. Reports omit raw logs, credentials, authenticated URLs.
- No live failure/rejoin cycle; real clients/data unchanged. Native Linux untested.

## 1.7.0 game joining — 2026-10-15

- Windows passed: 72 Rust tests, separate connection test, 74 desktop checks, formatting, Clippy, WinUI, separate installer/update tests.
- Tested: ranking/team space, missing ping, invalid IDs, matching user/place status, hidden status via peer session, unreadable sessions, explicit targets, saved-game identity, fallback groups, limited wait, all-or-none saves, presets/backups.
- UI: saved preferences/assignments, dialogs/navigation, wrapping/alignment passed.
- Packages: runtime, WebView2 loader, source, instructions, licenses, SHA256 checksums.
- Live read-only checks confirmed server fields and redacted friend status. Separate encrypted-record copies found peer server without launches/real-data changes.
- Live multi-account joining, access denial, slot races, region/local latency, native Linux unverified. Smart cannot guarantee region/local latency.

## Upcoming 1.6.0 navigation, Windows materials and capture fix — 2026-10-10

- Passed: 67 Windows Rust tests, separate connection test, 71 desktop checks, formatting, Clippy, release consistency, UI, installer/update tests.
- Capture: 2880×1800/5120×1440 sizes, restored DPI context, wrong-owner rejection, FHD–8K allocation, invalid sizes, oversized buffers.
- Navigation: fade/slide, interrupted motion, one active page/tab, highlight position, reduced motion, retained current-page scroll.
- Existing account/preset/backup/Discord/update/keyboard/dialog/small/wide-window flows passed.
- Light/dark workspace, dialogs, changelog images checked: wrapping, spacing, actions, footer, scrollbar space.
- Fonts: native Windows sizes, TEST/16205713724 widths, Linux Inter regular/semibold sizes, manifests, licenses.
- Shared Uno views built. Arch WSL/X11 passed native input, scroll reset, Save/Cancel, bulk Cancel, rendering; XTest only in tests.
- Capture dev package passed runtime/source/notices/checksums/startup checks.

Still needs device tests:

- Real Mica wallpaper/inactive effects; test PNGs omit OS backdrop.
- System high-contrast/transparency changes: builds, device behavior untested.
- Roblox GPU capture at 100%, 150%, 175%, 200%; bottom-right corner, resizing, mixed-DPI monitors.
- Native GNOME/KDE and live Sober. Unsupported/minimized/protected capture unavailable; no desktop fallback.

## 1.5.0 — 2026-10-08

- Windows passed: 65 Rust tests, separate connection test, Clippy, UI, 71 desktop checks, installer/package, version/beta tests.
- Discord tests: page IDs, old buttons, error types. No live messages sent.
- Arch WSL passed: 64 Rust tests, Clippy, isolation adapter, Linux updater.
- Native KDE concurrent games, correct status/screenshots, live Discord delivery unverified.

## 1.4.0 — 2026-10-07

- Passed: Rust, Clippy, formatting, desktop checks, WinUI pages/controls, imports/preset dialogs, installer handoff.
- Linux built; native input/Save/Cancel passed under WSL. Native live Sober untested.
- Hosted Windows/Ubuntu and package checks gate publishing.

## 1.3.0 workspace, saved games and Linux interactions — 2026-10-06

- Passed: 50 Windows/52 Arch WSL Rust tests, Windows connection test, 60 desktop checks, formatting/Clippy on both, installer tests, disposable Flatpak test.
- WinUI: navigation, saved games/destinations, close preference, changelog, bulk/presets, backups/history, validation, small windows.
- Uno: Ctrl+A, native clicks, authenticated actions, scroll, valid/invalid Save, Cancel/bulk Cancel, changelog, feedback; images inspected.
- Flatpak: input access, separate temporary/runtime/IPC/PID namespaces, shared networking. Needs Flatpak 1.15.6+.
- Games: names/validation, saved across restart, backup/restore, retained destinations after deletion, API authentication/origin rejection.
- Live game details, Sober setup/launch, matching desktops, concurrent games unverified.

## 1.2.0 Linux isolation, update channel and dropdown fixes — 2026-10-06

- Passed: 49 Windows/50 Arch WSL Rust tests, Windows connection test, 60 desktop checks on both, Clippy, formatting, release consistency, native WinUI/Uno tests.
- Linux net8 desktop checks used .NET 10 with major roll-forward.
- UI/updates: dropdown height, saved beta choice/labels, stable selection, authenticated preferences, numeric versions, exact URLs, verified downloads.
- Flatpak 1.16.3 reproduced shared locks despite separate homes; built-in sandbox launches got independent locks/private file/IPC/PID areas. Networking shared.
- Static C test never launched Sober/Roblox. Proves OS separation, not concurrent games or frozen-warning cause.
- Native setup, input/audio, ownership, recovery, socket conflicts unverified. Hosted Windows/Ubuntu checks gate publishing.
- Published files unchanged; stable-only 1.0.0/1.1.0 updaters could find 1.2.0.

[Integrated isolation](LINUX.md#experimental-concurrent-launch-isolation)

## 1.1.0 Linux/Sober beta — local checks, 2026-10-06

| Environment | Passed |
| --- | --- |
| Windows | 46 Rust tests, separate connection test, 53 desktop checks, WinUI, installer/update tests |
| Arch WSL/X11 | 47 Rust tests, encryption/identity/shared-log checks, Clippy, 53 desktop checks, three archive/rollback tests |
| Uno WSL shell | Build/render, filters/selection, editor binding/validation/Save/Cancel, bulk Cancel, pages/dialogs/feedback |

- Windows kept WinUI; Linux shared layouts through Uno/Skia. Full matching behavior not proven.
- Secret Service, WebKit sign-in, Sober recovery, tray, portals, native desktops, fonts/DPI, accessibility unverified.
- Record predates hosted beta tests. Publishing also needs Windows/Ubuntu tests and normal packaged-shell startup.

## Historical 1.0.0 validation

Recorded 2026-10-06.

## Passed automated checks

| Area | Coverage |
| --- | --- |
| Rust | 46 tests: storage/encryption, recovery, profiles, backups, API, legacy data |
| Desktop | 52 checks: editors, filters, selection, update validation/download/cancel |
| Real window | Navigation, accounts, update buttons, rendering, connection test |
| Installer | Checksum rejection, exit waiting, install/relaunch, retained data |
| Packages | Source/version, runtime/docs/licenses, ZIPs, checksums, startup |
| Quality | Formatting, Clippy warnings denied; hosted Windows CI on feature/promotion PRs |

## Limited live evidence

Earlier checks covered ticket requests, one-client process matching, existing disconnect logs.
Multi-account/full recovery support not proven.

## Still needs real-client testing

- Two-account launch/process/status matching; disconnects, teleports, private targets, fallback.
- Session repair, rate limits, lost/restored network, real Discord delivery.
- Total app usage with 0/10/50 clients; updates between two real public versions.
- [Native Linux acceptance matrix](LINUX.md#required-acceptance-matrix).

Uncertain clients must stay Unknown, Needs attention, or Unsupported.
Read-only RAM inspection allowed; never modify Roblox memory/client files.
Installer unsigned.

[1.0.0 notes](RELEASE-1.0.0.md) · [Performance](PERFORMANCE.md)

## Historical pre-1.0.0 checks

October 5, 2026; Windows x64. Live coverage limits stay in each release note.

| Version | Recorded result |
| --- | --- |
| 0.1.3 | 22 regression tests passed |
| 0.1.4 | 24 regression tests passed |
| 0.2.0 | 26 tests passed |
| 0.3.0 | 27 tests passed |
| 0.4.0 | 33 tests passed with simulated delivery; real Discord unverified |
| 0.5.0 | 34 Rust/14 desktop checks plus real-window/connection tests passed; live Roblox/Discord not repeated |
| 0.6.0 | 34 Rust/20 desktop checks plus real-window/connection tests passed |
| 0.6.1 | Normal/small-window previews and existing tests passed |
| 0.7.0 | Existing tests and real-window notice checks passed |
