# Session-close safety (PR07)

Baseline: `origin/dev` at `4e2b072f1900960fb7cf61906230f6796747f70c`.
This inventory was recorded before changing application code.

## Close-call inventory

| Baseline location | Entry and effect | Existing evidence / risk |
|---|---|---|
| `src/engine.rs:845` | Monitor discovers retired tracker; calls `platform::close`, escalating after five seconds. | Every matching process was closed; tombstones have no reason, operation binding, or ambiguity guard. |
| `src/engine.rs:1037` | Monitor closes current owned client for Stop, Queued, disconnect recovery, target fallback, or retained attempt. | Snapshot is refreshed after capture await, but command can race the effect; retained timer independently permits close and force. |
| `src/platform.rs:196–249` | Windows verified handle; `WM_CLOSE` through `EnumWindows`, or `TerminateProcess`. | Checks PID, creation FILETIME and installed image; engine owns intent. |
| `src/platform_linux.rs:177–209` | Linux pidfd, identity/tracker checks, SIGTERM / SIGKILL. | Kernel process handle pins termination target; engine owns intent. |
| `src/platform.rs:689` | Unsupported-platform close stub. | Returns unsupported; no process effect. |

All production Roblox close calls enter through the two monitor paths above.
`capture.rs` and `desktop.rs` kill their own helper children; `login_linux.rs` kills the login helper.
UI/login `WM_CLOSE` handlers close RoLauncher windows, not managed Roblox clients.

## Indirect command paths

- API and desktop Start/Stop/Restart enter `Engine::command` / `command_inner`.
- Discord Stop/Restart validates its existing connection/user rules, then enters `command_inner`.
- Preset launch applies settings then calls Start; Retry Now schedules existing Backoff work without closing directly.
- Saved-session resume normalizes account state in `Engine::open`; launch dispatch retains generation and mutex checks across asynchronous selection/ticket work.
- Disconnect signals enter Reconnecting; after 30 seconds the monitor schedules recovery and closes the owned client.
- TargetUnavailable follows the account fallback policy; only eligible automatic replacement may close.
- Stop of a pending tracker creates a durable retired-launch record; late discovery must distinguish explicit cancellation from obsolete work.

## Intent contract

| Reason | Source | Current close eligibility |
|---|---|---|
| `UserStop` | Explicit API/UI/Discord Stop, including cancellation of an issued launch. | Same account generation and operation; desired state is stopped; verified unique process. No launch coordination, target, network or commercial prerequisite. |
| `ManualRestart` | Explicit Restart of an existing client. | Same intent/generation/operation, desired running, Queued replacement, destination and OS coordination ready. |
| `ManualStartReplacement` | Explicit Start from an attention state that still owns a client. | Same checks as manual Restart. A fresh Start without a client grants no close intent to the new client. |
| `AutomaticRecovery` | Owned disk log confirms a terminal disconnect that persists for 30 seconds. | Recovery enabled, current destination, retry budget, connectivity and OS coordination; still Reconnecting or its scheduled Backoff. |
| `TargetReplacement` | TargetUnavailable schedules an eligible fallback/retry. | Current fallback/recovery settings, destination, retry budget, connectivity and OS coordination; scheduled replacement still valid. |
| `ObsoleteLaunch` | Start supersedes an unresolved tracker from an attention state. | Observation only. Obsolescence never authorizes closing a late client. |

`CloseIntent` binds a fresh intent UUID, reason, account generation and operation UUID, with a sticky denial flag.
An absent or incomplete intent never authorizes a close merely because status is Queued, desired state is false, or a timer exists.
PID, creation time, tracker and original launch generation remain in `ProcessIdentity`.

## Effect boundaries

1. The monitor refreshes account state after asynchronous discovery/capture work.
2. `close_owned` compares the expected account generation, operation, identity and intent with current state under the command mutex.
3. Fresh process discovery must have exactly one tracker match with the expected PID and creation time, with no conflicting current or retired owner.
4. The boundary persists intent before the synchronous native close and checks current eligibility again after discovery/disk work.
5. `close_retired` additionally requires the current explicit Stop, its account binding and a frozen, unique retired process identity.
6. Native Windows handle/creation-time/image checks and Linux pidfd/creation-time/tracker checks remain intact for every graceful or force call.

No authorization token or permit escapes these functions.
Commands linearize before or after each synchronous close effect under the existing mutex.
OS coordination remains a separate input; no license policy or licensing network request is introduced.

The shared monitor scan runs outside the command mutex and supplies passive retired observations.
Missing, ambiguous, obsolete or denied records add no discovery calls under the mutex.
Each actionable retired close requires one fresh scan under the mutex, including every force attempt; shared observations alone never authorize close.

## Graceful, force and denial behavior

