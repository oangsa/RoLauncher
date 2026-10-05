# RoLauncher 1.0.0

Reissued on 2026-10-06 under the RoLauncher name at the explicitly requested version 1.0.0. The Windows entry point is `RoLauncher.exe`, with `RoLauncher.Desktop` companion files. Application UI, tray actions, browser sign-in, Discord identity, user agents, exports, project names, documentation and update package matching now use the new brand. Packages are named `rolauncher-v1.0.0-*`; existing RbxTools release artifacts are preserved.

New installs use `%LOCALAPPDATA%\RoLauncher`. Existing `%LOCALAPPDATA%\RbxTools` data is reused when the new folder is absent, so accounts, profiles, history and encrypted backups remain available without credential copying. New `.rolbackup` files and existing `.rbxbackup` files are supported. Both old and new application coordination locks are held to prevent simultaneous supervision by the two brands.

Saved sessions and dependable recovery is the major product milestone. The release adds account groups, saved launch profiles and an atomic shared editor for selected or all accounts. The shared editor covers every editable per-account setting: aliases (including per-account patterns), Place/Job/private destinations, rejoin, group and fallback. Apply controls preserve omitted/mixed values; Cancel, invalid input, removed accounts and failed persistence cannot partially save the batch.

Profiles snapshot individual account settings and support review/apply/launch, replacement/renaming, deletion and JSON import/export. Profile launch uses the existing shared queue; already running clients remain open. Exported profiles contain private links and account metadata, but no authentication credentials.

The Recovery page shows reasons, retry countdowns and policy status, with Retry now for eligible scheduled retries and guided same-account browser repair. Fallback policies are Allow public (the previous default), Stay on destination, and Pause and notify; only definitive unavailability activates them. Rate-limit cooldowns, ownership checks and authentication pauses remain enforced.

Persistent history keeps 1,000 safe events with account filters. Support-report export uses allowlisted fields and excludes credentials, private links, trackers and raw errors/logs. Ten whole-file DPAPI backups support automatic previous-state/pre-upgrade protection, manual snapshots, validated stopped-account restore and offline corruption recovery. Restored accounts stay stopped. Backups remain bound to the Windows user.

Manual update discovery uses a configured public GitHub repository, stable numeric version tags and matching package/checksum links. It does not auto-install or execute downloads, and no publisher repository is assumed.

The Windows ZIP includes the self-contained desktop, WebView2Loader.dll, notices and AGENTS.md. Corresponding source and SHA256 checksums are included. Existing artifacts are not replaced.

Automated checks and real-client limitations are recorded in VALIDATION.md. Live multi-account launch, end-to-end Roblox recovery, real Discord delivery and 10/50-client resource measurements remain required before claiming those integrations are verified. This artifact does not claim those checks have passed.
