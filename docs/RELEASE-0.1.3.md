# RoLauncher 0.1.3

Bug-fix release, 2026-10-05, Windows x64.

Fixes a close/exit race that can produce Windows error 5 when Roblox has already terminated. The close path checks the process wait handle before termination and after a failed termination. The engine checks the exact process identity again before pausing recovery.

Recovery resumes after confirmed exit, including the legacy saved close-error pause when no managed PID or tracker remains. Retry counts and timing, the five-failure limit, Stop, and unrelated authentication/permission pauses are preserved. Genuine access denial on a live process remains an error.

Includes the authentication-ticket and current-client PID mapping fixes from 0.1.2. No process-memory access or client modification is used.

Validation: 22 regression tests cover the fixes, including a Windows child process that reproduces TerminateProcess error 5 after exit. Live ticket acquisition and single-client PID discovery were previously verified. Full Roblox multi-account and disconnect-recovery validation remains outstanding.

Upgrade: Exit the previous app through its tray menu, extract the entire release ZIP, and run RoLauncher.exe with WebView2Loader.dll beside it. Existing saved accounts and launch records use the same application data folder and schema.
