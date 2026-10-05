# RoLauncher 1.0.0

Roblox account launcher for Windows 10/11 x64, with a native WinUI 3 desktop and Rust supervisor. The minimal black-and-white workspace uses rounded cards, modern controls and readable status badges. Includes Accounts, Settings and Changelog pages, session-cookie import, isolated browser login, authenticated local API, persisted account/PID associations, and automatic rejoin.

Action feedback appears as a compact toast at the top center of the window. Toasts disappear after five seconds and can be closed early. Each new action restarts the timeout; recurring background warnings do not keep the toast open. Toasts overlay the page without shifting its content.

## Groups, bulk settings and launch profiles

Select a few accounts with Ctrl/Shift, use **Select visible** for the current filters, or use **Select all accounts** to clear filters and select every saved account. **Edit selected** opens the shared settings editor. **Edit all accounts** includes every saved account even when filters hide rows. The dialog freezes its account scope when opened.

Check **Apply** beside each setting you want to change: alias, destination, automatic rejoin, group and fallback policy. Unchecked settings retain each account's own value, including mixed values. Alias patterns support `{username}`, `{id}` and `{index}` (one-based dialog order). A destination replaces Place ID, Job ID and private link together; **Clear destination** explicitly removes it. Blank applied group removes membership. Invalid settings, a missing account, or a persistence failure reject the entire bulk save. Cancel discards all edits. Target changes take effect on the next launch; editing does not restart running clients.

Assign a group name in either editor, then filter the Accounts table by that group. To rename a group, select its visible accounts and apply the new group name. Groups organize accounts; they do not automatically change newly added members' settings.

In Settings, enter a profile name and choose **Save selected as profile**. Each profile captures its selected accounts' individual aliases, destinations, rejoin values, groups and fallback policies. **Review / launch** previews the account IDs and destination types before applying saved settings and queueing Start through the existing shared launch queue. Already running accounts stay open. **Apply settings** restores a profile without starting accounts. **Replace with selected** updates membership/settings and can rename the profile. Deleting an account removes it from profiles; empty profiles are removed.

Profiles can be exported/imported as JSON. Exports include account IDs, aliases and private-server links, but no session cookies, API token or Discord webhook. Keep private links private. Import requires the referenced accounts to be present, valid destinations, unique profile names and at most 50 profiles; it commits all profiles together or none.

## Recovery, history and backups

The **Recovery** page shows every saved account's status, detected reason, retry countdown and fallback policy. **Retry now** advances an eligible scheduled retry without resetting its failure count; rate-limit cooldowns and attention states require their normal resolution. **Sign in again** opens isolated browser sign-in for that account: the authenticated Roblox user ID must match, and a removed or different account cannot be inserted by repair. Alias, group, destination, profile membership and rejoin/fallback settings are retained. A repaired attention state remains stopped until Start.

The individual and bulk editors offer three fallback policies, used only after definitive destination-unavailable signals:

- **Allow public fallback**: retry in a public server of the same place. This preserves the previous default, including for private targets.
- **Stay on this destination**: retry the same destination with normal bounded backoff.
- **Pause and notify**: enter Needs attention. Local history records the pause; configured Discord alerts follow the existing notification settings.

Activity history retains the latest 1,000 state/recovery events across restarts and can filter by account. It records fixed summaries, timestamps, account IDs, Place IDs and failure counts. **Export support report** exports an allowlist of status/history fields; it excludes cookies, tokens, webhooks, aliases, private links, JobIds, trackers, authenticated launch URLs and raw errors/logs. Discord's separate Recent activity remains session-local.

Settings offers **Create backup** and **Restore backup**. Ten whole-file Windows-DPAPI-encrypted backups are kept in the data folder's `backups` subfolder. Changed writes preserve the previous valid state at most once per five minutes, and before schema upgrades. Manual backups are immediate. Restore validates the backup, requires stopped accounts and no pending owned launch, preserves the current API token, and creates a backup of current state first. Restored accounts have no saved process identity or desired-running state and stay stopped. Backups are bound to the same Windows user; they are not a portable credential export.

If database corruption prevents startup, close RoLauncher and Roblox clients, then run `RoLauncher.exe --restore-backup BACKUP_FILENAME` (plus `--data-dir` if applicable). Offline restore preserves the prior database as `accounts-before-restore-UUID.json`, leaves accounts stopped and rotates the API token on the next normal startup.

## Update checks

