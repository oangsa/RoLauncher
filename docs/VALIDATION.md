# Validation

Recorded checks below are historical results, not guarantees of live Roblox compatibility.
Fixtures use isolated data and simulated accounts unless stated otherwise.

## Upcoming 1.8.0 diagnostic logs — 2026-10-24

- Passed: 169 Windows Rust tests, 49 identity example tests, 98 desktop checks, formatting, and Clippy; six Rust tests remain explicitly ignored.
- WinUI interaction smoke, authenticated Rust/WinUI bootstrap, release metadata, policy checks, and isolated installer/update checks passed.
- Journal regressions cover correlation, command reuse, disk/memory/stale evidence, original error codes/timestamps, concurrent ordering, rotation, restart, partial writes, and write-failure recovery.
- Credential checks exclude aliases, session cookies, tickets, private destinations, raw errors, and the API token from diagnostic events and exports.
- Screenshots establish the reported symptoms; no live duplicate-launch or unexpected-pause reproduction was available, so this change instruments future incidents.
- Native Linux logging and live Roblox launch/rejoin behavior remain unverified locally.

## Upcoming 1.7.6 redirect recovery — 2026-10-23

- Compared 1.6.0 and 1.7.0: both allowed 30 seconds for reconnection before closing a client.
- The new log replay failed before the fix: a disconnect poll terminated the client before the later reconnect poll.
- Existing policy checks ruled out broken reconnect cancellation, ambiguous-event handling, and ownership checks before restoring grace.
- The replay now passes for generic connection loss and codes 267/273; reconnection retains the process, destination, and retry budget.
- Boundary checks cover 29/30 seconds, duplicate events, persistent kicks, verified exit, retry backoff, Stop, disabled recovery, and other accounts.
- Passed: 156 Windows Rust tests, 49 identity example tests, 98 desktop checks, formatting, and Clippy; six Rust tests remain explicitly ignored.
- Versioning, source packaging, beta publication, release promotion, hotfix policy, and owner-evidence checks passed.
- Brief generic redirects emit no Discord incident; persistent losses deduplicate, and known errors still notify promptly.
- WinUI smoke and authenticated Rust/WinUI bootstrap passed with isolated fixtures; desktop checks passed again after restoring the reconnect labels.
- Initial sandbox checks hit temporary-file restrictions; a later rebuild exhausted disk space. Isolated reruns passed after clearing generated incremental cache.
- No live Roblox redirect/rejoin cycle or native Linux recovery was tested; replay uses simulated clients and isolated logs.

## Upcoming 1.7.5 process termination before rejoin — 2026-10-23

- After merging upstream through 1.7.4: 155 Windows Rust tests, 49 identity example tests, 98 desktop checks, formatting, and Clippy passed; six Rust tests remain explicitly ignored.
- Versioning, source packaging, beta publication, release promotion, hotfix policy, and owner-evidence checks passed.
- Regression failed with upstream window-close behavior, then passed with immediate termination for confirmed disconnects 267/273/279.
- A disposable Windows process acknowledged window closure and stayed alive; forced closure then confirmed process exit.
- Recovery checks cover verified exit, retry backoff, other accounts, Stop, disabled rejoin, reconnection, and ownership verification.
- Earlier native kick and global cleanup changes shipped in 1.7.2–1.7.4; they remain regression coverage.
- No live Roblox tray/rejoin cycle was tested; native closure uses a disposable process fixture.

## 1.7.0 HTTP join recovery — 2026-10-15

- Passed on Windows: 75 Rust tests, separate authenticated Rust/WinUI bootstrap, 74 desktop checks, WinUI fixture, formatting, Clippy, and release consistency.
- Installer/update checks passed checksum rejection, cancellation, exit waiting, installation, restart, and data preservation.
- Regression failed before the fix. Read-only log replay then detected one recoverable HTTP join failure and zero connection signals.
- Coverage: public/instance/private HTTP failures, code 529, Unknown-to-Reconnecting, 30-second grace, repeated failures, successful reconnection, Stop, disabled rejoin, destinations, retry limits, and verified closure.
- Unrelated HTTP traffic and spoofed script output are ignored; diagnostics omit raw logs, credentials, and authenticated URLs.
- No live failure/rejoin cycle was performed; real clients/data were unchanged. Native Linux remains untested for this change.

## 1.7.0 game joining — 2026-10-15

- Passed on Windows: 72 Rust tests, separate bootstrap, 74 desktop checks, formatting, Clippy, WinUI interactions/rendering, and isolated installer/update checks.
- Coverage: ranking, full/team capacity, missing ping, invalid IDs, matching user/place presence, hidden presence through peer sessions, unreadable sessions, explicit targets, preset identity, fallback grouping, bounded waiting, atomic validation, persistence, presets, and backups.
- UI checks covered saved preferences/assignments, existing dialogs/navigation, wrapping, and alignment.
- Packages include runtime, WebView2 loader, corresponding source, instructions, licenses, and SHA256 checksums.
- Live read-only checks confirmed public-server fields and redacted friend presence; isolated encrypted-record copies resolved the peer's instance without launching clients or changing real data.
- Live multi-account joining, access denials, capacity races, region/local latency, and native Linux remain unverified. Smart cannot guarantee region or local latency.

## Upcoming 1.6.0 navigation, Windows materials and capture fix — 2026-10-10

