# Changelog

## [1.7.8] — Unreleased

- Avoid permanently pausing automatic rejoin when a closing client disappears before fresh process verification.

## [1.7.7] — 2026-10-24

- Close disconnected clients gracefully before five-second force escalation; keep reconnect cancellation and verified exit before rejoin.
- Resume automatic rejoin after a failed client close once the managed process has exited.
- Recover saved accounts paused by this failure when RoLauncher restarts and no client remains.

## [1.7.6] — 2026-10-23

- Restore 30-second reconnect grace so redirects can finish without closing and reopening the client.
- Keep brief connection-loss redirects silent in Discord; retain prompt alerts for known errors.

## [1.7.5] — 2026-10-23

- Rejoin after verified disconnected clients exit, including tray clients; retain retry backoff.

## [1.7.4] — Unreleased

- Recover from confirmed game kicks when Roblox omits numeric error 267, retaining retry limits and verified exit before replacement.

## [1.7.3] — 2026-10-21

- Clean up tray helper clients and tolerate exited processes while preserving visible games and verified managed clients.
- Retry opted-in background cleanup when Start is blocked by a lingering Roblox process.
- Move the all-client stop action to Accounts and remove Integration status from Dashboard.
- Give Game joining its own settings tab with separate joining preferences and Roblox instance controls.
- Show 10 history and recent activity entries per page, with pagination below each list.

## [1.7.2] — Unreleased

- Add Close all Roblox to stop rejoin and close external, hidden, and tray clients in the current Windows session.
- Rejoin after confirmed game kicks (267), with bounded retries and verified process exit before replacement.
- Prevent duplicate launches while an earlier launch remains unresolved; require discovery before releasing expired launch tracking.
- Honor Stop for verified clients discovered late, allowing cleanup after the launch discovery deadline.

## [1.7.1] — 2026-10-20

### Fixed

- Pause recovery for Roblox update error 280 and ban error 600; preserve duplicate-session error details.
- Ignore fake connection events inside oversized game output and detect join failures with spaced HTTP status fields.
- Handle malformed JSON and empty responses without leaving desktop refresh locked.
- Validate imports four at a time, save in submitted order, and give large batches longer to finish.
- Start controlled recovery immediately for confirmed disconnects, including 273 and 279; verify process exit before replacement.
- Keep RAM-only hints, ambiguous transport events, and isolated HTTP failures from closing healthy clients; pause bans, access restrictions, and updates.
- Recover recent disconnects through large log backlogs while keeping process-exit checks responsive during slow discovery.
- Search beyond 500 servers while considering capacity and stopping safely on repeated page cursors.
- Offer opt-in cleanup for conflicting background Roblox clients; default Off, preserving visible games and saved managed clients.
- Make multiple Roblox instances optional, default On; preference changes apply after restarting RoLauncher.

Newest first; local builds use unique dev suffixes.
1.0.0 was first public stable; earlier builds were local, and 1.1.0 was a Linux beta.
[Validation and limits](docs/VALIDATION.md).

## [1.7.0] — 2026-10-15

### Added

- Choose Smart/Lowest/Highest public-server selection; reported ping cannot guarantee region or local latency.
- Choose Friend for managed accounts with the same saved game/destination, using the peer’s session for hidden presence, or Random for independent selection.
- Persist game assignments across edits, presets, backups, and restarts; existing accounts must select their saved game again.
- Save preferences for future starts/rejoins, with Job IDs/private links taking priority; Roblox enforces access/capacity and unavailable peers use retry/attention.

### Fixed

- Recover failed HTTP joins, including timeout/Join Error 529 stuck at Unknown; retain reconnect grace, retry limits, and rejoin settings.

## [1.6.0] — 2026-10-09

### Changed

- Add page transitions and sliding sidebar highlight; preserve current-page scroll and honor reduced motion.
- Refresh Mica workspace, layered cards, and dialogs; retain theme, transparency, and high-contrast support.
- Use native Windows typography and bundled Inter on Linux.

### Fixed

- Capture full physical client dimensions on scaled Windows displays; handle monitor moves and skip frames resized during capture.

## [1.5.0] — 2026-10-08

### Added

- Discord Stop/Restart, username autocomplete, and automatically refreshed status messages; restrict access to configured server/users.
- Separate bot Save from Start/Stop/Restart and show connection status; resume previously started bots unless explicitly stopped.
- Show current runs and saved longest streaks on accounts/Dashboard; automatic rejoins retain runs, Stop/manual Restart ends them.

### Changed

- Click account rows/game cards to edit; select bulk accounts with checkboxes.
- Suppress short-redirect and clean-exit alert noise; include error details, game links, and affected-window screenshots when available.

### Fixed

- Forward Sober HOME inside Flatpak and read verified logs across mount namespaces; native KDE validation remains pending.

## [1.4.0] — 2026-10-07

### Added

- Saved System/Light/Dark theme, game names with Place ID fallback, and per-row Start/Stop/Restart.
- Import multiple cookies beside sign-in; ignore blanks/duplicates.

