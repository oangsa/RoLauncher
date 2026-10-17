# User guide

[README](../README.md) · [API](API.md) · [Linux setup](LINUX.md)

## Install and start

1. Download **setup-x64.exe** from [Releases](https://github.com/oangsa/RoLauncher/releases/latest).
2. Install, then open RoLauncher from Start.
3. Use **Sign in** or **Import cookies**: one cookie per line. Blanks and duplicates skipped.
4. Choose destinations, select accounts, press **Start**.

Needs Windows 10 (1809+)/11 x64 and [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) for sign-in. .NET included.
Portable: extract all files; keep `WebView2Loader.dll` and `desktop` beside `RoLauncher.exe`.

## Sidebar navigation

- Pages grouped under Workspace, Accounts, Settings, and Tools.
- **Settings → General → Appearance** saves System, Light, or Dark.
- Clicking current page keeps scroll. Effects follow Windows motion, transparency, and high-contrast settings.
- **Tools → Support** holds report export and API token.

## Manage accounts

- Edit alias, destination, group, fallback, or rejoin. **Save** keeps changes; **Cancel** discards.
- Search alias/username; filter status, group, rejoin. Select with checkboxes or Ctrl/Shift; Ctrl+A selects visible accounts.
- Click empty workspace to clear selection. Row Start/Stop/Restart affects one account; toolbar affects selection.
- **Edit selected** changes checked fields only; whole batch saves or none does. **Edit all accounts** includes filtered-out accounts.
- Stop before removing accounts. Alias patterns: `{username}`, `{id}`, `{index}`.
- Destination edits apply next launch. Missing names show Place ID; hover clipped names to read them.

## Saved games

1. **Games → Add game**: enter name, Place ID, optional Job ID/private-server link.
2. Choose **Saved game** in one-account or bulk editor.
3. Choose **Custom destination** to edit destination fields manually.

Cards support edit/delete; names/thumbnails use a saved Roblox session.
Game edits/deletion keep destinations already copied to accounts. Deletion clears game assignment.

## Groups and account presets

Set groups in account editors. Under **Accounts → Account presets**:

1. **Create preset**: find accounts with search and group/status/rejoin filters.
2. **Select visible** adds matches. **Clear selection** clears hidden choices too; filtering keeps selection.
3. **Use selection** confirms; **Cancel** discards. Edit settings, name preset, **Save preset**.
4. **Use preset**: preview saved accounts, then **Review & launch** or **Apply settings**.

Only preset accounts change. **More** offers replace/delete/import/export.
Exports include settings and private links, never cookies. Keep private links private.

## Rejoin and recovery

- Enable **Automatic rejoin** per account or with **Edit selected**. Check **Apply rejoin** first; mixed check means values differ.
- Rejoin Off cancels retries and keeps clients open.
- Disconnects get 30 seconds to reconnect; five failed attempts pause recovery.
- Windows may read recent RAM log messages when file logs stall. Read-only; detection not guaranteed.
- Sign-in errors or uncertain client ownership need attention. **Unknown** does not confirm connection.
- Error 280: update Roblox, then Start. Error 600: ask the game creator about the ban; restarting cannot remove it.
- **Accounts → More → Retry selected** advances allowed retries. **Sign in again** repairs one selected account; press Start afterward.

| Fallback policy | Destination confirmed unavailable |
| --- | --- |
| Allow public fallback | Public server in same place, even for private targets. |
| Stay on destination | Retry saved destination. |
| Pause and notify | Pause retries; request attention. |

Dashboard shows status, rejoin coverage, network, destinations, attention items.
History shows alias/username; 24-hour chart follows filters. Limit: 1,000 events, so busy days may be incomplete.

**Stop** closes managed client. **Settings → General → Window** chooses tray or full exit.
Full exit leaves Roblox open and stops monitoring. Without Linux tray support, close minimizes to taskbar.
Version icon opens latest two changelog entries.

## Server selection and joining together

Under **Settings → General → Game joining**:

| Setting | Choice |
| --- | --- |
| Smart | Fewest players within 25 ms of best reported ping. |
| Lowest / Highest | Fewest / most players with room. |
| Friend | Managed account with same assigned game and destination. |
| Random | Independent choice. |

- Smart checks all available pages; stops after two minutes. Reported ping cannot confirm region or your local latency.
- Job IDs/private links take priority. Preferences save automatically for future starts/rejoins.
- Existing accounts must select/save their game again for Friend grouping.
- First account seeks team space; others join its server after connection.
- If friend status is hidden, managed peer's saved session may find its server. Place must match.
- Missing friend status/full server means retry/attention. Roblox checks access/capacity; no slots reserved.

## Backups and data

- **Settings → Backups** keeps ten encrypted backups. Restore needs stopped accounts; they stay stopped.
- Windows backups require same Windows user. Older `.rbxbackup` files supported.
- New data: `%LOCALAPPDATA%\RoLauncher`. Use legacy `%LOCALAPPDATA%\RbxTools` until new folder has `accounts.json`.
- Update cache alone does not switch folders. Uninstall keeps data/backups.

If damaged data prevents startup, close app and clients, then run:

```powershell
.\RoLauncher.exe --restore-backup BACKUP_FILENAME
```

Custom folder: `--data-dir FOLDER`.

## Updates and Discord

- Update checks: startup, every six hours, or manually under **Settings → Updates**.
- **Download → Relaunch** verifies installer, backs up data, installs, reopens. Roblox stays open.
- **Include beta updates** defaults Off. Versions 1.0.0/1.1.0 need a later stable install first.
- **Settings → Discord**: save text-channel webhook, enable alerts, **Send test**.
- Dots mean saved webhook: untouched keeps it; new URL replaces it; **Remove webhook** clears it.
- Delivery not guaranteed; **Recent activity** also shows local alerts. [Bot setup](RELEASE-1.5.0.md#bot-setup).

## Troubleshooting

- Keep cookies/API tokens private. Share **Tools → Support → Export report**.
- If visible Roblox clients block multiple instances, close them and restart RoLauncher.
- Monitoring uses launch requests, process details, log files, and bounded read-only Windows RAM checks. Never modifies client memory/files.
- [Validation](VALIDATION.md) lists untested real-client behavior.

CLI: `--headless`, `--port NUMBER`, `--data-dir FOLDER`, `--diagnose`, `--restore-backup FILENAME`.

## Multiple instances and startup

- **Settings > Multiple Roblox instances** defaults On; restart RoLauncher to apply changes.
- Off allows one managed client at a time. Changing preference keeps existing clients open.
- Blocked startup closes hidden Roblox leftovers from this user's installation/session automatically.
- Visible games and verified saved clients stay open. No client memory/file writes.
