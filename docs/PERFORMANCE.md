# Performance

Total usage includes the Rust supervisor and WinUI desktop. Measure Roblox clients and browser sign-in separately.

```powershell
.\scripts\benchmark.ps1 -ProcessId PID -Seconds 30 -Output idle.json
```

Repeat with 0, 10, and 50 active clients, measuring both app processes.

An older headless 0.4.0 sample recorded 14.6 MiB working set, 2.1 MiB private memory, and 0.0% CPU at the counter's resolution over 15.24 seconds. It excludes the current desktop and is not a performance comparison. [Raw sample](idle-benchmark.json).

Current total app usage has not been benchmarked.
