# RoLauncher 0.1.4

Bug-fix release, 2026-10-05, Windows x64.

Recognizes the current Roblox [DFLog::NetworkClient] server-no-response failure associated with error 279. Previously the parser accepted only the older [FLog::Network] channel and missed this event. It now supports both channel formats, plus explicit GameJoinFailed errorCode: 279 in supported error/join channels.

For a verified managed instance with a uniquely owned log, the failure starts the existing 30-second reconnect grace period. Confirmed reconnection cancels recovery. Otherwise automatic recovery closes only the verified primary process and retries the configured destination under the existing backoff and five-failure limit. A timeout does not trigger public-server fallback. Stop and disabled automatic recovery prevent replacement.

HTTP, asset-download, chat/output and unrelated timeout messages do not trigger this recovery. Inaccessible or ambiguous log ownership remains Unknown. No memory inspection, dialog automation, injection or client patching is used.

Upgrade: Exit the old app, extract the entire ZIP and start the new executable with WebView2Loader.dll beside it. Existing account data is retained. Full multi-account/disconnect-recovery validation remains outstanding.
