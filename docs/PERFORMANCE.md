# Performance

Measure Rust supervisor, WinUI desktop, Roblox, and sign-in browser separately.
Repeat with 0, 10, and 50 clients:

```powershell
.\scripts\benchmark.ps1 -ProcessId PID -Seconds 30 -Output idle.json
```

- Historical headless 0.4.0: 14.6 MiB working set, 2.1 MiB private memory, 0.0% CPU over 15.24 seconds ([sample](idle-benchmark.json)).
- No desktop included; CPU below measurement resolution. Current total app usage still unmeasured.

## 1.7.1 hotfix measurements

October 17, 2026; Windows, 16 logical processors; compared with 1.7.0.
[Raw samples](hotfix-1.7.1-benchmark.json): three optimized test runs plus separate 20-second runs without UI.
These measure parts of the app, not overall speed or live Roblox recovery.

| Test, middle result of three runs | 1.7.0 | 1.7.1 | Change |
| --- | ---: | ---: | --- |
| Read cached 3,700,052-byte log 50 times | 647.32 ms | 44.91 ms | 93.1% less time |
| Reads before finding latest error 273 | 15 | 1 | 93.3% fewer reads |
| Check 50 Windows process handles | 1,454.61 µs | 9.84 µs | 99.3% less time |
| Rank 500 servers | 14.65 µs | 14.99 µs | 2.3% more time |
| Rank 10,000 servers | 296.82 µs | 285.56 µs | 3.8% less time |

- Ranking excludes requests, page downloads, JSON reading. Results varied; no overall query-speed gain claimed.
- 500 test rows contain 480 usable servers at 48 player counts. Keeping one per count means 90% fewer entries, not measured RAM savings.
- Old query missed page-six match; new local HTTP test finds it. More pages mean more requests/time.
- Exit test compares repeated open/time/path/wait calls with reused verified handles on a test process. Excludes discovery, logs, saving.
- Log test copies old 256 KiB reading behavior; same cached synthetic log/parser for both versions.

| Idle, no UI/accounts/clients | 1.7.0 | 1.7.1 development |
| --- | ---: | ---: |
| Machine CPU | 0.0188% | 0.0189% |
| Mean working set | 20.11 MiB | 21.05 MiB |
| Mean private memory | 2.03 MiB | 2.07 MiB |

Idle memory rose slightly; CPU effectively unchanged at this resolution.
Excludes WinUI, sign-in browser, Roblox, active RAM checks. No loaded-PC or total-app saving proven.

## Additional error-audit comparisons

Five optimized runs, October 17, 2026. Compare pre-audit `1.7.1-dev.be7b3852f8fb` with `1.7.1-dev.ee9b80524a47`.
These are additional development comparisons, not stable-release or whole-app speed claims.
[Raw samples](hotfix-1.7.1-audit-benchmark.json).

| Workload, median of five | Before | After | Time change |
| --- | ---: | ---: | ---: |
| Parse 2 million output lines containing a fake channel | 126.96 ms | 40.64 ms | 68.0% less |
| Parse 2 million unrelated asset lines | 115.12 ms | 19.94 ms | 82.7% less |
| Parse 2 million timestamped game-output lines | 135.78 ms | 44.21 ms | 67.4% less |
| Parse 2 million error-273 disconnects | 262.90 ms | 234.29 ms | 10.9% less |
| Parse 2 million confirmed connections | 444.87 ms | 387.22 ms | 13.0% less |
| Parse 2 million error-279 join failures | 212.00 ms | 143.01 ms | 32.5% less |
| Validate 50 simulated sessions | 1,537.16 ms | 402.16 ms | 73.8% less |

- Parser comparisons call the production parser directly. Early channel rejection avoids unrelated work; no log text reaches diagnostics.
- Import replay extracts production method with simulated validation/save adapters. Each request awaits 20 ms; Windows timer scheduling affects actual delay.
- Import excludes HTTP, encryption and disk saves. Ordered results, duplicate accounts, failures, cleared inputs and four-request ceiling checked separately.
- Four validations can use more short-lived request memory than sequential validation. No peak-RAM saving or live import-speed claim.

### Final build idle CPU and memory

Separate 20-second runs, 16 logical processors; empty databases, no desktop or Roblox clients.
Candidate: `1.7.1-dev.ee9b80524a47`. [Raw samples](hotfix-1.7.1-audit-benchmark.json).

| Metric | Stable 1.7.0 | Final development build |
| --- | ---: | ---: |
| Machine CPU | 0.0000% | 0.0193% |
| Mean working set | 20.07 MiB | 21.74 MiB |
| Mean private memory | 2.01 MiB | 2.05 MiB |

CPU differences are below useful comparison resolution. Working set rose 1.66 MiB; private memory rose 44 KiB.
No idle-resource saving proven. Active imports, UI and many Roblox clients remain unmeasured.

## Reproduce and limits

```powershell
.\scripts\benchmark-hotfix.ps1 -Baseline dist\rolauncher-v1.7.0\RoLauncher.exe -Candidate target\release\RoLauncher.exe
```

- Build candidate with current dev version first. Keep WebView2Loader.dll beside it.
- Script runs tests three times, then measures separate no-UI processes with empty temporary data. Real accounts untouched.
- Busy logs: read latest 256 KiB; drop partial first line. Older events may be skipped.
- Windows RAM checks accept recent timestamped UTF-8 log messages. Per check: 64 KiB scratch space, up to 2 MiB read, 10 ms target.
- One RAM check globally; each client waits at least 30 seconds between attempts. Unreadable memory skipped; scratch bytes cleared.
- Paging may exceed time target; exit checks keep running separately. Missing buffers/incomplete scans can miss events.
- References: [read-only memory access](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-readprocessmemory), [memory-region queries](https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualqueryex), [Roblox game APIs](https://create.roblox.com/docs/cloud/reference/domains/games).
