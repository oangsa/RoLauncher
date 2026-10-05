# Validation record

Implementation date: 2026-10-05.

## Automated verification

The suite covers cookie validation, destination validation, DPAPI encryption/corruption, partial log lines/truncation, false join signals, backoff/pause behavior, fallback classification, 50 simulated accounts, duplicate Start/Stop, stale generations, Stop during a pending ownership window, reconnect grace cancellation, persistence rollback, API authentication, Origin rejection, and response redaction.

The build uses Rust's native Windows GNU target in this environment. MSVC build instructions are provided; an MSVC build has not been performed here.

Results: **16 automated tests passed**, including hidden native-window creation and exit. `cargo clippy --locked --all-targets -- -D warnings` and `cargo fmt --check` passed. The packaged executable returned HTTP 401 without authentication and HTTP 200 with its generated token.

Read-only Windows process-metadata diagnostics passed outside the restricted execution sandbox. The sandbox denied WMI access; the unrestricted diagnostic reported metadata available and zero identifiable Roblox players. No authentication tickets or client memory were inspected.

A 15.24-second idle measurement of the release executable in **headless mode with zero imported accounts** recorded a mean working set of 15,306,752 bytes (14.6 MiB), mean private memory of 2,195,456 bytes (2.1 MiB), and 0.0% machine CPU at the counter's observed resolution. This is not a 10/50-client benchmark or a C# comparison. Raw counters are in `idle-benchmark.json`.

The executable import table contains no `ReadProcessMemory`, `WriteProcessMemory`, `VirtualAllocEx`, `CreateRemoteThread`, or related remote injection entrypoints. Runtime integrations use metadata queries, file ownership, process exit handles, and verified primary-process closure.

## Live checks still required

No account cookies/passwords were provided or extracted from other applications. Two-real-account compatibility is **unverified**:

1. Launch account A and B into a public test experience; verify each username against its mapped PID and creation time.
2. Confirm Restart Manager associates one existing log with each primary process. Confirm a recognized connection event produces Running.
3. Trigger a disconnect/kick through a controlled test experience; leave the player window open. Verify the 30-second grace, recovery delay, old-primary closure, and new PID.
4. Verify teleport/reconnection within the grace period does not replace the client.
5. Test a public JobId and a full private-server link. Close the server; verify only definitive unavailability triggers public fallback.
6. Stop during launch and backoff, close a player window, restart the supervisor, and verify desired-state persistence.
7. Test expired sessions, denied access, rate limits, and loss/restoration of Internet connectivity.
8. Measure idle and 10/50-client resource usage with the benchmark script. Measure WebView2 separately during and after sign-in.

If a current client hides metadata, rejects the protocol, does not expose usable log ownership, or changes log/private-page formats, the expected result is an explicit Unknown/NeedsAttention/Unsupported outcome. Do not work around it using process memory or client modification.

## Authentication-ticket fix verified on 2026-10-05

A live trace using the first account saved by RoLauncher reproduced HTTP 403 with a CSRF token, followed by HTTP 415 without a ticket. Explicit empty body alone did not fix the failure. Adding application/json media type returned HTTP 200 and a ticket; both an empty body and {} succeeded. Production now sends {} as JSON and preserves it for the CSRF retry. Diagnostics include the request stage and HTTP status, without response bodies, cookies, tickets, or launch URLs. Two regression tests were added (18 tests total).

This verifies ticket acquisition only. Two-account launching, PID mapping, log ownership and recovery still require live validation.

## PID mapping fix verified on 2026-10-05

Windows metadata for a live player showed a roblox-player protocol URI argument with a browsertrackerid that exactly matched the saved launch tracker. The old parser recognized only legacy -b arguments. The updated parser supports both formats, rejects multiple tracker fields and malformed IDs, and continues to require verified executable identity and creation time. A late unique match clears only the ownership-timeout pause; authentication/permission pauses remain intact. No raw command lines or authenticated launch URLs were printed.

The suite now has 20 tests, including current protocol mapping, ambiguity rejection, and late ownership-timeout resolution. Real two-account and disconnect-recovery validation remains outstanding.

Production PID discovery was then executed against the live client: verified PID 13932, Windows creation time 134356556662006982, and exactly one saved tracker match. All 20 tests, clippy and formatting passed. This validates discovery for the observed client; it does not yet validate multi-account launch or disconnect recovery.

## Close/exit race fix (0.1.2 hotfix)

