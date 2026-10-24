# Automatic release control checks

The publisher now obtains live controls through a dedicated GitHub App.
Merging an owner-authorized release intent approves publication after all required checks pass.
No local terminal, SSH key, Issue inbox, or per-release signature is required.

## One-time setup

1. Register a dedicated GitHub App and install it on **RoLauncher only**.
2. Grant repository **Administration: read and write** and the mandatory Metadata read permission; grant no Contents or Workflows permission.
3. Set repository variable `RELEASE_CONTROLS_APP_CLIENT_ID` to the App client ID.
4. Store its private key in Actions secret `RELEASE_CONTROLS_APP_PRIVATE_KEY`; never commit or print it.
5. Review and merge this governance change; keep the existing `RELEASE_PROMOTION_ENABLED=true` switch for activation.
6. Validate the flow in the release sandbox before production use, including denied reads and interrupted publication.

GitHub exposes ruleset bypass lists only to callers with ruleset write access.
The App credential therefore has real administrative capability; protect workflow changes and restrict who can edit Actions secrets.
The code uses an allowlist of control GET endpoints, but that does not reduce the credential's server-side permissions.

## Each release

1. Prepare the stable version and release intent, then merge the authorized source PR.
2. CI builds and validates the exact source with read-only credentials.
3. The trusted publisher verifies authority, CI, artifacts and checksums before requesting an App token.
4. It reads complete rulesets, bypass lists and native immutability settings automatically before staging and final publication.
5. It publishes verified assets and checks a separate immutable-release readback.

The App token is scoped to this repository, requests only Administration permission, and is revoked by the pinned token action at job completion.
The normal `GITHUB_TOKEN` performs release writes; candidate code never receives either publishing or administrative credentials.
Only trusted default-branch publisher code runs in the privileged job.

Missing App configuration, inaccessible bypass lists, disabled immutability, or invalid protections fail before release writes.
Slow uploads refresh control observations automatically; changed controls still fail strict validation.
Existing exact-source, fresh intent, asset digest, conflicting-release and consumed-receipt protections remain in force.

No collected controls snapshot is posted publicly or uploaded as an artifact.
The old public-disclosure digest, signing key and Issue policy are unused by automatic mode.
The old staged CLI remains for historical fixtures and manually selected attended recovery; the workflow never falls back to it.

## Recovery and migration

Rerun the trusted publisher after correcting configuration; do not rerun individual shell steps or replace assets.
Matching interrupted drafts resume missing assets; conflicting contents remain quarantined.
A governance merge advances main and invalidates older unpublished intents, so prepare a fresh recovery intent and successful exact-source CI before publication.

Existing runs retain their old workflow and still require the old attended gates or cancellation.
This change does not activate a GitHub App, change repository controls, alter version metadata, or publish a release by itself.

## Historical attended protocol

The following records the superseded manual protocol for audit and optional recovery.
Its pending-activation statements describe the original implementation, not the automatic workflow above.

# Owner-signed live publisher evidence

Status: implementation for independent review; publisher remains hard-disabled.
No production signer, inbox or disclosure approval is enrolled.
This document grants no activation or publication authorization.

## Authority and transport

Architecture C supplies evidence to the existing publisher; it does not grant release authority.
Schema-1 feature, schema-2 hotfix and owner-pushed beta authorization remain unchanged.
Every channel now requires explicit signed bypass evidence and native immutability, replacing feature-mode's administrative attestation fallback.

1. `workflow_run` checks out the trusted default-branch event SHA with nonpersisted credentials.
2. `--stage prepare` checks source authority, exact source CI/Linux jobs, metadata, archive digests, checksums and hotfix source tree, entirely read-only.
3. Only admitted candidates can generate a random 256-bit preflight challenge, uploaded by pinned `actions/upload-artifact`.
4. Owner helper downloads the exact run/attempt artifact through local GitHub CLI, validates its GitHub archive digest and bounded ZIP inventory, then displays source/version/intent.
5. After attended confirmation, helper collects live complete Ruleset inventory, every detail including bypass arrays, effective main/dev rules, and native immutability settings.
6. Owner reviews controls and signs canonical UTF-8 JSON through local OpenSSH, namespace `rolauncher-production-controls-v1`.
7. Helper posts one signed response to the approved repository Issue; Actions has Issues-read only.
8. `--stage stage` repeats admission, verifies preflight signature and live metadata, applies `protected_branches(strict=True)`, rechecks authority, and stages only missing draft assets.
9. A new final challenge requires a second attended collection and signature; preflight cannot authorize publication.
10. `--stage finish` repeats admission, requires local preflight acceptance for this binding, verifies fresh final controls, then rechecks authority, jobs, tag, release, assets and main before publication.

