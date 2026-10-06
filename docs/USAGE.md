# User guide

[README](../README.md) · [API reference](API.md)

## Install and start

1. Download the **setup-x64.exe** installer from [Releases](https://github.com/oangsa/RoLauncher/releases/latest).
2. Install and open RoLauncher from the Start menu.
3. Add accounts with **Browser sign-in**, cookie paste, or a file containing one cookie per line.
4. Edit an account's destination, select accounts, and press **Start**.

Requires 64-bit Windows 10 (1809+) or Windows 11. Browser sign-in needs [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/); .NET is included.

For the portable ZIP, extract everything and run `RoLauncher.exe`. Keep `WebView2Loader.dll` and the `desktop` folder beside it.

## Manage accounts

- Use the pencil icon to edit an alias, Place ID, optional Job ID/private link, and rejoin settings. **Save changes** saves; **Cancel** discards edits.
- Search by alias or username, or filter by status, group, and rejoin setting.
- Ctrl-click/Shift-click selects several rows. Ctrl+A selects visible accounts.
- **Edit selected** changes a batch. Check **Apply** only for fields you want to change. The whole batch saves together.
- **Edit all accounts** includes accounts hidden by filters. Removing accounts requires them to be stopped.

Alias patterns accept `{username}`, `{id}`, and `{index}`. Destination changes apply on the next launch.

## Groups and profiles

Set a group in the account editor to organize and filter accounts.

In Settings, select accounts and choose **Save selected as profile**. Use **Review / launch** to check and launch a saved setup, or **Apply settings** to restore settings without launching. Profiles can be replaced, renamed, deleted, imported, or exported.

Profile exports contain account settings and private links, but no session cookies. Keep private links private.

## Rejoin and recovery

Enable **Automatic rejoin** for accounts you want RoLauncher to recover. Turning it off cancels automatic retries and leaves running clients open.

The Recovery page shows retry reasons and countdowns. **Retry now** advances an eligible retry. **Sign in again** repairs an expired session using the same account; press Start afterward.

Choose a fallback policy:

| Policy | When the destination is confirmed unavailable |
| --- | --- |
| Allow public fallback | Join a public server in the same place, including for private targets. |
| Stay on this destination | Keep retrying the saved destination. |
| Pause and notify | Stop retrying and request attention. |

Disconnects allow 30 seconds to reconnect. Five failed attempts pause recovery. Authentication errors and uncertain process ownership require attention. **Unknown** means the app cannot confirm the connection; it does not mean Running.

**Stop** closes the managed client. Closing the app window hides it in the tray. **Exit** leaves Roblox open and stops supervision.

## Backups and data

Settings offers **Create backup** and **Restore backup**. The latest ten encrypted backups are kept. Restore requires stopped accounts and leaves restored accounts stopped. Backups work only for the same Windows user.

New installs save data under `%LOCALAPPDATA%\RoLauncher`. Existing `%LOCALAPPDATA%\RbxTools` data is reused until RoLauncher has its own `accounts.json`; an update cache alone does not change the selected folder. Uninstall keeps data and backups.

For startup corruption, close the app and clients, then run:

```powershell
.\RoLauncher.exe --restore-backup BACKUP_FILENAME
```

Use `--data-dir FOLDER` for a custom data folder. Older `.rbxbackup` files remain supported.

## Updates and Discord

Updates are checked at startup and every six hours. Press **Download**, then **Relaunch**. RoLauncher verifies the installer, backs up data, installs, and reopens. Roblox clients stay open. Settings also offers a manual update check.

Version 1.2.0 adds **Include beta updates** in Settings. It defaults to off; enabling it checks both stable and beta releases and labels beta updates in the banner. The preference is saved for automatic checks. Published 1.0.0 and 1.1.0 builds check stable releases only, so use their existing update check to install stable 1.2.0 before opting into future betas.

For Discord alerts, create a webhook in a regular text channel. Paste it in Settings, choose **Save webhook**, enable notifications, and press **Send test**. Delivery is best effort; **Recent activity** shows local alerts too.

## Troubleshooting

- Keep cookies and the local API token private. Use **Export support report** when reporting a problem.
- If existing Roblox clients prevent multiple instances, close them and restart RoLauncher. Client compatibility can change.
- RoLauncher uses launch requests, process metadata, and existing logs. It does not inspect memory or modify Roblox.
- See [validation notes](VALIDATION.md) for remaining real-client checks.

Command-line options: `--headless`, `--port NUMBER`, `--data-dir FOLDER`, `--diagnose`, and `--restore-backup FILENAME`.