The saved state had no remaining managed PID/tracker but retained NeedsAttention and Unable to close managed instance (Windows error 5); Windows process inventory showed no live Roblox player. This is consistent with a race between client exit and TerminateProcess, which Microsoft documents can return ERROR_ACCESS_DENIED after termination. The precise original interleaving was not observed.

Close now checks the process wait handle before termination and after a failed termination. A process that already disappeared or whose exit handle is signaled is treated as closed. The engine also rechecks the exact identity before pausing on a close error, and resumes a pending recovery after confirmed exit, including the legacy persisted error. Retry count/timing, five-failure pause, Stop and unrelated authentication failures are preserved. Genuine access denial on a live process remains an error.

Regression coverage includes a real Windows child process that has exited: TerminateProcess reproduces error 5 and the new close helper returns success. An engine test covers confirmed-exit recovery and negative cases.

Close hotfix verification: 22 tests passed, including the actual Windows exited-process/error-5 reproduction. Clippy with warnings denied and formatting checks passed. Full Roblox disconnect/relaunch testing remains unverified.

## Error 279 / current NetworkClient channel fix (0.1.4)

A recent client log contained Error [DFLog::NetworkClient] Failed to connect to server at a network address, no response, followed later by Client:Disconnect. The old parser recognized only [FLog::Network], so it missed these current-channel messages. Unrelated HTTP/asset failures were also present and are deliberately ignored.

Production Tail/parse_line were run against the observed failure log using existing-file reads only. Result: connected=0, disconnected=2, connection_failed=1, unavailable=0, permission=0. Raw URLs, log contents and credentials were not emitted by that diagnostic.

ConnectionFailed starts the existing 30-second grace without immediately counting a failure or changing destinations. Repeated signals preserve the grace timestamp; reconnection cancels recovery; Stop and stale generations are covered. Replacement requires a verified managed PID and uniquely owned log. The existing backoff and five-failure pause remain in effect. Full live end-to-end replacement after error 279 remains unverified.

24 tests, clippy with warnings denied and formatting checks passed.

## Broad session-loss recovery (0.2.0)

Sanitized scans of recent logs observed Client:Disconnect under DFLog::NetworkClient, Sending disconnect with reason and Connection lost under FLog::Network, and Connection closed under DFLog::RbxTransportDummyClient. The parser now accepts these channels/events and generic GameJoinFailed session-loss events, including unknown/absent codes and code 267. Definitive permission codes remain distinct.

The monitor reads fresh log signals before the recovery deadline decision and refreshes the account generation/identity. Explicit Stop/Restart, destination replacement and an already-started close take precedence. Recovery requires desired running state, automatic recovery, a verified instance and Reconnecting state beyond the grace period. Tests cover grace boundaries, reconnection, disabled recovery, Stop, unknown codes, code 267, permission classification and copied network tags inside script/output messages.

26 tests, clippy with warnings denied and formatting passed. Event labels were observed in actual files; full end-to-end replacement across all kick scenarios remains unverified. Detection is limited to recognized, accessible, uniquely owned log signals and process exit; it is not a guarantee for silent hangs or every possible dialog.

## Desktop tabs, autosave and bulk actions (0.3.0)

27 tests passed, including the expanded hidden native Win32 UI test with three simulated accounts. It exercises Accounts/Settings/Changelog controls, absence of Save, valid alias/target persistence, incomplete-target retention through tab changes, Ctrl+A's shared select-all handler, disabled multi-account field editing, mixed rejoin values, bulk rejoin toggles, selection preservation, and bulk Start/Stop/Restart/Remove. The backend test checks cancelling and resuming automatic backoff without resuming an authentication pause. No live Roblox clients are opened by these tests.

Formatting and clippy with warnings denied passed. The API retains auto_recovery for compatibility while desktop labels use rejoin. Incomplete editor drafts remain in memory only; valid changes persist atomically through the shared engine. Full live Roblox rejoin validation remains outstanding.

## Discord notifications (0.4.0)

33 automated tests pass, including native Settings controls for masked webhook input, save/enable/recovery preferences, test queueing, removal and local activity. Backend tests cover DPAPI persistence, default-off upgrades, safe API responses, authenticated settings/test routes, duplicate/stale kick signals, reconnection notices, and no alerts or setting changes after persistence rollback. Saving unchanged settings resets the delivery pause without replaying history.

Local mock HTTP tests verify embed payloads, disabled mentions, wait=true confirmation, rate-limit cooldowns/retry classification, server errors, rejected webhooks, redirect rejection, and connection-error redaction. Queue tests cover disabled/recovery filtering, 128-message bounds, 100-entry session history, configuration cancellation and shutdown. Tests never send to a real Discord webhook or launch Roblox clients.

