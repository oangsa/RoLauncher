# Local API reference

Default address: `http://127.0.0.1:38471`. Open Settings → Support and integrations and click API token to obtain your token. All routes require `Authorization: Bearer TOKEN`. Browser Origin and cross-site requests are rejected. No endpoint exports account credentials. Keep the API token private.

| Method | Route | Body / behavior |
| --- | --- | --- |
| GET | `/v1/status` | Accounts, saved game profiles, close preference, connectivity suspension, compatibility status |
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
| POST | `/v1/game-profiles` | Create/update `{id,name,game_name,thumbnail_url,target}`; UUID identifies the game profile, up to 100 |
| DELETE | `/v1/game-profiles/UUID` | Delete saved game; account destinations remain unchanged |
| GET | `/v1/games/PLACE_ID` | Authenticated Roblox game name and optional thumbnail lookup using a saved session |
| PATCH | `/v1/settings/window` | Save `{ "close_to_tray": true }` or `false`; snapshots include `close_to_tray` |
| POST | `/v1/profiles/import` | JSON array of credential-free profile definitions; atomic import |
| DELETE | `/v1/profiles/UUID` | Remove saved profile |
| POST | `/v1/profiles/UUID/apply` or `/start` | Apply profile settings / apply then Start; per-account launch results |
| GET | `/v1/activity?account_id=USER_ID` | Persistent newest-first history; account filter optional |
| GET | `/v1/diagnostics` | Sanitized status/history report |
| GET / POST | `/v1/backups` | List encrypted backup filenames / create a backup |
| POST | `/v1/backups/FILENAME/restore` | Restore validated backup while all accounts are stopped |
| PATCH | `/v1/updates` | Legacy API discovery repository; blank uses the official repository. Does not change the desktop's official publisher. |
| POST | `/v1/updates/check` | Discover the legacy configured public repository's release in the saved update channel |
| POST | `/v1/updates/official/check` | Check the official publisher; used by automatic desktop checks |
| PATCH | `/v1/settings/updates` | Save `{"include_beta":true}` to include beta releases, or `false` for stable releases only (default). The preference appears as `include_beta_updates` in snapshots. |
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

[Back to README](../README.md)

### Discord bot settings (upcoming 1.5.0)

`PATCH /v1/settings/discord` accepts an optional `bot` object with `enabled`, `token`, `guild_id` and `allowed_users` (array of Discord user ID strings). Omitted fields retain saved values. An empty token removes it. Enabling requires a token, server ID and nonempty allowlist. GET returns bot enabled/configured flags, guild ID, allowed user IDs and connection status plus `connection_state` (`online`, `starting`, `offline`), never the token. Saving does not start a stopped bot. `POST /v1/settings/discord/bot/start`, `/stop`, and `/restart` control the connection using saved settings; Stop retains credentials. The bot resumes on app launch if it was previously started; an explicit Stop keeps it offline until started again. `/status-register channel` persists a channel/message/page registration and updates the same message when account data changes, checked every 15 seconds. Re-registering the same channel reuses the message (or replaces a deleted one). The same loopback authentication/origin protections apply. Commands are opt-in and use the existing supervised Stop/Restart operations.

Account timing: `running_since` is the UTC start of the current managed run, or null before a confirmed connection and after Stop/manual Restart. Disconnects and automatic rejoins retain it. `longest_streak_seconds` stores the best elapsed run duration; clients display the maximum of this value and the current run. Both fields persist across app restarts. Discord bot Start/Restart persist the intent to run on the next app launch; Stop or disabling/removing the bot clears it.
