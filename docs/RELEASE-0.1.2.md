# RoLauncher 0.1.2

Released 2026-10-05 for Windows x64.

- Fixed authentication-ticket requests receiving HTTP 415 by sending a JSON body with the required content type, including on CSRF retry.
- Added request stage and HTTP status to sanitized Roblox request errors.
- Fixed PID discovery for clients that retain the roblox-player URI in their command line. Legacy -b tracker arguments remain supported; ambiguous trackers are rejected.
- A verified late PID match resolves the ownership-timeout pause without clearing unrelated authentication or permission failures.
- Packages include WebView2Loader.dll. Builds also place it beside the raw executable and fail packaging if the loader is missing.

Validation: 20 tests passed for the fixes; live ticket acquisition and PID discovery were verified. Full multi-account launch and disconnect-recovery compatibility remain unverified. No Roblox process-memory access or client modification is used.

To upgrade, use Exit in the old application's tray menu, leave Roblox clients open, and launch the new executable. Saved accounts and launch records use the existing application data folder and schema. Keep WebView2Loader.dll beside the executable.

## Close/exit race hotfix

Fixes false Windows error 5 when a managed process exits while the supervisor is closing it. Confirmed exit is treated as a successful close; pending recovery can resume without counting another failure. The legacy persisted close error is recovered on restart when no managed PID/tracker remains. Live access denial and unrelated attention states remain paused. See VALIDATION.md for evidence and tests.
