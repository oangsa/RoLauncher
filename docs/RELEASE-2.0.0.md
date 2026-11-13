# RoLauncher 2.0.0

Native GPUI desktop rewrite and live system/per-instance resource monitoring for Windows 10 (1809+) and Windows 11 x64.

Free major release based on published stable 1.9.2.

## Highlights

- Replaced the WinUI 3 / .NET 8 shell with a native Rust desktop built on official Zed GPUI (`DirectX 11` / `DirectComposition`).
- Preserved full feature parity across Accounts, Games, Dashboard, Presets, Settings, Discord, Backups, Updates, and Support over the authenticated loopback Rust supervisor (`127.0.0.1`).
- Integrated the Phase 1 Resource Monitor (**Workspace -> Resources**): live `/v1/resources/events` SSE streaming, 60-second CPU/RAM charts, system and managed totals, optional GPU utilization, sortable per-instance metrics, and explicit empty/unavailable/stale states.
- Added Discord `/resources` snapshot cards and `/resources-register` auto-refreshing status embeds alongside `/action` and `/status-register`.
- Added dark, light, and system themes, keyboard shortcuts (`Ctrl+F`, `Ctrl+A`, `Esc`), and reduced-motion-friendly transitions.
- Reduced the packaged desktop binary payload from `209.31 MiB` (`542` files) to `15.55 MiB` (`RoLauncher.Desktop.exe`) with ~`4x` faster cold/warm startup and ~`47-50%` lower working-set memory; see [Performance](PERFORMANCE.md).

## Compatibility and requirements

- Windows 10 version 1809 (`build 17763`) or later and Windows 11 x64 with Direct3D 11 / DirectComposition support; Linux remains discontinued.
- Microsoft Edge WebView2 Runtime remains required only for interactive browser sign-in (`WebView2Loader.dll` is bundled).
- No `.NET` runtime installation is required.
