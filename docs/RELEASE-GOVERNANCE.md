# Release governance

Status: preparation only; publication hard-disabled pending independent review and owner activation.
See [stable hotfix release flow](HOTFIX-RELEASE-STRATEGY.md) for release steps and current hotfix status.

## Trust boundary

1. CI builds and tests candidate code with contents-read and nonpersisted checkout credentials.
2. A separate `workflow_run` publisher checks out the trusted default-branch event SHA, never the candidate SHA.
3. It reads candidate files and archives as data, verifies authority and evidence, then uploads without executing candidate code.

Only the publishing job requests contents-write, Actions-read, PR-read and Issues-read.
The workflow's literal `false` guard and missing/false `RELEASE_PROMOTION_ENABLED` each prevent activation.
Neither successful CI nor a branch name grants publication authority.

The default-branch and token separation follow GitHub's [workflow_run security model](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run).
Do not expose repository secrets, write credentials or shared executable caches to build jobs.

## Feature intent: schema 1

The existing PR01 contract remains unchanged: one newly added `.github/release-intents/<UUIDv4>.json`.
Its exact fields are `schema=1`, `id=<UUIDv4>`, `version=<MAJOR.MINOR.PATCH>` and `channel=stable`.
Only an official `dev` PR merged into official `main` by repository owner `oangsa` qualifies.

The intent must first appear in that promotion, with exactly one path-history entry.
Development versions, changed/reintroduced intents, foreign PRs and ambiguous evidence fail closed.
Future Premium feature releases still require this independent feature path after development is complete.

## Hotfix intent: schema 2

Only official `hotfix/<version>` PRs merged into `main` by the owner qualify.
Merge commits are required: ordered parents must equal authorized main base and PR head; merged tree must equal head tree.
Squash/rebase promotion requires a separately reviewed policy change.

| Field | Exact requirement |
|---|---|
| `schema`, `source`, `channel` | Integer `2`, `hotfix`, `stable` |
| `id` | Fresh canonical UUIDv4; filename matches; never edited/reused |
| `version` | Plain stable version, exactly baseline PATCH + 1 |
| `baseline_tag`, `baseline_sha` | Published stable tag and its resolved commit |
| `base_sha` | Exact authorized main base; only reviewed governance history may follow baseline |
| `reviewed_sha` | Final reviewed application/version/docs commit, before intent-only commit |
| `commits` | Ordered complete commit list from base through reviewed SHA |
| `paths` | Sorted complete net changed-file list |
| `patch_sha256` | Hash of canonical reviewed patch records, as printed by Branch flow |
| `review.bug` | Concrete approved bug and scope, at least 20 characters |
| `review.dependencies` | Explicit prerequisite/cherry-pick and dependency analysis, at least 20 characters |
| `review.premium_excluded` | Boolean `true`, attested by owner's merge |

No additional fields are accepted.
The final hotfix commit must add only that intent file, with sole parent `reviewed_sha`.
This avoids a self-referencing commit hash while binding approval to the actual promotion and exact reviewed source.

Branch flow prints commit IDs, paths and patch hash before intent creation.
The owner reviews complete blobs/dependencies at those IDs, adds the intent in a separate final commit, and authorizes its merge.
No executable intent or patch-version metadata exists in this preparation.

## Recovery intent: schema 3

Schema 3 is only for a hotfix source PR already merged by owner `oangsa` without its schema-2 intent. It does not relax schema 2 or admit a patch that was not owner-merged.

| Field | Exact requirement |
|---|---|
| `schema`, `source`, `channel` | Integer `3`, `hotfix-recovery`, `stable` |
| `id` | Fresh canonical UUIDv4; filename matches; never edited or reused |
| `version` | Plain stable version, exactly published baseline PATCH + 1 |
| `baseline_tag`, `baseline_sha` | Published stable tag and its resolved commit |
| `source_pr`, `source_merge_sha` | Original owner-merged hotfix PR number and exact merge commit |
| `base_sha`, `reviewed_sha` | Original source PR base and reviewed head, not recovery-PR SHAs |
| `commits`, `paths`, `patch_sha256` | Exact original source report, in order/sorted form and canonical digest |
| `review` | Same bug, dependency/prerequisite and Premium-exclusion attestations as schema 2 |

The original PR must be official `hotfix/<version>`, owner-merged to `main`, and have merge parents `[base_sha, reviewed_sha]` with the reviewed tree unchanged. Governance history from the stable baseline to `base_sha` and the complete hotfix report are checked again.

The recovery PR branch must be `hotfix-intent/<version>` and start directly at `source_merge_sha`. It has one commit, adding only the fresh intent file. Its owner-merged commit must have parents `[source_merge_sha, recovery_head]` and a tree identical to that head. Any intervening `main` change, mismatched source PR, changed patch or extra file is rejected.

The recovery implementation itself can enter `main` only through `codex/hotfix-recovery-governance`, based directly on current `main`. Branch flow permits governance paths only and requires Cargo package version to remain the exact next patch over latest stable. This exception does not enable the publisher or satisfy owner-signed live controls.