- Automatic recovery allows 30 seconds for reconnection before terminating the verified disconnected process; reconnection cancels recovery without spending a retry.
- Persistent confirmed disconnects terminate the process, including its tray lifetime; replacement waits for verified exit and backoff.
- Other close reasons start with graceful submission and an in-memory timer bound to account, full intent and exact process identity.
- Discovery/disk latency before submission does not count toward the five-second grace interval.
- Every force attempt repeats current intent, generation, operation, unique ownership and native identity checks.
- A failed discovery query defers the effect without spending a retry or denying intent; a successful scan with conflicting ownership still latches denial.
- A new Stop or Restart starts a fresh graceful phase even when it targets the same process.
- Each recovery episode has a fresh intent UUID and must satisfy current eligibility; reconnection revokes further automatic termination.
- Changed recovery settings, recovered connection, lost coordination, ambiguous ownership or native errors stop escalation.
- Denial retains client/process evidence, reports an attention state and does not consume another recovery retry.
- Failed close attempts are latched in memory even if persisting their denial fails; no tick can silently retry with stronger force.
- Unchanged denied/attention state performs no storage call or snapshot broadcast; initial transitions emit through the commit boundary once.
- Failed denial saves remain retryable until durable, while the in-memory latch blocks close; a new explicit command starts fresh intent.
- An unsaved denial also blocks newly classified automatic intents for the same account/process; a new episode UUID cannot restore authority.
- A natural exit after a denied close does not silently resume replacement. An explicit valid command is required.
- Confirmed exit of a current client preserves explicit Stop authority for separate retired clients still awaiting cleanup.
- Eligible recovery persists its retry reservation before termination; replacement waits for confirmed process exit and the retry deadline.
- Process exit retains tracking until a fresh post-exit scan finds a unique same-tracker successor or confirms absence after handoff settlement.
- Settlement requires five seconds and the remaining original launch-discovery deadline; a successor gets fresh connection evidence and no inherited automatic close authority.
- Activity recording, status notifications, durable-before-visible state changes and snapshot delivery are preserved.

## Retired ownership and legacy data

New retired records retain the reason and authorizing operation/generation.
Only an explicit Stop can authorize a retired close; an obsolete replacement remains observation-only.
The first unique discovery freezes PID and creation time durably before any close.

Discovery after the original observation deadline does not grant tracker-only close authority.
The late process is observed and preserved; an explicit Stop can authorize the frozen identity after inspection.
A frozen record is never rebound to a reused PID, a changed creation time or a different tracker match.

Legacy accounts and tombstones deserialize missing intent as `None`; missing retired process binding also defaults to `None`.
An incomplete legacy intent without its episode UUID defaults to nil and grants no close authority; explicit Stop creates fresh evidence.
No destructive migration infers Stop from historical desired state or status strings.
Unique legacy matches enter an actionable attention state, retaining ownership evidence until explicit Stop or confirmed exit.

Ambiguous tracker matches authorize no close, including after another monitor tick.
Resolve ambiguity by inspecting clients and closing unwanted instances yourself; once discovery has a unique identity, use Stop again.
No automatic recovery dispatch occurs from the resulting attention state.

Launch selection, pre-credential launch entry and final serialized dispatch reject active retired observation windows or known retired clients.
Start/Restart, account removal and backup restore cannot discard known unresolved retired ownership.
Confirmed exit of the exact frozen identity permits its tombstone to be removed; metadata errors preserve it.

Database schema remains version 2 with additive, defaulted fields.
Backups retain evidence, while restoring a backup clears runtime identity, generation, operation and close intent as before.
No application release version or credential format changes.

## Deterministic validation

The tests exercise engine commands/signals, injected process discovery and close effects, persistence/reopen, and asynchronous/concurrent command ordering.
All managed-client effect tests use synthetic identities; no test launches or closes Roblox.
The native Windows negative test refuses graceful and force close against the test host with a mismatching creation identity.
Discovery-counter tests cover 256 passive records, mixed passive/actionable cleanup, concurrent commands and stale observations before graceful or force close.
Storage-counter and event tests cover repeated owned/retired denials, failed persistence, sticky recovery denial and fresh Stop/Restart intent.

Run on Windows or Linux:

```text
cargo test --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

The existing Windows pipeline also runs desktop/API integration, native process tests, installer handoff and packaging checks.
Existing Ubuntu and Arch workflows remain enabled and unchanged.
Hosted results must be checked at the exact Draft PR head before review or merge.

## Limits and rollback

- A submitted `WM_CLOSE`, termination request or Linux signal cannot be recalled by a later command; this change orders subsequent effects.
- Native discovery is synchronous under the command mutex only when a close needs verification. Slow WMI or storage can delay commands; no new timeout or unsafe unlocked fallback is added.
- Process discovery and an OS call cannot be made atomic against external processes. Existing native checks remain the final identity guard, including Windows window-lifetime races.
- If storage fails, no new destructive effect is submitted before the required pre-effect save. A denial that cannot be written survives only in the running supervisor; force timers never survive restart.
- Real Roblox launch, disconnect, late-start and window-close integration was not performed. Do not infer live Roblox or platform-policy clearance from synthetic tests.
- This is an intent/ownership safety foundation, not commercial authorization enforcement. Future licensing must revalidate at these same effect boundaries.

Emergency disable: stop RoLauncher to stop issuing further engine effects; existing Roblox clients remain alive.
Back up the data directory before any rollback, and inspect/close managed clients yourself before using an older supervisor.
Older builds ignore the new intent fields and may destructively interpret reasonless tombstones; do not downgrade with unresolved ownership records.

No licensing, credential, release, version or production settings changes are part of PR07.
