# RoLauncher 0.5.0

Replaces the default desktop with WinUI 3 and a minimal black-and-white workspace. The Rust launcher, encrypted account store and recovery supervisor remain responsible for account operations.

- Rounded cards, restrained typography, black primary buttons, white navigation tabs, modern native inputs and checkboxes, and colored status badges with text labels.
- Clear account table and details editor, empty state, per-account bulk-action feedback, and scrollable pages that accommodate smaller windows and Windows DPI scaling.
- Retains cookie/file import, isolated browser sign-in, 600 ms autosave of valid edits, incomplete in-memory destination drafts, multi-selection and Ctrl+A, bulk Start/Stop/Restart/Remove and mixed automatic-rejoin settings.
- Retains encrypted Discord webhook settings, test delivery and recent activity. Webhook and cookie inputs remain masked.
- Window close hides to the system tray. Tray Open restores the desktop; Exit leaves Roblox clients running and stops supervision. The shell closes if its Rust supervisor exits.
- The Rust supervisor reserves its loopback API port before starting the desktop. Its API token is passed over an inherited anonymous pipe, never process arguments, a token file or diagnostics. HTTP redirects and proxies are disabled in the desktop client.
- Adds authenticated `POST /v1/login` for isolated WebView2 sign-in. The existing API authentication and browser-origin rejection also apply to this route.

The Windows ZIP includes the complete self-contained WinUI/.NET desktop, compiled XAML resources, WebView2Loader.dll and dependency license notices. The source ZIP includes Rust, XAML/C#, both package lock files, checks, scripts and AGENTS.md. SHA256 checksums accompany both ZIPs. Extract the entire ZIP and run RoLauncher.exe; keep its desktop folder intact.

Verification uses simulated accounts and a local HTTP fixture. It does not open real Roblox clients or send real Discord webhooks. Existing live compatibility limits still apply. Build requirements and validation details are in README.md and VALIDATION.md.
