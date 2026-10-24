# Architecture C full lifecycle validation plan

Status: **not authorized or executed** by the implementation PR.
Scope of any future approval must name only `oangsa/RoLauncher-release-sandbox` (ID `1423257712`).
Production `oangsa/RoLauncher` must remain inaccessible to the test publisher and unchanged.

## Owner approval required

Request explicit authorization to replace sandbox canary tag conditions, provision sandbox-only controls/key/inbox, and create synthetic sandbox tags, drafts and GitHub Releases.
Approval must also cover deliberate denied mutation attempts against immutable synthetic assets/tags and public signed test evidence.
Use sandbox-only names such as `v0.0.900` and `beta-v0.0.900`, never production `v1.7.1`; no production intent or production credentials.

Do not run by changing production constants in a deployed publisher.
Review a sandbox-only port of this exact implementation, with repository/owner/workflow identities and policy explicitly pinned to sandbox and a synthetic source/receipt fixture.
Use an installation restricted to sandbox or its sandbox `GITHUB_TOKEN`; demonstrate production endpoints and repository identity are rejected.

## Preparation and fixture correction

1. Export existing sandbox Rulesets and record IDs, definitions and bypass lists before changing anything.
2. Correct fixture `25300842` from doubled `refs/tags/refs/tags/sandbox-canary-*` to `refs/tags/sandbox-canary-*`.
3. Correct fixture `25300914` from doubled `refs/tags/refs/tags/sandbox-empty-*` to `refs/tags/sandbox-empty-*`.
4. Preserve the intentionally nonempty RepositoryRole `5`, `always` fixture and explicit empty-array fixture; verify actual GET bodies through owner CLI after correction.
5. Provision production-shaped main/dev required-check Rulesets with explicit empty bypasses, immutable stable/beta tag rules and separate allowed beta creation bypass.
6. Enable native sandbox release immutability; verify actual settings API, not a screenshot or owner assertion alone.
7. Pin a sandbox-only key distinct from production and previous PoC keys; approve public evidence disclosure and dedicated inbox explicitly.
8. Review workflow permissions: builder contents-read, publisher contents-write/Actions-read/PR-read/Issues-read, no Administration or private owner key.

The doubled-prefix fixtures have **not** been changed by this PR.
Their previous evidence visibility test did not prove enforcement of actual tag names.
Read-only permission probes must precede publishing tests; missing bypass arrays or immutability settings stop the test.

## Execution matrix

1. Run current unit/regression suites on Windows and Ubuntu; record exact commit, run/attempt and uploaded artifact digests.
2. Prove read-only Actions runtime visibility, owner CLI explicit nonempty/empty arrays, and fresh signed owner observations at both gates.
3. Submit forged source/intent/merger, stale CI/Linux attempt, missing/unauthorized bypass, bad/revoked key, expired/replayed nonce, wrong run/repository/phase, edited/duplicate/conflicting response and disabled/missing immutability evidence.
4. Confirm every denied admission/preflight creates **zero** tags/releases/assets; failed final gate must leave only its matching unpublished draft and staged assets.
5. Let the owner-attended positive run create one synthetic tag, matching draft, exact asset set/checksums/receipt, then publish only after final gate.
6. GET Release separately and require `immutable: true`; download every asset and compare SHA256 to receipt/checksum files, and resolve tag to exact synthetic source.
7. Attempt asset replacement/deletion/addition and tag update/deletion using the exact sandbox publisher credential; require GitHub rejection and unchanged readback.
8. Test native immutability separately from tag Rulesets: use an independently approved sandbox-only control case so a Ruleset denial alone cannot masquerade as native immutability proof.
9. Repeat completed publication with fresh challenges; require `consumed`, byte-identical receipt/assets/tag and **zero** mutation calls.
10. Interrupt after tag creation, draft creation, one upload, all uploads, before PATCH, and immediately after committed PATCH with lost response.
11. Rerun each with fresh nonce/run-attempt binding; resume only missing identical assets, or return consumed after published readback. Never replace existing bytes.
12. Change source, artifact digest, receipt, target, tag or asset during recovery; require quarantine and no repair/delete/overwrite.
13. Cancel during both human waits, let waits expire, change main/source attempt while waiting, and race duplicate/edited responses before writes; require fail-closed outcomes.
14. Test schema-1 feature and owner-tagged beta contracts alongside schema-2 hotfix, retaining beta prerelease and `make_latest=false` behavior.