`release-promotion.json` bytes and identity remain unchanged.
Challenge binding additionally covers the complete intent, source and Linux run identities/attempts, and receipt SHA256.
It also binds repository numeric ID `1406138209`, owner ID `71574088`, workflow path, trusted SHA, publisher run/attempt, policy digest, inbox, nonce and phase.

The three steps share only the trusted job's local files; no downloaded checkpoint executes candidate code.
Each step recomputes candidate evidence, artifacts and receipt; a changed receipt cannot resume.
Accepted preflight bookkeeping is not reused as live final Ruleset evidence.

## Permissions and disclosure

Publisher uses `contents: write`, `actions: read`, `pull-requests: read`, and `issues: read`.
Candidate builders retain read-only credentials; no owner credential, private signing key, App key or Administration permission enters Actions.
No change to `RELEASE_PROMOTION_ENABLED` is included.

GitHub's [Ruleset API](https://docs.github.com/en/rest/repos/rules#get-a-repository-ruleset) requires Ruleset write access to expose bypass actors.
The [immutability settings API](https://docs.github.com/en/rest/repos/repos#check-if-immutable-releases-are-enabled-for-a-repository) requires Administration-read and returns `enabled` plus `enforced_by_owner`.
The helper uses the owner's existing locally authorized GitHub CLI, not a newly provisioned unrestricted administrator token.

A public inbox discloses Ruleset definitions, bypass actor identifiers, source/intent, run identities and timestamps.
Before enrollment, owner must separately approve that disclosure and choose an open Issue dedicated to evidence.
Both reviewed policy `public_disclosure_approved: true` and helper `--approve-public-evidence` are required; this PR leaves them unconfigured.

Public disclosure has a separate fail-closed boundary, independent of release-path protection checks.
Policy `disclosure_sha256` must pin SHA256 of the canonical **entire collected controls snapshot**, including all active, inactive and inherited Rulesets, effective branch rules and immutable settings.
The helper validates before displaying controls, signing or posting; the publisher repeats validation on authenticated evidence at both gates.

Every bypass actor must have exactly `actor_type`, `actor_id`, and `bypass_mode`: only `RepositoryRole`, integer `5`, `always` is a permitted disclosure category.
User, Team, App/Integration, unknown actors, extra actor fields and all other identities/modes are rejected, including on unrelated Rulesets; this does not authorize a bypass on protected paths.
Inventory and Ruleset envelope fields are exact; every nested condition, rule parameter and effective branch field is covered by the reviewed snapshot digest.

No fields are redacted, and the complete collected snapshot remains the input to unchanged strict verification.
Any changed value, new Ruleset, field or public evidence category fails against the pinned digest and requires separate owner disclosure review through a governance PR; adding an actor category also requires reviewed validator changes.
Generic `SIGN` confirmation cannot approve extra disclosure, and no helper response automatically updates policy.

The helper endpoint allowlist permits repository reads and one mutation: a comment POST to the pinned inbox.
No release, tag, Ruleset mutation, credential export or shell interpolation is implemented in the helper.
Payloads are limited to 32 KiB, comments to 48 KiB, challenge ZIPs to 64 KiB, challenge JSON to 16 KiB, and owner CLI responses to 2 MiB.

## Freshness, failures and recovery

Each phase permits 20 minutes for attendance; whole publisher timeout is 60 minutes.
Collection plus controls confirmation must finish within 45 seconds; evidence expires 120 seconds after collection starts.
Review source first, then collect; if controls review takes longer, stop and restart with fresh collection.

Publisher checks expiry immediately before every mutation, including uploads.
It re-fetches accepted comments and checks for duplicate/conflicting responses before writes; edited, removed, invalid or unsigned matching responses fail closed.
Only the pinned owner author ID and an active approved signing key can satisfy a gate; revoked keys cannot verify.

Nonce consumption is recorded in trusted job-local acceptance files; responses also bind publisher run and attempt.
Reruns generate new random nonces and artifacts, so old responses cannot authorize another attempt.
Do not rerun individual shell steps manually or copy acceptance files between jobs.

Timeout/cancellation before preflight produces no publication writes.
Failure after staging may leave the exact tag and matching draft; final timeout leaves them unpublished.
Cancel the workflow in Actions to stop; helper also checks run state before sending evidence, although cancellation and an already in-flight API call are not atomic.

Never automatically retry a mutating API request after a lost response.
Inspect actual state, then rerun the trusted publisher attempt with fresh evidence at both gates; it resumes only matching missing assets.
A lost comment POST response requires inspecting the inbox before any repost; duplicates fail closed, so prefer cancel/rerun when uncertain.

Consumed reconciliation requires matching receipt, complete identical assets, exact tag/source/channel, fresh gates and `immutable: true` Release readback.
Publication also requires `immutable: true` from a separate GET after PATCH; missing/false means quarantine and investigation, never replacement or automatic repair.
Large uploads may exhaust preflight's 120-second write window; completed matching assets remain available for a fresh attended retry.

## Windows owner procedure — future authorization only

Prerequisites: reviewed trusted checkout, Python 3.13+, Windows OpenSSH supporting `ssh-keygen -Y`, GitHub CLI authenticated as owner, enrolled dedicated production key, approved inbox/disclosure, and separately activated publisher.
Do not run helper from candidate artifacts or an unreviewed PR checkout.
This implementation PR does not authorize those prerequisites or a v1.7.1 release.

1. Review exact source/version/intent, required CI and independently reviewed publisher commit before authorizing a release merge.
2. Open Actions, then **Protected release promotion**, and inspect the admitted run's preflight challenge artifact.
3. Record publisher run ID, trusted default-branch SHA and exact promoted source SHA from independently checked GitHub pages.
4. In the trusted checkout, run the helper below; substituting a known SHA does not authorize a different source.
5. At each phase, read displayed source/version/intent, then type `APPROVE preflight 1.7.1` or `APPROVE final 1.7.1`.
6. Review freshly printed Rulesets and immutable settings; type `SIGN` only if correct, then unlock the local key if prompted.
7. Helper posts each response once and waits for the next challenge; keep terminal open and attend both phases.
8. Inspect Actions conclusion, signed inbox comments and audit artifact; then independently inspect release/tag/receipt/checksums if publication was authorized.

```powershell
python scripts/test-owner-evidence.py
python scripts/owner_evidence.py `
  --run-id PUBLISHER_RUN_ID `
  --trusted-sha TRUSTED_MAIN_SHA `
  --source-sha AUTHORIZED_SOURCE_SHA `
  --version 1.7.1 `
  --key "$env:USERPROFILE\.ssh\rolauncher-release-evidence" `
  --approve-public-evidence
```

Use explicit executable paths if GitHub CLI/OpenSSH are absent from PATH; the helper recognizes their standard Windows installation locations.
No command creates a key or posts production evidence in this PR.
Local policy must match the trusted workflow checkout's policy digest exactly.

## Enrollment and revocation — separate owner actions

1. Independently review this implementation and complete the [sandbox lifecycle plan](SANDBOX-RELEASE-LIFECYCLE.md) after separate test authorization.
2. Separately approve public disclosure and create/designate the production inbox; do not reuse sandbox Issue #1.
3. On a trusted owner workstation, create a **new dedicated encrypted Ed25519 signing key**, after explicit provisioning approval; never reuse the sandbox or unit-test keys.
4. Inspect its public key and fingerprint using `ssh-keygen -lf PATH.pub`; verify out of band during review.
5. Locally collect the complete controls snapshot without signing/posting; independently inspect **every** inventory/detail/branch/settings field for confidential information, including unrelated/inactive/inherited Rulesets.
6. Through a reviewed governance PR, set inbox number, disclosure approval, public `ssh-ed25519 BASE64`, and `disclosure_sha256` computed with `owner_evidence.digest(snapshot)` in `.github/owner-evidence-policy.json`; this PR leaves the digest `null`.
7. Keep private key and passphrase outside checkout, build artifacts and GitHub secrets; do not upload the key to GitHub account authentication/signing-key settings.
8. Independently verify owner CLI can retrieve all explicit bypass arrays, including an intentionally nonempty canary in sandbox, and `/immutable-releases` settings.
9. Review enrollment exact commit; keep publisher hard-disabled until lifecycle proof and a separately authorized activation PR/settings action.

Compute the digest only after independent local snapshot review; never publish a rejected snapshot merely to request approval.
The digest is disclosure authorization, not cached runtime evidence: both gates still collect fresh live data and signatures.
Operational cost: any Ruleset/effective-rule/settings change requires a reviewed digest update before release; inherited changes can block publication until reviewed.

For planned rotation, enroll a second verified public key through review, test it, then remove the old public key and add its `SHA256:...` fingerprint to `revoked`.
Maximum four active entries; revoked fingerprints override enrolled keys.
Never enroll by Issue comment, artifact, workflow input or owner-helper output alone.

On suspected compromise: cancel all running/queued publishers, disable workflow, keep/remove activation variable as appropriate, revoke owner CLI sessions if implicated, and quarantine incomplete drafts.
Review key revocation and replacement policy in a governance PR before any new publication.
An in-flight job pins old policy, so editing main alone does not revoke its signer: cancellation and disabling are mandatory first.

## Threat assessment and activation blockers

Candidate compromise cannot provide privileged code, credentials or self-authorized intent; existing owner merge/source/CI/artifact authority remains mandatory.
Stolen signing key alone cannot bypass owner comment identity; stolen owner CLI alone cannot generate a valid signature.
Compromise of both owner workstation factors, trusted main/workflow, or GitHub remains outside this boundary.

Signed evidence is an owner observation, not a GitHub-signed attestation.
Fresh metadata comparison catches inventory/configuration drift visible to Actions; hidden bypass changes and immutability setting changes within the short observation window remain a control-plane race.
Readback catches missing immutability after publication but cannot undo an already committed GitHub mutation; account/default-branch control and emergency cancellation remain necessary.

Inbox spam, malformed signed data, API outages, expired artifacts and ambiguous state can deny service; none may authorize publication.
Comment deletion/edit or concurrent responses after the last check remain an API race; repeated rechecks narrow it without claiming atomicity.
Audit artifacts retain challenges, signed payloads, signatures and accepted comment IDs for independent verification; public comments alone are mutable records.

## Implementation verification

Original implementation validation: 30 release-promotion, 14 hotfix-policy, 6 legacy beta and 21 owner-evidence tests passed (71 total).
F-01 adds regressions for unrelated active/inactive Rulesets with User/Team/App/Integration actors, unknown/confidential fields, zero signing/comment POST/publication writes on helper rejection, and disclosure rejection at both publisher gates.
The revised suite contains 25 owner-evidence tests (75 governance/beta tests total), including expiry after a committed upload, fresh-attempt recovery and preservation of existing asset identities/bytes.
The new suite uses temporary, automatically deleted unit-test keys and in-memory GitHub fixtures; it makes no network requests or production enrollment changes.
Release/version consistency and `git diff --check` also passed.

Coverage includes both staged live gates, unapproved/revoked/tampered/wrong-namespace signatures, nested source/run/attempt/intent/receipt bindings, freshness/replay, missing arrays/unauthorized actors, conflicts/edits/timeouts, immutable settings/readback, interrupted upload and lost publish response reconciliation.
Earlier [sandbox protocol run](https://github.com/oangsa/RoLauncher-release-sandbox/actions/runs/39030105254) proved Windows-to-Ubuntu transport for the sandbox implementation only.
Current production-shaped full lifecycle, real publishing permissions, native immutable mutation denial and production transport are not yet proven.

Read-only owner-helper API collection also passed against production: all five Rulesets (`25257022`, `25257602`, `25257860`, `25257891`, `25257968`) exposed explicit bypass arrays, and unchanged strict protection validation passed.
Native settings returned `enabled: true`, `enforced_by_owner: false`; zero writes, signatures or inbox posts occurred.
Tooling: Python 3.13.10, GitHub CLI 2.102.0 and Windows OpenSSH 9.5p2; this proves the local owner credential's reads, not the publisher runtime token's privileges.

Remaining blockers: independent review, production key/inbox/disclosure enrollment, demonstrated production-shaped API permissions and Windows-to-Ubuntu transport, full lifecycle/immutability/retry proof, and separate activation approval.
Native production immutability was read as enabled during implementation; this does not prove future Actions token permissions or future settings.
No successful CI or protocol test establishes real Roblox runtime compatibility.
