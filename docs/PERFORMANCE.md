# Performance benchmarks

Measure the Rust supervisor and GPUI desktop separately from running Roblox clients and temporary WebView2 sign-in processes. Sampling in the Rust Resource Monitor runs on its own 1 Hz background ticker, independent of GPUI frame rate.

## Release GPUI vs. WinUI 3 comparison

Both desktop shells were measured in Release configuration on Windows 11 x64 (`16` logical processors, `168` DPI) against the same synthetic loopback API fixtures (`0`, `10`, and `50` simulated accounts, `10.0s` idle sampling per run).

| Metric | WinUI 3 (v1.9.2 baseline) | GPUI (v2.0.0) | Delta |
| --- | --- | --- | --- |
| Desktop footprint | `209.31 MiB` (`542` files) | `15.55 MiB` (`RoLauncher.Desktop.exe`) | `-92.6%` |
| Cold startup to first window | `6100.2 ms` | `1489.3 ms` | `4.1x` faster |
| Warm startup (`0` accounts) | `1555.4 ms` | `378.9 ms` | `4.1x` faster |
| Warm startup (`50` accounts) | `359.4 ms` | `238.6 ms` | `1.5x` faster |
| Clean `WM_CLOSE` exit (`cold_0`) | `1321.2 ms` | `149.3 ms` | `8.9x` faster |
| Idle RAM (`0` accounts, WS / private) | `169.54 / 67.46 MiB` | `90.26 / 65.77 MiB` | `-46.8%` WS |
| Idle RAM (`10` accounts, WS / private) | `187.82 / 86.28 MiB` | `94.60 / 70.61 MiB` | `-49.6%` WS |
| Idle RAM (`50` accounts, WS / private) | `202.82 / 102.56 MiB` | `100.48 / 78.28 MiB` | `-50.5%` WS |
| Idle CPU (`50` accounts, machine %) | `1.0347%` | `0.3709%` | `-64.2%` |
| Minimized idle CPU (`0` accounts) | `0.5466%` | `0.1464%` | `-73.2%` |
| Live Resource Monitor SSE (`50` accts, 1 Hz) | Not measured separately | `0.1171%` CPU / `90.91 MiB` WS (`12` events) | — |
| Full pair startup (`RoLauncher.exe` + shell) | `3274.8 ms` | `4043.0 ms` | `+768.2 ms` |

Full-pair supervisor steady-state idle remained `0.0098%` CPU (`23.80 MiB` WS, `2.91 MiB` private).

## Historical and backend baselines

- Historical headless `0.4.0`: `14.6 MiB` working set, `2.1 MiB` private memory, `0.0%` CPU over `15.24 s` ([sample](idle-benchmark.json)).
- Phase 1 Windows Resource Monitor debug-test sampler benchmark across disposable `cmd.exe` processes (`5` ticks each, including PDH GPU query setup): `71.09 ms/tick` (`15.44 MiB` WS) at `0` targets, `19.39 ms/tick` (`16.34 MiB` WS) at `10` targets, and `41.68 ms/tick` (`16.51 MiB` WS) at `50` targets.
- Live multi-client Roblox GPU counters and real client launch/recovery churn remain unmeasured outside controlled fixtures.
