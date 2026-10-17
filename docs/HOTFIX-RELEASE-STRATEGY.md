# Isolated stable hotfix release lane

Historical record: October 16, 2026 release-rule preparation; **not activated**.
At that point app stayed `1.7.0`: no fix, 1.7.1 metadata, release intent, tag, or release prepared.
Draft PR requires independent security review before merge/activation.

## Verified baseline and separation

| Reference | Verified value |
| --- | --- |
| Latest published stable | `v1.7.0` |
| Stable tag and live main | `4daae1bfc14db1336e1366134367ccb16efb7d4d` |
| Premium dev | `c4d1c73d12dd4a4307871e62db79fe82fda7002a` |
| Draft PR #27 head | `49026dc7a470e1d8ea814a60ec7986a31c656f87` |
| Preparation branch | `codex/hotfix-release-lane`, from stable SHA above |
| Proposed future patch | `1.7.1`, after specific bug approval only |

Live release/branch/tag/PR evidence refreshed before edits. Stale local `main` neither used nor moved.
Premium branches/PR #27 unchanged; Premium work paused for this task.
Unfinished identity/licensing code in `dev` cannot supply stable hotfixes.

Features keep `feature/* → dev → main`; isolated fixes use verified stable history, reviewed release rules, then `hotfix/* → main`.
Post-stable hotfix bases may include reviewed release-rule commits only, never dev app changes.

## Legacy versus guarded behavior

| Area | Stable v1.7.0 | Proposed lane |
| --- | --- | --- |
| Main updates | Successful builds publish automatically | CI only; missing intent means no writes |
| Write credentials | Proposed checkout runs publisher | Separate trusted default-branch publisher; proposed files treated as data |
| Main PR flow | Dev only | Dev unchanged; separate narrow preparation/hotfix check |
| Approval | Main event plus passing builds | Owner-merged fresh intent tied to exact source |
| Hotfix source | No separate policy | Published base, reviewed release-rule history, exact commits/patch, human code review |
| Retry | Old drafts may replace assets | Matching receipt adds missing assets only; preserve published/conflicting bytes |
| Activation | Old automatic publisher present | Literal false guard, separate owner variable, verified controls |

## Selective PR01 audit and port

Reference: dev `c4d1c73d12dd4a4307871e62db79fe82fda7002a`.
Read files individually with `git show`; no dev merge/cherry-pick/full-tree import.

| Component | Decision |
| --- | --- |
| CI/Linux/beta workflows | Remove write publishers; don't save checkout credentials; artifacts identify run attempt; add tests/narrow hotfix checks |
| release-promotion workflow | Separate trusted workflow-run publisher, serialize runs, add hard-disabled guard |
| release_promotion.py | Keep intent/owner/exact-source/check/artifact/Ruleset/archive/retry controls; add schema 2 |
| test-release-promotion.py | All 30 synthetic tests, including unchanged feature/beta rules |
| Publishing entry points/test.ps1 | Shared trusted gate and Windows policy tests |
| prepare-release.py/check-release.py | Read; keep stable bytes unchanged: patch/version checks sufficient |
| release_files.py | Keep stable checksum/payload validator; reuse in trusted publisher |
| package.ps1 | Stable source from exact committed Git tree; dev packaging unchanged |
| Release docs | Replace dev-only assumptions with both source contracts/transition plan |
| PR02 inventory/security tools/identity PoC/provisioning/Premium app code | Exclude; no runtime/dependency/license changes imported |

## Safe main transition

1. Independently review exact preparation head/diff/both policy suites; retain `1.7.0`.
2. Owner audits/cancels queued/running old privileged jobs; forbids pre-transition job/tag reruns.
3. Owner separately approves repository protections below. Keep publisher disabled and variable false/unset.
4. Require PR's Branch flow, Windows, both Linux-platform checks. Merge reviewed PR only; no direct push/bypass.
5. Verify merged main: no stable/beta write job, version change, or intent. Ordinary completion events cannot publish.
6. Prove permissions/digests/unchanged assets/reruns in disposable repository; obtain independent security review.
7. Only later owner-approved activation PR may remove literal `false`. Verify controls/approval before enabling variable.
8. Approve specific bug before hotfix branch, version metadata, executable patch intent.

