# Development

## Build and test

Windows prerequisites: Rust 1.99.0, .NET 8 SDK, Python 3.13, Inno Setup 6.7+, Visual Studio C++ Build Tools, and Windows SDK.
Dependencies are pinned. Linux: [setup and checks](LINUX.md).

```powershell
.\scripts\build.ps1
.\scripts\test.ps1
cargo clippy --locked --all-targets -- -D warnings
.\scripts\test-installer.ps1
```

Output: `dist\rolauncher`; UI previews: `target\ui-preview`. Tests use isolated apps and simulated accounts.

## Git flow

Remote: `git@github.com:oangsa/RoLauncher.git`.

1. Branch from `dev` as `feature/description` or `codex/description`.
2. Open a PR into `dev`; pass CI.
3. Promote `dev` into `main` through a PR with a merge commit.

Required publication controls and all required Windows/Linux checks: [release governance](RELEASE-GOVERNANCE.md).

## Releases

Follow [AGENTS.md](../AGENTS.md). Documentation-only edits need no app release.

| KIND | Increment |
| --- | --- |
| `feature` | MINOR; reset PATCH |
| `fix` | PATCH |
| `major` | MAJOR; reset MINOR/PATCH |

- Run `python scripts/prepare-release.py KIND` against the latest published stable. Offline: use an explicitly verified `--base VERSION`.
- Local versions use `MAJOR.MINOR.PATCH-dev.<12 random hex digits>`.
- Build/package scripts refresh the identity; the upcoming numeric target stays fixed.
- Cargo/desktop builds reuse it. Run `python scripts/versioning.py` before a new direct local build.
- Use `-UseCurrentVersion` (Linux: `--use-current-version`) for checks, nested builds, and CI.
- Keep manifest, lockfile, README, runtime/desktop versions, installer labels, source, and filenames consistent.

For an intentional stable release:

1. Run `python scripts/prepare-release.py KIND --stable`.
2. Finish consolidated release notes and both changelogs.
3. Run `python scripts/check-release.py`.
4. Package with `scripts/package.ps1 -UseCurrentVersion`.

Main CI checks and packages every merge; stable publication requires a fresh, owner-merged [Release Intent](RELEASE-GOVERNANCE.md#release-intent-contract).
Publication stays disabled until separately approved controls and activation are complete.
Published artifacts stay unchanged; matching interrupted drafts can resume without replacing assets.

## Local packaging

```powershell
.\scripts\package.ps1
python scripts/verify-package.py --smoke-shell
```

Alternate output: `-OutputRootDirectory FOLDER` and verifier `--dist-directory FOLDER`.
Existing artifacts are never overwritten. The installer is unsigned.

[Validation](VALIDATION.md) · [Performance](PERFORMANCE.md) · [API](API.md)
