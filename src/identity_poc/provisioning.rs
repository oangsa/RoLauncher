//! Local synthetic validation only. Policy fixtures cannot establish native Windows guarantees.

pub const LAB_CONFIRMATION: &str = "AUTHORIZED DISPOSABLE WINDOWS LAB";

pub fn authorize_mutation(
    explicit_lab: bool,
    confirmation: &str,
    hosted: bool,
) -> Result<(), String> {
    if !explicit_lab || confirmation.trim() != LAB_CONFIRMATION || hosted {
        return Err(
            "mutation requires human-confirmed disposable lab; hosted execution denied".into(),
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Principal {
    Provisioner,
    System,
    Authorized,
    Unauthorized,
    Administrators,
    Other,
}

#[derive(Clone)]
pub struct Grant {
    pub principal: Principal,
    pub mask: u32,
    pub flags: u8,
    pub allow: bool,
}

pub struct KeyAcl {
    pub owner: Principal,
    pub protected: bool,
    pub grants: Vec<Grant>,
}

pub fn validate_machine_acl(acl: &KeyAcl) -> Result<(), String> {
    if acl.owner != Principal::Provisioner || !acl.protected || acl.grants.len() != 3 {
        return Err("unreviewed owner or DACL; recovery required".into());
    }
    let mut seen = [false; 3];
    for grant in &acl.grants {
        if !grant.allow || grant.flags != 0 {
            return Err("unexpected ACE type or inheritance".into());
        }
        let index = match grant.principal {
            Principal::Provisioner => 0,
            Principal::System => 1,
            Principal::Authorized => 2,
            _ => return Err("unexpected machine-key principal".into()),
        };
        let permitted = if index == 2 {
            // Experimental GR request; provider expansion may include read/control/synchronize.
            // No write, delete, WRITE_DAC or WRITE_OWNER rights. Native signing still must succeed.
            grant.mask != 0 && grant.mask & !0x80120089 == 0 && grant.mask & (0x80000000 | 1) != 0
        } else {
            [0x10000000, 0x001f01ff, 0xd01f01ff].contains(&grant.mask)
        };
        if seen[index] || !permitted {
            return Err("unexpected key rights or duplicate principal".into());
        }
        seen[index] = true;
    }
    Ok(())
}

pub struct EnrollmentEvidence {
    pub finalized: bool,
    pub acl_accepted: bool,
    pub ordinary_user: bool,
    pub machine_key: Option<[u8; 32]>,
    pub user_key: Option<[u8; 32]>,
    pub authorized_sid: [u8; 32],
    pub live_sid: [u8; 32],
}

/// Validates local policy inputs only; signature verification remains mandatory at the native seam.
pub fn validate_enrollment(
    evidence: &EnrollmentEvidence,
    expected: &super::Challenge,
    supplied: &super::Challenge,
) -> Result<(), String> {
    expected.canonical()?;
    if !evidence.finalized
        || !evidence.acl_accepted
        || !evidence.ordinary_user
        || evidence.authorized_sid != evidence.live_sid
        || evidence.authorized_sid != expected.windows_sid
        || evidence.machine_key != Some(expected.installation_key)
        || evidence.user_key != Some(expected.user_key)
        || expected.installation_key == expected.user_key
        || expected != supplied
    {
        return Err("local enrollment policy rejected; recovery required".into());
    }
    Ok(())
}

use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    schema: u8,
    committed: bool,
    reservation: serde_json::Value,
    evidence: Option<serde_json::Value>,
}

/// Caller must independently protect the parent directory and verify native ACLs.
/// Exclusive creation reserves one fixed slot; a torn or incomplete journal blocks reuse.
pub struct Registration {
    file: File,
    reservation: serde_json::Value,
}

impl Registration {
    #[cfg(test)]
    pub fn reserve(path: &Path, reservation: serde_json::Value) -> Result<Self, String> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| "registration exists or is inaccessible; no overwrite")?;
        Self::from_new_file(file, reservation)
    }

    pub fn from_new_file(file: File, reservation: serde_json::Value) -> Result<Self, String> {
        let mut journal = Self { file, reservation };
        journal.append(None)?;
        Ok(journal)
    }

    fn append(&mut self, evidence: Option<serde_json::Value>) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(&Entry {
            schema: 1,
            committed: evidence.is_some(),
            reservation: self.reservation.clone(),
            evidence,
        })
        .map_err(|_| "registration encoding failed")?;
        if bytes.len() > 16384 {
            return Err("registration too large; reservation retained".into());
        }
        bytes.push(b'\n');
        self.file
            .write_all(&bytes)
            .and_then(|_| self.file.sync_all())
            .map_err(|_| "registration incomplete; retain recovery evidence".into())
    }

    pub fn commit(mut self, evidence: serde_json::Value) -> Result<(), String> {
        if evidence.is_null() {
            return Err("missing public evidence; reservation retained".into());
        }
        self.append(Some(evidence))
    }
}

