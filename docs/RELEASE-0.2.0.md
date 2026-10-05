# RoLauncher 0.2.0

Recovery feature release, 2026-10-05, Windows x64.

Broadens recovery for players that remain open after a kick or lost session:

- Generic GameJoinFailed events in recognized join/error channels are recoverable, even when the error code is unknown or absent. Code 267 is no longer automatically treated as a permission failure.
- Detects observed Network connection-lost events and RbxTransportDummyClient connection-closed events, in addition to existing disconnect and no-response events. Both FLog and DFLog channel formats are supported.
- The first log channel is authoritative, so script/output messages containing copied network tags do not trigger recovery.
- Reads fresh log events before evaluating the reconnect-grace deadline, allowing a confirmed reconnect to cancel replacement. Explicit Stop/Restart and an already-started close still take precedence over log events.

For a desired-running account with automatic recovery enabled, verified PID and unambiguous log ownership, session loss allows 30 seconds to reconnect, then closes only the verified primary instance and retries the configured destination. Repeated events do not reset the grace timer or immediately count multiple failures. Existing exponential backoff, five-failure pause and generation checks remain in force.

Definitive permission errors (524, 533, 773) and expired authentication still require attention. Only recognized destination-unavailability events trigger public-server fallback. No access restriction is bypassed.

Coverage is limited to observable, recognized events in existing logs. Missing or ambiguous ownership remains Unknown; this release does not guarantee detection of every possible dialog or silent hang. No process-memory access, injection, client modification or executor is used.

Upgrade: Exit the old app, extract the complete ZIP and run RoLauncher.exe with its companion DLL. Existing saved account data is retained. Real multi-account and full recovery testing remain outstanding.
