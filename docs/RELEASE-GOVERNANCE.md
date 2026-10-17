# Release governance

Preparation only; publishing disabled until independent review and owner activation.
[Hotfix strategy](HOTFIX-RELEASE-STRATEGY.md): transition, settings, evidence, owner decisions.

## Trust boundary

1. CI builds/tests proposed code with contents-read; checkout credentials not saved.
2. Separate `workflow_run` publisher checks out trusted default-branch event SHA, never proposed-code SHA.
3. It checks proposed files/archives as data, verifies approval/evidence, uploads without running proposed code.

Only publishing job requests contents-write, Actions-read, PR-read.
Literal `false` guard and missing/false `RELEASE_PROMOTION_ENABLED` each block activation.
Passing CI or matching branch name cannot approve publication.

Follow GitHub's [workflow_run security model](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run).
Never give build jobs repository secrets, write credentials, or shared executable caches.

## Feature intent: schema 1

PR01 contract unchanged: add one fresh `.github/release-intents/<UUIDv4>.json` with exactly:
`schema=1`, `id=<UUIDv4>`, `version=<MAJOR.MINOR.PATCH>`, `channel=stable`.
Only official `dev` PR merged into official `main` by owner `oangsa` qualifies.

Intent must first appear in that merge, with one path-history entry.
Reject dev versions, edited/reintroduced intents, foreign PRs, uncertain evidence.
Future Premium releases still need this separate path after development finishes.

## Hotfix intent: schema 2

Only owner-merged official `hotfix/<version>` PRs into `main` qualify.
Require merge commit: ordered parents equal approved main base then PR head; merged tree equals head tree.
Squash/rebase needs a separately reviewed policy change.

| Field | Required value |
| --- | --- |
| `schema`, `source`, `channel` | Integer `2`, `hotfix`, `stable` |
| `id` | Fresh canonical UUIDv4; matching filename; never edit/reuse |
| `version` | Plain stable version; baseline PATCH + 1 |
| `baseline_tag`, `baseline_sha` | Published stable tag/resolved commit |
| `base_sha` | Exact approved main base; only reviewed release-rule commits after baseline |
| `reviewed_sha` | Last reviewed app/version/docs commit before intent-only commit |
| `commits` | Complete ordered commits from base through reviewed SHA |
| `paths` | Complete sorted net changed-file list |
| `patch_sha256` | Canonical reviewed-patch hash from Branch flow |
| `review.bug` | Approved bug/scope, at least 20 characters |
| `review.dependencies` | Required changes/cherry-picks/dependencies reviewed; at least 20 characters |
| `review.premium_excluded` | Boolean `true`, confirmed by owner's merge |

Reject extra fields. Final commit adds only intent file; only parent is `reviewed_sha`.
This avoids hashing a commit into itself while tying approval to exact source/merge.

Branch flow prints commits, paths, patch hash before intent creation.
Owner reviews full files/dependencies at those commits, adds final intent commit, approves merge.
Original preparation contained no executable intent or patch-version metadata.

## Scope evidence

- Hash JSON records containing `filename`, `status`, blob `sha`, GitHub `patch`, sorted by filename.
- Serialize UTF-8 with sorted keys, separators `(',', ':')`, no newline.
- `scripts/hotfix_policy.py`: `patch_report`/`hotfix_report`; Branch flow prints report for review.
- Check net diff and every commit; reverting experiments cannot hide unrelated history.
- Hotfix history must be linear from approved base and separate from `dev`; reject dev ancestors/merges.
- Check each main commit after published stable for release-rule paths only.
- Reject unexpected paths, missing patches, renames, binary-only diffs, oversized evidence, known Premium/identity/licensing/payment markers.
- Cargo manifests/lockfiles may change root version only. Reject desktop project/package-lock edits.
- Broader compatibility work needs a separate reviewed rules PR, never bypass.

Checks cannot prove program behavior or every required change.
Owner must read full source, callers, all dependencies, tests; API diff previews may omit detail.
Required review statement flags partial cherry-picks for human review; not proof of complete dependencies.

## Checks and artifacts

- Main CI: Branch flow, Windows checks, Linux checks and package (Ubuntu 24.04).
- Separate Linux workflow: Linux checks (Ubuntu), Linux checks (Arch); exact same main SHA.
- Require successful official push runs; recheck latest attempts and named jobs before publishing.
- Artifacts must match run ID/SHA/attempt, fixed filenames, required GitHub archive SHA256 digests.
- Reject duplicate/path/link/encrypted/unsupported ZIP entries. Payload checksum set must match exactly.
- Hotfix source ZIP file list/every Git blob hash must match exact promoted commit tree.
- Stable source: `git archive HEAD`, clean committed tree. Local dev packages include working edits.
- Package checks retain executable, WebView2Loader.dll, source, AGENTS.md, GPL/attribution, dependency licenses.

| Archive limit | Maximum |
| --- | --- |
| Each outer archive / total expansion | 256 MiB / 512 MiB |
| Each entry / checksum file | 256 MiB / 1 MiB |
| Separate hotfix-source expansion | 256 MiB |
| Hotfix-source directory entries | Fewer than 5,000 |
| Hotfix-source central directory | 2 MiB |

Reject truncated/oversized lists. Review any limit increase first.

## Publication and recovery

- `release-promotion.json` records intent, source, channel, run/attempt, artifact IDs/digests, final file hashes.
- Receipt hash binds draft body. Identical finished release returns `consumed` without writes.
- Matching interrupted drafts get missing assets only. Set aside conflicting bytes/identity; never replace.
- Require exact checked tags. Publisher creates lightweight stable tags.
- Reject wrong/stale tags, historical releases without matching receipt, dev versions, versions that do not advance.
- Baseline may advance to exact target only to reconcile its matching consumed receipt.
- Final check repeats approval, main head, active Rulesets, workflow state, source-run checks.
- GitHub API changes are not atomic: emergency disable must also cancel running jobs.
- Native release immutability, trusted owner/default branch, independent activation proof still required.

## Beta compatibility

- Beta builds stay read-only; no tag-local publishing.
- Trusted publisher keeps PR01's owner-pushed `beta-vMAJOR.MINOR.PATCH`, dev-history, prerelease/never-latest rules.
- Historical tags still contain old workflows. Restrict new beta tags; never rerun old privileged jobs.
- Legacy beta library kept only for compatibility tests; CLI uses protected publisher.
- No workflow calls that library directly or gives it build credentials with write access.
