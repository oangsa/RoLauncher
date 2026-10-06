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

Bump once per release with `python scripts/prepare-release.py KIND`:

| KIND | Version change |
| --- | --- |
| `feature` | Next MINOR, PATCH reset to zero |
| `fix` | Next PATCH |
| `major` | Next MAJOR, MINOR/PATCH reset to zero |

Follow [AGENTS.md](../AGENTS.md), including explicitly requested versions. Finish the generated release notes and changelogs, then run `python scripts/check-release.py`. Documentation-only edits do not require a new app release.

Main CI tests and publishes an installer, portable ZIP, source ZIP, and SHA256 checksums. Existing published versions are preserved. An interrupted draft can resume only at the same commit.

For local packaging:

```powershell
.\scripts\package.ps1
python scripts/verify-package.py --smoke-shell
```

Use `-OutputRootDirectory FOLDER` and verifier `--dist-directory FOLDER` for another output folder. Existing artifacts are never overwritten. The installer is unsigned.

[Validation](VALIDATION.md) · [Performance](PERFORMANCE.md) · [API](API.md)