In Settings, configure the publisher's public GitHub repository as `owner/repository`, save it and choose **Check updates**. No repository is assumed, and no background checks occur. The checker uses GitHub's public latest-release endpoint without credentials and accepts stable MAJOR.MINOR.PATCH tags with matching versioned Windows ZIP/checksum assets from that repository. It compares numeric versions and offers download/checksum links. It does not install or execute downloaded files. Create a backup, verify SHA256, exit RoLauncher and extract the entire new ZIP into a new folder. The upgrade keeps the existing data directory and backs up schema migrations. See [GitHub's release API](https://docs.github.com/en/rest/releases/releases#get-the-latest-release).

## Discord notifications

In your Discord server, create a webhook for a regular text channel under the channel's **Integrations** settings and copy its URL. In RoLauncher **Settings**, paste the URL, click **Save webhook**, turn on **Enable Discord notifications**, then click **Send test**. No bot account is needed. Webhooks for forum/media channels and thread query parameters are not supported. See the [Discord webhook documentation](https://docs.discord.com/developers/resources/webhook).

Alerts use readable, colored messages with the account alias/username, PlaceId, timestamp, and next action. Recognized kicks (code 267), disconnects, failed connections (279), unexpected client exits, monitoring interruptions, connectivity suspension, and accounts needing attention are reported. **Also notify for rejoin and back online** includes scheduled retries, confirmed reconnections, and restored connectivity. Initial successful launches and intentional Stop/Restart do not produce alerts. Repeated log signals are deduplicated; a generic disconnect may be followed by a more specific kick alert if code 267 arrives later.

**Recent activity** keeps the latest 100 alerts for the current session, including when Discord is off. Notification delivery runs separately from account recovery. A bounded 128-message queue sends messages sequentially, respects Discord rate-limit delays, and retries rate limits/server errors up to three attempts. Connection failures are reported without retrying ambiguous sends. Delivery is best effort: failed/full-queue alerts remain in Recent activity; pending alerts are not replayed after Exit. Changing settings cancels queued/retrying messages; a request already issued may finish. A rejected webhook pauses automatic delivery until settings are saved again; Send test can check it directly.

The webhook URL is masked in the UI and encrypted with Windows DPAPI in the account database. Blank keeps the saved webhook; **Remove webhook** erases it and disables notifications. API responses never return the URL. Alerts use known summaries and disable mentions; they never forward raw errors/logs, cookies, tickets, trackers, JobIds, or private links. Notifications are disabled until you configure and enable them. Detection retains the existing verified-process/log constraints and cannot detect every silent hang or kick dialog.

**No Roblox process-memory reads/writes, injection, executable patches, client-file changes, executors, or external Account Manager dependency.** The only interaction with a managed player is launch, Windows metadata/exit queries, existing-log reading, and closing its verified primary process when requested or recovering it.

## Run

Extract the entire Windows release ZIP and run `RoLauncher.exe`. Keep `WebView2Loader.dll` and the complete `desktop` folder beside it. The desktop includes its .NET and Windows App SDK runtimes; no separate .NET install is required. Windows 10 version 1809 or later is required. Browser sign-in requires Microsoft's [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/); cookie import works without it.

1. Choose Browser sign-in, paste a session cookie, or import a UTF-8 file containing one cookie per line. Do not send cookies to anyone, including in bug reports.
2. Click a row's pencil icon to view account details and edit its alias, PlaceId, optional JobId or private link, and automatic rejoin. Click **Save changes** to persist the edits or **Cancel** to discard them. Invalid destinations keep the modal open and leave all saved settings unchanged. Private links support full game URLs containing `privateServerLinkCode` and `/share?code=...&type=Server` links through a conservative response adapter. Changed response formats report Unsupported rather than guessing a destination.
3. Select accounts in the table, then click Start. PID verified but unverified log/connection is shown as Unknown. Running requires a recognized connection signal in a process-owned log.
4. Use Settings to turn Automatic rejoin on/off for the selected accounts and view the local API token. A mixed checkbox means selected accounts have different values. Turning rejoin off cancels a pending automatic retry; it does not close a running client. Turning it on resumes only retries paused by that switch, not expired sessions or other attention states.
5. Search by alias or username and combine status and rejoin filters. **Clear** resets search and filters. Ctrl+A while the account table is focused selects all visible accounts; Ctrl-click and Shift-click select multiple rows. Start, Stop, Restart and Remove apply independently to selected visible accounts, with per-account errors reported. Hidden rows are deselected when filters or status updates exclude them. Each row's trash icon deletes that account after confirmation; bulk Remove also asks for confirmation. Removal requires stopped accounts. Matching selections are retained during status refreshes.
6. Stop cancels rejoin and closes the verified primary client. Window close minimizes to tray; Exit leaves clients open and suspends supervision. Previously started accounts resume on the next app start. The Changelog tab shows release history.

If Roblox already owns the singleton mutex, close clients yourself and restart the tool before multi-instance use. The application owns its own OS coordination mutex; it never modifies another process's handles. Acquiring it does **not** prove current Roblox versions support multiple instances.

Settings and encrypted credentials live in `%LOCALAPPDATA%\RoLauncher\accounts.json` for new installs. When that folder is absent and an existing `%LOCALAPPDATA%\RbxTools` folder is present, RoLauncher reuses the existing folder, including accounts, profiles, history and backups. An existing RoLauncher folder takes priority; `--data-dir` explicitly selects another folder. New encrypted backups use `.rolbackup`; existing `.rbxbackup` files remain supported. DPAPI binds credentials and the API token to your Windows user. Database corruption or unsupported schema stops startup rather than replacing the database. Do not use the same data directory from multiple Windows sessions.

Arguments: `--headless`, `--port 38471`, `--data-dir FOLDER`, `--diagnose`, `--restore-backup FILENAME`. Headless mode stays active until Ctrl+C. `--diagnose` writes a sanitized `compatibility.json` in the data folder without launching accounts or accessing credentials.

## Local API

Default address: `http://127.0.0.1:38471`. Click API token in the native UI to obtain your token. All routes require `Authorization: Bearer TOKEN`. Browser Origin and cross-site requests are rejected. No endpoint exports account credentials. Keep the API token private.

| Method | Route | Body / behavior |
| --- | --- | --- |
| GET | `/v1/status` | Accounts, connectivity suspension, compatibility status |
| POST | `/v1/login` | Open the isolated browser sign-in window; HTTP 202 means queued |
| GET | `/v1/accounts` | Safe account/status records |
| POST | `/v1/accounts` | `{"cookies":["SESSION_VALUE"]}`; per-index import errors |
| PATCH | `/v1/accounts/USER_ID` | Alias, target, `auto_recovery`, `group`, `fallback_policy`, or explicit `clear_target` |
| DELETE | `/v1/accounts/USER_ID` | Remove a stopped account with no pending owned instance |
| POST | `/v1/accounts/USER_ID/start` | Queue launch; return operation ID |
| POST | `/v1/accounts/USER_ID/stop` | Cancel pending recovery and close verified client |
| POST | `/v1/accounts/USER_ID/restart` | Replace the current verified client |
| GET | `/v1/operations/UUID` | Queued/completed/failed/cancelled operation |
| GET | `/v1/events` | SSE snapshots; reconnect receives current snapshot |
| PATCH | `/v1/accounts/bulk` | `account_ids` (1–50 distinct IDs), `patch`; atomic settings update, alias patterns |
| POST | `/v1/accounts/USER_ID/retry` | Advance an eligible retry; retain failure count and respect cooldowns |
| POST | `/v1/accounts/USER_ID/repair` | Queue isolated sign-in requiring the same account ID |
| GET / POST | `/v1/profiles` | List / save current settings for `name`, `account_ids`, optional existing `id` |
| POST | `/v1/profiles/import` | JSON array of credential-free profile definitions; atomic import |
| DELETE | `/v1/profiles/UUID` | Remove saved profile |
| POST | `/v1/profiles/UUID/apply` or `/start` | Apply profile settings / apply then Start; per-account launch results |
| GET | `/v1/activity?account_id=USER_ID` | Persistent newest-first history; account filter optional |
| GET | `/v1/diagnostics` | Sanitized status/history report |
| GET / POST | `/v1/backups` | List encrypted backup filenames / create a backup |
| POST | `/v1/backups/FILENAME/restore` | Restore validated backup while all accounts are stopped |
| PATCH | `/v1/updates` | `repository`: public GitHub owner/repository; blank disables checks |
| POST | `/v1/updates/check` | Manually check the configured publisher's stable release |
| GET | `/v1/settings/discord` | Enabled/configured flags, recovery preference, delivery status, recent activity; no webhook URL |
| PATCH | `/v1/settings/discord` | Optional `enabled`, `notify_recovery`, `webhook_url`; empty URL removes it (also set `enabled:false`) |
| POST | `/v1/settings/discord/test` | Queue a test to the saved webhook, even with notifications off; HTTP 202 means queued |

Account IDs and Windows creation-time ticks are strings; PIDs are integers. A process record includes PID, creation time, tracker, and generation. Operations are session-local, with completed entries bounded to 1,000; account desired state is persisted. Target changes take effect at the next launch. Start/Stop are idempotent; conflicting pending Restart is rejected.

```powershell
$headers = @{ Authorization = 'Bearer YOUR_TOKEN' }
Invoke-RestMethod 'http://127.0.0.1:38471/v1/accounts' -Headers $headers
$target = @{ target = @{ place_id = 1818 }; auto_recovery = $true } | ConvertTo-Json
Invoke-RestMethod 'http://127.0.0.1:38471/v1/accounts/USER_ID' -Method Patch -Headers $headers -ContentType 'application/json' -Body $target
Invoke-RestMethod 'http://127.0.0.1:38471/v1/accounts/USER_ID/start' -Method Post -Headers $headers
```

## Recovery behavior and compatibility limits

- One shared launch queue, with five seconds between launch requests. No process-order PID guessing. Unmatched/ambiguous ownership pauses recovery; use Stop before retrying.
- A cancelled issued launch retains a durable retired tracker for identifying and closing a late-arriving primary process. New Start waits for the 90-second discovery window to expire.
- Disconnects have a 30-second grace period for reconnection/teleports. Process exits also recover when enabled. Five consecutive failed attempts pause at NeedsAttention. Retry delays increase from 10 seconds with jitter; HTTP rate limiting waits at least 60 seconds. A confirmed connection lasting two minutes resets failures.
- Network failures suspend launches without counting as failed attempts. Expired sessions/permission failures pause for intervention.
- Only recognized explicit target-unavailable events permit fallback to a public server in the same place when **Allow public fallback** is selected, **including private targets**. Authentication errors, unknown page formats, rate limits, and network failures do not permit fallback.
- Roblox launch arguments, tracker visibility, connection/error log signatures, and Restart Manager log ownership are external compatibility assumptions. If unavailable, status remains Unknown or NeedsAttention rather than reading memory or inventing ownership. Error/log signatures are conservative and need live-client verification.
- Full private URLs use the reference application's page access-code format; share links resolve through Roblox's sharelinks endpoint. Changed page/response formats are reported as Unsupported. Sessions are not kept alive through artificial background refresh; reauthenticate expired sessions.

Current validation is recorded in [docs/VALIDATION.md](docs/VALIDATION.md). Automated tests do not prove two real Roblox accounts work. Live tests require your accounts and the installed client; they have not been performed during implementation.

## Build and test

Recommended: install Rust stable for `x86_64-pc-windows-msvc`, Visual Studio Build Tools with Desktop development with C++, .NET 8 SDK and Python 3. The pinned Windows App SDK and Windows SDK build tools restore from NuGet. Run:

```powershell
.\scripts\build.ps1
.\scripts\test.ps1
```

Build output is `dist\rolauncher`, including the WinUI shell in `desktop`. The scripts also support workspace-local Rust/GCC and .NET toolchains, without changing machine-wide environment variables. `Cargo.lock` and the desktop package lock files pin dependencies. Run `cargo fmt` after Rust source edits. `scripts\test.ps1` runs Rust tests, desktop model/editor checks and the actual WinUI shell against a loopback fixture with simulated accounts. It renders previews under `target\ui-preview`; it does not use saved accounts, launch Roblox or send Discord messages.

After tests and `cargo clippy --locked --all-targets -- -D warnings` pass, `scripts\package.ps1` builds versioned Windows/source ZIPs and SHA256 checksums. It refuses to replace existing release artifacts.

## Resource usage

The Rust engine has two async worker threads, shared discovery, batched log ownership queries, and incremental bounded log reads. The WinUI 3 desktop runs in a separate .NET process and uses native controls. WebView2 is used only for browser sign-in. The self-contained desktop increases download size and memory compared with the older Win32 interface; measure both `rolauncher` and `RoLauncher.Desktop` for total application resource usage.

Use `scripts\benchmark.ps1 -ProcessId PID -Seconds 30 -Output idle.json` against the running supervisor. It samples OS CPU/memory counters only. Repeat with 10 and 50 active accounts; measure Roblox clients and temporary WebView2 login processes separately. No performance advantage is claimed without comparative measurements.

Previously measured 0.4.0 supervisor baseline (headless, no accounts, 15.24 seconds): **14.6 MiB working set, 2.1 MiB private memory, 0.0% recorded machine CPU** at the sampled counter resolution. This excludes the new WinUI process and does not predict total Roblox resource usage or demonstrate an advantage over C#.

## License and provenance

GPL-3.0-only. Protocol behavior was referenced from [ic3w0lf22/Roblox-Account-Manager](https://github.com/ic3w0lf22/Roblox-Account-Manager), archived October 2024. See `THIRD_PARTY_NOTICES.md`. The tool does not redistribute Account Manager or its `handle.exe` utility.
