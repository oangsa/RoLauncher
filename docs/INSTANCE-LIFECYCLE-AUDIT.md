# Free instance lifecycle review candidate

Recorded 9 November 2026, Asia/Bangkok. This is implementation evidence, not independent approval or live Roblox certification.
Scope is RoLauncher Free only; no Premium repository was opened or changed.

## Candidate identity

- Repository: [oangsa/RoLauncher](https://github.com/oangsa/RoLauncher).
- Fetched main baseline: `12c64ed36f9c1265f1de9af24db379abd3a03039`.
- Development PR base: `bf24ff0ff59e8cc86801e430a7c0552d7a85a8b2` (`dev`).
- Immutable implementation: `18666bfbbe7e0e067fc712743f7e942479373379`.
- Branch: `codex/free-instance-lifecycle`; [PR #55](https://github.com/oangsa/RoLauncher/pull/55), targeting `dev`.
- Development build: `1.9.0-dev.376e7eb20a57`, based on verified published stable 1.8.0.
- [Synchronization manifest](INSTANCE-LIFECYCLE-SYNC.json): 21 changed implementation/test/UI paths with exact Git blob SHA-1 values.
- Later documentation commits retain these implementation blobs. No merge or stable publication is authorized by this record.

The worktree and index were clean before implementation; origin was fetched and main matched the reviewed baseline.
Local main/dev refs were brought to their corresponding origin refs before creating the dedicated branch.
The PR targets dev under the repository's admission policy; main contributes only its existing promotion merge above dev.

## Findings and corrections

Each row distinguishes a confirmed code-path defect from protective refusal or unverified client behavior.
Severity describes potential impact; fixture evidence does not establish prevalence in real clients.

| ID | Severity / status | Source | Evidence and root cause | Correction and regression | Residual risk |
| --- | --- | --- | --- | --- | --- |
| F01 | High; confirmed dependency problem | `src/platform_discovery.rs:39`, `src/engine/close_safety.rs:137` | Baseline global discovery required WMI; missing metadata could hide native observations. Initial attachment also lacked the shared ownership-conflict check. | Native same-session snapshots separate observation from attribution; handle-bound metadata and cross-account uniqueness gate attachment. Discovery/ownership fixtures cover missing metadata, recovery, changed creation times and competing claims. | New PID/window/name alone never grants ownership; ambiguous or inaccessible candidates still defer decisions. Real reused-client behavior remains unverified. |
| F02 | High; reproduced | `src/platform.rs:29`, `src/platform.rs:60` | Whitespace splitting extracted `-b` from inside a quoted payload. The new quoted-payload assertion failed against the original parser. | Preserve quote/backslash argument boundaries; accept only standalone `-b` or outer protocol fields. Exact numeric matching and duplicate rejection remain. | No undocumented argument format is accepted. A retained protocol URI is not proof of account identity without the existing launch tracker/operation checks. |
| F03 | High; confirmed permissive path check | `src/platform_image.rs:8`, `src/platform_image.rs:24` | The original substring check accepted copied fixture executables beneath arbitrary `Roblox/Versions` directories. Existing native fixtures demonstrated that authorization. | Require the canonical current-user installation root and current Windows session. Production rejects the disposable copied executable; explicit private fixture verifiers exercise native closure independently. | This is trusted-installation-path verification, not Authenticode publisher attestation. Hostile code running as the same Windows user is outside this trust boundary; alternate installations require separate evidence. |
| F04 | Medium; controlled reproduction | `src/platform_discovery.rs:104`, `src/platform_discovery.rs:139`, `src/platform.rs` (`ExitWatch`) | Requerying WMI at every close unnecessarily depended on metadata availability after attachment. The original exit watch also ignored its identity argument. | Retain live process handles plus PID/creation/tracker, refresh metadata through one worker, reject stale watch identities, and preserve Engine account/generation/intent checks. Outage/recovery and stale-handle fixtures pass. | Cache is runtime-only; a cold restart does not recreate destructive authority from a saved PID. Unknown new candidates block decisions; refresh conflicts revoke old attribution. |
| F05 | High; reproduced | `src/engine.rs:300`, `src/engine/close_safety.rs:648` | Stop set Stopped before exit; a retired discovery error was treated as ownership denial. Reopening could return an unrelated completed operation or obscure a denied Stop's attention state. | Keep Stopping until exit settles; bind restored operations; preserve denied attention across reopen; defer query outages without starting force timing. Native failures fail the operation while preserving the process. | Graceful acceptance still is not exit. Stop can remain unresolved during metadata outages; this protects against duplicate replacement. |
| F06 | Medium; controlled partial-discovery evidence | `src/engine/close_safety.rs:238` | A discovery failure aborted explicit Stop All before processing other verifiable candidates. | Cancel relaunch durably, inspect natively without WMI, process verified candidates, and return inspection/close failures with per-process journal outcomes. Partial-failure and failed-save fixtures pass. | Global scope remains installed Roblox players in the current session. Bootstrapper, crash-handler, other-session and unrelated executables are not added. |
| F07 | Medium; reproduced serial-wait behavior | `src/engine.rs:1121`, `src/engine.rs:1214`, `src/engine.rs:1467` | One awaited preparation blocked the launch loop; a held server search prevented bounded shutdown in the baseline replay. A failed discovery could also bypass ownership timeout indefinitely. | Allow four preparations, one per account, cancel superseded generations, and serialize five-second handoffs. Preserve same-preset ordering and single-instance gating; retain tracker reservations on timeout. | Requests already submitted to a remote service cannot be recalled. Final generation/gate/ownership checks prevent their stale results from dispatching. Live ticket/protocol acceptance is unverified. |
| F08 | No defect confirmed | `src/platform.rs` (`MultiInstanceGuard`) | Existing named-mutex tests exercise immediate acquisition, contention, abandonment, delayed readiness and shutdown while waiting. | Retain existing coordination and final dispatch gate checks; add no Event. Existing native coordination regressions pass. | Named Event behavior in MultiHub is comparative evidence only; it does not justify changing Roblox coordination semantics. |
| F09 | High; reproduced Windows log substitution | `src/logs.rs:248`, `src/logs.rs:316`, `src/platform.rs:705` | Windows reopened a log path without file identity checks. Disabling the new identity check reproduced acceptance of replacement content. | Bind discovery and tail attachment to volume/file identity; rediscover after replacement. Preserve unique Restart Manager PID/start ownership; incomplete resource scans fail closed and candidates are capped at 512. Native multiple-log/start-time fixtures pass. | A single Restart Manager call is synchronous and has no hard wall-clock cancellation here. One background discovery task prevents monitor/command deadlock or unbounded workers. Live 50-client log discovery remains unverified. |
| F10 | Protections retained; fixtures verified | `src/engine.rs`, `src/engine/close_safety.rs`, `src/logs.rs` | Existing policy regressions cover confirmed/unconfirmed evidence, reconnect cancellation, handoff, network suspension, backoff, retry limits and stale generations. | Preserve 30-second grace, five-failure cap, durable retry reservation, retired-launch fencing and unique successor adoption. Explicit 264/267/273/279 paths are covered. | No inference that 273 establishes another physical device. Real redirects, teleports, handoff and sudden client exits still require acceptance testing. |
| F11 | High; reproduced API/UI inconsistency | `src/model.rs`, `src/api/compat_tests.rs`, `desktop/Models.cs` | Stop's accepted operation could accompany Stopped while its process remained alive. | Add serialized `stopping`, WinUI labels/filters/dashboard handling and Discord status; keep HTTP 202 and existing operation shapes. HTTP and desktop compatibility assertions pass. | External consumers must handle the additive status value; existing clients that default unknown values can display Unknown until updated. |
| F12 | Medium; visibility gap addressed | `src/engine.rs` (`discover_players`, `expire_unowned_launches`), `src/engine/management.rs` | Prior diagnostics lacked native unowned/inaccessible counts and accurate Stop All/timeout progress. | Record bounded observation timings/counts/fingerprints, timeout, partial outcomes and operation/generation/desired state in Support Export. Credential-exclusion and secret scans pass. | `close_result.success` describes native request acceptance; only exit confirmation or successful explicit global exit waiting establishes exit. |

## Lifecycle and safety review

1. Commands persist desired state, generation, operation and explicit close intent before visibility or OS effects.
2. Preparation runs outside the command mutex, with four task slots and one serialized protocol slot; stale generations are aborted and checked again before handoff.
3. Native observations validate session, canonical image location and creation time; one metadata worker parses tracker fields while holding exact process handles.
4. Engine uniqueness checks bind a candidate to the current account and reject other current/retired claims; cached PID alone authorizes nothing.
5. Every close rechecks current intent and fresh native discovery under the command mutex; native handles pin identity through graceful or force submission.
6. Graceful requests retain the existing five-second escalation policy. Exact exit, successor reconciliation and retired-launch settlement precede replacement or final Stop completion.
7. Windows log ownership requires an exact Restart Manager PID/start pair and unique candidate; replacing the file revokes the tail's authority.

The simpler alternative was to retain WMI-only queries and add retry messages.
That leaves the reproduced global-stop dependency and serial-preparation stall, so the implementation adds only the native observation/metadata seam and bounded preparation.
No unsafe new-PID/window fallback, client writing, injection, executable patching, licensing or commercial gate is introduced.

This source/call-path review is the implementer's review, not independent approval.
Reviewers should inspect cache invalidation, refresh conflicts, native image/session checks, command linearization and log file identity before accepting synchronization.

## Comparative reference

Reviewed [MultiHub Launcher.cs](https://github.com/Roblox-MultiHub/Roblox-MultiHub/blob/6ff19e69716f3fa422e6d260fd3b82fe17010384/src/Launcher.cs),
[WindowLayout.cs](https://github.com/Roblox-MultiHub/Roblox-MultiHub/blob/6ff19e69716f3fa422e6d260fd3b82fe17010384/src/WindowLayout.cs),
[MainForm.Monitor.cs](https://github.com/Roblox-MultiHub/Roblox-MultiHub/blob/6ff19e69716f3fa422e6d260fd3b82fe17010384/src/MainForm.Monitor.cs), and
[Models.cs](https://github.com/Roblox-MultiHub/Roblox-MultiHub/blob/6ff19e69716f3fa422e6d260fd3b82fe17010384/src/Models.cs).

Its tracker substring lookup, new-PID/new-window fallbacks, PID-only monitoring/termination and broad global cleanup were not adopted.
Its Mutex/Event strategy supplied no reproducible reason to add an Event here.
No reference code was copied; all tests use local fixtures, not real accounts or Roblox client modifications.

Native API behavior was checked against Microsoft's [process access rights](https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights)
and [command-line argument rules](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-commandlinetoargvw).

## Verification

Platform: native Windows 10.0.26300 x64; Rust/Cargo 1.99.0; .NET SDK 8.0.425; Python 3.13.
Commands source `scripts/environment.ps1`; disposable storage uses `target/lifecycle-temp`.
Network-dependent tests use loopback fixtures with explicit sandbox access; real Roblox actions are disabled in launch fixtures.

| Check | Platform | Result | Evidence |
| --- | --- | --- | --- |
| Rust tests | Windows x64 | Passed: 197 library tests and 49 identity example tests | `cargo test --locked`, `RUST_TEST_THREADS=1`; `target/lifecycle-verified-rust.log` |
| Windows process tests | Windows x64 | Passed | Native hidden/tray closure, fifty exit watches, session/path/creation checks and mutex lifecycle in the Rust suite |
| WinUI model tests | Windows/.NET 8 | Passed: 100 checks | Locked restore and `dotnet run --project desktop.tests/RoLauncher.Desktop.Tests.csproj --no-restore` |
| WinUI interactions | Windows WinUI 3 | Passed | `scripts/build-desktop.ps1 -Smoke`; `python scripts/test-ui-smoke.py`; `target/lifecycle-winui-smoke.log` |
| Authenticated bootstrap | Windows Rust + WinUI | Passed | Explicit ignored `desktop::tests::winui_bootstrap_pipe_connects_to_real_rust_api_and_exits`; `target/lifecycle-bootstrap.log` |
| API compatibility | Windows/loopback | Passed | HTTP Stop retains PID, returns queued operation and serializes Stopping; existing authentication/origin and settings regressions pass |
| Recovery/log regressions | Windows fixtures | Passed | 264/267/273/279, grace, reconnect cancellation, retry reservation, handoff, missing logs, stale identity, rotation and native Restart Manager ownership |
| Formatting/check/Clippy | Windows GNU toolchain | Passed | `cargo fmt --all -- --check`; included-file `rustfmt --check`; `cargo check --locked`; `cargo clippy --locked --all-targets -- -D warnings` |
| Secret/security policy | Windows | Passed | 21 policy tests; pinned gitleaks tracked-content scan reports clean; `target/lifecycle-security-policy.log`, `target/lifecycle-secrets.log` |
| License/source and NuGet | Windows | Passed | Pinned cargo-deny reports no findings; desktop NuGet audit reports clean |
| RustSec | Windows; fresh advisory database | **BLOCKING findings** | 13 findings on both baseline and candidate; same dependency versions; database commit `7a40350e1b8ba5b656d909f88222a8059a83d88d` |
| Version/source/release policies | Windows/Python | Passed | Versioning/source assertions; beta 6, promotion 30, hotfix 25 and owner-evidence 32 policy tests; `python scripts/check-release.py` |
| Installer/update fixtures | Windows | Passed | `scripts/test-installer.ps1`: rejection, cancellation, exit handles, installation, restart and preserved fixture data |
| Review packaging and smoke | Windows | Passed for final development identity | `scripts/package.ps1 -UseCurrentVersion -OutputRootDirectory dist/lifecycle-review`; `python scripts/verify-package.py --smoke-shell --dist-directory dist/lifecycle-review` |
| Performance | Windows; optimized fixtures | Passed: five explicit fixtures | Release backlog, exit-watch, ranking, recovery-policy and native 1/10/50-process benchmarks; `target/lifecycle-performance.log` |
| Diff whitespace | Git | Passed | `git diff --check` |
| Hosted Windows/Linux CI | GitHub | Implementation Linux/security jobs passed; Windows job pending | [Implementation CI](https://github.com/oangsa/RoLauncher/actions/runs/42567461057) passed Linux tests/shell/package and branch flow. [Implementation security run](https://github.com/oangsa/RoLauncher/actions/runs/42567461013) completed source/history/dependency scans and Windows/Linux NuGet jobs. Final documentation-head checks may rerun. |
| Real Roblox acceptance | Windows Roblox | **Unverified** | No live account/client was launched, stopped or changed |
| Independent approval | External review | **Pending** | Implementer does not self-approve |

Seven ignored definitions comprise five explicit performance fixtures, the separate authenticated bootstrap and the disposable child-window entry point.
The bootstrap ran separately; native tests invoke the child entry point only against their own disposable processes.
Performance completion is recorded separately from the normal unit suite.

## Performance evidence

Twenty native snapshot samples per size used disposable Windows processes, canonical fixture paths and exact creation times.
These measurements exclude WMI cold attachment, Restart Manager latency, live Roblox, network traffic and total app CPU/memory usage.

| Disposable clients | Native discovery p50 | Native discovery p95 |
| --- | --- | --- |
| 1 | 9.61 ms | 12.67 ms |
| 10 | 12.68 ms | 16.34 ms |
| 50 | 20.09 ms | 24.05 ms |

- Fifty cached exit-handle polls averaged 13.71 microseconds per round; this excludes discovery, persistence and parsing.
- Fifty 3.7 MB log-backlog replays completed in 30.59 ms with the existing bounded reader, one poll per replay.
- Recovery replay p95 accepted-event-to-controlled-close was 63.89 ms without backlog and 35.36 ms with a 3.5 MB backlog.
- That fixture explicitly elapses the 30-second reconnect grace; these numbers do not promise real client shutdown in milliseconds.
- Existing server ranking averaged 22.23 microseconds for 500 entries and 423.45 microseconds for 10,000; it was slower than its legacy fixture baseline.
- Four-preparation limits and five-second handoff spacing are asserted separately through Engine fixtures for 1/10/50 queued accounts.

Measurements were collected on `fadd79ff73c935be3d3074877e444e4dca1386d8`; the subsequent restoration-only correction does not alter these measured paths.
Legacy `baseline`/`new` benchmark labels compare fixture algorithms, not end-to-end stable-release performance or improvements attributable to this PR.
Current total application usage and real 50-client performance remain unverified.

Repeat with `cargo test --release --locked --lib hotfix_benchmark -- --ignored --nocapture --test-threads=1`, then the same command with filters `recovery_policy_latency_fixture` and `lifecycle_native_discovery_latency`.

Local review archives reside under `dist/lifecycle-review`; no GitHub release or installer was published.
All 21 manifest implementation blobs match the corresponding files in the generated source archive.
Generated-archive/distribution secret scanning reports clean, and package verification checked WebView2Loader, notices, source, AGENTS.md and full development versions.

| Local artifact | SHA256 |
| --- | --- |
| `rolauncher-v1.9.0-dev.376e7eb20a57-windows-x64.zip` | `2d7c165aaadba8adf4ab46e818727394067941c4d5551dc8d15b6f16485963b6` |
| `rolauncher-v1.9.0-dev.376e7eb20a57-source.zip` | `39a268e14f4f1224afd45a0d6d54015b25ebce1122f7ac133df6ccc1d6b3fd3d` |
| `rolauncher-v1.9.0-dev.376e7eb20a57-setup-x64.exe` | `47120dfa3a8d0fd90bee1fb14dc7c71ae0276bbca1a03ea10359a5bd608ea1d5` |

The archived implementation matches the immutable candidate; later review documentation is supplemental and does not replace already-built artifacts.

## Experiment ledger

| Experiment | Observation | Conclusion |
| --- | --- | --- |
| Baseline quoted payload | New assertion returned a tracker from quoted arbitrary text | Confirmed parser authority leak; quote-aware parsing required |
| Baseline Stop | Status was `stopped` while the exact process remained attached | Confirmed status/operation inconsistency |
| Baseline retired outage | Injected query error latched denial | Temporary metadata failure was incorrectly treated as conflicting ownership |
| Restored serial loop | Held selection prevented the two-second shutdown bound | Bounded independently cancellable preparation required |
| Disabled Windows file check | Rotated pathname supplied new content without ownership rediscovery | Confirmed Windows log replacement gap |
| Sandbox fixture setup | Temporary file replacement and localhost fixtures failed before behavioral assertions | Reran with workspace temp storage and required access; did not weaken checks |
| Native exit race | Signaled fixture lost session/image queryability | Honor the exact signaled handle before requiring metadata from an exited process |
| Loaded parallel suite | Unchanged 29-second grace-boundary assertion once crossed its one-second wall-clock margin | Deterministic scheduling passes with all assertions retained; internal concurrency fixtures remain concurrent |
| HTTP fixture setup | Default database schema zero was rejected before the API call | Initialize the fixture with supported schema 2; behavioral assertions unchanged |
| Denied Stop reopen | New restoration assertion showed Stopping instead of saved NeedsAttention | Preserve the attention state and denial fence; start a fresh development package identity |
| Baseline/candidate RustSec comparison | Both report the same 13 findings | Existing dependency security debt remains a synchronization blocker |

Raw local evidence is under `target/lifecycle-*`; it is excluded from published source and contains only disposable fixture data or sanitized scanner output.
The checked-in tests and commands above make the behavioral evidence repeatable; no historical incident is reclassified as proven live compatibility.

## Security blockers and acceptance

RustSec identifies affected `rustls` 0.22.4/0.23.45, `rustls-webpki` 0.102.8/0.103.15 and `webpki-roots` 1.0.9.
Serenity 0.12.5 retains the older websocket/TLS stack; `cargo update -p serenity --dry-run` found no compatible newer release.
No dependency graph or scanner policy was changed to suppress findings.

The earlier hosted scanner installation failed twice; the same three pinned Linux archives checksum-verified locally without execution.
A subsequent run on the final implementation SHA installed all scanners and completed the source-security job successfully, clearing that installation gate.
The audit command reports findings without failing the workflow, so workflow success does not independently accept the RustSec findings.

The database also reports existing unmaintained Linux zbus-related packages and an unsound Tokio AsyncFd advisory.
Required independent security disposition must cover these warnings as well as the 13 findings.
The fresh scan result is not represented as a clean security gate.

1. Remediate or independently disposition the inherited dependency findings under repository policy.
2. Independently review the Free diff and verify every manifest blob against the implementation SHA.
3. Run authorized real-client acceptance for installed layouts, tracker extraction/reuse, handoff, 1/10/50 clients, log rotation and 264/267/273/279 recovery.
4. Review hosted CI and document any unavailable platform/live gate before considering a separate Premium synchronization task.

No Premium synchronization is authorized by this PR. The manifest marks changed source/test/UI files for reviewed delta porting and excludes Free version/changelog/release metadata.

**Premium handoff status: `BLOCKED`.**
