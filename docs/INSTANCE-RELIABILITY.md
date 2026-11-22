# Instance reliability and recovery

Baseline: `origin/dev` at `6e8c73e`, published stable `2.0.1`.
Development target: `2.0.2`; no stable release or automatic merge is requested.

## Root causes and evidence

| Cause confirmed by code | Deterministic evidence | Live evidence required |
| --- | --- | --- |
| Empty external cleanup always errored and disabled-cleanup warnings persisted after gate recovery. | Named Windows mutex (`empty_cleanup_resamples_conflict_instead_of_treating_zero_as_failure`) and `effective_compatibility_clears_all_resolved_startup_conflict_prefixes`. | Natural startup-conflict disappearance. |
| Connectivity probes required an empty preparation queue, could be cancelled by notifications, and polled too eagerly when healthy. | `connectivity_recovers_while_all_preparation_slots_are_pending` and `connectivity_probe_ignores_non_network_disconnects`. | Actual internet and Roblox service outages. |
| Temporary network/gate loss permanently denied otherwise eligible recovery. | `temporary_network_or_launch_gate_loss_defers_recovery_without_denial`. | Tray client restart during connectivity changes. |
| Process exits and disconnects during a shared outage spent account attempts. | `six_account_shared_outage_mixed_reconnect_and_exit_preserves_budget` and `confirmed_exit_during_shared_outage_does_not_exhaust_retry_budget`. | Multiple real clients exiting and reconnecting. |
| Completed absence inspection could not release expired current launches. | `absent_expired_launch_recovers_only_after_fresh_complete_inspection` and `legacy_tracker_gets_bounded_inspection_and_other_verified_accounts_do_not_strand_it`. | Slow native bootstrap and metadata recovery. |
| Restart reset rate-limit deadlines; duplicate-session (264/273) lacked cooldown; clean exit overwrote cooldown; idle 278 lacked classification. | `duplicate_session_restart_keeps_cooldown_after_verified_exit`, `concurrent_restart_and_clean_exit_preserve_single_operation_and_rate_limit_cooldown`, `afk_disconnect_278_recovers_after_grace_and_cancels_on_reconnect`. | Actual 264/273 and 278 recovery and latency. |
| Repeated force ticks during post-termination teardown could hit `ERROR_ACCESS_DENIED` and latch `NeedsAttention`. | `slow_post_force_teardown_defers_retermination_and_recovers_on_confirmed_exit`, `force_termination_timeout_transitions_to_needs_attention_and_recovers_if_late_exit_arrives`, `manual_restart_and_process_exit_during_pending_force_termination_do_not_reterminate_or_pause`. | Modified or slow-exiting client teardown. |
| `FailureKind::Network` always set global `network_suspended` without per-account `next_retry`, letting healthy reachability probes trigger rapid preparation loops during partial API outages. | `partial_api_outage_with_healthy_reachability_paces_retries_without_loop_or_global_suspension`, `one_account_succeeds_while_another_fails_and_rate_limit_remains_respected`, `six_accounts_simultaneous_api_failure_and_complete_outage_recovery`. | Actual partial Roblox service outages and host internet toggles. |

The reported screenshot symptom does not prove stale mutex state or absence of native processes.
No new arbitrary DLL compatibility claim follows from synthetic lifecycle tests.

## Decisions and invariants

1. Keep the existing generation, operation, tracker and exact-process controller; add no competing attempt state machine.
2. Keep parallel preparation and serialized dispatch, including five-second spacing; changing spacing needs live throughput evidence.
3. Probe Roblox reachability independently with a five-second request bound, 2/5/10/20/30-second failure delays plus 0–500 ms jitter, and a 15-second healthy interval; probe only for shared suspension or network-related disconnects.
4. Treat probes as Roblox service reachability, not proof of host internet loss or permission to terminate a healthy client.
5. Defer environmental failures only when every other close-policy check remains valid; allow a single accepted force termination up to 30 seconds for kernel teardown before reporting an actionable timeout (`NeedsAttention`).
6. Keep global `network_suspended` decoupled from single-endpoint `FailureKind::Network` errors by triggering `Roblox::reachable()` probes instead of immediately suspending all accounts; pace per-account transient API retries with bounded adaptive backoff + jitter without spending the 5-strike retry budget or letting healthy reachability probes clear unrelated backoff/rate-limit deadlines.
7. Release expired current trackers only after complete, bootstrapper-free inspection started after their deadline, with no matching or unidentified client.
8. Retain any live candidate, denied intent, authentication restriction, durable Stop or unresolved retired launch.
9. Keep reconnect grace; classify 278 idle kicks (`AfkKick`); add a 60-second duplicate-session cooldown (`SessionConflict`) preserved across clean exit and Restart; expose waiting reasons and coordination warnings in the desktop UI.

Unidentified clients still block absence proof; fully inspected clients with other trackers do not strand an expired reservation.
Legacy reservations lacking a deadline receive a fresh 90-second observation window on reopen.
Metadata becoming available can resume an unsent queued account without spending an attempt.
Generic unknown/285, missing logs and quiet games still cannot authorize destructive recovery.

## Validation

Validation date: 22 November 2026, Windows.
Synthetic fixtures never launch authenticated Roblox clients; native fixtures use disposable child processes.
Live tests are restricted to the user's Elicia accounts and configured targets.

- `python scripts/check-release.py`: passed (`Version 2.0.2-dev.c7c89d58da1b`).
- `cargo fmt --all -- --check` and `rustfmt --edition 2024 --check src/platform_startup_tests.rs`: passed.
- `cargo clippy --workspace --locked --all-targets -- -D warnings`: passed (0 warnings).
- `./scripts/test.ps1`: passed — Python policy suites (6 beta, 30 release promotion, 25 hotfix policy, 32 owner evidence, plus development-source and versioning checks), `rolauncher` library suite (339 passed, 10 explicit ignored benchmarks/fixtures), `windows-device-identity` (49 passed), `rolauncher-desktop` (17 passed), GPUI `ui-smoke` (`scripts/test-ui-smoke.py`), and `desktop_bootstrap_pipe_connects_to_real_rust_api_and_exits`.
- `./scripts/test-installer.ps1`: passed (checksum rejection, two-phase cancellation, exit handles, installation, restart, preserved data).
- Read-only `.\target\debug\RoLauncher.exe --diagnose --data-dir target\live-diagnose` (with live supervised `RoLauncher` and `RobloxPlayerBeta` running untouched): exited `0`, confirming isolated diagnostics write (`metadata_available: false` on single synchronous scan while background metadata thread was pending, `live_account_tests: "not performed"`).

Interactive live multi-account launches, simulated internet toggles, tray/Home restarts, 264/273/278 sessions, and live median/p95 latencies remain unverified because an active user session was running on the host.

## Compatibility and rollback

No schema, credential, authentication, licensing or Premium behavior changes.
Existing optional fields retain their meaning; `recovery_reason` gains `NetworkDisconnect`, `SessionConflict`, and `AfkKick` values.
The existing global suspension flag remains runtime state; durable running/Stop intent is preserved.

Before rollback, stop the supervisor and resolve outstanding client ownership using the current build.
Keep the account database backup; older builds retain the recovery defects described above.
Do not downgrade while an old client or late launch remains unresolved.
