# Resource usage

The Rust engine has two async worker threads, shared discovery, batched log ownership queries, and incremental bounded log reads. The WinUI 3 desktop runs in a separate .NET process and uses native controls. WebView2 is used only for browser sign-in. The self-contained desktop increases download size and memory compared with the older Win32 interface; measure both `rolauncher` and `RoLauncher.Desktop` for total application resource usage.

Use `scripts\benchmark.ps1 -ProcessId PID -Seconds 30 -Output idle.json` against the running supervisor. It samples OS CPU/memory counters only. Repeat with 10 and 50 active accounts; measure Roblox clients and temporary WebView2 login processes separately. No performance advantage is claimed without comparative measurements.

Previously measured 0.4.0 supervisor baseline (headless, no accounts, 15.24 seconds): **14.6 MiB working set, 2.1 MiB private memory, 0.0% recorded machine CPU** at the sampled counter resolution. This excludes the new WinUI process and does not predict total Roblox resource usage or demonstrate an advantage over C#.
