# Stable hotfix release flow

Status snapshot: 2026-10-19. PR #32 merged governance checks; PR #29 merged the v1.7.1 hotfix to `main` at `9cafa02f36dd32d1e44944fc4e6ac469a27d1852`. Latest published stable remains `v1.7.0`. No `v1.7.1` intent, tag or Release exists. Schema-3 recovery support is being prepared locally and still needs review and merge.

The publisher remains hard-disabled by the literal `false` in `.github/workflows/release-promotion.yml`. PR #29 lacks its schema-2 intent, so it cannot authorize publication under the current contract. A proposed schema-3 path binds a new intent-only PR to PR #29's exact owner-merged patch. Do not create a tag or Release manually; recovery support, activation and owner-signed controls remain required.

## Release sequence

1. **Choose target.** Read GitHub's latest published stable Release and resolve its tag to a commit. For a patch, use `python scripts/prepare-release.py fix` during development. Use `python scripts/prepare-release.py fix --stable` only for an intentionally approved stable release. Never select from a development build.
2. **Isolate hotfix.** Create `hotfix/<version>` from verified `origin/main`. Confirm every commit after the stable baseline is reviewed governance-only history. Never branch from or merge `dev`.
3. **Prepare candidate.** Make only approved bug fixes and required tests. Align `Cargo.toml`, root package in `Cargo.lock`, `README.md`, both changelogs and `docs/RELEASE-<version>.md`. Run required checks and `python scripts/check-release.py`.
4. **Review exact patch.** Open PR from `hotfix/<version>` to `main`. Branch flow reports full commit IDs, changed paths and canonical patch SHA256. Owner reviews source blobs, call paths, tests, dependencies, cherry-pick prerequisites and Premium exclusion. CI must pass on final PR head.
5. **Add release intent.** After review, add one fresh `.github/release-intents/<UUIDv4>.json` as final commit in same PR. Use schema 2, `source: hotfix`, `channel: stable`, exact baseline/version/base/reviewed SHA, ordered commits, sorted paths, patch digest and semantic review. Final commit must contain only this new intent file, parent must equal `reviewed_sha`; do not edit or reuse intent UUID. See [release governance](RELEASE-GOVERNANCE.md) for exact fields.
6. **Merge by owner.** Recheck CI after intent commit. Owner merges PR with merge commit. Publisher requires merge parents `[authorized base, reviewed PR head]` and merge tree identical to reviewed head. Squash/rebase merge fails closed.
7. **Verify source runs.** On merged `main` SHA, require successful `CI and release` jobs (`Branch flow`, `Windows checks`, `Linux checks and package (Ubuntu 24.04)`) and `Linux development port` jobs (`Linux checks (Ubuntu)`, `Linux checks (Arch)`). Verify current attempts, source SHA, artifact IDs/digests, payload checksums and source ZIP against exact Git tree.
8. **Activate separately.** Publication needs a separately reviewed activation change and explicit Owner authorization. Both workflow guard and `RELEASE_PROMOTION_ENABLED=true` must allow the trusted publisher. Verify branch/tag Rulesets, publisher permissions, enrolled signer, evidence inbox, disclosure approval and native Release immutability first. Details: [owner evidence protocol](OWNER-EVIDENCE.md).
9. **Attend both gates.** After activation, publisher performs read-only admission and issues a fresh preflight challenge. Owner reviews live controls and signs. Publisher stages only matching tag, Draft Release and verified assets, then issues a separate final challenge. Owner reviews and signs again; publisher rechecks authority and immutability before publishing receipt.
10. **Read back publication.** Verify stable tag, immutable Release, exact `release-promotion.json`, source archive, checksums, package files and final publisher audit artifact. Preserve published bytes; corrections require a new version.

Successful CI, merged source or signed preflight alone never authorizes publication. Reruns need fresh challenges and signatures. On ambiguous state, stop and inspect GitHub; never retry a write blindly.

## Recovery when source merged without intent

PR #29 was owner-merged before an intent was added. Schema 2 correctly rejects retroactive intent because it requires the source patch and intent in the same PR.

1. Merge reviewed recovery support through `codex/hotfix-recovery-governance`, based on the exact current `main`. Its diff must contain governance paths only and preserve the already-merged `1.7.1` package version.
2. After that governance PR merges, create `hotfix-intent/1.7.1` directly on current `main`. The source merge must be an ancestor, with only governance commits between it and the intent PR base; package metadata must remain unchanged.
3. Bind that intent to PR #29, its merge SHA, its original base/head SHAs, exact ordered commits, sorted paths and canonical patch SHA256. Include the original bug, dependency and Premium review.
4. Owner reviews and merges the intent PR with a merge commit. Publisher verifies PR #29's owner merge, tree, patch, the intent PR's exact source parent and its own merge tree.