## Scope evidence

The patch hash covers JSON records containing `filename`, `status`, blob `sha` and GitHub `patch`, sorted by filename.
Serialization uses sorted keys and separators `(',', ':')`, UTF-8, without a newline.
`scripts/hotfix_policy.py` exposes `patch_report` and `hotfix_report`; Branch flow emits their reviewable report.

Both net patch and each individual commit are checked; reverted experimental commits cannot hide in a clean net diff.
Hotfix commits must be linear descendants of authorized base and diverge from `dev`; dev ancestors/merges are denied.
Main history since published stable is separately checked for governance-only paths, including each commit.

Unexpected paths, missing patches, renamed files, binary-only diffs, oversized evidence and known Premium/identity/licensing/payment markers are denied.
Cargo manifests and lockfiles may change only the root version, except a hotfix may add the allowlisted
`Win32_System_Diagnostics_Debug`, `Win32_System_Diagnostics_ToolHelp` and `Win32_System_RemoteDesktop` features to
the existing `windows-sys` 0.61 Windows dependency. Existing features must remain, and dependency versions or other
dependency metadata must not change; desktop project/package-lock changes are denied.
Compatibility changes outside this narrow policy need a separate reviewed governance PR, never a bypass.

These controls do not prove program semantics or detect every missing prerequisite.
The owner must inspect full source blobs, call paths, dependency closure and tests; API patch previews may omit detail.
The explicit review attestation flags partial cherry-picks for human review and is not an automated proof of dependency completeness.

## Checks and artifacts

- Required main CI jobs: Branch flow, Windows checks, Linux checks and package (Ubuntu 24.04).
- Separate Linux workflow: Linux checks (Ubuntu) and Linux checks (Arch), same exact main SHA.
- All runs must be official successful pushes; exact latest attempts and named successful jobs are rechecked before final publication.
- Artifacts must match run ID/SHA/attempt, fixed basenames and mandatory GitHub archive SHA256 digests.
- ZIP budgets reject duplicate/path/link/encrypted/unsupported entries; payload checksum sets must match exactly.
- Hotfix source ZIP inventory and every Git blob hash must equal the exact promoted commit tree.
- Stable source packaging uses `git archive HEAD` from a clean committed tree; local development packaging retains working changes.
- Existing package verification retains executable, WebView2Loader.dll, source, AGENTS.md, GPL/attribution and dependency-license checks.

Outer archives are limited to 256 MiB each, 512 MiB total expansion, 256 MiB per entry and 1 MiB per checksum file.
Hotfix source has a separate 256 MiB expansion budget, fewer than 5,000 directory entries and a 2 MiB central-directory limit.
Truncated/oversized collections fail closed; limits need review before increasing.

## Publication and recovery

`release-promotion.json` binds intent, source, channel, run/attempt, artifact IDs/digests and final file hashes.
Its hash binds the draft body; identical completed publication returns `consumed` without writes.
Matching interrupted drafts upload only missing assets; conflicting bytes or identity are quarantined, never replaced.

Only exact checked tags qualify; stable tags created by this publisher are lightweight.
Existing wrong/stale tags, historical releases without matching receipt, development versions and nonadvancing versions are denied.
The published baseline may advance to the exact target solely to reconcile its matching consumed receipt.

The final gate repeats authority, main-head, active Rulesets, workflow state and source-run checks.
GitHub API operations are not atomic; emergency disable must cancel running jobs as well as disable the workflow.
Native release immutability, owner/default-branch trust and independent activation proof remain required.

## Owner-signed live controls

See [owner evidence protocol and operations](OWNER-EVIDENCE.md) and the separately authorized [sandbox lifecycle plan](SANDBOX-RELEASE-LIFECYCLE.md).
Production policy is deliberately unenrolled: no key, inbox or public disclosure approval.
Literal `false &&` remains, and activation variable must remain absent pending separate owner authorization.

Read-only admission precedes two attended challenge-artifact/Issue-response gates, with pinned OpenSSH signatures and exact source/run/attempt/receipt binding.
Both gates use complete fresh Ruleset/bypass evidence with `protected_branches(strict=True)` and signed live native immutability settings.
Feature and beta source authority remain unchanged; missing bypass evidence no longer qualifies through administrative attestation.

Each write requires unexpired evidence; final publication rechecks source authority, checks, tag, release and asset identity after attendance.
Published and consumed releases require separate `immutable: true` readback; identical consumed reconciliation performs no writes.
Independent review, signing policy enrollment, public disclosure approval, real sandbox lifecycle proof and separate activation authorization remain outstanding.

## Beta compatibility

Beta builds remain read-only; tag-local publication is removed.
The trusted publisher retains PR01's owner-pushed `beta-vMAJOR.MINOR.PATCH`, dev-history and prerelease/never-latest policy.
Historical beta tags retain legacy workflows; restrict new beta tag creation and never rerun old privileged jobs.

The legacy beta library seam remains solely for compatibility tests; its CLI delegates to the protected boundary.
No workflow calls that library directly or gives it candidate build credentials with write access.

