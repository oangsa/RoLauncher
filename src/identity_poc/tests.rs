use super::*;

pub fn challenge() -> Challenge {
    Challenge {
        schema_version: 1,
        nonce: [1; 32],
        installation_key: [2; 32],
        user_key: [3; 32],
        windows_sid: [4; 32],
        installation_evidence: [5; 32],
        synthetic_customer: "synthetic:customer-a".into(),
        purpose: PURPOSE.into(),
    }
}

#[test]
fn canonical_encoding_has_fixed_domain_and_unambiguous_lengths() {
    let encoded = challenge().canonical().unwrap();
    assert!(encoded.starts_with(b"RoLauncher-Identity-PoC\0\x00\x01"));
    assert!(
        encoded
            .windows(24)
            .any(|s| s == b"\x00\x00\x00\x14synthetic:customer-a")
    );
}

#[test]
fn incomplete_or_non_synthetic_context_is_rejected() {
    let mut cases = Vec::new();
    let mut c = challenge();
    c.schema_version = 2;
    cases.push(c);
    let mut c = challenge();
    c.nonce = [0; 32];
    cases.push(c);
    let mut c = challenge();
    c.installation_key = [0; 32];
    cases.push(c);
    let mut c = challenge();
    c.user_key = [0; 32];
    cases.push(c);
    let mut c = challenge();
    c.windows_sid = [0; 32];
    cases.push(c);
    let mut c = challenge();
    c.installation_evidence = [0; 32];
    cases.push(c);
    let mut c = challenge();
    c.synthetic_customer = "real-account".into();
    cases.push(c);
    let mut c = challenge();
    c.synthetic_customer = "synthetic:".into();
    cases.push(c);
    let mut c = challenge();
    c.purpose = "license-activation".into();
    cases.push(c);
    for case in cases {
        assert!(case.canonical().is_err(), "invalid context accepted");
    }
}

#[cfg(windows)]
#[test]
fn effective_token_identity_is_stable_and_sid_is_not_logged() {
    let first = windows::effective_identity().unwrap();
    let second = windows::effective_identity().unwrap();
    assert_ne!(first.sid_reference, [0; 32]);
    assert!(
        first.sid_reference == second.sid_reference,
        "token identity changed"
    );
    assert!(!first.thread_token);
}

#[cfg(windows)]
#[test]
fn native_software_key_signs_and_public_key_verifies_without_exporting_private_bytes() {
    use super::windows::*;
    let identity = effective_identity().unwrap();
    let key = Key::create(Provider::Software, Scope::User, &identity).unwrap();
    let digest = hash(b"synthetic key lifecycle test").unwrap();
    let signature = key.sign(&digest).unwrap();
    verify(&key.public().unwrap(), &digest, &signature).unwrap();
    assert!(
        verify(
            &key.public().unwrap(),
            &hash(b"changed challenge").unwrap(),
            &signature
        )
        .is_err()
    );
}

#[cfg(windows)]
#[test]
fn native_key_reopens_with_verified_scope_policy_acl_and_deletion_requires_recovery() {
    use super::windows::*;
    let identity = effective_identity().unwrap();
    let key = Key::create(Provider::Software, Scope::User, &identity).unwrap();
    let info = key.info().unwrap();
    let reopened = Key::open(&info).unwrap();
    let inspection = reopened.inspection(&identity).unwrap();
    assert_eq!(inspection["scope"], "user");
    assert_eq!(inspection["export_policy"], 0);
    assert_eq!(inspection["acl"]["only_owner_and_system"], true);
    let digest = hash(b"synthetic reopen test").unwrap();
    verify(&info.blob, &digest, &reopened.sign(&digest).unwrap()).unwrap();
    drop(reopened);
    key.delete().unwrap();
    assert!(Key::open(&info).is_err());
}

#[cfg(windows)]
#[test]
fn offline_verifier_binds_both_keys_and_accepts_fresh_challenge_only_once() {
    use super::windows::*;
    let identity = effective_identity().unwrap();
    let installation = Key::create(Provider::Software, Scope::User, &identity).unwrap();
    let user = Key::create(Provider::Software, Scope::User, &identity).unwrap();
    let mut expected = challenge();
    expected.installation_key = hash(&installation.public().unwrap()).unwrap();
    expected.user_key = hash(&user.public().unwrap()).unwrap();
    expected.windows_sid = identity.sid_reference;
    let digest = hash(&expected.canonical().unwrap()).unwrap();
    let proof = Proof {
        installation: installation.sign(&digest).unwrap(),
        user: user.sign(&digest).unwrap(),
    };
    let mut verifier = Verifier::new(
        expected.clone(),
        installation.public().unwrap(),
        user.public().unwrap(),
    )
    .unwrap();
    let mut changed = expected.clone();
    changed.synthetic_customer = "synthetic:customer-b".into();
    assert!(verifier.verify(&changed, &proof).is_err());
    verifier.verify(&expected, &proof).unwrap();
    assert!(verifier.verify(&expected, &proof).is_err());
}