### Changed

- Dedicated settings pages and Dashboard status/rejoin/connectivity/attention summaries.
- Separate preset use/creation with searchable account lookup, persistent filtered selection, and confirm/cancel.
- Move rejoin to individual/bulk editors; lock saved-game destinations until Custom destination is chosen.
- Fill wide windows, scroll narrow tables, and clear selection on empty-space clicks.

### Fixed

- After startup with Roblox already open, close those clients and retry Start without restarting RoLauncher.

## [1.3.0] — 2026-10-06

### Added

- Sidebar navigation and saved games with names, thumbnails, public/private/Job ID targets, individual/bulk selection, and encrypted backup support.
- Read-only Dashboard counts, 24-hour trends, persistent history, and recent activity; recovery actions remain in Accounts.
- Version-icon changelog showing two releases plus full-history link; saved tray/exit preference.

### Changed

- Rename Recovery & Access to Auto Rejoin; remove header Exit.

### Fixed

- Reset page scroll, target clicked-row actions correctly, and preserve text selection with Ctrl+A.
- Grant Sober input access on every isolated launch/retry; requires Flatpak 1.15.6+.

## [1.2.1] — 2026-10-06

### Fixed

- Center update-banner Download/Cancel download/Relaunch buttons on Windows and Linux.

## [1.2.0] — 2026-10-06

### Added

- Saved beta-update opt-in, off by default, with labeled banners and verified publisher/checksums.
- Built-in per-account Flatpak runtime/IPC isolation for launches/retries; no adapter needed.
- Linux preview alongside stable Windows; concurrent games and full parity remain unverified.

### Fixed

- Discover Sober logs immediately and accept inherited logs within one verified sandbox; ambiguity stays Unknown and diagnostics expose no credentials.
- Match account group dropdown and button heights.

## [1.1.0] — 2026-10-06 (beta)

### Added

- Linux/Sober preview using shared Uno layouts; Windows retains WinUI.
- Linux encrypted credentials, browser login, process/log monitoring, desktop integration, and verified archive updates.
- Native Ubuntu/Arch GNOME/KDE parity and live-player checks remain incomplete; credentials/backups are OS/user specific, requiring sign-in again.

## [1.0.0] — 2026-10-06

### Added

- Per-user Windows installer, shortcuts, uninstall, and portable package.
- Startup/six-hour update checks; verified Download → Relaunch backs up data and keeps Roblox open.
- Groups, atomic bulk edits, and saved account profiles with review/apply/launch/import/export/management.
- Recovery reasons/countdowns, eligible retry, sign-in repair, and public/stay/pause fallback policies.
- Persistent filtered history, sanitized reports, ten encrypted backups, and offline corruption recovery.
- CI release packages with source, notices, AGENTS.md, and SHA256 checksums.

### Changed

- RoLauncher name/icon; preserve legacy accounts/backups.
- Separate usage/API/build guides from README.

### Fixed

- Keep legacy data accessible when update downloads create the new data folder.

## [0.7.0] — 2026-10-05 (local development)

### Changed

- Top-center toasts preserve layout and close after five seconds/dismissal; new actions reset the timer, while repeated background warnings do not.

## [0.6.1] — 2026-10-05 (local development)

### Fixed

- Match status/rejoin filter heights to search and Clear controls.

## [0.6.0] — 2026-10-05 (local development)

### Added

- Account dialog with Save/Cancel replaces autosave; add search, status/rejoin filters, and row edit/delete.

### Changed

- Bulk actions affect selected visible accounts; confirm deletion.

## [0.5.0] — 2026-10-05 (local development)

### Changed

- WinUI 3 desktop with native controls, cards, badges, and editing; retain login/accounts/Discord/tray and bundle the runtime.

## [0.4.0] — 2026-10-05 (local development)

### Added

- Optional Discord webhook alerts for disconnects/exits/attention and recovery; encrypted settings, test delivery, and local activity.

## [0.3.0] — 2026-10-05 (local development)

### Added

- Accounts/Settings/Changelog tabs, bulk account actions/rejoin, Ctrl+A selection, and valid-edit autosave.

## [0.2.0] — 2026-10-05 (local development)

### Changed

- Recognize more kick/disconnect/failed-join logs; confirmed reconnection cancels recovery during 30-second grace.

## [0.1.4] — 2026-10-05 (local development)

### Fixed

- Recognize error 279 and wait through reconnect grace; ignore unrelated timeouts and avoid public fallback on connection failure alone.

## [0.1.3] — 2026-10-05 (local development)

### Fixed

- Avoid false Windows error 5 when clients exit during closure; resume recovery while retaining genuine access/authentication errors.

## [0.1.2] — 2026-10-05 (local development)

### Fixed

- Fix ticket requests/current-client discovery, redact request secrets, and include WebView2 loader.

## [0.1.0] — Local development

### Added

- Native Windows account launcher with encrypted sessions and local API.