A real HTTP fault proxy or instrumented sandbox-only API adapter must record each mutation and simulate a lost response **after** server commit.
Offline fixtures alone do not prove GitHub's real permissions, immutable-release behavior or partial publication semantics.
Capture denied request status codes and before/after resource hashes without logging tokens or authenticated storage URLs.

## Production-shaped upload freshness and recovery (F-02)

This matrix is a plan only: synthetic publishing and failure injection still require the separate sandbox authorization above.
Keep evidence TTL at **120 seconds**; do not extend it to make uploads pass.
An upload admitted while evidence is fresh can finish after expiry; no later mutation is authorized by expired evidence, and publication always needs a new final gate.

1. Package synthetic Windows installer/portable/source ZIP and Linux TAR.GZ using the production asset names, checksums and receipt contract, without production application/source identities.
2. Record current production-shaped compressed sizes locally, then test at least those sizes and a 2x-size stress case; include a Windows asset of at least 256 MiB and Linux asset of at least 128 MiB, within actual API/artifact limits.
3. Upload builder artifacts through real Actions, record GitHub artifact digests, and verify publisher download limits, archive inventory, checksum/source checks and memory/time behavior before any release mutation.
4. Pin and independently review the full sandbox disclosure snapshot digest; no sandbox response may disclose unapproved categories.
5. Use a sandbox-only bounded adapter to delay upload requests/responses so evidence expires (a) before the first asset POST, (b) between assets, and (c) during one in-flight upload; retain real GitHub API calls and a timestamped mutation ledger.
6. For (a), require zero asset POSTs; for (b), no further mutation after freshness rejection; for (c), permit only the already admitted in-flight request to finish, then require no subsequent mutation under expired evidence.
7. Interrupt connections before server acceptance and after committed upload with a lost response; inspect GitHub state without replaying POST automatically. Incomplete, unexpected or mismatched asset bytes must quarantine rather than be replaced.
8. Start a fresh publisher run/attempt for each recovery, collect a new preflight signature, and verify exact source, intent, source CI/Linux attempts and byte-identical deterministic receipt; publisher run/attempt changes must not change the receipt identity.
9. Require existing matching asset IDs, sizes and downloaded SHA256 to remain unchanged; ledger may contain uploads only for missing names, with no DELETE, replacement, renamed duplicate or tag rewrite.
10. Repeat expired recovery until the complete draft is staged; if one asset cannot fit supported transport/timeout limits or fresh retries cannot make progress, report a production activation blocker rather than weaken TTL or integrity checks.
11. Obtain the new final challenge **after** complete staging, sign fresh final live controls, and verify complete draft/receipt/tag readback before the sole publication PATCH.
12. Expire or interrupt final evidence before PATCH: require the draft remains unpublished and a fresh attended run is needed. Lose PATCH response after server commit: require fresh-run consumed reconciliation, exact receipt/assets, immutable readback and zero mutation calls.
13. Separately GET the published Release and require `immutable: true`; download and hash all assets, resolve exact tag/source, and perform the separately approved denied replacement/deletion checks.

Record asset names/sizes/IDs/SHA256, receipt bytes, evidence collection/expiry times, request start/end times, run/attempt URLs, actual HTTP outcomes and before/after resource hashes.
Do not log credentials, authenticated upload/download URLs or private keys; distinguish offline fault coverage from real API results.
Success requires both Windows and Linux large assets, expired and interrupted recovery, a fresh final gate, immutable readback, unchanged existing assets and a zero-write consumed retry.

## Acceptance and cleanup

Attach exact workflow/source commits, run URLs/attempts, owner helper platform/OpenSSH versions, Ruleset exports, signed challenges/responses, API status evidence, asset downloads and mutation ledger.
Independently verify receipt/source identity, two fresh gates, native immutability readback, tag enforcement and no-write consumed retry.
Document whether GitHub's immutable beta behavior permits the existing beta contract before production activation.

Immutable published sandbox tags/releases may be undeletable; obtain owner acceptance of permanent synthetic fixtures before testing.
Cancel and disable test workflows, revoke sandbox credentials/keys and remove any temporary bypass only through separately reviewed cleanup actions.
Production activation, production enrollment and v1.7.1 publication still require separate owner authorization after independent review.
