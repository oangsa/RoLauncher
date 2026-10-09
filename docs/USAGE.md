# User guide

[README](../README.md) · [API reference](API.md)

## Install and start

1. Download the **setup-x64.exe** installer from [Releases](https://github.com/oangsa/RoLauncher/releases/latest).
2. Install and open RoLauncher from the Start menu.
3. Add accounts with **Browser sign-in**, paste one or multiple cookies (one per line) and press **Import cookies**, or import a file containing one cookie per line. Blank lines and duplicate cookies are ignored; the paste field clears when import starts.
4. Edit an account's destination, select accounts, and press **Start**.

Requires 64-bit Windows 10 (1809+) or Windows 11. Browser sign-in needs [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/); .NET is included.

For the portable ZIP, extract everything and run `RoLauncher.exe`. Keep `WebView2Loader.dll` and the `desktop` folder beside it.

## Sidebar navigation

The highlighted pill and marker show the current page. Changing pages uses a short fade and slide; clicking the current page keeps its scroll position. Windows' **Accessibility → Visual effects → Animation effects** preference controls navigation motion. Acrylic follows the system's transparency preference, and high contrast uses solid system colors. Choose System, Light or Dark in **Settings → General → Appearance**.

The sidebar groups focused tabs under Workspace, Accounts, Settings, and Tools. Set automatic rejoin in the account settings modal using the pencil icon, or use **Edit selected** for multiple accounts. General, Discord notifications, Backup and restore, and Updates each have their own Settings tab. Support reports and the API token are on the dedicated **Tools → Support and integrations** page.

A saved Discord webhook displays dots. Type a new URL to replace it, leave the field untouched to keep it, or choose **Remove webhook** to clear it. The saved URL is never loaded into the field.

## Manage accounts

- Use the pencil icon to edit an alias, choose a saved game or enter a Place ID, optional Job ID/private link, and rejoin settings. **Save changes** saves; **Cancel** discards edits.
- Search by alias or username, or filter by status, group, and rejoin setting.
- Ctrl-click/Shift-click selects several rows. Ctrl+A selects visible accounts. Click empty workspace space to clear selection.
- The **Game** column shows the destination game name, with a place number when metadata is unavailable. Hover over a clipped name to see it in full.
- Use the row play, stop and restart icons to act on that account immediately. The toolbar Start, Stop and Restart buttons apply to the selected accounts.
- **Edit selected** changes a batch. Check **Apply** only for fields you want to change. The whole batch saves together.
- **Edit all accounts** includes accounts hidden by filters. Removing accounts requires them to be stopped.

Alias patterns accept `{username}`, `{id}`, and `{index}`. Destination changes apply on the next launch.

## Saved games

Open **Games → Add game**, name the profile, and enter its Place ID and optional private-server link or Job ID. RoLauncher fetches the game name and thumbnail using a saved Roblox session. Edit or delete profiles from the game cards. Choose **Saved game** in the account editor or bulk editor to copy its destination; changes apply on the next launch. In the account editor, a saved game locks its Place ID, Job ID, and private link; choose **Custom destination** to edit these manually. Editing or deleting a profile leaves destinations already saved on accounts intact.

## Groups and account presets

Set a group in the account editor to organize and filter accounts.

Under **Accounts → Account presets**, press **Choose accounts…** in **Create a preset**. The lookup modal contains the account table, search by alias or username, and group, status and rejoin filters. **Select visible** adds matching accounts; **Clear selection** clears all choices, including hidden ones. Choices persist across filters. **Use selected accounts** confirms the selection; **Cancel** discards changes. The preset page shows only the confirmed selection count. You can edit those accounts, enter a name and press **Save preset**. In **Use a saved preset**, choose one to preview its accounts, then use **Review and launch** or **Apply settings only**. **More** contains replace, delete, import, and export actions.

Profile exports contain account settings and private links, but no session cookies. Keep private links private.

## Rejoin and recovery

Enable **Automatic rejoin** for accounts you want RoLauncher to recover. Turning it off cancels automatic retries and leaves running clients open.

The **Dashboard** shows live summary cards, a status breakdown, automatic rejoin coverage (enabled with a configured destination), network and destination status, and a focused list of accounts needing attention. Select an account in that list to inspect or edit it. The responsive activity chart shows retained status events over the last 24 hours; history filters also filter the chart. Persistent history retains the latest 1,000 events, so the chart can be incomplete on busy days. History identifies saved accounts by alias and username. Use **Accounts → More → Retry selected now** to advance an eligible retry. Select one account and choose **More → Sign in again** to repair an expired session; press Start afterward.

Choose a fallback policy:

| Policy | When the destination is confirmed unavailable |
| --- | --- |
| Allow public fallback | Join a public server in the same place, including for private targets. |
| Stay on this destination | Keep retrying the saved destination. |
| Pause and notify | Stop retrying and request attention. |

Disconnects allow 30 seconds to reconnect. Five failed attempts pause recovery. Authentication errors and uncertain process ownership require attention. **Unknown** means the app cannot confirm the connection; it does not mean Running.

**Stop** closes the managed client. In **Settings → General → Window**, choose whether closing minimizes to tray or closes completely. Closing completely leaves Roblox clients open and stops supervision. Linux minimizes to the taskbar when a tray host is unavailable. The changelog icon beside the version opens the latest two releases and links to the full changelog.

## Backups and data

**Settings → Backup and restore** offers **Create backup** and **Restore backup**. The latest ten encrypted backups are kept. Restore requires stopped accounts and leaves restored accounts stopped. Backups work only for the same Windows user.

New installs save data under `%LOCALAPPDATA%\RoLauncher`. Existing `%LOCALAPPDATA%\RbxTools` data is reused until RoLauncher has its own `accounts.json`; an update cache alone does not change the selected folder. Uninstall keeps data and backups.

For startup corruption, close the app and clients, then run:

```powershell
.\RoLauncher.exe --restore-backup BACKUP_FILENAME
```

Use `--data-dir FOLDER` for a custom data folder. Older `.rbxbackup` files remain supported.

## Updates and Discord

Updates are checked at startup and every six hours. Press **Download**, then **Relaunch**. RoLauncher verifies the installer, backs up data, installs, and reopens. Roblox clients stay open. **Settings → Updates** also offers a manual update check.

Version 1.2.0 adds **Include beta updates** in Settings. It defaults to off; enabling it checks both stable and beta releases and labels beta updates in the banner. The preference is saved for automatic checks. Published 1.0.0 and 1.1.0 builds check stable releases only, so use their existing update check to install stable 1.2.0 before opting into future betas.

For Discord alerts, create a webhook in a regular text channel. Paste it in **Settings → Discord notifications**, choose **Save webhook**, enable notifications, and press **Send test**. Delivery is best effort; **Recent activity** shows local alerts too.

## Troubleshooting

- Keep cookies and the local API token private. Use **Tools → Support and integrations → Export support report** when reporting a problem.
- If existing Roblox clients prevent multiple instances, close them and restart RoLauncher. Client compatibility can change.
- RoLauncher uses launch requests, process metadata, and existing logs. It does not inspect memory or modify Roblox.
- See [validation notes](VALIDATION.md) for remaining real-client checks.

Command-line options: `--headless`, `--port NUMBER`, `--data-dir FOLDER`, `--diagnose`, and `--restore-backup FILENAME`.

## Appearance and account workflows

Choose **Settings → General → Theme** to use System, Light, or Dark. The choice saves automatically; System follows device appearance.

Manage **Automatic rejoin** from **Accounts**. Use an account’s pencil icon for individual settings, then **Save changes**. For many accounts, search or filter the table, select the matching accounts (Ctrl+A selects visible accounts), and choose **Edit selected**. Check **Apply automatic rejoin**, choose on or off, then **Apply changes**. A mixed check means the selected accounts have different values. **Cancel** discards changes. **Selection → Edit all accounts** applies settings across all saved accounts.

In **Account presets**, choose a saved preset to preview its accounts, then Review and launch or Apply settings only. To create one, choose accounts in Create a preset, edit their settings if needed, enter a name, and Save preset. More contains replace, delete, import, and export actions. Applying a preset changes its saved accounts, not the accounts currently chosen in the creation list.