`cargo clippy --locked --all-targets -- -D warnings` and formatting checks pass. Windows/source release packaging includes WebView2Loader.dll, license notices, corresponding source and AGENTS.md, with SHA256 checksums. No live Discord delivery or real kick-to-alert flow was verified; use Send test and a controlled test experience to confirm them. Roblox detection retains the existing verified metadata/log constraints.

## WinUI 3 desktop (0.5.0)

34 Rust tests pass, with one additional desktop bridge test executed separately after building the smoke shell. The new API test confirms browser sign-in requires the bearer token, rejects browser Origin requests and does not return the token. Existing account, recovery, Discord and hidden Win32 regression checks remain passing.

14 desktop model/editor checks pass. They cover account/Discord JSON compatibility, incomplete drafts, public/job/private/share destinations, conflicting or external targets, and row updates that retain selection identity.

The actual compiled WinUI shell passes a loopback fixture smoke test with three simulated accounts: single and multiple selection, alias autosave, retention of an incomplete destination, drafts across page changes, mixed and bulk rejoin, bulk Stop, browser sign-in requests, page navigation, smaller-window rendering and empty state. The fixture never opens real sign-in, Roblox or Discord connections. Accounts, Settings, Changelog and resized/empty page PNGs were rendered from the app's own XAML content and visually inspected. Checks caught and corrected missing published XAML resources, initial DPI sizing, default blue accents and unwrapped changelog text.

A separate Rust test launches the smoke shell using the production anonymous-pipe bootstrap against a real authenticated Rust API and an isolated empty database. It verifies the Rust runtime version arrives correctly, the shell can read status and exit, and the Rust host returns cleanly. It does not contend for the application's instance mutex or touch the existing running app or saved accounts.

The shell builds with .NET SDK 8.0.425 and the pinned Windows App SDK 1.8.260921001 for Windows x64. The source/package scripts include both lock files, the self-contained desktop and its compiled PRI resources, dependency license notices, Rust source and AGENTS.md. The older headless memory baseline excludes the new desktop; total WinUI resource usage has not been benchmarked. Real-account launch/rejoin and real Discord delivery were not retested for this UI release.

## Account modals and table filters (0.6.0)

34 Rust tests pass, alongside the separate inherited-pipe WinUI/Rust bridge test, formatting and clippy with warnings denied. 20 desktop model/editor checks pass, including case-insensitive alias/username search, @ prefixes, combined status/rejoin filtering and Unknown status.

The compiled WinUI shell passes interaction checks against three simulated accounts: actual modal footer Save/Cancel buttons, invalid-target rejection without partial persistence, input retention during refresh and after API rejection, renamed rows leaving the active search, preservation of matching selections, and disabling stale modal saves after external deletion. Row deletion is confirmed/cancelled and targets a single account independently of the bulk selection. Search and combined filters deselect hidden rows, Ctrl+A selects visible rows, and bulk Stop affects only the visible selection. Mixed/bulk rejoin, navigation and isolated sign-in bridge checks also pass.

Rendered Accounts, details modal, delete confirmation, no-results, empty, Settings, Changelog and smaller-window previews were inspected. The fixture never uses saved accounts, opens Roblox or sends Discord webhooks. The editor now commits only through Save changes; Cancel discards edits. No live Roblox compatibility or Discord delivery checks were repeated for this UI feature release.

## Filter height alignment (0.6.1)

The status and automatic-rejoin filters stretch to the Accounts toolbar row height with centered content. Normal and smaller-window WinUI previews were inspected to confirm alignment with the search field and Clear button. The existing 34 Rust tests, 20 desktop checks, WinUI interaction smoke, separate Rust/desktop bridge, formatting and clippy checks pass.

## Top feedback toasts (0.7.0)

34 Rust tests, 20 desktop model/editor checks, the separate inherited-pipe Rust/WinUI bridge, formatting and clippy with warnings denied pass. The compiled WinUI interaction smoke verifies the real Send test handler shows a toast at the top center without resizing page content, a repeated user action restarts its five-second timeout, automatic dismissal stops the timer, the actual close button dismisses early, and repeated background failures do not extend the timeout. Existing modal, filter, bulk-action, navigation and resize checks also pass.

