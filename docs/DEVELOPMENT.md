# Development

## Build and test

Windows: Rust 1.99.0, .NET 8 SDK, Python 3.13, Inno Setup 6.7+, Visual Studio C++ Build Tools, Windows SDK.
Dependency versions locked. Linux: [setup/checks](LINUX.md).

```powershell
.\scripts\build.ps1
.\scripts\test.ps1
cargo clippy --locked --all-targets -- -D warnings
.\scripts\test-installer.ps1
```

Output: `dist\rolauncher`; previews: `target\ui-preview`. Tests use separate data and simulated accounts.

## Git flow

Remote: `git@github.com:oangsa/RoLauncher.git`.

1. Branch from `dev` as `feature/description`.
2. PR into `dev`; pass CI.
3. PR into `main`; use merge commit.

Recommended dev/main protection: require PRs, **Branch flow**, **Windows checks**; block force pushes/deletion.

## Releases

Follow [AGENTS.md](../AGENTS.md). Docs-only edits need no app release.

| KIND | Version change |
| --- | --- |
| `feature` | MINOR; reset PATCH |
| `fix` | PATCH |
| `major` | MAJOR; reset MINOR/PATCH |

1. Run `python scripts/prepare-release.py KIND` from latest published stable. Offline: explicitly verify `--base VERSION` first.
2. Local builds/packages use fresh `MAJOR.MINOR.PATCH-dev.<12 random hex digits>`; keep numeric target fixed.
3. Direct Cargo/desktop builds reuse identity; run `python scripts/versioning.py` before a new local build.
4. Checks/nested builds/CI: `-UseCurrentVersion`, or Linux `--use-current-version`.
5. Match manifest, lockfile, README, runtime/desktop versions, installer labels, source, filenames.

Authorized stable release:

1. `python scripts/prepare-release.py KIND --stable`.
2. Finish one release-note file and both changelogs.
3. `python scripts/check-release.py`.
4. `scripts/package.ps1 -UseCurrentVersion`.

Publishing rejects dev versions. Main CI tests/packages installer, portable/source ZIPs, SHA256 checksums; publishing follows [governance](RELEASE-GOVERNANCE.md).
Never replace published artifacts. Interrupted drafts require same commit.

## Local packaging

```powershell
.\scripts\package.ps1
python scripts/verify-package.py --smoke-shell
```

Other output folder: `-OutputRootDirectory FOLDER`; verifier: `--dist-directory FOLDER`.
Never overwrite existing artifacts. Installer unsigned.

[Validation](VALIDATION.md) · [Performance](PERFORMANCE.md) · [API](API.md)
