# RoLauncher 1.5.0

2026-10-08 · Changes since stable 1.4.0. Linux remains a preview.

- Click account rows/game cards to edit; use account checkboxes for bulk selection.
- Discord `/action username action` supports Stop/Restart with autocomplete.
- `/status-register channel` shows ten accounts/page with Previous/Refresh/Next; refreshes changed data every 15 seconds and retains page/message across reconnects/restarts.
- Bot Save keeps credentials without starting; Start/Stop/Restart show Online/Starting/Offline. Previously started bots resume on app launch; explicit Stop keeps them off.
- Account/Dashboard running-time records survive app restarts. Timing begins on confirmed connection, includes automatic rejoins, and ends on Stop/manual Restart.
- Generic disconnect alerts wait through 30-second grace; confirmed errors notify promptly. Stop/clean exits are silent.
- Error alerts include error/code, game link when available, and the affected window's screenshot.
- Linux forwards HOME into Flatpak and reads verified logs across mount namespaces, rejecting reused descriptors.

## Bot setup

1. Create a dedicated Discord bot; install with `bot` and `applications.commands` scopes.
2. Save token, server ID, and allowed user IDs under Discord settings; press Start.
3. Grant View Channel, Send Messages, and Embed Links in status channels.

- Leave Interactions Endpoint URL unset; Gateway events need only the unprivileged Guilds intent and no inbound listener.
- Webhooks still deliver notifications. Tokens stay encrypted and are never returned by the settings API.
- Run one connection per bot token; if removed from the server, reinstall and Restart.
- Local removal disconnects the bot; Discord commands remain until the application is removed from the server.

## Screenshots

- Windows: only the verified process's visible, non-minimized window.
- Linux: Python 3, libX11/libXres/libXcomposite, composited X11/XWayland, and XRes-verified host PID.
- Native Wayland, protected/minimized/closed windows, missing libraries, or ambiguous ownership yield explained text-only alerts.
- No desktop-wide fallback; screenshots stay in memory. Destroyed windows cannot be captured.

## Validation

- Windows: 65 Rust tests, separate bootstrap, Clippy, UI smoke, 71 desktop checks, installer/package checks, and versioning/beta fixtures passed.
- Discord fixtures covered pagination IDs, legacy buttons, and error classification; no live messages were sent.
- Arch WSL: 64 Rust tests, Clippy, isolation adapter, and Linux updater checks passed.
- Native KDE concurrent games, status/screenshot attribution, and live configured Discord delivery remain unverified.
