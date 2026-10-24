# Recovery comparison and evidence

Compared published Windows sources from v1.0.0, v1.2.0, v1.2.1, v1.3.0, v1.4.0, v1.5.0, v1.6.0, v1.7.0, and v1.7.8.
The 1.8.0 development changes retain current ownership and close safeguards while correcting three reproducible defects.
Historical sources and a credential-free local event summary were inspected without launching Roblox or changing account data.

## Complete flow comparison

| Stage | v1.7.0 and earlier sampled releases | v1.7.8 / previous development behavior | Current correction |
| --- | --- | --- | --- |
| Detect | Disk connection/disconnect signals; v1.7.0 also recognizes failed HTTP joins. | Separates confirmed terminal disk evidence from ambiguous disk/RAM hints. | Preserve that separation and source timestamps/codes. |
| Verify | PID, process start time, installed image and tracker; native checks before closure. | Adds fresh unique process discovery and durable intent for each close. | A failed discovery query defers the close; actual conflicting ownership still denies it. |
| Reconnect | Thirty seconds for a connection to recover. | Thirty seconds restored in 1.7.6; current successful reconnect cancels automatic closure. | Retain grace and cancellation. |
| Close | Graceful request, five-second force escalation; less explicit intent binding. | Current intent, operation, process identity and fresh verification govern each effect. | Retain binding; never apply the old client's automatic close permission to a successor. |
| Confirm exit | Exact process exit immediately clears launch tracking and permits retry. | Same immediate release remained after later safety improvements. | Keep tracking until a fresh scan identifies a unique successor or confirms absence after handoff settlement. |
| Rejoin | Backoff and five-failure limit; earlier releases had weaker unresolved-launch guards. | No replacement while a current/retired launch remains unresolved. | Preserve those guards across process handoff; retain the existing retry reservation. |
| Auto-pause | Authentication/access failures, ownership timeout, native errors and retry cap. | A discovery-query error also latched permanent attention. | Defer query failures; preserve actual ownership denials, explicit settings, access/update pauses and native-error handling. |
| Notify | Retained retry counts could repeat online alerts during brief redirects. | That condition remained despite redirect grace. | A brief unreported redirect no longer repeats the online alert; genuine recovery still does. |

An older version is not a safe blanket replacement: it lacks several current stale-command, ownership, late-launch and spoofed-log protections.

## Local incident timeline

Times below are **Asia/Bangkok, 24 October 2026**, correlated from saved activity and selected Roblox signal metadata.
Account identity, private destinations, authentication data and raw command lines are omitted.

| Time | Evidence | What it establishes |
| --- | --- | --- |
| 03:29–03:30 | One client log records connection, redirect and reconnection; earlier retry count remains one. | Repeated online notices can occur within one client and do not establish another OS launch. |
| 03:35:48–03:35:49 | A client log records connection while launcher activity changes from Unknown to Backoff. | The exit/retry decision needs to account for a surviving client; historical PID/tracker bindings were not retained. |
| 03:36:12 | Launcher activity enters Launching. | Another launch was scheduled locally. |
| 03:36:30–03:36:32 | A second client log records connection, then the earlier client's log reports 273. | Overlapping client sessions are present; the code alone cannot identify a physical device. |
| 05:25:10–05:25:15 | NeedsAttention with one failure, followed by Backoff. | The five-failure limit did not cause this pause, and the saved timeline shows it resumed five seconds later. |

These records support the investigation but do not prove every historical PID transition or the exact error behind that pause.
The new logs retain the missing process, intent, query and handoff decisions for future incidents.

## Reproductions and fixes

1. **Discovery failure:** an injected failed process query paused recovery before or after graceful submission. It now performs no close or launch, spends no retry, and re-verifies on the next successful scan.
2. **Exited process with surviving client:** the previous exit transition cleared the tracker despite a same-tracker successor. It now adopts only one uniquely claimed, verified successor and waits for its own connection evidence.
3. **Early exit without a visible successor:** tracking now survives a minimum five-second settlement and the remaining original 90-second ownership window. Failed or pre-exit scans cannot release it.
4. **Repeated online notices:** a two-second redirect with one retained retry generated another online alert. The alert now requires a reported disconnect or a genuine joining/recovery transition.

The first three regression assertions failed before their fixes; the redirect notification regression also failed before its one-condition correction.
Counterexamples cover conflicting/shared trackers, the old process still appearing, stale commands, explicit Stop/Restart, retained permission pauses, retry deadlines and another account.
One combined replay covers redirect, handoff, kick, unavailable verification, graceful/force close, final exit and eligible retry.

## Boundaries

- A successor must keep the same tracker and have unique current process ownership; another tracker is never guessed to be the account's client.
- Connection is confirmed from that successor's own verified disk log; an old process's signal or RAM hint grants no automatic close authority.
- Initial discovery remains asynchronous and shared, so a slow query does not block monitoring other accounts.
- Stop/Restart retain their current explicit intent; independent attention states remain paused.
- A normal long-running exit waits at least five seconds for a fresh absence check; early launch exits can wait until the original 90-second deadline.
- Real Roblox handoff/273 recovery and native Linux behavior still need live validation; isolated replay proves the corrected decisions.

[Diagnostic logs](DIAGNOSTICS.md) · [Validation](VALIDATION.md)
