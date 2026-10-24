# Launch and recovery logs

RoLauncher records launch and recovery decisions automatically from startup.
Each JSONL row has UTC time, elapsed milliseconds, a run ID, sequence number, launcher PID, and full build version.
Logging records behavior; it does not by itself fix duplicate sessions or recovery pauses.

## After an unexpected launch or pause

1. Open **Support → Export report** soon after the incident.
2. Include the approximate incident time and affected account ID with the exported JSON.
3. For a longer timeline, copy the `logs` folder shown under `recovery_log.directory` in that export.

Exports include up to 500 recent events within 512 KiB, plus recording status and write-failure counts.
Check `recording`, `write_failures`, and `last_io_error` if events are missing.
A disk failure does not stop launching or recovery; later events retry logging automatically.

## Files and retention

- Current file: `<data folder>/logs/recovery.jsonl`.
- Older files: `recovery.1.jsonl` through `recovery.4.jsonl`; `.1` is the most recent archive.
- Retention: five files of at most 2 MiB each; the oldest events are removed as files rotate.
- Windows normally uses `%LOCALAPPDATA%\RoLauncher`; existing installations may use `%LOCALAPPDATA%\RbxTools`.
- A custom `--data-dir` keeps logs beside that installation's account database; Linux uses its application data folder.
- Logs survive restarts; `run_id` and `sequence` distinguish launcher runs and preserve ordering when wall clocks change.

## Reading the timeline

- `command_result`: Start, Stop, Restart, or Retry acceptance, source, and operation ID; repeated operation IDs identify reused commands.
- `launch_preparing`, `launch_submitted`, `launch_result`: preparation and each OS handoff, linked by account, generation, operation, and tracker.
- `launch_result.accepted_by_os` confirms protocol handoff; a verified connection requires a later disk-log signal.
- `launch_discovery`: matching process candidates; PID and creation fingerprint distinguish reused PIDs and ambiguous ownership.
- `connection_signal`: parsed signal/code, source log timestamp, disk or unverified-memory source, terminal confirmation, ownership match, and prior status.
- `close_verification`, `close_denied`, `close_deferred`, `close_submitted`, `close_result`: ownership checks, rejected/deferred closes, graceful/force attempts, and OS results.
- `process_exit_confirmed`: verified exit; replacement can follow only when existing retry and ownership rules allow it.
- `account_state_changed`: before/after status, safe reason, retry count/deadline, recovery evidence, close intent, and recovery settings.
- Network suspension, launch coordination, retired launches, save failures, restoration, and shutdown have separate events.

For a pause, find `account_state_changed` with `after.status` equal to `needs_attention` and inspect its reason and preceding events.
For error 264 or 273, compare preceding `launch_submitted` trackers and process identities to see whether this launcher handed off another launch.
An error code alone cannot prove another physical device launched the account.

## Privacy and limits

Logs use selected fields and fixed reason codes, excluding cookies, tokens, authentication tickets, aliases, private links, command lines, and raw external errors.
Account IDs, place IDs, tracker numbers, process IDs, and timing remain useful for correlation; review them before sharing publicly.
Unrecognized error text becomes `details_omitted`; recognized numeric HTTP/Windows/Roblox codes remain available without the original text.

Completed rows are written immediately to the operating system; power loss can still lose recent writes.
A partial final row after a crash is separated on restart and skipped by exports.
