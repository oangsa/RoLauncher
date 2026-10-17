# Local API reference

## Access

- Base: `http://127.0.0.1:38471`.
- Token: **Tools → Support → API token**. Every route needs `Authorization: Bearer TOKEN`.
- Reject browser Origin/cross-site requests. Never return account credentials. Keep token private.
- Account imports accept 1–50 cookies; validate up to four concurrently and save in submitted order. Errors retain original input indexes.
- Desktop import deadline: ten minutes. Other desktop API requests: 30 seconds.

## Game joining

- Status: `server_selection` (`smart`, `lowest`, `highest`), `join_type` (`friend`, `random`).
- Preferences apply next start/rejoin. Smart uses reported ping/player count, not verified region/local latency.
- Smart follows pages for two minutes; Lowest/Highest stop at first sorted page with room. Job IDs/private links override.
- Assign `game_profile_id` with matching `target`. Stale/mismatched assignments reject whole update.
- Custom target without ID or `clear_target` clears assignment. Omitted destination stays unchanged.
- Presets/backups keep assignments. Friend groups explicitly assigned accounts with equal destinations.
- Friend status uses joining account/managed peer's saved session; place must match. Roblox checks access/capacity.

## Routes

| Method | Route | Body / result |
| --- | --- | --- |
| PATCH | `/v1/settings/instances` | `{ "multi_instance_enabled": true / false }`; default On; apply after app restart. Status includes saved/active values. |
| PATCH | `/v1/settings/game` | `{ "server_selection": "smart" / "lowest" / "highest", "join_type": "friend" / "random" }`; both required; saved together for next launches. |
| GET | `/v1/status` | Accounts, games, close preference, network pause, compatibility status |
| POST | `/v1/login` | Open separate browser sign-in; HTTP 202 means queued |
| GET | `/v1/accounts` | Safe account/status records |
| POST | `/v1/accounts` | `{"cookies":["SESSION_VALUE"]}`; errors indexed by input |
| PATCH | `/v1/accounts/USER_ID` | Alias, target, `auto_recovery`, `group`, `fallback_policy`, explicit `clear_target` |
| DELETE | `/v1/accounts/USER_ID` | Remove stopped account without pending owned client |
| POST | `/v1/accounts/USER_ID/start` | Queue launch; return operation ID |
| POST | `/v1/accounts/USER_ID/stop` | Cancel recovery; close verified client |
| POST | `/v1/accounts/USER_ID/restart` | Replace verified client |
| GET | `/v1/operations/UUID` | Queued/completed/failed/cancelled operation |
| GET | `/v1/events` | SSE status; reconnect gets current snapshot |
| PATCH | `/v1/accounts/bulk` | `account_ids` (1–50 distinct IDs), `patch`; all-or-none save; alias patterns |
| POST | `/v1/accounts/USER_ID/retry` | Advance allowed retry; keep failure count/cooldowns |
| POST | `/v1/accounts/USER_ID/repair` | Separate sign-in; same account ID required |
| GET / POST | `/v1/profiles` | List/save `name`, `account_ids`, optional existing `id` |
| POST | `/v1/game-profiles` | Create/update `{id,name,game_name,thumbnail_url,target}`; UUID identifies profile; limit 100 |
| DELETE | `/v1/game-profiles/UUID` | Delete game/assignments; keep account destinations |
| GET | `/v1/games/PLACE_ID` | Roblox game name/optional thumbnail via saved session |
| PATCH | `/v1/settings/window` | `{ "close_to_tray": true }` or `false`; snapshots include `close_to_tray` |
| POST | `/v1/profiles/import` | JSON array without credentials; whole import succeeds or fails |
| DELETE | `/v1/profiles/UUID` | Remove preset |
| POST | `/v1/profiles/UUID/apply` or `/start` | Apply settings / apply then Start; launch result per account |
| GET | `/v1/activity?account_id=USER_ID` | Saved newest-first history; optional account filter |
| GET | `/v1/diagnostics` | Safe status/history report |
| GET / POST | `/v1/backups` | List encrypted backups / create backup |
| POST | `/v1/backups/FILENAME/restore` | Validate/restore backup; all accounts must be stopped |
| PATCH | `/v1/updates` | Legacy API repository; blank means official. Desktop publisher stays official. |
| POST | `/v1/updates/check` | Check legacy configured public repository in saved update channel |
| POST | `/v1/updates/official/check` | Official publisher; used by automatic desktop checks |
| PATCH | `/v1/settings/updates` | `{"include_beta":true}` includes beta; `false` means stable only (default). Snapshot field: `include_beta_updates`. |
| GET | `/v1/settings/discord` | Enabled/configured, recovery preference, delivery status, recent activity; never webhook URL |
| PATCH | `/v1/settings/discord` | Optional `enabled`, `notify_recovery`, `webhook_url`; empty URL removes it (also set `enabled:false`) |
| POST | `/v1/settings/discord/test` | Test saved webhook even with alerts Off; HTTP 202 means queued |

## Records and operations

- Account IDs/Windows creation ticks: strings. PIDs: integers.
- Process fields: PID, creation time, tracker, generation.
- Operations last this app session; limit 1,000 completed entries. Desired account state stays saved.
- Target edits apply next launch. Repeated Start/Stop safe; reject conflicting pending Restart.

```powershell
$headers = @{ Authorization = 'Bearer YOUR_TOKEN' }
Invoke-RestMethod 'http://127.0.0.1:38471/v1/accounts' -Headers $headers
$target = @{ target = @{ place_id = 1818 }; auto_recovery = $true } | ConvertTo-Json
Invoke-RestMethod 'http://127.0.0.1:38471/v1/accounts/USER_ID' -Method Patch -Headers $headers -ContentType 'application/json' -Body $target
Invoke-RestMethod 'http://127.0.0.1:38471/v1/accounts/USER_ID/start' -Method Post -Headers $headers
```

### Discord bot settings

- `PATCH /v1/settings/discord` accepts `bot`: `enabled`, `token`, `guild_id`, `allowed_users` (user ID strings).
- Omitted fields unchanged; empty token removes it. Enable requires token/server ID/nonempty allowed-user list.
- GET: configuration flags, guild/users, status, `connection_state` (`online`, `starting`, `offline`); never token.
- Save does not start stopped bot. POST `/v1/settings/discord/bot/start`, `/stop`, `/restart` controls saved connection.
- Start/Restart save startup choice. Stop keeps credentials but clears startup choice; disable/remove clears it too.
- `/status-register channel` saves channel/message/page; refreshes changed data every 15 seconds.
- Re-register reuses message or replaces deleted one. Commands opt-in; same managed Stop/Restart.
- Same local authentication/origin checks apply.

## Account timing

- `running_since`: UTC run start; null before confirmed connection or after Stop/manual Restart. Disconnects/auto-rejoin keep it.
- `longest_streak_seconds`: saved best run; display larger of saved/current duration.
- Both survive app restarts.

[Back to README](../README.md)