pub fn read_registration(path: &Path) -> Result<(serde_json::Value, serde_json::Value), String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "registration missing; recovery required")?
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|_| "registration unreadable; recovery required")?;
    decode_registration(&bytes)
}

pub fn decode_registration(bytes: &[u8]) -> Result<(serde_json::Value, serde_json::Value), String> {
    if bytes.len() > 32768 || !bytes.ends_with(b"\n") {
        return Err("registration torn or oversized; preserve all recovery evidence".into());
    }
    let entries: Vec<Entry> = bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| {
            let entry: Entry = serde_json::from_slice(line)
                .map_err(|_| "registration corrupt; recovery required")?;
            // Only harness-written canonical journal lines are accepted. This also rejects
            // duplicate nested JSON keys that Value would otherwise silently collapse.
            if serde_json::to_vec(&entry).map_err(|_| "invalid registration encoding")? != line {
                return Err("registration noncanonical or ambiguous; recovery required");
            }
            Ok(entry)
        })
        .collect::<Result<_, _>>()?;
    if entries.len() != 2
        || entries.iter().any(|e| e.schema != 1)
        || entries[0].committed
        || entries[0].evidence.is_some()
        || !entries[1].committed
        || entries[0].reservation != entries[1].reservation
    {
        return Err("registration partial or identity changed; recovery required".into());
    }
    Ok((
        entries[1].reservation.clone(),
        entries[1]
            .evidence
            .clone()
            .ok_or("registration evidence missing")?,
    ))
}