First main push uses new CI-only workflow; workflow-run publisher stays disabled even with variable already set.
Old queued/rerun jobs keep old code. If owner cannot stop those runs or satisfy PR checks, stop migration.
Never restore automatic publishing or bypass protection.

## Owner settings and activation blockers

Inspection then found `main.protected=false`, zero Rulesets.
Connector could not read repository variables; missing admin access does not mean false/unset.
No settings changed; activation blocked.

1. Audit owner/admins, collaborators, Apps, deploy keys, workflow-edit rights, secrets. Build jobs get no production credentials.
2. Default Actions token: contents-read. No fork-PR write tokens/secrets.
3. Active branch Rulesets: `refs/heads/main`, `refs/heads/dev`; require PR/up-to-date branch; block force-push/deletion.
4. Required checks: Branch flow, Windows checks, Linux checks and package (Ubuntu 24.04), Linux checks (Ubuntu), Linux checks (Arch); GitHub Actions App ID `15368`.
5. Hotfix promotion uses merge commits. Empty branch bypass lists; separate review for workflow/release-rule changes.
6. Stable tag Ruleset `refs/tags/v*`: block update/deletion, empty bypass list, allow publisher's initial tag creation.
7. Separate beta tag Ruleset `refs/tags/beta-v*`: restrict creation/update/deletion. Creation bypass only sole trusted owner/admin role (`RepositoryRole=5`, `always`).
8. Separately enable/test native release immutability. Preserve old releases/assets; record owner verification.
9. Record effective Ruleset IDs/scope/checks/integration IDs/strictness/bypasses/reviewer/date. Recheck after access/control changes.
10. Prove runtime token can read evidence without Administration/write secrets. Then separately approve activation PR and `RELEASE_PROMOTION_ENABLED=true`.

