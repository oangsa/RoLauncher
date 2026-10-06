# Validation

## 1.2.0 Linux isolation, update channel and dropdown fixes — 2026-10-06

Windows: 49 Rust tests plus the separate authenticated Rust/WinUI bootstrap fixture passed. Linux in Arch WSL: 50 Rust tests passed, plus the separately invoked Flatpak isolation regression. Clippy passed with warnings denied on both platforms. All 60 desktop model/editor/updater checks passed on Windows and Linux (the Linux net8 test assembly ran on the existing .NET 10 runtime with major roll-forward). Native WinUI and Uno rendering fixtures passed; both measured equal heights for the group dropdown and adjacent buttons. WinUI also verified beta opt-in persistence, labeled beta banners and returning to stable updates. Tests cover authenticated preference storage across restarts, numeric release selection, exact beta download URLs and checksum-verified beta downloads. Formatting and release consistency checks passed.

The disposable Flatpak 1.16.3 fixture reproduced a shared temporary lock between two normal launches with distinct homes. Two launches using the app's built-in command builder acquired the lock independently and had distinct temporary filesystems, runtime directories, IPC and PID namespaces while retaining the host network namespace. Isolation is automatic for every launch and retry; no user-installed adapter or environment flag is needed. The fixture uses a static C probe and never starts Sober or Roblox.

This proves the chosen OS isolation behavior, not concurrent Roblox games or the cause of the reported frozen warning. Native Arch/KDE tests of two accounts, rendering/input/audio, onboarding, per-account status, Stop/Restart and recovery remain required. Host-network socket conflicts remain possible. This record covers local validation; hosted Windows and Ubuntu checks gate release packaging/publication. No published application artifacts are replaced. Published 1.0.0 and 1.1.0 installations can use their stable update check to install 1.2.0 and gain the beta update setting. See [integrated Linux isolation](LINUX.md#experimental-concurrent-launch-isolation).

## 1.1.0 Linux/Sober beta — local checks, 2026-10-06

Windows retains native WinUI. Linux reuses its XAML and interaction handlers through Uno/Skia. This is a preview, not completion of the required 100% UI/UX parity.

| Environment | Passed local checks |
| --- | --- |
| Windows | 46 Rust tests plus the separate Rust/WinUI bootstrap check; 53 desktop model/editor/updater checks; real WinUI interaction/rendering fixture; isolated installer/update regression checks |
| Arch in WSL, X11 | 47 Rust tests, including authenticated encryption, process identity and shared-log rejection; Clippy with warnings denied; 53 shared desktop checks; three archive/rollback tests |
| Uno Linux shell in WSL | Build and rendering; search/filter/selection; account editor binding, invalid Save rejection, Save/Cancel; bulk editor Cancel; Accounts, Settings, Recovery, Changelog, dialogs and feedback captures |

These checks use simulated accounts. The Secret Service session, WebKitGTK sign-in, current Sober launches/logs/recovery, tray host and desktop file portals remain unverified. The Ubuntu/Arch × GNOME/KDE × X11/Wayland acceptance matrix, font matching, DPI, accessibility and complete Windows parity remain pending. See [Linux acceptance requirements](LINUX.md). This local record predates hosted beta validation. The beta publication workflow also requires Windows regression checks, Ubuntu 24.04 checks and normal packaged-shell startup before publishing a prerelease.

## Historical 1.0.0 validation

Checked on 2026-10-06 for RoLauncher 1.0.0. The following record applies to that release.

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
