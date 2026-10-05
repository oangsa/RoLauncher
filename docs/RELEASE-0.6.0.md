# RoLauncher 0.6.0

Moves account details into a WinUI modal and adds table search, filters and per-row actions while retaining the minimal black-and-white theme.

- Each row has two icons: view/edit details and delete. The details modal shows username, ID, status, PID, failures and last error, and edits alias, destination and automatic rejoin.
- Save changes persists a single validated patch. Cancel discards edits. Invalid destinations and rejected API requests keep the modal open; no partial alias or rejoin changes are saved when a destination is invalid. Status polling preserves typed fields and updates live account information. External account removal disables Save.
- Case-insensitive name search matches aliases and usernames, including an optional @ prefix. Status and automatic-rejoin filters combine with search; Clear resets them. A separate no-results message distinguishes filtering from an empty account store.
- Filtering deselects hidden rows. Ctrl+A and bulk operations apply to visible selected accounts. Matching selections survive status refreshes; renamed accounts are reevaluated against the search.
- Row deletion targets that row regardless of bulk selection. Row and bulk deletion ask for confirmation, defaulting to Cancel, and retain the existing stopped-account requirement.

The desktop editor now uses explicit Save and Cancel instead of autosave. Account import, launch/recovery behavior, the local API and Discord settings continue to use the existing Rust supervisor.

The Windows ZIP includes the self-contained WinUI desktop, compiled XAML resources, WebView2Loader.dll, license notices and AGENTS.md. The source ZIP and SHA256 checksums accompany it. Extract the entire Windows ZIP and run RoLauncher.exe.

Automated verification uses simulated accounts and isolated local APIs. It does not launch real Roblox clients or send real Discord webhooks. See VALIDATION.md for checks and existing live compatibility limits.