Hotfix checks reject missing branch/tag bypass evidence; PR01 feature mode instead accepts explicit admin confirmation.
[Reading bypass lists requires ruleset write access](https://docs.github.com/en/rest/repos/rules#get-a-repository-ruleset); minimal publishing credentials may not suffice.
**Still blocked:** prove a minimal-access evidence path in disposable repository. Never add admin token or weaken guard to enable publishing.

## Future patch procedure — commands not executed

Only after approved rules migration, verified unchanged stable, specific bug approval:

```powershell
git fetch origin main --tags
git rev-parse v1.7.0
git log --oneline v1.7.0..origin/main
git diff --name-status v1.7.0..origin/main
git switch --create hotfix/1.7.1 origin/main
```

Require latest stable `v1.7.0`, resolving to verified SHA above.
Review every later main commit as release-rule-only; unexpected stable/main changes mean stop/reassess.
Never branch from/merge dev or reuse a Premium branch.

1. Approved fix/tests only. Check prerequisites before selective backport/cherry-pick.
2. `python scripts/prepare-release.py fix`: target `1.7.1`, fresh local dev identities per AGENTS.md.
3. Authorized release only: `python scripts/prepare-release.py fix --stable`; finish README, both changelogs, `docs/RELEASE-1.7.1.md`.
4. Verify Cargo/lockfile, desktop informational version, artifact references. Desktop runtime uses full existing build version.
5. Reviewed hotfix PR: Branch flow prints exact commits/paths/patch hash. Missing intent cannot publish.
6. Owner reviews full patch/files/callers/tests/dependencies; records approved bug, prerequisites, Premium exclusion.
7. Final commit adds schema-2 intent only: preceding reviewed SHA, exact main base. Never edit/reuse UUID path.
8. Pass exact-source checks. Owner merges unchanged reviewed tree onto approved base; activated publisher checks independently.
9. Verify published receipt/source/checksums/package. Stable must update dev builds of same numeric target.

[Release governance](RELEASE-GOVERNANCE.md) defines full intent/hash contract.
Versions follow latest published stable, never dev identities/unfinished features.
Original rules preparation had no shipped app change, so no release-note entry.

## Backport, rollback and recovery

1. After publishing, create separate dev-targeted branch for approved app fix/tests only.
2. Review prerequisites; cherry-pick individually; resolve/test against dev separately.
3. Never blindly merge hotfix metadata, intent, or release-rule history into Premium dev.

Emergency: disable Protected release promotion, cancel running/queued publishers, remove activation variable or set false.
Keep CI-only workflows; revert faulty rules through reviewed PR. Never restore old publishers.
Keep drafts/tags/receipts/artifacts for investigation; never delete/move published versions to roll back.

Rerun trusted publisher only with same source run/attempt to finish matching interrupted release.
Changed main/attempt, expired artifacts, conflicting tags/assets, or stale intent need owner investigation and fresh approval.
Published corrections need new patch; leave mismatched-receipt drafts set aside.

## Independent security-review checklist

- [ ] Verify live stable/tag/main/exact Draft PR head; compare every changed file with stable.
- [ ] Confirm Premium branches/PRs, runtime, dependencies, version, published history unchanged.
- [ ] Separate PR admission from publishing approval; ordinary main/hotfix merges cannot approve themselves.
- [ ] Check both intent schemas, UUID freshness, merge parents/tree, full files, dependency/prerequisite review statements.
- [ ] Check each commit/net diff for hidden/reverted Premium changes or unrelated refactors.
- [ ] Trace build-token separation, trusted checkout, redirects, archive limits, source hashes, payload checksums.
- [ ] Test failed/missing Windows/Linux evidence, old/replayed events/tags, races, interrupted uploads, consumed receipts.
- [ ] Check old queued/rerun/tag jobs blocked; migration disabled, without 1.7.1 metadata/intent.
- [ ] Independently inspect active Rulesets/bypasses, token permissions, native immutability, activation proof.
- [ ] Keep disabled until real minimal-access/digest/immutable-draft proof passes. No production release for testing.

Still trust GitHub/hosted runners, owner/default-branch rules, pinned checkout.
Pattern checks/review statements cannot prove arbitrary code behavior or reproducible builds.
API check/write races remain; final rechecks/native protections reduce risk.

## Validation evidence

| Local check | Result |
| --- | --- |
| Promotion tests | 30 passed; feature/beta approval and replay rules retained |
| Hotfix policy tests | 14 passed, including 27 rejection cases and migration/source/history checks |
| Beta/archive tests | 6 beta, 3 Linux update/archive tests passed |
| Version/check-release | Passed; preparation stays 1.7.0 |
| Rust | 75 passed; WinUI connection test initially ignored, passed separately |
| Desktop/WinUI | 74 checks, UI tests, Rust/WinUI connection passed |
| Formatting/Clippy | Passed; locked all-target Clippy, warnings denied |
| Workflow/Python | Four workflows: actionlint 1.7.12 passed; Python compiles; shellcheck unavailable |
| Linux Sober adapter locally | Windows rejects POSIX entry point (`WinError 193`); native hosted Linux required |
| Production publication/settings | Never run/changed; synthetic API tests only |

New approval/merge-tree/per-commit/source-byte/branch/hidden-bypass tests failed before guards, then passed.
Draft PR's current-head checks must supply native Linux/Windows packaging and exact hosted checkout evidence.
Local tests do not prove real token permissions, artifact-digest availability, or immutable-release completion.

## Exact preparation file list

- `.github/workflows/ci-release.yml`
- `.github/workflows/linux.yml`
- `.github/workflows/beta-release.yml`
- `.github/workflows/release-promotion.yml`
- `scripts/release_promotion.py`
- `scripts/hotfix_policy.py`
- `scripts/branch-flow.py`
- `scripts/test-release-promotion.py`
- `scripts/test-hotfix-policy.py`
- `scripts/publish-release.py`
- `scripts/publish-beta.py`
- `scripts/test.ps1`
- `scripts/package.ps1`
- `docs/RELEASE-GOVERNANCE.md`
- `docs/HOTFIX-RELEASE-STRATEGY.md`
