# RoLauncher 1.7.4

Upcoming hotfix based on published stable 1.7.3.

## Fixed

- Recover from confirmed game kicks when Roblox logs the native kick reason without numeric error 267.

Recovery retains bounded retries and waits for verified client exit before launching a replacement. Access restrictions, client rejection, and required updates still pause recovery.

[Validation](VALIDATION.md). Native Linux and live Roblox recovery require separate validation.

## Validation

Windows: 153 Rust tests, 98 desktop checks, native WinUI smoke and API bootstrap, formatting, Clippy, and isolated installer/update checks passed. Regression fixtures cover the native kick followed by transport closure, spoofed output rejection, retry limits, and verified exit before replacement.

Release CI rebuilds and verifies Windows and Linux packages from the exact committed source. Live Roblox recovery was not exercised.