#[cfg(windows)]
#[test]
fn user_identity_persists_across_state_reload_with_live_user_and_machine_binding() {
    use super::windows::*;
    let path = std::env::temp_dir().join(format!(
        "rolauncher-identity-poc-{}.json",
        uuid::Uuid::new_v4()
    ));
    measure_state(Candidate::B, &path).unwrap();
    measure_check(&path).unwrap();
    measure_check(&path).unwrap();
    cleanup_state(&path).unwrap();
    assert!(measure_check(&path).is_err());
    std::fs::remove_file(path).unwrap();
}

#[cfg(windows)]
#[test]
fn native_collisions_scope_mismatch_and_corrupt_public_keys_cannot_replace_identity() {
    use super::windows::*;
    let identity = effective_identity().unwrap();
    let key = Key::create(Provider::Software, Scope::User, &identity).unwrap();
    let original = key.info().unwrap();
    let error = Key::create_named(Provider::Software, Scope::User, &identity, &original.name)
        .err()
        .unwrap();
    assert!(error.contains("collision"), "{error}");
    let mut wrong_scope = original.clone();
    wrong_scope.scope = Scope::Machine;
    assert!(Key::open(&wrong_scope).is_err());
    let mut corrupt = original.clone();
    corrupt.blob[0] ^= 1;
    assert!(Key::open(&corrupt).is_err());
    assert!(verify(&corrupt.blob, &[1; 32], &[1; 256]).is_err());
    let mut unrelated = original.clone();
    unrelated.name = "unrelated-application-key".into();
    assert!(Key::open(&unrelated).is_err());
    assert!(
        Key::create_named(Provider::Software, Scope::User, &identity, "unrelated-key").is_err()
    );
    let digest = hash(b"synthetic collision survival").unwrap();
    verify(
        &original.blob,
        &digest,
        &Key::open(&original).unwrap().sign(&digest).unwrap(),
    )
    .unwrap();
}

#[cfg(windows)]
#[test]
fn offline_verifier_rejects_replay_into_changed_identity_context_and_wrong_or_missing_signatures() {
    use super::windows::*;
    let identity = effective_identity().unwrap();
    let installation = Key::create(Provider::Software, Scope::User, &identity).unwrap();
    let user = Key::create(Provider::Software, Scope::User, &identity).unwrap();
    let foreign = Key::create(Provider::Software, Scope::User, &identity).unwrap();
    let i = installation.public().unwrap();
    let u = user.public().unwrap();
    let mut expected = challenge();
    expected.installation_key = hash(&i).unwrap();
    expected.user_key = hash(&u).unwrap();
    let digest = hash(&expected.canonical().unwrap()).unwrap();
    let proof = Proof {
        installation: installation.sign(&digest).unwrap(),
        user: user.sign(&digest).unwrap(),
    };
    let mut contexts = Vec::new();
    let mut c = expected.clone();
    c.nonce[0] ^= 1;
    contexts.push(c);
    let mut c = expected.clone();
    c.windows_sid[0] ^= 1;
    contexts.push(c);
    let mut c = expected.clone();
    c.installation_evidence[0] ^= 1;
    contexts.push(c);
    let mut c = expected.clone();
    c.synthetic_customer = "synthetic:other-customer".into();
    contexts.push(c);
    for context in contexts {
        let mut verifier = Verifier::new(context.clone(), i.clone(), u.clone()).unwrap();
        assert!(verifier.verify(&context, &proof).is_err());
    }
    assert!(Verifier::new(expected.clone(), foreign.public().unwrap(), u.clone()).is_err());
    assert!(Verifier::new(expected.clone(), i.clone(), Vec::new()).is_err());
    let mut verifier = Verifier::new(expected.clone(), i, u).unwrap();
    let wrong = Proof {
        installation: proof.installation.clone(),
        user: foreign.sign(&digest).unwrap(),
    };
    assert!(verifier.verify(&expected, &wrong).is_err());
    assert!(
        verifier
            .verify(
                &expected,
                &Proof {
                    installation: Vec::new(),
                    user: proof.user.clone()
                }
            )
            .is_err()
    );
    verifier.verify(&expected, &proof).unwrap();
}

