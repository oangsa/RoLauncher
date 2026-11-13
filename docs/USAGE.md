# User guide

[README](../README.md) · [API](API.md) · [Linux setup](LINUX.md)

## Install and start

1. Download **setup-x64.exe** from [Releases](https://github.com/oangsa/RoLauncher/releases/latest).
2. Install and open RoLauncher from Start.
3. Use **Sign in** or **Import cookies**. Paste or import one cookie per line; blanks and duplicates are ignored.
4. Choose account destinations, select accounts, and press **Start**.

Requires Windows 10 (1809+) or Windows 11 (x64, Direct3D 11 / DirectComposition support), and [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) for sign-in. No .NET runtime is required.
For portable use, extract everything; keep `WebView2Loader.dll` and `desktop` beside `RoLauncher.exe`.

## Sidebar navigation

- Workspace, Accounts, Settings, and Tools group related pages.
- **Settings → General → Appearance** saves System, Light, or Dark.
- Clicking the current page keeps its scroll position. Windows animation, transparency, and high-contrast preferences control effects.
- **Tools → Support** contains report export and the API token.

## Manage accounts

- Open an account editor to change alias, destination, group, fallback, or automatic rejoin. **Save** commits; **Cancel** discards.
- Search alias/username or filter status, group, and rejoin. Checkboxes and Ctrl/Shift selection choose accounts; Ctrl+A selects visible accounts.
- Click empty workspace space to clear selection. Row Start/Stop/Restart affects that account; toolbar actions affect the selection.
- **Edit selected** applies only checked fields and saves the batch atomically. **Edit all accounts** includes filtered-out accounts.
- Stop accounts before removing them. Alias patterns accept `{username}`, `{id}`, and `{index}`.
- Destination changes apply on the next launch. Missing game names fall back to Place ID; hover clipped names for full text.

## Saved games

1. Open **Games → Add game**; enter a name, Place ID, and optional Job ID/private-server link.
2. Choose **Saved game** in an individual or bulk account editor.
3. Choose **Custom destination** to unlock destination fields for manual editing.

Game names/thumbnails use a saved Roblox session. Click game cards to edit or delete.
Changing or deleting a profile retains destinations already copied to accounts; deletion clears its assignment.

## Groups and account presets

Set groups in account editors. Under **Accounts → Account presets**:

1. In **Create preset**, choose accounts using search and group/status/rejoin filters.
2. **Select visible** adds matches; **Clear selection** clears even hidden choices. Selection survives filtering.
3. **Use selection** confirms; **Cancel** discards. Edit settings if needed, name the preset, then **Save preset**.
4. In **Use preset**, preview saved accounts, then **Review & launch** or **Apply settings**.

Applying affects the preset's saved accounts. **More** offers replace, delete, import, and export.
Exports contain settings and private links, but no cookies. Keep private links private.

## Rejoin and recovery

- Enable **Automatic rejoin** per account or through **Edit selected**. Check **Apply rejoin** before applying; a mixed check means values differ.
- Disabling rejoin cancels retries and leaves clients open.
- Disconnects allow 30 seconds to reconnect; five failed attempts pause recovery.
- Authentication errors or uncertain ownership require attention. **Unknown** does not confirm a running connection.
- **Accounts → More → Retry selected** advances eligible retries. **Sign in again** repairs one selected account; press Start afterward.

| Fallback policy | Confirmed unavailable destination |
| --- | --- |
| Allow public fallback | Join a public server in the same place, including for private targets. |
| Stay on destination | Retry the saved destination. |
| Pause and notify | Stop retries and request attention. |

Dashboard shows status, rejoin coverage, connectivity, destinations, and accounts needing attention.
Its 24-hour chart follows history filters; retention is capped at 1,000 events, so busy days can be incomplete.
History identifies accounts by alias and username.

## Resource monitor

- Open **Workspace → Resources** for live host CPU/RAM, aggregated managed Roblox usage, 60-second charts, and sortable instance metrics.
- Active 1-second sampling runs while viewing **Resources** or streaming `/v1/resources/events`, and suspends automatically afterward.
- CPU utilization is normalized across all logical processors (`0–100%`). GPU utilization appears when Windows counters support it and shows `Unavailable` otherwise.
- Process metrics remain read-only and never alter client memory or account lifecycle states.

**Stop** closes the managed client. **Settings → General → Window** chooses tray minimization or full exit.
Full exit leaves Roblox open and ends supervision; Linux uses taskbar minimization without a tray host.
The icon beside the version opens the latest two changelog entries.

## Server selection and joining together

Under **Settings → General → Game joining**:

| Setting | Behavior |
| --- | --- |
| Smart | Fewest players within 25 ms of the best reported ping. |
| Lowest / Highest | Fewest / most players, excluding full servers. |
| Friend | Join a managed account using the same saved game and destination. |
| Random | Choose independently. |

- Scans check up to 500 servers. Reported ping cannot guarantee region or local latency.
- Explicit Job IDs and private links take priority. Preferences save automatically for future starts/rejoins.
- Select and save the game again on existing accounts to establish Friend grouping.
- The first account seeks room for the team; others join its instance after it connects.
- Hidden friend presence can use the managed peer's own session; the place must match.
- Missing presence or full servers trigger retry/attention. Roblox enforces access and capacity; slots are not reserved.

## Backups and data

- **Settings → Backups** keeps ten encrypted backups. Restore requires stopped accounts and leaves them stopped.
- Backups work only for the same Windows user. Older `.rbxbackup` files remain supported.
- New data: `%LOCALAPPDATA%\RoLauncher`. Legacy `%LOCALAPPDATA%\RbxTools` is reused until the new folder has `accounts.json`.
- An update cache alone does not switch data folders. Uninstall retains data/backups.

For startup corruption, close the app and clients, then run:

```powershell
.\RoLauncher.exe --restore-backup BACKUP_FILENAME
```

Use `--data-dir FOLDER` for a custom folder.

## Updates and Discord

- Updates check at startup and every six hours; manual checks live in **Settings → Updates**.
- **Download → Relaunch** verifies the installer, backs up data, installs, and reopens. Roblox stays open.
- **Include beta updates** defaults off. Versions 1.0.0/1.1.0 must first install a later stable version to gain this option.
- For alerts, save a text-channel webhook in **Settings → Discord**, enable notifications, then **Send test**.
- Saved webhooks show dots: untouched keeps the URL, a new URL replaces it, and **Remove webhook** clears it.
- Delivery is best effort; **Recent activity** also shows alerts locally. Bot commands include `/action`, `/status-register`, `/resources`, and `/resources-register` ([1.5.0 notes](RELEASE-1.5.0.md#bot-setup)).

## Troubleshooting

- Keep cookies and API tokens private. Share **Tools → Support → Export report**.
- If existing Roblox clients block multiple instances, close them and restart RoLauncher.
- Launch requests, process metadata, and existing logs drive monitoring; client memory/files are not modified.
- See [validation](VALIDATION.md) for unverified real-client behavior.

CLI options: `--headless`, `--port NUMBER`, `--data-dir FOLDER`, `--diagnose`, `--restore-backup FILENAME`.
