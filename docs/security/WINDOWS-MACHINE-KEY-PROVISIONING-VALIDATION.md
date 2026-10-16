# Windows machine-key provisioning validation — Phase 1

Status: **Draft; independent security review and human lab execution pending**, October 16, 2026.
Baseline: remote `dev` at `c4d1c73d12dd4a4307871e62db79fe82fda7002a` (GitHub PR #26 merged).
Recommendation: **Partially feasible, requiring additional evidence or Owner Decisions.**

This package supplies isolated validation code and a manual P/A/B procedure, not an approved provisioning architecture.
The owner confirmed a disposable lab is available and will run privileged and cross-user operations manually.
No new machine keys, key ACL changes, accounts, services, installer changes or production integration were executed on the developer machine.

## Evidence and stop conditions

| Evidence category | Actual result |
|---|---|
| Local recovery preflight | 173 receipts: 160 deletion successes, 5 never persisted, 4 confirmed absent, **4 incomplete** |
| Receipt integrity during this task | 173 initial hashes compared again; zero changed or missing; incomplete records preserved |
| Earlier receipt overwrites | Not established: no pre-task hash baseline was available; internal identity/public-evidence continuity is checked |
| Developer-machine reuse | **Blocked** by outstanding recovery; no native key experimentation or cleanup performed |
| Read-only Windows inspection | Windows 26H2 build 26300.9457; unelevated unrestricted interactive process token; software flags 34 and TPM flags 1; both report descriptor/RSA support |
| Current default receipt directory | 56 deletion successes and zero incomplete; this narrower scope does **not** supersede aggregate preflight |
| Portable policy/journal tests | 15 passed; synthetic inputs and filesystem fixtures, not OS isolation evidence |
| Windows lab validation tests | 12 passed: two existing guards and ten deterministic review regressions; no key creation |
| Receipt/source tests | 5 passed; receipt corruption, ambiguous deletion status, exclusive baselines, hash changes and source isolation |
| Application Rust regression | 111 passed; existing UI bootstrap test remains ignored locally |
| Desktop regression | 74 existing model/editor/update checks passed; isolated installer fixture also passed locally during the review correction |
| Governance regression | 30 promotion tests and existing versioning fixtures passed; version remains 1.7.0 |
| Native P/A/B provisioning, management rights, reboot | **Pending human lab execution**, including failure/unsupported outcomes |
| Windows/Linux hosted checks | Exact-source runs linked below; conclusions belong to those runs and the Draft PR, not the manual lab matrix |

Sandbox fixture permissions initially prevented unrelated application/governance tests; rerunning those synthetic suites outside the sandbox passed.
Existing native PoC key tests are deliberately deferred to clean hosted Windows execution rather than reusing the blocked developer host.
Prior PoC observations remain historical evidence in [the original report](WINDOWS-DEVICE-IDENTITY-POC.md), not new results for this implementation.

Stop on unavailable genuine users, unexpected management success, broad/inherited ACLs, missing public evidence, ambiguous keysets or unresolved recovery.
Do not repair existing developer-host receipts under this task; the independent owner must separately authorize any privileged remediation.
Preserve every partial journal and receipt; never enumerate provider keys or infer deletion from `NTE_BAD_KEYSET`.

## Architecture comparison

Here A/B are **provisioning approaches**, not permission to replace cryptographic Candidate A with the earlier Candidate B.
The approved installation model still needs persistent machine M, user U, authorized SID, protected enrollment and future authenticated customer binding.

| Approach | Investigation and implications | Decision status |
|---|---|---|
| A. One-time administrator provisioning | P creates machine M and restricts its owner/DACL; A later opens/signs without elevation. P retains management/recovery authority. Root and metadata integrity, effective rights and lifecycle require native evidence. | Lab harness supplied; not yet demonstrated |
| B. Per-user installer plus separate setup mechanism | Existing `{localappdata}` installer uses `PrivilegesRequired=lowest`. A separate reviewed provisioner could reserve machine state before ordinary-user use; it must authenticate P, select A safely and serialize registration. | Design only; no production helper/service or installer change |
| C. TPM-backed provisioning | Explicit Platform Crypto Provider may provide hardware-backed possession. Provider availability, algorithm, export/usage/scope, ACLs and A's ordinary operation must each pass; inherited PCP ACLs may remain unsupported. | No software fallback; hardware attestation remains separate |

The per-user installer cannot be treated as an authorized machine-provisioning authority merely because CNG permits some calls.
Approach B needs a separately approved distribution, elevation, principal-selection and recovery mechanism; this command-line experiment is not that component.
Neither approach establishes customer authorization or an installation allocation service.

Upgrades/uninstall currently have no identity hooks; CNG keys and lab registration live outside the application directory.
Persistence across actual upgrade/uninstall/reinstall is **pending**, not inferred from that source inspection.
A production uninstall, retention, reprovisioning and ownership-transfer policy remains an Owner Decision.

## Local registration and trust boundary

1. P reserves one fixed machine lab directory under Windows' known ProgramData folder; a changed lab UUID cannot select another machine slot.
2. The directory and machine journal require Administrators ownership, a protected DACL, SYSTEM/Administrators management, and only A/B read/traverse rights.
3. P writes and synchronizes a reservation containing the exact planned disposable CNG identity before creating M; the existing protected receipt is synchronized before CNG creation.
4. M is committed only after finalization, public-key capture, provider/scope/export/usage checks and strict owner/DACL validation.
5. A exclusively reserves its user slot and creates U using existing user-key/recovery checks; the user journal binds the machine reservation and exact U.
6. Every check reopens pinned M/U, validates the live A token and ACLs, signs a fresh canonical synthetic challenge with both keys, and rejects replay in that verifier.

Existing directories, journals and key names are never overwritten, adopted or automatically repaired.
Concurrent reservation has one winner; interrupted reservation, missing journal under an existing slot, lost M/U or changed evidence requires recovery.
Registration readers accept only the harness's canonical two-line journal, rejecting torn, reordered, duplicate-field, oversized or inconsistent state.

The public machine record can be read by B, but B cannot become its authorized A through a copied user journal.
The native check pins A's live SID and both exact public keys; MachineGuid and container names are not cryptographic identity.
The synthetic account label establishes no customer ownership and every lab report retains `enrollment_accepted=false`.

The filesystem owner/DACL checks are a local OS boundary, not a remote trust anchor or anti-rollback service.
Privileged directory/key replacement, parent-path races, snapshots, profile restoration and total enrollment loss require independent review and backend policy.
No local-only ledger can prove that a previously deleted installation never consumed a customer allocation.

## Least-privilege experiment

| Operation/authority | Requested policy | Required native evidence |
|---|---|---|
| M owner | P, distinct from A/B | Owner readback; unexpected owner rejects |
| M management | P and SYSTEM full control | P provisioning and exact-receipt deletion; ordinary users receive no such grants |
| A use of M | `GENERIC_READ`; narrowly allowed read/control/synchronize expansion | Actual open, public export, security-policy read, fixed-challenge signing and verification |
| A management of M | No write, delete, `WRITE_DAC`, `WRITE_OWNER` or generic full control | Sacrificial-snapshot native DACL-write, ownership and delete attempts must be denied |
| Private export | Export policy 0; signing-only usage | Native private export attempt fails; any returned buffer is zeroized and never printed/persisted |
| B use of M | No B ACE | Genuine B open/sign result recorded separately; public-file read is not private-key access |
| U | Existing owner A/SYSTEM PoC policy | Genuine B cannot exercise A's private U; no claim that U's owner has no management rights |
| Registration management | Administrators/SYSTEM; A/B read only | Native competing registration and metadata-write attempts in disposable snapshots |

`GENERIC_READ` is an experimental starting mask, not an asserted provider-independent minimum for signing.
Policy accepts no inherited/unexpected ACEs and never broadens rights to make a provider pass.
Readback is necessary; native successful use and denied management must be measured independently.

`lab-rights` is explicitly destructive **if permissions are unexpectedly broad**: it can change ownership or delete the exact disposable M.
Run it only in a separate sacrificial lab snapshot after persistence measurements; retain P's receipts even if A reports successful deletion.
Failed/unsupported operations are reported as such; only explicit permission statuses count as native denial, and `0x80090016` remains ambiguous.

## Manual lab procedure

Use a disposable Windows installation and three existing accounts: administrator P, standard A and standard B with genuinely different SIDs.
No command creates accounts, assigns administrator membership, changes TPM firmware or supplies credentials.
Use a fresh snapshot for each provider and destructive scenario; never reuse the blocked developer installation.

### 1. Build and preflight

```powershell
. ./scripts/environment.ps1
cargo build --locked --example windows-device-identity
$poc = './target/debug/examples/windows-device-identity.exe'
python scripts/identity-lab.py --check-sources
& $poc lab-inspect
```

The Python preflight defaults to known local PoC receipt roots; add `--receipt-root $localReceiptRoot` for every other retained ledger.
Use `--save-local-baseline $privateBaseline` once, then `--compare-local-baseline $privateBaseline`; the private manifest contains paths and must remain local.
Exit 2 means blocked/unverified; the CNG command's active-directory summary alone is insufficient to clear other ledgers.

Place the executable in a lab folder executable by P/A/B, and retain separate private recovery directories for P and A.
Set `ROLAUNCHER_IDENTITY_POC_RECOVERY_DIR` to that principal's local lab directory before mutations and keep its value stable across recovery.
Record only sanitized Windows build, token classification and provider results publicly; inspect account membership and raw SID comparisons locally.

### 2. Administrator P provisions

P manually prepares a strictly local JSON configuration, reviewing the real account SIDs independently:

```json
{
  "lab_id": "<fresh UUIDv4, retained for this lab>",
  "provisioner_sid": "<local P SID>",
  "authorized_sid": "<local standard A SID>",
  "unauthorized_sid": "<local standard B SID>",
  "provider": "software"
}
```

Use `"tpm"` only in a separate fresh snapshot; no automatic fallback or alternate-provider retry exists.
These placeholders are deliberately invalid and contain no account data.
Do not upload this configuration, the machine/user journals, public blobs, key names or recovery receipts.

```powershell
# Human launches an elevated interactive console as P in the authorized lab.
& $poc lab-provision $privateConfig --authorized-disposable-lab
```

The command requires an interactive console, the explicit flag and typed phrase `AUTHORIZED DISPOSABLE WINDOWS LAB`.
Hosted/CI environment markers reject mutations before configuration/key operations; an elevated diagnostic cannot enroll A.
Record the actual native result; a failed call does not authorize a retry that discards the reserved slot.

### 3. Standard A enrolls and reopens

```powershell
# Human signs in as genuine standard A; no elevation.
& $poc lab-inspect
& $poc lab-enroll --authorized-disposable-lab
& $poc lab-check
```

The standard-account check also rejects deny-only Administrators membership, so an unelevated administrator is insufficient.
Close the process and repeat `lab-check`; reboot Windows, sign in as A and repeat again, retaining the exact same local records.
Compare original public references locally; only executed checks on unchanged pinned registration establish continuity.

### 4. Genuine B probes a public copy

A first runs `lab-check` successfully in its genuine standard-user session and retains its original private user journal.
A computes the SHA256 of that original file locally and gives P the digest through a separate authenticated human handoff.
Do not derive the expected digest from B's copy, publish it, or include it in evidence uploads.

```powershell
# A: $originalUserJournal is A's original local journal, not a supplied public copy.
& $poc lab-check
$localComparison = (Get-FileHash -LiteralPath $originalUserJournal -Algorithm SHA256).Hash
```

P obtains a byte-identical public copy from A and runs this command in the authorized disposable lab:

```powershell
# P elevated; type the digest independently obtained from A when prompted.
& $poc lab-pin-user $reviewedOriginalUserJournalCopy --authorized-disposable-lab
```

The command compares one bounded in-memory copy with P's separately entered digest and validates the canonical journal, machine binding, `user_name` and public metadata.
It exclusively creates an administrator-owned protected local reference readable by A/B; it never overwrites an existing reference or changes any key ACL.
The human-controlled P/A comparison is an explicit lab trust prerequisite, not hardware attestation or production enrollment authority.

A separately supplies B a public copy without changing CNG ACLs; keep bytes unchanged.
In a genuine B interactive session:

```powershell
& $poc lab-inspect
& $poc lab-observe $sharedPublicUserJournal
& $poc lab-check
```

Require `designated_user_b_live_token=true` and a standard account; inspect independent M/U native open and sign outcomes.
Require `original_user_evidence=matched_protected_independent_reference` before attributing U operations to A's enrolled key.
If the reference is missing, torn, unprotected, stale or mismatched, the result is `unverified` and U's native probe is not attempted.

`lab-check` must fail authorization; successful `lab-observe` means observations were collected, not that isolation passed.
Missing/unreadable user-store keys and unsupported operations remain their actual outcomes; no simulated SID test establishes ACL enforcement.

### 5. Sacrificial management and failure scenarios

```powershell
# Standard A, separate disposable snapshot; M may be changed/deleted if policy fails.
& $poc lab-rights --sacrificial-snapshot --authorized-disposable-lab
```

The probe attempts private export, writing the same DACL, taking ownership as A and deleting the exact registered PoC M.
Record every native status and stop on any unexpected success; no private output or backing-file access is exposed.
Do not treat unsupported/error results as proof of ACL denial.

In separate snapshots, the human administrator tests ACL broadening/inheritance, lost M/U, interrupted provisioning, torn registration, duplicate/concurrent setup and metadata-write denial.
Run A's `lab-check` after each fault; require failure without replacement, and preserve the exact journals/receipts.
No automatic fault injection, real-profile modification or privileged remediation is provided.

### 6. Recovery

```powershell
# A, for its exact user-key receipt; then P elevated, for its exact machine receipt.
& $poc lab-recover-user $exactUserReceipt --authorized-disposable-lab
& $poc lab-recover-machine $privateConfig $exactMachineReceipt --authorized-disposable-lab
& $poc recovery-summary
```

These commands verify PoC namespace, provider, scope, live owner, exact public key, current ACL and immutable receipt history before deletion.
Machine recovery additionally requires the complete protected fixed machine registration, matching lab UUID/configuration and exact reserved/committed key identity.
Foreign-lab receipts and incomplete/torn registration are rejected before opening a key or accepting a prior cleanup result; partial receipts remain untouched.

Missing public evidence, changed owner/ACL, denied access, torn receipts and ambiguous keysets retain incomplete accounting.
Old owner/SYSTEM-only machine receipts are not automatically adopted into the new ACL model; their cleanup remains separately authorized.

Remove local public lab files only after the relevant principal's cleanup is confirmed and the reviewer has dispositioned every partial record.
The harness never removes machine/user slot markers or automatically re-enrolls after cleanup.
Resetting a disposable snapshot is a human lab action, not proof that native deletion succeeded.

## Genuine P/A/B acceptance matrix

All native rows below are **pending**, despite green portable/hosted tests.

| Scenario | Required result/evidence |
|---|---|
| P creates/finalizes M | Native statuses plus strict post-finalization policy/owner/DACL readback |
| A opens/signs M | Native success under reviewed ACL, without administrator membership/elevation |
| A uses U | Fixed-challenge signature verifies against pinned original U |
| B opens/signs M | Explicit denial or separately reviewed restricted result; no OS isolation claim for shared signing |
| B attempts U | Native original-U use unavailable/denied; public-copy or user-store substitution cannot satisfy pinned proof |
| B attempts complete proof | Live-principal authorization failure even if public metadata is readable |
| A process restart / Windows reboot | Same pinned M/U reopen and verify; each sequence recorded separately |
| A elevated versus ordinary | Elevated enrollment rejected; diagnostic observation does not become enrollment acceptance |
| M/U unavailable | Explicit failure and retained recovery, without replacement or another allocation |
| ACL broadened/inherited | Enrollment rejects; native effect measured separately from textual comparison |
| Concurrent/duplicate setup | One reserved machine slot; loser cannot overwrite or adopt partial registration |
| Corrupt/missing/partial registration | Fail closed; existing slots/receipts preserved; total loss remains a backend/Owner Decision |
| Upgrade/uninstall/reinstall | Human executes unchanged installer sequence; records key/registration persistence and retention limits |
| Cleanup denied/interrupted | Incomplete accounting prevents complete-success reporting; no invented absence |

## Validation, TDD and hosted CI

Approved seams follow the task: authorization policy, registration persistence, runner authorization, sanitized evidence and the existing offline two-key verifier/native harness.
Observed red checkpoints: missing lab authorization (1 failure); ACL/management policy (2); authorization/key/challenge policy (3); sanitized outcome classification (3); null/duplicate registration (2).
Each checkpoint subsequently passed; registration concurrency and existing-behavior regressions were added around those slices, without claiming they prove native Windows security.

The Python receipt/source suite first failed because its implementation did not exist; a later ambiguous-deletion fixture also failed before validation was strengthened.
All five tests now pass.
Initial exclusive journal persistence was implemented together with its first fixture; it has **no separate pre-implementation red record**, so strict TDD provenance for that slice remains a review limitation.
Privileged native behavior is compiled and manually gated, not demonstrated by portable test doubles.

```powershell
cargo test --locked --example windows-device-identity provisioning::tests
cargo test --locked --example windows-device-identity windows::lab::tests
python scripts/test-identity-lab.py
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
```

Clean hosted Windows runs the original 20 PoC tests plus 27 new tests; Linux runs the portable subset.
The existing identity evidence job now probes only user keys and Candidate B diagnostics; automatic machine probes and A/C creation are removed.
Legacy machine create/recovery commands reject the unguarded path; no required check names, workflows, permissions, release intent or publishing configuration changed.

Elevated hosted runners remain diagnostic only; green CI never establishes A/B separation, reboot, effective machine ACLs or customer authorization.
Only the existing sanitized `measurements.json` artifact is uploadable; no additional local ledger/state artifact is published.
Hosted execution for implementation commit `fc1cb5c70fe3841e0c6cfe5af0126229b38d68af`:

- [Windows identity feasibility, including all original 20 PoC tests](https://github.com/oangsa/RoLauncher/actions/runs/38904809974).
- [Windows checks and Ubuntu application/package checks](https://github.com/oangsa/RoLauncher/actions/runs/38904809866).
- [Ubuntu and Arch container checks](https://github.com/oangsa/RoLauncher/actions/runs/38904809856).
- [Security inventory and evidence checks](https://github.com/oangsa/RoLauncher/actions/runs/38904809964).

These runs were queued when linked; consult their actual conclusions and [Draft PR #27](https://github.com/oangsa/RoLauncher/pull/27) for subsequent status.
Later documentation-only commits do not convert those runs into native lab evidence.

### Independent review corrections

Reviewed head: `955449bb80f7d07ecc78ee8da24df354af63b398`; the owner reported its hosted checks green.
The following changes address P2-01/P2-02; updated exact-head CI results and links are recorded in Draft PR #27.

- **P2-01:** Recovery compares the requested full lab configuration with independently protected registration and matches the complete receipt key to its planned/committed identity.
- **P2-02:** Observations enforce `user_name` consistency and exact U equality against the separate P-protected reference; unverified inputs never cause a U probe.
- **TDD:** Three recovery regressions and three observation regressions failed against the extracted prior behavior before their fixes; all now pass.
- **Ambiguous cleanup:** A further failing regression now rejects prior-deletion/absence labels lacking sufficient native status, including `NTE_BAD_KEYSET`.
- **Controls:** Authentic receipt/copy, terminal cleanup attribution, per-field key substitution and independently compared journal bytes add three passing regression controls.

The ten added review tests use synthetic references and temporary journals; they do not provision keys or prove native P/A/B isolation.
All 173 pre-existing receipt hashes still match the retained baseline; all four incomplete records remain preserved.
No privileged provisioning, reference capture, ACL mutation or cleanup ran on the developer machine.

Files modified by this review correction: `src/identity_poc/lab.rs`, `src/identity_poc/windows.rs`, `src/identity_poc/provisioning.rs`, `scripts/identity-lab.py`, and this document.

## Proposed Owner Decisions and blockers

1. Review whether Candidate A may rely on one-time privileged provisioning, and which delivery/authorization mechanism is acceptable; no automatic Candidate B substitution.
2. Approve principal selection, machine enrollment ownership, minimum effective rights, multiple-user behavior and upgrade/uninstall retention only after genuine P/A/B results.
3. Decide authenticated customer binding, trusted discovery, duplicate-allocation prevention, total-state-loss recovery and anti-rollback requirements at the backend boundary.
4. Decide TPM support/unsupported UX and whether separately verified hardware attestation is required; provider flags alone are insufficient.
5. Commission independent review and separately disposition the four old incomplete recovery records before developer-host reuse.

Remaining blockers: privileged provisioning and standard-A operation unmeasured; genuine B native isolation unmeasured; effective management denial unmeasured; reboot/lifecycle/fault matrix pending.
Protected discovery, filesystem race resistance, interrupted privileged recovery and production operations also require review.
The report deliberately stops at a Draft PR; it grants no production enrollment or architecture approval.

## Independent security-review checklist

- [ ] Confirm production startup, installer, DPAPI, dependencies, versions, allocation rules and Roblox behavior are unchanged.
- [ ] Trace every native create/set/delete path to explicit human authorization; verify hosted paths are user-only.
- [ ] Review raw SID parsing, effective token selection, standard-account membership and distinct P/A/B observations.
- [ ] Verify M owner/DACL acceptance, provider/scope/export/usage readback and actual A/B private operations.
- [ ] Execute sacrificial export/DACL/owner/delete probes; separate denied, unsupported, ambiguous and successful outcomes.
- [ ] Review ProgramData discovery, ancestor/path races, protected file creation, exclusive slots and canonical journal parsing.
- [ ] Confirm both pinned keys, live authorized SID, reservation binding, synthetic purpose and fresh nonce reach the existing verifier.
- [ ] Exercise partial writes, key loss, broadened ACLs and interrupted cleanup; retain every incomplete record.
- [ ] Inspect only sanitized public evidence; keep configuration, names, blobs, profiles, receipts and local baselines private.
- [ ] Record actual P/A/B, restart/reboot and lifecycle results before accepting feasibility or recommending a production design.

## Exact changed files

- `docs/security/WINDOWS-MACHINE-KEY-PROVISIONING-VALIDATION.md`: review package, limitations and manual acceptance procedure.
- `src/identity_poc/provisioning.rs`: portable authorization/ACL/journal/report policy and 15 tests.
- `src/identity_poc/lab.rs`: guarded native lab commands and two read-only guard/configuration tests.
- `src/identity_poc/mod.rs`: example-only policy module declaration.
- `src/identity_poc/windows.rs`: lab dispatch and legacy machine-creation rejection.
- `src/identity_poc/recovery.rs`: reuse protected file creation and reject unguarded machine recovery.
- `scripts/identity-lab.py`: read-only aggregate preflight, local baseline comparison and source checks.
- `scripts/test-identity-lab.py`: portable receipt/source fixtures.
- `scripts/test-identity-poc.ps1`: run source/receipt fixtures and remove automatic machine provisioning.

## Microsoft references

- [Persisted CNG creation, machine scope and no-overwrite behavior](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptcreatepersistedkey).
- [Key/provider properties, security descriptors, export policy and usage](https://learn.microsoft.com/en-us/windows/win32/seccng/key-storage-property-identifiers).
- [Security property flags and persistence](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptsetproperty), [persisted-key open](https://learn.microsoft.com/en-us/windows/win32/api/ncrypt/nf-ncrypt-ncryptopenkey).
- [Standard rights including delete, DACL and ownership management](https://learn.microsoft.com/en-us/windows/win32/secauthz/standard-access-rights), [access masks](https://learn.microsoft.com/en-us/windows/win32/secauthz/access-rights-and-access-masks).
- [TPM key attestation and its trust requirements](https://learn.microsoft.com/en-us/windows-server/identity/ad-ds/manage/component-updates/tpm-key-attestation).

These references define APIs and security concepts; they do not establish observed success on the target lab.