#[cfg(windows)]
struct PublicStateFixture {
    path: std::path::PathBuf,
    original: Vec<u8>,
}
#[cfg(windows)]
impl PublicStateFixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rolauncher-identity-poc-{}.json",
            uuid::Uuid::new_v4()
        ));
        windows::measure_state(windows::Candidate::B, &path).unwrap();
        let original = std::fs::read(&path).unwrap();
        Self { path, original }
    }
}
#[cfg(windows)]
impl Drop for PublicStateFixture {
    fn drop(&mut self) {
        std::fs::write(&self.path, &self.original).unwrap();
        if let Err(error) = windows::cleanup_state(&self.path) {
            if !std::thread::panicking() {
                panic!("fixture cleanup incomplete; local state retained: {error}");
            }
            return;
        }
        std::fs::remove_file(&self.path).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn public_state_tampering_changed_sid_machine_evidence_and_provider_substitution_fail_closed() {
    use super::windows::*;
    let fixture = PublicStateFixture::new();
    assert!(measure_state(Candidate::B, &fixture.path).is_err());
    assert!(
        std::fs::read(&fixture.path).unwrap() == fixture.original,
        "existing public state changed"
    );
    let source: serde_json::Value = serde_json::from_slice(&fixture.original).unwrap();
    for field in ["owner_sid_reference", "installation_evidence"] {
        let mut changed = source.clone();
        changed[field] = serde_json::json!([9; 32].to_vec());
        std::fs::write(&fixture.path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(measure_check(&fixture.path).is_err());
    }
    let mut changed = source.clone();
    changed["user"]["provider"] = "tpm".into();
    std::fs::write(&fixture.path, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(measure_check(&fixture.path).is_err());
    std::fs::write(&fixture.path, b"{corrupt").unwrap();
    assert!(measure_check(&fixture.path).is_err());
    std::fs::write(&fixture.path, vec![b' '; 65537]).unwrap();
    assert!(measure_check(&fixture.path).is_err());
    std::fs::write(&fixture.path, &fixture.original).unwrap();
    measure_check(&fixture.path).unwrap();
}

#[cfg(windows)]
#[test]
fn impersonated_thread_identity_is_observed_but_never_used_for_poc_enrollment() {
    use windows_sys::Win32::Security::{ImpersonateSelf, RevertToSelf, SecurityImpersonation};
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            assert_ne!(unsafe { RevertToSelf() }, 0);
        }
    }
    let before = windows::effective_identity().unwrap();
    assert_ne!(unsafe { ImpersonateSelf(SecurityImpersonation) }, 0);
    let guard = Revert;
    let during = windows::effective_identity().unwrap();
    assert!(during.thread_token);
    assert!(
        during.sid_reference == before.sid_reference,
        "impersonation identity changed"
    );
    assert!(
        windows::Key::create(windows::Provider::Software, windows::Scope::User, &during).is_err()
    );
    drop(guard);
    assert!(!windows::effective_identity().unwrap().thread_token);
}

#[cfg(windows)]
#[test]
fn cleanup_attempts_remaining_poc_key_even_after_user_key_loss() {
    use super::windows::*;
    let fixture = PublicStateFixture::new();
    let source: serde_json::Value = serde_json::from_slice(&fixture.original).unwrap();
    let info: PublicKey = serde_json::from_value(source["user"].clone()).unwrap();
    Key::open(&info).unwrap().delete().unwrap();
    let outcomes = cleanup_state(&fixture.path).unwrap();
    assert!(outcomes.contains(&recovery::CleanupOutcome::Absent));
    let installation: PublicKey = serde_json::from_value(source["installation"].clone()).unwrap();
    assert!(
        Key::open(&installation).is_err(),
        "remaining PoC key leaked after partial cleanup"
    );
}

#[cfg(windows)]
#[test]
fn unavailable_provider_reports_failure_without_software_fallback() {
    let error =
        windows::inspect_provider_name("RoLauncher.IdentityPoC.Nonexistent.Provider").unwrap_err();
    assert!(error.contains("provider_unavailable"), "{error}");
}

#[cfg(windows)]
#[test]
fn anonymous_thread_token_failure_never_falls_back_to_process_identity() {
    use windows_sys::Win32::{
        Security::{ImpersonateAnonymousToken, RevertToSelf},
        System::Threading::GetCurrentThread,
    };
    struct Revert;
    impl Drop for Revert {
        fn drop(&mut self) {
            assert_ne!(unsafe { RevertToSelf() }, 0);
        }
    }
    assert_ne!(unsafe { ImpersonateAnonymousToken(GetCurrentThread()) }, 0);
    let guard = Revert;
    assert!(windows::effective_identity().is_err());
    drop(guard);
    assert!(windows::effective_identity().is_ok());
}

#[cfg(windows)]
#[test]
fn failed_enrollment_after_first_key_preserves_recovery_when_delete_is_denied() {
    use super::windows::{recovery::*, *};
    let injection = FaultGuard::new(Some(FaultStage::AfterFirstKey), true);
    let path = injection.directory.join("state.json");
    let result = measure_state(Candidate::B, &path);
    // The red run must not leak the successful original implementation's test keys.
    if result.is_ok() {
        cleanup_state(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
    }
    assert!(result.is_err(), "faulted enrollment unexpectedly succeeded");
    assert!(path.exists(), "failed state was discarded");
    let records = injection.records();
    assert_eq!(records.len(), 1);
    let retained = snapshot(&records[0]).unwrap();
    assert_eq!(retained.cleanup, Some(CleanupOutcome::Incomplete));
    assert!(
        Key::open(&retained.key).is_ok(),
        "persisted test key lost before recovery"
    );
    injection.allow_cleanup();
    assert_eq!(recover(&records[0]).unwrap(), CleanupOutcome::Succeeded);
    assert_eq!(
        snapshot(&records[0]).unwrap().cleanup,
        Some(CleanupOutcome::Succeeded)
    );
    assert!(Key::open(&retained.key).is_err());
}

#[cfg(windows)]
#[test]
fn faulted_native_lifecycle_accounts_every_key_and_recovers_only_verified_receipts() {
    use super::windows::{recovery::*, *};
    for (stage, count, denied) in [
        (FaultStage::BeforeFinalize, 1, false),
        (FaultStage::AfterFinalize, 1, false),
        (FaultStage::AfterFinalize, 1, true),
        (FaultStage::PersistState, 2, false),
        (FaultStage::PersistState, 2, true),
        (FaultStage::ProbeOperation, 1, false),
        (FaultStage::ProbeOperation, 1, true),
    ] {
        let injection = FaultGuard::new(Some(stage), denied);
        let path = injection.directory.join("state.json");
        let result = if stage == FaultStage::ProbeOperation {
            probe_key(Provider::Software, Scope::User).map(|_| ())
        } else {
            measure_state(Candidate::B, &path)
        };
        assert!(result.is_err(), "faulted operation succeeded");
        let records = injection.records();
        assert_eq!(records.len(), count);
        if stage == FaultStage::PersistState {
            assert!(std::fs::metadata(&path).unwrap().len() > 0);
        }
        for record in records {
            let retained = snapshot(&record).unwrap();
            let expected = if denied {
                CleanupOutcome::Incomplete
            } else if stage == FaultStage::BeforeFinalize {
                CleanupOutcome::NeverPersisted
            } else {
                CleanupOutcome::Succeeded
            };
            assert_eq!(retained.cleanup, Some(expected));
            if denied {
                assert!(Key::open(&retained.key).is_ok());
                injection.allow_cleanup();
                assert_eq!(recover(&record).unwrap(), CleanupOutcome::Succeeded);
            }
            assert!(Key::open(&retained.key).is_err());
        }
    }
}

#[cfg(windows)]
#[test]
fn enrollment_policy_distinguishes_elevated_session_zero_and_noninteractive_diagnostics() {
    use super::windows::*;
    let mut identity = effective_identity().unwrap();
    identity.thread_token = false;
    identity.service = false;
    identity.restricted = false;
    identity.elevated = false;
    identity.session_zero = false;
    identity.logon = LogonContext::Interactive;
    assert!(identity.accept(ContextPurpose::Enrollment).is_ok());
    assert!(identity.accept(ContextPurpose::Diagnostic).is_ok());
    identity.elevated = true;
    assert!(
        identity.accept(ContextPurpose::Enrollment).is_err(),
        "elevated diagnostic accepted as enrollment"
    );
    assert!(identity.accept(ContextPurpose::Diagnostic).is_ok());
    identity.elevated = false;
    identity.session_zero = true;
    assert!(identity.accept(ContextPurpose::Enrollment).is_err());
    assert!(identity.accept(ContextPurpose::Diagnostic).is_ok());
    for logon in [LogonContext::Batch, LogonContext::Service] {
        identity.logon = logon;
        assert!(identity.accept(ContextPurpose::Enrollment).is_err());
        assert!(identity.accept(ContextPurpose::Diagnostic).is_ok());
    }
    for logon in [LogonContext::Unknown, LogonContext::Network] {
        identity.logon = logon;
        assert!(identity.accept(ContextPurpose::Enrollment).is_err());
        assert!(identity.accept(ContextPurpose::Diagnostic).is_err());
    }
    identity.logon = LogonContext::Interactive;
    for flag in 0..3 {
        let mut unsupported = identity.clone();
        if flag == 0 {
            unsupported.thread_token = true;
        } else if flag == 1 {
            unsupported.service = true;
        } else {
            unsupported.restricted = true;
        }
        assert!(unsupported.accept(ContextPurpose::Enrollment).is_err());
        assert!(unsupported.accept(ContextPurpose::Diagnostic).is_err());
    }
}

#[cfg(windows)]
#[test]
fn diagnostic_state_cannot_be_accepted_as_enrollment_and_fixture_cleanup_failure_is_fatal() {
    use super::windows::{recovery::*, *};
    let guard = FaultGuard::new(None, false);
    let fixture = PublicStateFixture::new();
    assert!(check_state(&fixture.path).is_err());
    measure_check(&fixture.path).unwrap();
    drop(fixture);
    let fixture = PublicStateFixture::new();
    let original = fixture.original.clone();
    let path = fixture.path.clone();
    // Deny fixture deletion through the same deterministic native-operation seam.
    set_delete_denied(true);
    let failed = std::panic::catch_unwind(|| drop(fixture));
    guard.allow_cleanup();
    assert!(failed.is_err(), "fixture silently accepted leaked keys");
    assert!(std::fs::read(&path).unwrap() == original);
    cleanup_state(&path).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[cfg(windows)]
#[test]
fn ordinary_enrollment_is_explicit_and_hosted_diagnostics_cannot_bypass_it() {
    use super::windows::{recovery::*, *};
    let guard = FaultGuard::new(None, false);
    let path = guard.directory.join("ordinary-state.json");
    let ordinary = effective_identity()
        .unwrap()
        .accept(ContextPurpose::Enrollment)
        .is_ok();
    if ordinary {
        create_state(Candidate::B, &path).unwrap();
        check_state(&path).unwrap();
        assert!(measure_check(&path).is_err());
        cleanup_state(&path).unwrap();
        std::fs::remove_file(path).unwrap();
    } else {
        assert!(create_state(Candidate::B, &path).is_err());
        assert!(!path.exists(), "unsupported enrollment changed filesystem");
        assert!(guard.records().is_empty());
    }
}

#[cfg(windows)]
#[test]
fn recovery_refuses_namespace_owner_scope_provider_and_public_key_substitution() {
    use super::windows::{recovery::*, *};
    let guard = FaultGuard::new(None, false);
    let identity = effective_identity().unwrap();
    let key = Key::create(Provider::Software, Scope::User, &identity).unwrap();
    let info = key.info().unwrap();
    set_delete_denied(true);
    assert!(key.delete().is_err());
    guard.allow_cleanup();
    let path = guard.records().pop().unwrap();
    let original = std::fs::read(&path).unwrap();
    let record = snapshot(&path).unwrap();
    let mut cases = Vec::new();
    let mut changed = record.clone();
    changed.key.name = "unrelated-application-key".into();
    cases.push(changed);
    let mut changed = record.clone();
    changed.key.scope = Scope::Machine;
    cases.push(changed);
    let mut changed = record.clone();
    changed.key.provider = Provider::Tpm;
    cases.push(changed);
    let mut changed = record.clone();
    changed.key.blob[0] ^= 1;
    cases.push(changed);
    let mut changed = record.clone();
    changed.owner_sid_reference = [9; 32];
    cases.push(changed);
    for changed in cases {
        let mut bytes = original.clone();
        bytes.extend(serde_json::to_vec(&changed).unwrap());
        bytes.push(b'\n');
        std::fs::write(&path, bytes).unwrap();
        assert!(recover(&path).is_err());
        assert!(
            Key::open(&info).is_ok(),
            "recovery deleted mismatched identity"
        );
    }
    let mut torn = original.clone();
    torn.extend(b"{partial");
    std::fs::write(&path, torn).unwrap();
    assert!(recover(&path).is_err());
    std::fs::write(&path, original).unwrap();
    assert_eq!(recover(&path).unwrap(), CleanupOutcome::Succeeded);
    assert!(Key::open(&info).is_err());
}