pub fn uploadable_observation(
    result: Result<(), String>,
    genuine_other_user: bool,
    incomplete_cleanup: usize,
) -> serde_json::Value {
    let status = result
        .as_ref()
        .err()
        .and_then(|error| error.split("0x").nth(1))
        .and_then(|hex| hex.get(..8))
        .and_then(|hex| u32::from_str_radix(hex, 16).ok());
    let outcome = if result.is_ok() {
        "succeeded"
    } else {
        match status {
            Some(0x80090010 | 0x80070005) => "denied",
            Some(0x80090016) => "ambiguous",
            _ => "failed",
        }
    };
    let category = result.as_ref().err().map(|error| {
        [
            "lab machine create",
            "lab set machine owner and ACL",
            "lab finalize machine key",
            "reopen key",
            "sign synthetic challenge",
            "fixed lab rights operation",
        ]
        .into_iter()
        .find(|prefix| error.starts_with(prefix))
        .unwrap_or("policy_or_local_state")
    });
    // Allowlisted fields only: never serialize an error, path, SID, name, blob or caller label.
    serde_json::json!({"outcome": outcome, "native_status": status, "operation_category":category,
        "cross_user": if genuine_other_user { "distinct_live_token_observed_requires_review" } else { "not_established" },
        "incomplete_cleanup": incomplete_cleanup, "complete_success": false,
        "enrollment_accepted": false, "customer_authenticated": false,
        "independent_review": "pending", "reboot": "pending_manual_test"})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privileged_runner_requires_explicit_disposable_lab_and_human_confirmation() {
        assert!(authorize_mutation(false, LAB_CONFIRMATION, false).is_err());
        assert!(authorize_mutation(true, "yes", false).is_err());
        assert!(authorize_mutation(true, LAB_CONFIRMATION, true).is_err());
        authorize_mutation(true, LAB_CONFIRMATION, false).unwrap();
    }

    fn acl() -> KeyAcl {
        KeyAcl {
            owner: Principal::Provisioner,
            protected: true,
            grants: vec![
                Grant {
                    principal: Principal::Provisioner,
                    mask: 0x10000000,
                    flags: 0,
                    allow: true,
                },
                Grant {
                    principal: Principal::System,
                    mask: 0xd01f01ff,
                    flags: 0,
                    allow: true,
                },
                Grant {
                    principal: Principal::Authorized,
                    mask: 0x80000000,
                    flags: 0,
                    allow: true,
                },
            ],
        }
    }

    #[test]
    fn broadened_or_inherited_machine_acl_and_user_ownership_are_rejected() {
        validate_machine_acl(&acl()).unwrap();
        let mut changed = acl();
        changed.grants.push(Grant {
            principal: Principal::Other,
            mask: 0x80000000,
            flags: 0,
            allow: true,
        });
        assert!(validate_machine_acl(&changed).is_err());
        let mut changed = acl();
        changed.protected = false;
        assert!(validate_machine_acl(&changed).is_err());
        let mut changed = acl();
        changed.grants[2].flags = 0x10;
        assert!(validate_machine_acl(&changed).is_err());
        let mut changed = acl();
        changed.owner = Principal::Authorized;
        assert!(validate_machine_acl(&changed).is_err());
    }

    #[test]
    fn key_use_does_not_grant_key_management_rights() {
        for mask in [
            0x10000000, 0x40000000, 0x00010000, 0x00040000, 0x00080000, 0x00000100,
        ] {
            let mut changed = acl();
            changed.grants[2].mask |= mask;
            assert!(
                validate_machine_acl(&changed).is_err(),
                "management rights accepted"
            );
        }
        let mut expanded = acl();
        expanded.grants[2].mask = 0x80120089;
        validate_machine_acl(&expanded).unwrap();
    }

    fn enrollment() -> EnrollmentEvidence {
        EnrollmentEvidence {
            finalized: true,
            acl_accepted: true,
            ordinary_user: true,
            machine_key: Some([2; 32]),
            user_key: Some([3; 32]),
            authorized_sid: [4; 32],
            live_sid: [4; 32],
        }
    }

    #[test]
    fn unauthorized_user_and_invalid_provisioning_never_satisfy_enrollment() {
        let challenge = crate::identity_poc::tests::challenge();
        validate_enrollment(&enrollment(), &challenge, &challenge).unwrap();
        let mut evidence = enrollment();
        evidence.live_sid = [9; 32];
        assert!(validate_enrollment(&evidence, &challenge, &challenge).is_err());
        let mut evidence = enrollment();
        evidence.finalized = false;
        assert!(validate_enrollment(&evidence, &challenge, &challenge).is_err());
        let mut evidence = enrollment();
        evidence.acl_accepted = false;
        assert!(validate_enrollment(&evidence, &challenge, &challenge).is_err());
    }

    #[test]
    fn changed_or_missing_machine_and_user_keys_fail_closed() {
        let challenge = crate::identity_poc::tests::challenge();
        for (machine_key, user_key) in [
            (None, Some([3; 32])),
            (Some([2; 32]), None),
            (Some([8; 32]), Some([3; 32])),
            (Some([2; 32]), Some([8; 32])),
        ] {
            let mut evidence = enrollment();
            evidence.machine_key = machine_key;
            evidence.user_key = user_key;
            assert!(validate_enrollment(&evidence, &challenge, &challenge).is_err());
        }
    }

    #[test]
    fn stale_challenge_and_elevated_diagnostics_cannot_authorize_enrollment() {
        let challenge = crate::identity_poc::tests::challenge();
        for field in 0..3 {
            let mut supplied = challenge.clone();
            match field {
                0 => supplied.nonce = [9; 32],
                1 => supplied.installation_evidence = [9; 32],
                _ => supplied.synthetic_customer = "synthetic:unrelated".into(),
            }
            assert!(validate_enrollment(&enrollment(), &challenge, &supplied).is_err());
        }
        let mut evidence = enrollment();
        evidence.ordinary_user = false;
        assert!(validate_enrollment(&evidence, &challenge, &challenge).is_err());
    }

    #[test]
    fn registration_is_exclusive_and_incomplete_state_preserves_recovery_evidence() {
        let dir =
            std::env::temp_dir().join(format!("rolauncher-registration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("registration.jsonl");
        let journal =
            Registration::reserve(&path, serde_json::json!({"synthetic_receipt": "retained"}))
                .unwrap();
        assert!(read_registration(&path).is_err());
        assert!(Registration::reserve(&path, serde_json::json!({})).is_err());
        journal
            .commit(serde_json::json!({"machine_key": "synthetic:original"}))
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(Registration::reserve(&path, serde_json::json!({})).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let (reservation, evidence) = read_registration(&path).unwrap();
        assert_eq!(reservation["synthetic_receipt"], "retained");
        assert_eq!(evidence["machine_key"], "synthetic:original");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{torn")
            .unwrap();
        assert!(read_registration(&path).is_err());
        assert!(std::fs::read(&path).unwrap().starts_with(&original));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_registration_has_one_winner_and_cannot_retry_partial_enrollment() {
        let dir =
            std::env::temp_dir().join(format!("rolauncher-registration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("registration.jsonl");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    Registration::reserve(&path, serde_json::json!({"synthetic":true})).is_ok()
                })
            })
            .collect();
        assert_eq!(
            threads
                .into_iter()
                .map(|t| usize::from(t.join().unwrap()))
                .sum::<usize>(),
            1
        );
        assert!(read_registration(&path).is_err());
        assert!(Registration::reserve(&path, serde_json::json!({})).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn null_commit_cannot_report_completed_registration_and_retains_reservation() {
        let dir =
            std::env::temp_dir().join(format!("rolauncher-registration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("registration.jsonl");
        let journal = Registration::reserve(&path, serde_json::json!({"synthetic":true})).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(journal.commit(serde_json::Value::Null).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(read_registration(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ambiguous_duplicate_json_fields_in_registration_are_rejected() {
        let dir =
            std::env::temp_dir().join(format!("rolauncher-registration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("registration.jsonl");
        Registration::reserve(&path, serde_json::json!({"synthetic":true}))
            .unwrap()
            .commit(serde_json::json!({"public_evidence":"synthetic:original"}))
            .unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            original.replace(
                "\"synthetic\":true",
                "\"synthetic\":false,\"synthetic\":true",
            ),
        )
        .unwrap();
        assert!(read_registration(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_provisioning_and_cleanup_never_become_accepted_enrollment() {
        let report = uploadable_observation(
            Err("finalize: permission_denied (0x80090010)".into()),
            false,
            1,
        );
        assert_eq!(report["complete_success"], false);
        assert_eq!(report["enrollment_accepted"], false);
        assert_eq!(report["native_status"], 0x80090010u32);
        assert_eq!(report["outcome"], "denied");
        assert_eq!(report["incomplete_cleanup"], 1);
        assert_eq!(
            uploadable_observation(Ok(()), false, 1)["complete_success"],
            false
        );
    }

    #[test]
    fn genuine_second_user_observations_are_separate_from_simulated_sid_tests() {
        let simulated = uploadable_observation(Ok(()), false, 0);
        assert_eq!(simulated["cross_user"], "not_established");
        let native = uploadable_observation(Ok(()), true, 0);
        assert_eq!(
            native["cross_user"],
            "distinct_live_token_observed_requires_review"
        );
        assert_eq!(native["complete_success"], false);
    }

    #[test]
    fn uploadable_evidence_omits_sensitive_strings_and_bad_keyset_stays_ambiguous() {
        let error = "S-1-5-21-sensitive MachineGuid=secret C:\\Users\\person\\receipt.jsonl RoLauncher.IdentityPoC.sensitive private_blob (0x80090016)";
        let report = uploadable_observation(Err(error.into()), false, 1);
        let text = report.to_string();
        for secret in [
            "S-1-5",
            "MachineGuid",
            "Users",
            "receipt.jsonl",
            "IdentityPoC",
            "private_blob",
            "person",
        ] {
            assert!(!text.contains(secret));
        }
        assert_eq!(report["outcome"], "ambiguous");
        assert_eq!(report["native_status"], 0x80090016u32);
    }

    #[test]
    fn extra_users_administrators_duplicate_grants_and_deny_aces_are_not_accepted() {
        for principal in [
            Principal::Unauthorized,
            Principal::Administrators,
            Principal::Other,
            Principal::Authorized,
        ] {
            let mut changed = acl();
            changed.grants[0].principal = principal;
            assert!(validate_machine_acl(&changed).is_err());
        }
        let mut changed = acl();
        changed.grants[2].allow = false;
        assert!(validate_machine_acl(&changed).is_err());
        let mut changed = acl();
        changed.grants.remove(2);
        assert!(validate_machine_acl(&changed).is_err());
    }

    #[test]
    fn missing_corrupt_or_changed_registration_is_not_repaired_or_reallocated() {
        let dir =
            std::env::temp_dir().join(format!("rolauncher-registration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("registration.jsonl");
        assert!(read_registration(&path).is_err());
        assert!(!path.exists());
        Registration::reserve(&path, serde_json::json!({"synthetic":true}))
            .unwrap()
            .commit(serde_json::json!({"public_evidence":"synthetic:original"}))
            .unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        for changed in [
            original.replacen("\"synthetic\":true", "\"synthetic\":false", 1),
            original.replacen("\"schema\":1", "\"schema\":2", 1),
            "{corrupt\n".into(),
        ] {
            std::fs::write(&path, &changed).unwrap();
            assert!(read_registration(&path).is_err());
            assert!(Registration::reserve(&path, serde_json::json!({})).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), changed);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
