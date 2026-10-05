# RoLauncher 0.4.0

Discord notifications for account incidents, with native Settings controls and a readable Recent activity log.

- Save a channel webhook, enable alerts, send a test, and optionally include scheduled rejoin/back-online updates. Remove webhook disables notifications and erases the saved URL.
- Colored Discord messages identify the account, PlaceId, time and next action. Recognized kicks, disconnects, failed joins, unexpected client exits, paused accounts, monitoring interruptions, and connectivity changes are summarized without raw logs or errors.
- Repeated signals are deduplicated. A later explicit kick code can refine an earlier generic disconnect. Successful initial launches and intentional Stop/Restart do not send alerts.
- The webhook is masked and DPAPI encrypted; authenticated API settings never export it. Outbound requests accept only Discord webhook endpoints, reject redirects, disable mentions, and use an independent bounded worker with timeouts and rate-limit handling.
- Recent activity keeps 100 entries for the session, including with notifications disabled. Delivery is best effort; failed/full-queue alerts remain local. Queued/retrying alerts are cancelled on configuration changes or Exit, and are not replayed after restart.
- Adds authenticated Discord settings/test API routes. Existing databases load with notifications off and recovery updates selected by default.

Automated verification covers event classification, duplicate/stale signals, reconnects, manual actions, settings persistence/encryption/redaction, transaction rollback, bounded queues and cancellation, mock HTTP delivery/rate limits/errors/redirects, and native Settings controls. No live Discord webhook or real account kick was used during this implementation.

Upgrade: Exit the previous app, extract the entire Windows ZIP, and run RoLauncher.exe with WebView2Loader.dll beside it. Existing saved accounts retain their data folder.
