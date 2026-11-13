# Validation

Recorded checks below are historical results, not guarantees of live Roblox compatibility.
Fixtures use isolated data and simulated accounts unless stated otherwise.

## Upcoming 2.0.0 Phase 1 Resource Monitor — 2026-11-13

- Validated the read-only Resource Monitor subsystem (`src/resources.rs`, `src/platform_resources.rs`, `/v1/resources*` HTTP/SSE routes, Discord `/resources` and `/resources-register`, and temporary WinUI `Resources` page) on native Windows x64 (`2.0.0-dev.1c5f27a17cc4`).
- Passed on native Windows x64: `315` Rust library tests (`10` ignored), `49` `windows-device-identity` example tests, separate Rust/WinUI bootstrap pipe test, `109` desktop model/UI checks, WinUI interaction smoke (`Resources` tab rendering inspected), Python policy/versioning/packaging suites, `test-security.py` (`18` passed, `3` skipped), `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, isolated installer fixture tests, and packaged shell smoke (`verify-package.py --smoke-shell`).
- Explicit Windows 0/10/50 real-process benchmark (`native_benchmark_0_10_50_real_fixture_processes`, `--ignored`): `64.42 ms` average tick at 0 targets (`15.45 MiB` WS), `16.57 ms` at 10 targets (`16.50 MiB` WS), and `31.22 ms` at 50 targets (`16.51 MiB` WS).
- Best-effort PDH GPU Engine sampling (`\\GPU Engine(*)\\Utilization Percentage`) falls back explicitly to `Unavailable` when counters are absent or warming up; live multi-client Roblox GPU counters, Linux/Uno runtime execution, and interactive Discord gateway roundtrips remain `NOT VERIFIED` locally.

## Upcoming 1.9.1 multi-instance launch reliability — 2026-11-12

- Reproduced and fixed multi-instance launch and lifecycle defects:
  - Target-scoped destructive verification (`ProcessScan::verified_for_target` and `ProcessScan::verified_for_tracker`) so explicit Stop, Restart, and automatic recovery can safely close a verified managed instance alongside unrelated confirmed-trackerless Roblox processes (`scan.unowned > 0 && scan.uninspected == 0`) while keeping global `ProcessScan::verified()` strict and failing closed whenever any process is inaccessible, uninspected (`scan.uninspected > 0`), or has an unverified child handoff.
  - Pinned verified kernel process handle (`PINNED_CLOSES`) across graceful `WM_CLOSE` and 5-second force escalation so `TerminateProcess` operates on the original verified process object.
  - Evidence-based `LaunchCoordinator` readiness policy distinguishing `ProtocolAccepted`, `BootstrapperStarted`, `ProcessObserved`, `OwnershipVerified`, `Connected`, `StartupUnresolved`, and `StartupFailed`: active bootstrapper or uninspected client startup defers subsequent dispatches past the 8-second readiness window (up to the 35-second verification bound), silent readiness timeouts transition the unresolved launch to `NeedsAttention` while preserving its 90-second tracker reservation for late client recovery, and uninspected `RobloxPlayerBeta.exe` processes block new dispatches until WMI inspection completes.
  - Concurrent multi-account ticket preparation blocked when `active_multi_instance` is false (including pending-restart settings changes), and non-destructive launch attachment decoupled from unrelated unowned processes once `scan.uninspected == 0`.
- Added correlated launch lifecycle journal events (`launch_requested`, `launch_queued`, `launch_preparing`, `launch_ready_for_dispatch`, `launch_waiting_for_slot`, `launch_submitted`, `launch_result`, `launch_stage_transition`, `launch_process_observed`, `launch_ownership_verified`, `launch_connected`, `launch_readiness_deferred`, `launch_readiness_timeout`, `launch_blocked_ambiguous_handoff`, `launch_cancelled`, `launch_coordination_unavailable`) with secret-exclusion checks.
- Passed on native Windows x64: Rust library and example suites, 100 desktop checks, WinUI smoke and authenticated Rust/WinUI bootstrap, formatting, Clippy with `-D warnings`, release/versioning/packaging policy scripts, and local development build (`scripts/build.ps1 -UseCurrentVersion`).
- No live Roblox account or client was launched, stopped, or modified; live multi-account Roblox client acceptance remains `NOT VERIFIED` and requires an interactive desktop with real accounts.

## Upcoming 1.9.0 Free instance lifecycle — 2026-11-09

- Passed on native Windows x64: 197 Rust library tests, 49 identity example tests, 100 desktop checks, WinUI interactions/bootstrap, native process/log fixtures, formatting and Clippy.
- Local development packaging, SHA256/source/blob consistency, packaged shell smoke, secret scans, license/NuGet audits and isolated installer/update checks passed.
- Both fetched main and the candidate retain 13 RustSec findings; independent security/code review and real Roblox acceptance remain required.
- Hosted Linux and source-security checks passed on the implementation SHA; earlier scanner-install failures remain recorded, and final-head Windows checks are tracked in the PR.

[Findings, commands, experiment ledger and limitations](INSTANCE-LIFECYCLE-AUDIT.md) · [Exact-path synchronization manifest](INSTANCE-LIFECYCLE-SYNC.json)

## Upcoming 1.8.0 recovery and diagnostic logs — 2026-10-24

- Passed: 177 Windows Rust tests, 49 identity example tests, 98 desktop checks, formatting, and Clippy; six Rust tests remain explicitly ignored.
- WinUI interaction smoke, authenticated Rust/WinUI bootstrap, release metadata, policy checks, and isolated installer/update checks passed.
- Journal regressions cover correlation, command reuse, disk/memory/stale evidence, original error codes/timestamps, concurrent ordering, rotation, restart, partial writes, and write-failure recovery.
- Credential checks exclude aliases, session cookies, tickets, private destinations, raw errors, and the API token from diagnostic events and exports.
- Compared sources from v1.0.0 through v1.7.0 and v1.7.8; correlated sanitized local log/activity records with the reported incident times.
- Reproduced permanent discovery-query pauses, premature exit/tracker release, and duplicate online alerts with failing tests before fixing them.
- Counterexamples retain Stop/Restart, permission pauses, unique ownership, stale-command rejection, retry deadlines, and another account's state.
- A combined replay covers redirect, handoff, kick, failed/fresh verification, graceful/force closure, final exit, and eligible retry.
- No live Roblox client was launched or closed; native Linux and real Roblox handoff/rejoin behavior remain unverified locally.

[Historical comparison, incident evidence, and remaining limits](RECOVERY-AUDIT.md)

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