Normal and smaller-window toast previews, plus the same Settings page after automatic dismissal, were visually inspected. Tests use simulated accounts and a loopback API fixture; they do not launch Roblox or send real Discord messages. The existing live integration limits remain unchanged.

## Session management (1.0.0)

44 Rust tests pass, with the inherited-pipe Rust/WinUI bridge test passing separately. 29 desktop model/editor checks pass. Formatting and clippy with warnings denied pass. The release version is derived from Cargo for the Rust runtime and passed through the desktop bootstrap; build scripts set the matching desktop assembly version.

New engine/API regressions cover atomic selected-account updates, missing/duplicate account rejection, omitted-field preservation, per-account alias expansion, persistence rollback, individual profile snapshots, profile import membership/duplicate-name validation and rollback, profile persistence and launch results, and cleanup after account removal. Fallback tests exercise Allow public, Stay and Pause against definitive unavailability and confirm other failure types cannot activate fallback. Retry now retains failures, reuses pending operations and respects authentication pauses and rate-limit cooldowns.

Repair tests use simulated authenticated user records: wrong-account and removed-account repairs fail without inserting accounts, valid repairs retain account settings, and credentials remain DPAPI encrypted. History tests check persistence, repeated-signal deduplication, kick summaries, the 1,000-event bound and support-report redaction. Backup tests exercise whole-file DPAPI encryption, ten-file rotation, refusal while accounts are active, stopped restore without stale process/tracker identity, API-token retention, path rejection and offline recovery of corrupt JSON while preserving the old file. Update tests check numeric version comparison, stable tags, repository validation and matching package/checksum links; no live update feed is configured or contacted by tests.

The actual compiled WinUI shell passes the loopback interaction smoke: bulk Cancel, invalid destination rejection without partial saves, API rejection with draft retention, selected-account Save through the real footer button, alias patterns, group/fallback/rejoin settings, group filtering and deselection, select/edit all accounts, saving a profile and confirming its settings review/apply, creating a backup, recovery/history loading and account filtering, and history label refresh after alias changes. Existing modal, toast, navigation, deletion, bulk-action, resize and bridge checks pass. Rendered bulk-editor top/policy sections, profile review, profiles/backup settings and recovery pages were visually inspected. History renders 100 events per page and does not rebuild unchanged lists.

`scripts/verify-package.py --smoke-shell` verifies current-version SHA256 checksums, ZIP integrity, matching source/manifests/instructions, required runtime and license files, absence of build caches or smoke code, and published desktop startup against an isolated API fixture.

All automated checks use isolated databases, simulated accounts and local HTTP fixtures. Live two-account Roblox launching/PID/log ownership, controlled disconnect/rejoin/private-fallback flows, expired-session browser repair, real Discord delivery and total idle/10/50-client resource measurements remain unverified for 1.0.0. The live-check matrix above is still required; no completed live integration or performance claim is made by this release.

## RoLauncher rename and 1.0.0 reissue (2026-10-06)

The Cargo package and library are `rolauncher`, the Windows entry point is `RoLauncher.exe`, and both desktop projects and assemblies use `RoLauncher.Desktop`. UI, tray, login, Discord identity, user agents, export names, documentation and update asset matching use RoLauncher. Runtime versions still derive from Cargo and pass through the inherited-pipe bootstrap. The requested version stays 1.0.0; new `rolauncher-v1.0.0-*` packages are separate from preserved RbxTools artifacts.

46 Rust tests pass, including two new regressions for data-folder selection without credential copying and acceptance of new `.rolbackup` and existing `.rbxbackup` names. The existing encrypted-backup/restore suite passes with the new extension. New installs select the RoLauncher folder; existing legacy data is reused only when the new folder is absent. `--data-dir` remains explicit. Both application mutex names are held to exclude simultaneous supervision by the old and renamed builds.

29 desktop model/editor checks, formatting and clippy with warnings denied pass. The renamed compiled WinUI interaction smoke and the separate inherited-pipe Rust/desktop bridge pass. The first interaction attempt failed the toast/page-height assertion; an unchanged rerun passed. Accounts and smaller-window previews were inspected and show RoLauncher with v1.0.0. The release Rust executable builds successfully.

Release package verification checks SHA256, ZIP integrity, matching source/version/instructions, renamed project/runtime files, required WebView2/runtime/license files, absence of old desktop assemblies or smoke code, and published-shell startup against an isolated loopback fixture. These checks do not use saved accounts, launch Roblox or send Discord messages. Existing live integration and performance limitations remain unchanged. No public publishing repository is configured in this workspace.
