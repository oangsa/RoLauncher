# Development

## Build and test

For the Linux development port, see [Linux prerequisites, builds and parity acceptance](LINUX.md). Windows continues to use native WinUI and the checks below.

Use Windows with Rust 1.99.0, .NET 8 SDK, Python 3.13, and Inno Setup 6.7+. For MSVC builds, install Visual Studio C++ Build Tools and the Windows SDK. Dependencies are pinned in the lockfiles.

```powershell
.\scripts\build.ps1
.\scripts\test.ps1
cargo clippy --locked --all-targets -- -D warnings
.\scripts\test-installer.ps1
```

Build output: `dist\rolauncher`. Tests use simulated accounts and isolated apps. UI previews go in `target\ui-preview`.

## Git flow

Remote: `git@github.com:oangsa/RoLauncher.git`.

1. Branch from `dev` as `feature/description`.
2. Open a PR into `dev` and wait for CI to pass.
3. Promote `dev` through a PR into `main`, using a merge commit.

Recommended branch protection: require PRs, **Branch flow**, and **Windows checks** on dev/main; disable force pushes and deletion.

## Releases

Choose the upcoming version from the latest published GitHub stable release with `python scripts/prepare-release.py KIND`. This defaults to a local `MAJOR.MINOR.PATCH-dev.<12 random hex digits>` preview. Use `--base 1.3.0` for an explicitly verified offline baseline.

| KIND | Version change |
| --- | --- |
| `feature` | Next MINOR, PATCH reset to zero |
| `fix` | Next PATCH |
| `major` | Next MAJOR, MINOR/PATCH reset to zero |

Local `scripts/build.ps1`, `scripts/build-linux.sh`, and `scripts/package.ps1` refresh the random identity once per build/package while keeping the same upcoming numeric target. Cargo, the lockfile, README, runtime strings, desktop informational version, installer label, source and filenames share that identity. Direct Cargo/desktop builds reuse the current identity; run `python scripts/versioning.py` first for a new local build. `-UseCurrentVersion` (or Linux `--use-current-version`) preserves the identity during checks, nested builds and CI.

For an intentional stable release, run `python scripts/prepare-release.py KIND --stable`, review the consolidated notes, then package with `scripts/package.ps1 -UseCurrentVersion`. Publishing scripts reject local dev identities. Stable versions use the numeric rules below and bump once for the full release, not for local iterations.

Follow [AGENTS.md](../AGENTS.md), including explicitly requested versions. Finish the generated release notes and changelogs, then run `python scripts/check-release.py`. Documentation-only edits do not require a new app release.

Main CI tests and publishes an installer, portable ZIP, source ZIP, and SHA256 checksums. Existing published versions are preserved. An interrupted draft can resume only at the same commit.

For local packaging:

```powershell
.\scripts\package.ps1
python scripts/verify-package.py --smoke-shell
```

Use `-OutputRootDirectory FOLDER` and verifier `--dist-directory FOLDER` for another output folder. Existing artifacts are never overwritten. The installer is unsigned.

[Validation](VALIDATION.md) · [Performance](PERFORMANCE.md) · [API](API.md)
