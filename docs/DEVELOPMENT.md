# Development and releases

The SSH remote is `git@github.com:oangsa/RoLauncher.git`. Start a `feature/description` branch from `dev`, open a PR into `dev`, then promote `dev` through a PR into `main`. CI checks branch direction, Rust tests/format/clippy, desktop tests, the real WinUI fixture and bootstrap bridge, isolated installation/update handoff, and the packaged Windows app.

Before a future release, run `python scripts/prepare-release.py feature` (MINOR), `fix` (PATCH), or `major` (MAJOR) once on a feature branch. Replace the generated draft notes in CHANGELOG.md, desktop/CHANGELOG.txt and docs/RELEASE-VERSION.md. Respect an explicitly requested version. `python scripts/check-release.py` checks version consistency. Runtime versions derive from Cargo. Version 1.0.0 is the first public installer release requested by the owner; previous local archives are preserved.

Main CI packages `rolauncher-vVERSION-setup-x64.exe`, the portable ZIP, corresponding source ZIP and SHA256SUMS. A separate job with scoped `contents: write` permissions verifies checksums, creates a draft GitHub release at the main commit, uploads all four files, verifies uploads and publishes `vVERSION` with the reviewed versioned notes. No personal access token or additional release secret is required. Interrupted drafts can resume only for the same commit. Published versions and existing tags pointing to other commits are never replaced. A manual workflow run on main can retry publication.

Branch protection should require pull requests, `Branch flow` and `Windows checks` on both dev and main, with force pushes and deletion disabled. A single-owner repository may use zero required approving reviews while still requiring a PR and passing CI. Main accepts promotion PRs from dev only. Avoid squash-merging dev into main: retain shared ancestry using merge commits.

Windows builds require Rust 1.99.0 (MSVC plus the C++ build tools and Windows SDK, or the workspace GNU toolchain), .NET 8 SDK, Python 3.13 and Inno Setup 6.7+. The hosted Windows runner supplies Inno Setup. `scripts/environment.ps1` supports optional workspace tools. Use `scripts/test.ps1`, `scripts/test-installer.ps1`, then `scripts/package.ps1` and `python scripts/verify-package.py --smoke-shell`. For another local output parent use `scripts/package.ps1 -OutputRootDirectory dist/installer-release` and pass the same folder with verifier `--dist-directory`; existing artifacts are never overwritten.

The updater fetches public release metadata without Roblox credentials. Only stable newer releases from `oangsa/RoLauncher` can be downloaded by the desktop. Downloads are capped, verified against SHA256SUMS and rechecked before installation. Relaunch creates a DPAPI backup, starts a hidden two-phase helper, and closes RoLauncher itself. The helper holds original process exit handles, waits for both app processes, runs the per-user installer silently, and restarts with the same data directory and API port. It never closes Roblox clients. The installer keeps data outside its install folder and refuses downgrades. Portable installations can also update in place when their directory is writable.

The app and installer are not code-signed. SHA256 protects against corruption and mismatched downloads; it depends on the integrity of the official GitHub publisher. Windows may show an unknown-publisher prompt for a new download. Signing requires an owner-provided certificate or signing service.

## Build and test

Recommended: install Rust 1.99.0 for `x86_64-pc-windows-msvc`, Visual Studio Build Tools with Desktop development with C++, .NET 8 SDK, Python 3.13 and Inno Setup 6.7+. The pinned Windows App SDK and Windows SDK build tools restore from NuGet. Run:

```powershell

.\scripts\build.ps1

.\scripts\test.ps1

```

Build output is `dist\rolauncher`, including the WinUI shell in `desktop`. The scripts also support workspace-local Rust/GCC and .NET toolchains, without changing machine-wide environment variables. `Cargo.lock` and the desktop package lock files pin dependencies. Run `cargo fmt` after Rust source edits. `scripts\test.ps1` runs Rust tests, desktop model/editor checks and the actual WinUI shell against a loopback fixture with simulated accounts. It renders previews under `target\ui-preview`; it does not use saved accounts, launch Roblox or send Discord messages.

After tests and `cargo clippy --locked --all-targets -- -D warnings` pass, `scripts\package.ps1` builds versioned Windows/source ZIPs and SHA256 checksums. It refuses to replace existing release artifacts.

See [Resource usage](PERFORMANCE.md) for measurement instructions.
