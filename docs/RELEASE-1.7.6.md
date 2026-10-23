# RoLauncher 1.7.6

Upcoming bug-fix release based on published stable 1.7.5. Local packages carry a unique development suffix.

## Changes

- Restore the 30-second reconnect grace used in 1.7.0 and earlier, allowing redirects to finish before closing a client.
- Suppress Discord disconnect alerts for brief generic connection losses; known errors still notify promptly.

## Behavior and limits

Reconnection cancels pending recovery without closing the client or consuming a retry.
Confirmed disconnects persisting for 30 seconds still terminate the verified client, including its tray lifetime.
Replacement waits for confirmed process exit and retry backoff.

Stop, disabled rejoin, retry limits, access restrictions, and process ownership checks still apply.
Ambiguous log events and RAM-only hints never authorize automatic closure.

[Validation](VALIDATION.md)