Any non-governance `main` change, source PR mismatch, changed patch or extra recovery-PR change fails closed. Schema 3 grants no authority until its governance change and intent PR are separately reviewed and owner-merged.

## Historical migration proposal (October 16, 2026)

Status: governance preparation, October 16, 2026; **not activated**.
Application remains `1.7.0`; no bug fix, 1.7.1 metadata, release intent, tag or release is prepared.
The Draft PR must receive independent security review before any merge or activation.

## Verified baseline and separation

| Reference | Verified value |
|---|---|
| Latest published stable | `v1.7.0` |
| Stable tag and live main | `4daae1bfc14db1336e1366134367ccb16efb7d4d` |
| Premium dev | `c4d1c73d12dd4a4307871e62db79fe82fda7002a` |
| Draft PR #27 head | `49026dc7a470e1d8ea814a60ec7986a31c656f87` |
| Preparation branch | `codex/hotfix-release-lane`, based directly on stable SHA above |
| Proposed future patch | `1.7.1`, only after a specific bug is approved |

Live release, branch/tag and PR evidence were refreshed before editing.
Local `main` was stale; it was not used as the preparation base or moved.
Premium local/remote branch references and PR #27 remain untouched; Premium work is paused in this task.

`dev` contains unfinished identity/licensing work and cannot supply immediate stable hotfixes.
Feature releases retain `feature/* → dev → main`; isolated patches use verified stable history, reviewed governance, then `hotfix/* → main`.
After transition, a hotfix base may follow stable only through reviewed governance commits, never application changes from dev.

## Legacy versus guarded behavior

| Concern | Stable v1.7.0 workflow | Proposed lane |
|---|---|---|
| Main updates | Successful main builds invoke stable publisher automatically | CI-only; missing intent returns without writes |
| Write credentials | Candidate build checkout runs publication code | Separate trusted default-branch publisher; candidate data only |
| Main PR flow | Only dev accepted | Dev unchanged; narrow preparation/hotfix scope checked independently |
| Release authority | Main event and successful builds | Owner-merged, fresh source-specific intent |
| Hotfix source | No isolated policy | Published baseline, governance ancestry, exact commits/patch and semantic review |
| Retry | Legacy draft asset replacement possible | Matching receipt resumes missing assets; published/conflicting bytes preserved |
| Activation | Legacy publishing already present | Literal false guard plus separate owner variable and control verification |

## Selective PR01 audit and port

Reference: dev at `c4d1c73d12dd4a4307871e62db79fe82fda7002a`.
Files were read individually with `git show`; no dev merge/cherry-pick or whole-tree import occurred.

| Component | Decision |
|---|---|
| CI, Linux and beta workflows | Port removal of write publishers, nonpersisted credentials, attempt-qualified artifacts and tests; add scoped hotfix admission |
| release-promotion workflow | Port trusted workflow-run separation and serialization; add hard-disabled activation guard |
| release_promotion.py | Port intent, owner, exact-source/check/artifact, Ruleset, bounded archive and idempotency controls; add schema 2 |
| test-release-promotion.py | Port all 30 synthetic regressions, including unchanged feature/beta contracts |
| Publishing entry points/test.ps1 | Port common trusted gate and Windows policy regression hook |
| prepare-release.py/check-release.py | Reviewed and retained byte-for-byte from stable; existing patch/version consistency behavior sufficient |
| release_files.py | Retained stable checksum/payload validator; reused by trusted publisher |
| package.ps1 | Narrow stable-source archive change: exact committed Git tree; development packaging preserved |
| Governance documentation | Replace dev-specific assumptions with concise dual-source contracts and this transition plan |
| PR02 inventory, security tools, identity PoC, provisioning, Premium app code | Excluded; no runtime/dependency/license behavior imported |

## Safe main transition