- Passed: 67 Windows Rust tests, separate bootstrap, 71 desktop checks, formatting, Clippy, release consistency, WinUI fixtures, and isolated installer/update checks.
- Capture fixtures checked 2880×1800/5120×1440 physical dimensions, DPI context restoration, owner rejection, FHD through 8K allocation, invalid dimensions, and oversized buffers.
- Navigation checks covered fade/slide, interrupted transitions, one active page/tab, highlight placement, reduced motion, and current-page scroll retention.
- Existing account, preset, backup, Discord, update, keyboard, dialog, and minimum/wide-window workflows passed.
- Light/dark workspace/dialog/changelog images were inspected for wrapping, spacing, actions, footer, and scrollbar clearance.
- Font checks covered native Windows metrics, TEST/16205713724 label widths, Linux Inter regular/semibold metrics, manifests, and package licenses.
- Shared Uno views compiled; Arch WSL/X11 passed native input, scroll reset, Save/Cancel, bulk Cancel, and rendering. XTest is fixture-only.
- Capture development packaging verified runtime/source/notices/checksums and isolated startup.

Device testing still required:

- Real Mica wallpaper/inactive effects; fixture PNGs omit the OS backdrop.
- System high-contrast/transparency toggles; resources compile but device behavior is unverified.
- Roblox GPU capture, including the bottom-right corner at 100%, 150%, 175%, and 200%, resizing, and mixed-DPI monitors.
- Native GNOME/KDE parity and live Sober. Unsupported/minimized/protected capture stays unavailable; no desktop fallback.

## 1.3.0 workspace, saved games and Linux interactions — 2026-10-06

- Passed: 50 Windows/52 Arch WSL Rust tests, separate Windows bootstrap, 60 desktop checks, formatting/Clippy on both platforms, installer fixtures, and disposable Flatpak probe.
- WinUI covered navigation, saved games/destinations, close preference, changelog, bulk/presets, backups/history, validation, and small windows.
- Uno covered Ctrl+A, native clicks, authenticated account actions, scrolling, valid/invalid Save, Cancel, bulk Cancel, changelog, and feedback. Screenshots were inspected.
- Flatpak probe confirmed input grants, separate temporary/runtime/IPC/PID namespaces, and shared networking. Flatpak 1.15.6+ is required.
- Saved-game checks covered names/validation, restart persistence, backup/restore, retained destinations after deletion, and API authentication/origin rejection.
- Live game metadata, Sober onboarding/launches, desktop parity, and concurrent games remained unverified.

## 1.2.0 Linux isolation, update channel and dropdown fixes — 2026-10-06

- Passed: 49 Windows/50 Arch WSL Rust tests, separate Windows bootstrap, 60 desktop checks on both platforms, Clippy, formatting, release consistency, and native WinUI/Uno fixtures.
- Linux net8 desktop checks ran on .NET 10 with major roll-forward.
- UI/update checks covered dropdown height, beta persistence/labels, stable selection, authenticated preferences, numeric ordering, exact URLs, and verified downloads.
- Flatpak 1.16.3 probe reproduced shared locks with distinct homes; built-in sandbox launches acquired locks independently with private filesystems/IPC/PID namespaces and shared networking.
- The static C probe never launched Sober/Roblox. It proves OS isolation, not concurrent games or the frozen-warning cause.
- Native desktop onboarding, input/audio, ownership, recovery, and socket conflicts remained unverified. Hosted Windows/Ubuntu checks gate publication.
- Published artifacts stayed unchanged; stable-only 1.0.0/1.1.0 updaters could discover 1.2.0.

[Integrated isolation](LINUX.md#experimental-concurrent-launch-isolation)

## 1.1.0 Linux/Sober beta — local checks, 2026-10-06

| Environment | Passed |
| --- | --- |
| Windows | 46 Rust tests, separate bootstrap, 53 desktop checks, WinUI fixture, installer/update fixtures |
| Arch WSL/X11 | 47 Rust tests, encryption/identity/shared-log checks, Clippy, 53 desktop checks, three archive/rollback tests |
| Uno WSL shell | Build/rendering, filtering/selection, editor binding/validation/Save/Cancel, bulk Cancel, pages/dialogs/feedback |

- Windows retained WinUI; Linux shared its layouts through Uno/Skia. Complete parity was not established.
- Secret Service, WebKit sign-in, Sober recovery, tray, portals, native desktops, fonts/DPI, and accessibility remained unverified.
- This local record predates hosted beta checks; publication also requires Windows/Ubuntu tests and normal packaged-shell startup.

## Historical 1.0.0 validation

Recorded 2026-10-06.

## Passed automated checks

| Area | Coverage |
| --- | --- |
| Rust | 46 tests: storage/encryption, recovery, profiles, backups, API, legacy data |
| Desktop | 52 checks: editors, filters, selection, update validation/download/cancel |
| Real window | Navigation, accounts, update buttons, rendering, bootstrap |
| Installer | Checksum rejection, exit waiting, install/relaunch, retained data |
| Packages | Source/version, runtime/docs/licenses, ZIPs, checksums, startup |
| Quality | Formatting, Clippy with warnings denied; hosted Windows CI on feature/promotion PRs |

## Limited live evidence

Earlier ticket acquisition, one-client process mapping, and existing disconnect logs were checked.
This does not establish multi-account or complete recovery compatibility.

## Still needs real-client testing

- Two-account launch/process/status attribution; disconnects, teleports, private targets, fallback.
- Session repair, rate limits, network loss/recovery, and real Discord delivery.
- Total app usage with 0/10/50 clients; updates between two real public versions.
- [Native Linux acceptance matrix](LINUX.md#required-acceptance-matrix).

Ambiguous clients must remain Unknown, Needs attention, or Unsupported.
Read-only memory inspection is permitted; modifying Roblox memory/client files is prohibited.
The installer is unsigned.

[1.0.0 notes](RELEASE-1.0.0.md) · [Performance](PERFORMANCE.md)
