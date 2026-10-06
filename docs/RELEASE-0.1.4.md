# RoLauncher 0.1.4

2026-10-05 · Windows x64

- Recognized current-client connection failures, including error 279.
- Allowed the existing reconnect grace period before retrying.
- Ignored unrelated timeouts; connection failure alone did not allow public fallback.

24 regression tests passed. Full live recovery remained unverified.