1. Independently review this exact preparation head, source diff and both policy suites; retain `1.7.0` throughout.
2. Owner audits/cancels queued or running legacy privileged jobs and forbids reruns of pre-transition jobs/tags.
3. Owner separately authorizes required repository protections below; keep publisher hard-disabled and variable false/unset.
4. Review PR's Branch flow, Windows and both Linux-platform checks; merge only through reviewed PR, with no direct-push/bypass.
5. Confirm merged main has no stable/beta write job, unchanged version and no intent; ordinary completion events cannot publish.
6. Complete disposable-repository permission/digest/immutability/rerun proof and independent security review.
7. Only a later owner-approved activation PR may remove literal `false`; enable the variable only after verified controls and approval.
8. Approve a specific bug before creating hotfix branch, version metadata or executable patch intent.

The first main push uses the replacement CI-only workflow; the new workflow-run publisher remains hard-disabled even if a variable already exists.
Old queued/rerun workflows retain old code and cannot be retroactively secured by this change.
If owner cannot fence those runs or satisfy reviewed PR checks, stop migration; do not restore automatic publishing or bypass protection.

## Owner settings and activation blockers

Read-only GitHub inspection found `main.protected=false` and zero Rulesets.
Repository-variable state was not available through the connector; do not infer false/unset from missing administrative access.
No setting was changed, and activation remains blocked.

1. Audit owner/admins, collaborators, Apps, deploy keys, workflow-edit rights and repository secrets; keep build jobs free of production credentials.
2. Set default Actions token to contents-read; disable write tokens/secrets for fork PRs.
3. Create active branch Rulesets for `refs/heads/main` and `refs/heads/dev`: require PR, prevent force-push/deletion, require up-to-date branches.
4. Require Branch flow, Windows checks, Linux checks and package (Ubuntu 24.04), Linux checks (Ubuntu), Linux checks (Arch), each from GitHub Actions App ID `15368`.
5. Use merge commits for hotfix promotion; keep branch bypass lists empty and separately review workflow/governance changes.
6. Create stable tag Ruleset `refs/tags/v*`: restrict updates/deletion, empty bypass list, permit initial publisher tag creation.
7. Create separate beta tag Ruleset `refs/tags/beta-v*`: restrict creation/update/deletion; creation bypass only sole trusted owner/admin role (`RepositoryRole=5`, `always`).
8. Enable/test native release immutability separately; preserve historical releases/assets and record owner verification.
9. Export effective Ruleset IDs, scope, checks, integration IDs, strictness, bypass lists, reviewer and date; reverify after access/control changes.
10. Prove runtime token can read required evidence without Administration/write secrets; then separately approve activation PR and `RELEASE_PROMOTION_ENABLED=true`.

