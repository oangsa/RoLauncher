# Performance

Measure both Rust supervisor and WinUI desktop; measure Roblox and browser sign-in separately.
Repeat with 0, 10, and 50 clients:

```powershell
.\scripts\benchmark.ps1 -ProcessId PID -Seconds 30 -Output idle.json
```

- Historical headless 0.4.0: 14.6 MiB working set, 2.1 MiB private memory, 0.0% CPU over 15.24 seconds ([sample](idle-benchmark.json)).
- That sample excludes today's desktop; CPU was below counter resolution.
- Phase 1 native Windows resource sampler debug-test benchmark across disposable `cmd.exe` fixtures (5 ticks each, including PDH GPU query setup): 0 processes = 71.09 ms/tick (cold PDH init, 15.44 MiB WS), 10 processes = 19.39 ms/tick (16.34 MiB WS), 50 processes = 41.68 ms/tick (16.51 MiB WS).
- Live Roblox multi-client desktop and GPU load measurements remain unmeasured outside fixtures.
