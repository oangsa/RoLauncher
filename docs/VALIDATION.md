# Validation

Checked on 2026-10-06 for RoLauncher 1.0.0.

## Passed automated checks

| Area | Coverage |
| --- | --- |
| Rust | 46 tests; account storage, encryption, recovery, profiles, backups, API access, and legacy data selection |
| Desktop | 52 checks; editors, filters, selection, update validation, downloads, and cancellation |
| Real app window | Navigation, account workflows, update buttons, rendering, and Rust/WinUI bootstrap |
| Installer | Checksum rejection, exit waiting, installation, relaunch, and data-path preservation |
| Packages | Matching source/version, required runtime/docs/licenses, ZIP integrity, SHA256, and app startup |
| Code quality | Formatting and Clippy with warnings denied |

Hosted Windows CI passed on the feature and promotion PRs. Tests use isolated data and simulated accounts; they do not prove real Roblox or Discord behavior.

## Limited live evidence

Earlier checks verified ticket acquisition and process mapping for one observed Roblox client. Recognized disconnect messages were also checked against existing logs. This does not establish multi-account or complete recovery compatibility.

## Still needs real-client testing

- Launch two accounts and confirm each process and connection status.
- Check disconnect recovery, teleports, private destinations, and fallback policies.
- Check expired-session repair, rate limits, and network loss/recovery.
- Send real Discord notifications.
- Measure total app resource usage with 0, 10, and 50 clients.
- Confirm updating between two real public versions.

Missing or ambiguous client information must produce Unknown, Needs attention, or Unsupported. RoLauncher must not use memory inspection or client modification to work around it.

The installer is unsigned. See [release notes](RELEASE-1.0.0.md) and [performance notes](PERFORMANCE.md).