Production integration denies omitted branch/tag bypass evidence for every channel; original PR01 feature-mode's administrative-attestation fallback no longer authorizes publication.
GitHub documents that [bypass lists require ruleset write access](https://docs.github.com/en/rest/repos/rules#get-a-repository-ruleset); minimal publisher credentials may therefore be insufficient.
Architecture C's non-publishing sandbox proof validated owner CLI/OpenSSH evidence transport in [run 39030105254](https://github.com/oangsa/RoLauncher-release-sandbox/actions/runs/39030105254).
The [production integration](OWNER-EVIDENCE.md) keeps privileged credentials local and strict verification intact; signing policy remains unenrolled and publisher hard-disabled.
Full [sandbox lifecycle verification](SANDBOX-RELEASE-LIFECYCLE.md), independent review, public disclosure/key enrollment and separate activation approval remain blockers; the protocol proof did not publish a release or establish real tag enforcement.

## Future patch procedure — commands not executed

After approved governance migration, verified unchanged stable and specific bug approval:

```powershell
git fetch origin main --tags
git rev-parse v1.7.0
git log --oneline v1.7.0..origin/main
git diff --name-status v1.7.0..origin/main
git switch --create hotfix/1.7.1 origin/main
```

Require `v1.7.0` to resolve to the verified SHA above and latest stable still to be `v1.7.0`.
Review every post-stable main commit as governance-only; if stable/main changed unexpectedly, stop and reassess.
Never branch from or merge dev, or reuse another Premium branch for this work.

1. Implement only approved fix and related tests; inspect prerequisites before any selective backport/cherry-pick.
2. Use `python scripts/prepare-release.py fix` for local development, retaining target `1.7.1` with fresh development identities per AGENTS.md.
3. When an actual release is authorized, use `python scripts/prepare-release.py fix --stable`; complete README, both changelogs and consolidated `docs/RELEASE-1.7.1.md`.
4. Verify Cargo/lockfile, desktop informational version and artifact references; desktop runtime derives full version from existing build metadata.
5. Open reviewed hotfix PR; Branch flow prints exact commits, paths and patch hash, while missing intent cannot publish.
6. Owner reviews full patch/blobs, call paths, tests and dependencies; record approved bug, prerequisite analysis and Premium exclusion.
7. Add schema-2 intent alone in final commit, using preceding reviewed SHA and exact main base; no edits/reuse of its UUID path.
8. Obtain passing exact-source checks; owner merges with unchanged reviewed tree and authorized base; activated publisher independently verifies promotion.
9. Verify published receipt, source, checksums and package contents; stable must supersede development builds of same numeric target.

The complete intent contract and hash algorithm are in [Release governance](RELEASE-GOVERNANCE.md).
Version selection still anchors latest published stable, not development identities or unfinished features.
No release-note entry is added for this preparation because it changes no shipped application behavior.

## Backport, rollback and recovery

1. After publication, create a separate dev-targeted branch for approved application fix/tests only.
2. Review prerequisites and cherry-pick those commits individually; resolve compatibility and test against dev independently.
3. Do not merge hotfix release metadata, intent or governance history blindly into Premium development.

For emergency disable, disable Protected release promotion, cancel running/queued publishers, and remove/set activation variable false.
Keep CI-only workflows; revert faulty governance through reviewed PR without reinstating legacy publishers.
Retain drafts, tags, receipts and artifacts for investigation; never delete/move published versions as rollback.

Rerun only trusted promotion with the same source run/attempt to reconcile matching interrupted publication.
Changed main, attempts, expired artifacts, conflicting tags/assets or stale intent require owner investigation and fresh reviewed authority.
Published corrections require a new patch; partial drafts with mismatched receipt stay quarantined.

## Independent security-review checklist

- [ ] Verify live stable/tag/main and exact Draft PR head; compare every changed file against stable.
- [ ] Confirm Premium branch/PR references, runtime code, dependencies, version and published history remain unchanged.
- [ ] Trace PR admission versus publication authority; verify ordinary main/hotfix merges cannot authorize themselves.
- [ ] Review both intent schemas, UUID freshness, merge parents/tree, full patch blobs and dependency/prerequisite attestations.
- [ ] Check each commit and net diff for hidden/reverted Premium changes and unrelated refactors.
- [ ] Trace build-token isolation, trusted checkout, redirects, archive bounds, source-tree hashes and payload checksum sets.
- [ ] Exercise missing/failed Windows/Linux evidence, stale/replayed events/tags, races, interrupted uploads and consumed receipts.
- [ ] Review legacy queued/rerun/tag fencing and hard-disabled migration with no 1.7.1 metadata or intent.
- [ ] Independently inspect all active Rulesets/bypasses, token permissions, native immutability and activation evidence.
- [ ] Keep activation blocked until real least-privilege/digest/immutable-draft proof succeeds; no production release for testing.

Remaining trust includes GitHub/hosted runners, trusted owner/default-branch governance and pinned checkout.
Pattern checks and review attestations cannot establish arbitrary program semantics or reproducible binary provenance.
API check/write races cannot be made atomic; final rechecks and native protections narrow that risk.

## Validation evidence

| Local check | Result |
|---|---|
| Existing promotion fixtures | 30 passed; feature/beta authority and replay behavior retained |
| Hotfix policy fixtures | 14 passed, including 27 denial scenarios and migration/source/history checks |
| Existing beta/archive fixtures | 6 beta and 3 Linux update/archive tests passed |
| Version fixtures/check-release | Passed; repository remains 1.7.0 |
| Rust tests | 75 passed; one WinUI bridge test initially ignored, then passed separately |
| Desktop and native WinUI | 74 checks, UI smoke and Rust/WinUI bridge passed |
| Formatting/Clippy | Passed, locked all-target Clippy with warnings denied |
| Workflow/Python validation | Four workflows pass actionlint 1.7.12; Python modules compile; shellcheck unavailable |
| Linux-only Sober adapter locally | Windows cannot execute its POSIX entry point (`WinError 193`); native hosted Linux required |
| Production publication/settings | Not run or changed; synthetic API fixtures only |

New hotfix authority, merge-tree, per-commit scope, source-byte, branch-admission and hidden-bypass cases were exercised red before their guards passed.
Native Linux/Windows packaging and exact hosted checkout evidence belong with the Draft PR's current head checks.
Local fixtures do not prove real publisher token permissions, artifact digest availability or immutable-release completion.

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

