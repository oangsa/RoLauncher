//! Human-operated disposable lab only. Never called by native contract tests or production.
use super::*;
use crate::identity_poc::provisioning::{self as policy, Grant, KeyAcl, Principal, Registration};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{IsTerminal, Read},
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::*;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    lab_id: uuid::Uuid,
    provisioner_sid: String,
    authorized_sid: String,
    unauthorized_sid: String,
    provider: Provider,
}

impl Config {
    fn validate(&self) -> Result<(), String> {
        if self.lab_id.get_version_num() != 4 {
            return Err("lab requires a UUIDv4 reservation".into());
        }
        let principals = [
            &self.provisioner_sid,
            &self.authorized_sid,
            &self.unauthorized_sid,
        ];
        for sid in principals {
            // Local/domain account SIDs only; prevent SDDL injection and broad group selection.
            if !sid.starts_with("S-1-5-21-")
                || sid.len() > 184
                || !sid
                    .bytes()
                    .all(|b| b == b'S' || b == b'-' || b.is_ascii_digit())
            {
                return Err("lab requires three distinct reviewed account SIDs".into());
            }
            let mut pointer = null_mut();
            if unsafe { ConvertStringSidToSidW(wide(sid).as_ptr(), &mut pointer) } == 0 {
                return Err("invalid lab principal".into());
            }
            let allocation = LocalAllocation(pointer);
            if sid_text(pointer)? != *sid {
                return Err("noncanonical lab principal".into());
            }
            drop(allocation);
        }
        if self.provisioner_sid == self.authorized_sid
            || self.provisioner_sid == self.unauthorized_sid
            || self.authorized_sid == self.unauthorized_sid
        {
            return Err("P, A and B must have distinct account SIDs".into());
        }
        Ok(())
    }

    fn role(&self, sid: &str) -> Principal {
        if sid == "S-1-5-18" {
            Principal::System
        } else if sid == "S-1-5-32-544" {
            Principal::Administrators
        } else if sid == self.provisioner_sid {
            Principal::Provisioner
        } else if sid == self.authorized_sid {
            Principal::Authorized
        } else if sid == self.unauthorized_sid {
            Principal::Unauthorized
        } else {
            Principal::Other
        }
    }

    fn shared_descriptor(&self) -> String {
        format!(
            "O:BAD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FRFX;;;{})(A;;FRFX;;;{})",
            self.authorized_sid, self.unauthorized_sid
        )
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineReservation {
    config: Config,
    planned: PublicKey,
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "local lab input unavailable")?
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|_| "local lab input unreadable")?;
    if bytes.len() > 32768 {
        return Err("local lab input oversized".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "invalid local lab input".into())
}

fn machine_directory() -> Result<PathBuf, String> {
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath},
    };
    let mut pointer = null_mut();
    if unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, 0, null_mut(), &mut pointer) } < 0 {
        return Err("trusted ProgramData discovery unavailable".into());
    }
    let mut length = 0;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
        .map_err(|_| "invalid ProgramData path");
    unsafe {
        CoTaskMemFree(pointer.cast());
    }
    Ok(PathBuf::from(result?).join("RoLauncher-IdentityLab"))
}

fn user_path(config: &Config) -> Result<PathBuf, String> {
    Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable")?)
            .join("RoLauncher-IdentityLab")
            .join(config.lab_id.to_string())
            .join("user.jsonl"),
    )
}

fn sid_text(sid: PSID) -> Result<String, String> {
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err("invalid security principal".into());
    }
    let mut text = null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err("principal conversion failed".into());
    }
    let allocation = LocalAllocation(text.cast());
    let mut length = 0;
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
        .map_err(|_| "invalid principal encoding".into());
    drop(allocation);
    result
}

fn descriptor_acl(descriptor: PSECURITY_DESCRIPTOR, config: &Config) -> Result<KeyAcl, String> {
    if unsafe { IsValidSecurityDescriptor(descriptor) } == 0 {
        return Err("invalid descriptor".into());
    }
    let mut owner = null_mut();
    let mut defaulted = 0;
    let mut present = 0;
    let mut acl = null_mut();
    let mut control = 0;
    let mut revision = 0;
    if unsafe { GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted) } == 0
        || unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) }
            == 0
        || present == 0
        || acl.is_null()
        || unsafe { IsValidAcl(acl) } == 0
        || unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0
    {
        return Err("missing owner or valid DACL".into());
    }
    let mut grants = Vec::new();
    for index in 0..unsafe { (*acl).AceCount } as u32 {
        let mut ace = null_mut();
        if unsafe { GetAce(acl, index, &mut ace) } == 0 {
            return Err("ACE unavailable".into());
        }
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        if header.AceType != 0 || (header.AceSize as usize) < size_of::<ACCESS_ALLOWED_ACE>() {
            return Err("unreviewed ACE type or size".into());
        }
        let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        let sid = (&allowed.SidStart as *const u32).cast_mut().cast();
        grants.push(Grant {
            principal: config.role(&sid_text(sid)?),
            mask: allowed.Mask,
            flags: header.AceFlags,
            allow: true,
        });
    }
    Ok(KeyAcl {
        owner: config.role(&sid_text(owner)?),
        protected: control & SE_DACL_PROTECTED != 0,
        grants,
    })
}

fn check_machine_acl(key: &Key, config: &Config) -> Result<(), String> {
    key.check_policy()?;
    let bytes = property(
        key.handle.0,
        NCRYPT_SECURITY_DESCR_PROPERTY,
        OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
    )?;
    let mut aligned = vec![0usize; bytes.len().div_ceil(size_of::<usize>())];
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), aligned.as_mut_ptr().cast(), bytes.len());
    }
    policy::validate_machine_acl(&descriptor_acl(aligned.as_mut_ptr().cast(), config)?)
}

fn check_shared_path(path: &Path, config: &Config) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "protected lab registration unavailable")?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("reparse point rejected; recovery required".into());
    }
    let name = wide(path.to_str().ok_or("unsupported lab path")?);
    let mut descriptor = null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err("cannot inspect protected lab path".into());
    }
    let allocation = LocalAllocation(descriptor);
    let acl = descriptor_acl(descriptor, config)?;
    if acl.owner != Principal::Administrators || !acl.protected || acl.grants.len() != 4 {
        return Err("lab path must be administrator-owned with protected reviewed ACL".into());
    }
    let mut roles = Vec::new();
    for grant in acl.grants {
        if grant.flags != 0 || !grant.allow {
            return Err("unexpected lab path ACE".into());
        }
        match grant.principal {
            Principal::System | Principal::Administrators
                if [0x10000000, 0x001f01ff].contains(&grant.mask) => {}
            Principal::Authorized | Principal::Unauthorized
                if grant.mask & !0xa01200a9 == 0 && grant.mask & (0x80000000 | 1) != 0 => {}
            _ => return Err("lab registration path rights rejected".into()),
        }
        if roles.contains(&grant.principal) {
            return Err("duplicate path principal".into());
        }
        roles.push(grant.principal);
    }
    drop(allocation);
    Ok(())
}

fn protected_new(path: &Path, descriptor: &str, directory: bool) -> Result<Option<File>, String> {
    use std::os::windows::io::FromRawHandle;
    let mut pointer = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(descriptor).as_ptr(),
            1,
            &mut pointer,
            null_mut(),
        )
    } == 0
    {
        return Err("lab descriptor rejected".into());
    }
    let allocation = LocalAllocation(pointer);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: pointer,
        bInheritHandle: 0,
    };
    let path = wide(path.to_str().ok_or("unsupported lab path")?);
    if directory {
        if unsafe { CreateDirectoryW(path.as_ptr(), &attributes) } == 0 {
            return Err(
                "lab slot exists or creation denied; preserve interrupted registration".into(),
            );
        }
        drop(allocation);
        return Ok(None);
    }
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_GENERIC_WRITE | FILE_GENERIC_READ,
            FILE_SHARE_READ,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    drop(allocation);
    if handle == INVALID_HANDLE_VALUE {
        return Err("lab registration exists or is inaccessible; no overwrite".into());
    }
    Ok(Some(unsafe { File::from_raw_handle(handle) }))
}

fn human_gate(args: &[String]) -> Result<(), String> {
    let hosted = ["CI", "GITHUB_ACTIONS", "RUNNER_ENVIRONMENT"]
        .iter()
        .any(|key| std::env::var_os(key).is_some());
    let explicit = args
        .last()
        .is_some_and(|a| a == "--authorized-disposable-lab");
    if !explicit || hosted || !std::io::stdin().is_terminal() {
        return Err(
            "interactive disposable-lab authorization required; hosted mutations denied".into(),
        );
    }
    eprintln!(
        "Disposable authorized lab only. Type: {}",
        policy::LAB_CONFIRMATION
    );
    let mut confirmation = String::new();
    std::io::stdin()
        .read_line(&mut confirmation)
        .map_err(|_| "human confirmation unavailable")?;
    policy::authorize_mutation(explicit, &confirmation, hosted)
}

fn ordinary(identity: &Identity) -> Result<(), String> {
    identity.accept(ContextPurpose::Enrollment)?;
    // Reject deny-only Administrators membership too; unelevated admin is not a standard account.
    let mut handle = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
        return Err("token groups unavailable".into());
    }
    let token = Token(handle);
    let mut size = 0;
    unsafe {
        GetTokenInformation(token.0, TokenGroups, null_mut(), 0, &mut size);
    }
    if !(16..=65536).contains(&size) {
        return Err("invalid token group size".into());
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenGroups,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err("token groups unavailable".into());
    }
    let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
    if std::mem::offset_of!(TOKEN_GROUPS, Groups)
        + groups.GroupCount as usize * size_of::<SID_AND_ATTRIBUTES>()
        > size as usize
    {
        return Err("invalid token groups".into());
    }
    for group in
        unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize) }
    {
        if unsafe { IsWellKnownSid(group.Sid, WinBuiltinAdministratorsSid) } != 0 {
            return Err("standard account required; administrator membership observed".into());
        }
    }
    Ok(())
}

fn provision(config_path: &Path) -> Result<(), String> {
    let identity = effective_identity()?;
    identity.accept(ContextPurpose::Diagnostic)?;
    let config: Config = read_json(config_path)?;
    config.validate()?;
    if !identity.elevated
        || identity.logon != LogonContext::Interactive
        || identity.session_zero
        || identity.sid != config.provisioner_sid
    {
        return Err("interactive elevated provisioning principal P required".into());
    }
    let accounting = recovery::summary()?;
    if accounting["incomplete"] != 0 || accounting["intentionally_retained"] != 0 {
        return Err("outstanding local recovery blocks provisioning; no automatic repair".into());
    }
    let directory = machine_directory()?;
    protected_new(&directory, &config.shared_descriptor(), true)?;
    check_shared_path(&directory, &config)?;
    let path = directory.join("machine.jsonl");
    let name = format!("RoLauncher.IdentityPoC.{}", uuid::Uuid::new_v4());
    let planned = PublicKey {
        name: name.clone(),
        provider: config.provider,
        scope: Scope::Machine,
        blob: Vec::new(),
    };
    let reservation = MachineReservation {
        config: config.clone(),
        planned: planned.clone(),
    };
    let file = protected_new(&path, &config.shared_descriptor(), false)?
        .ok_or("registration handle missing")?;
    check_shared_path(&path, &config)?;
    let journal = Registration::from_new_file(
        file,
        serde_json::to_value(reservation).map_err(|_| "reservation encoding failed")?,
    )?;
    let mut receipt = recovery::Receipt::prepare(planned.clone(), &identity)?;
    let provider = provider(config.provider)?;
    let mut handle = 0;
    let created = unsafe {
        NCryptCreatePersistedKey(
            provider.0,
            &mut handle,
            NCRYPT_RSA_ALGORITHM,
            wide(&name).as_ptr(),
            0,
            NCRYPT_MACHINE_KEY_FLAG | NCRYPT_SILENT_FLAG,
        )
    };
    if created != 0 {
        receipt.finish(
            recovery::CleanupOutcome::NeverPersisted,
            Some(created as u32),
        )?;
        return cng("lab machine create", created);
    }
    // No destructor deletion: every ambiguous native or persistence failure retains its exact receipt.
    let mut key = Key {
        handle: Object(handle),
        _provider: provider,
        name,
        kind: config.provider,
        scope: Scope::Machine,
        delete_on_drop: false,
        finalize_attempted: false,
        recovery: Some(receipt),
        initial_acl_error: None,
    };
    let result = (|| {
        key.recovery.as_mut().unwrap().record.phase = recovery::Phase::Created;
        key.recovery.as_mut().unwrap().save()?;
        set_number(handle, NCRYPT_LENGTH_PROPERTY, 2048)?;
        set_number(handle, NCRYPT_EXPORT_POLICY_PROPERTY, 0)?;
        set_number(handle, NCRYPT_KEY_USAGE_PROPERTY, NCRYPT_ALLOW_SIGNING_FLAG)?;
        let descriptor = wide(&format!(
            "O:{}D:P(A;;GA;;;SY)(A;;GA;;;{})(A;;GR;;;{})",
            config.provisioner_sid, config.provisioner_sid, config.authorized_sid
        ));
        let mut pointer = null_mut();
        let mut length = 0;
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                descriptor.as_ptr(),
                1,
                &mut pointer,
                &mut length,
            )
        } == 0
        {
            return Err("machine descriptor conversion failed".into());
        }
        let allocation = LocalAllocation(pointer);
        cng("lab set machine owner and ACL", unsafe {
            NCryptSetProperty(
                handle,
                NCRYPT_SECURITY_DESCR_PROPERTY,
                pointer.cast(),
                length,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | NCRYPT_PERSIST_FLAG,
            )
        })?;
        drop(allocation);
        key.recovery.as_mut().unwrap().record.phase = recovery::Phase::FinalizeAttempted;
        key.recovery.as_mut().unwrap().save()?;
        key.finalize_attempted = true;
        cng("lab finalize machine key", unsafe {
            NCryptFinalizeKey(handle, NCRYPT_SILENT_FLAG)
        })?;
        let public = key.info()?;
        let receipt = key.recovery.as_mut().unwrap();
        receipt.record.key.blob = public.blob.clone();
        receipt.record.phase = recovery::Phase::Finalized;
        receipt.save()?;
        check_machine_acl(&key, &config)?;
        let receipt = key.recovery.as_mut().unwrap();
        receipt.record.phase = recovery::Phase::RetainedForState;
        receipt.save()?;
        journal.commit(serde_json::to_value(public).map_err(|_| "public evidence encoding failed")?)
    })();
    if let Err(error) = &result {
        let status = policy::uploadable_observation(Err(error.clone()), false, 0)["native_status"]
            .as_u64()
            .map(|code| code as u32);
        key.recovery
            .as_mut()
            .unwrap()
            .finish(recovery::CleanupOutcome::Incomplete, status)?;
    }
    result
}

fn machine() -> Result<(Config, PublicKey, [u8; 32]), String> {
    let directory = machine_directory()?;
    let path = directory.join("machine.jsonl");
    let (reservation, evidence) = policy::read_registration(&path)?;
    let binding = hash(&serde_json::to_vec(&reservation).map_err(|_| "invalid reservation")?)?;
    let reservation: MachineReservation =
        serde_json::from_value(reservation).map_err(|_| "invalid machine reservation")?;
    reservation.config.validate()?;
    check_shared_path(&directory, &reservation.config)?;
    check_shared_path(&path, &reservation.config)?;
    let key: PublicKey =
        serde_json::from_value(evidence).map_err(|_| "missing machine public evidence")?;
    recovery::valid_name(&key.name)?;
    if key.name != reservation.planned.name
        || key.provider != reservation.planned.provider
        || key.scope != reservation.planned.scope
        || key.provider != reservation.config.provider
        || key.scope != Scope::Machine
        || key.blob.is_empty()
        || !reservation.planned.blob.is_empty()
    {
        return Err("machine registration identity mismatch".into());
    }
    Ok((reservation.config, key, binding))
}

fn enroll() -> Result<(), String> {
    let identity = effective_identity()?;
    ordinary(&identity)?;
    let (config, info, binding) = machine()?;
    if identity.sid != config.authorized_sid {
        return Err("unauthorized Windows principal".into());
    }
    let key = Key::open(&info)?;
    check_machine_acl(&key, &config)?;
    let path = user_path(&config)?;
    let directory = path.parent().ok_or("invalid user path")?;
    std::fs::create_dir_all(directory.parent().ok_or("invalid user parent")?)
        .map_err(|_| "user registration parent unavailable")?;
    std::fs::create_dir(directory)
        .map_err(|_| "user slot exists; no replacement after lost or partial registration")?;
    let file = recovery::private_new_file(&path, &identity)?;
    let name = format!("RoLauncher.IdentityPoC.{}", uuid::Uuid::new_v4());
    let journal = Registration::from_new_file(
        file,
        serde_json::json!({"binding":binding,"user_name":name}),
    )?;
    let mut user = Key::create_measured(
        config.provider,
        Scope::User,
        &identity,
        &name,
        true,
        ContextPurpose::Enrollment,
    )?;
    let info = user.info()?;
    user.recovery.as_mut().unwrap().record.phase = recovery::Phase::RetainedForState;
    user.recovery.as_mut().unwrap().save()?;
    user.delete_on_drop = false;
    journal.commit(serde_json::to_value(info).map_err(|_| "user public evidence unavailable")?)?;
    check()
}

fn user_info(config: &Config, binding: [u8; 32]) -> Result<PublicKey, String> {
    decode_user_journal(
        config,
        binding,
        policy::read_registration(&user_path(config)?)?,
    )
}

fn decode_user_journal(
    config: &Config,
    binding: [u8; 32],
    journal: (serde_json::Value, serde_json::Value),
) -> Result<PublicKey, String> {
    let (reservation, evidence) = journal;
    if reservation.get("binding") != Some(&serde_json::json!(binding)) {
        return Err("installation registration changed".into());
    }
    let info: PublicKey =
        serde_json::from_value(evidence).map_err(|_| "user public evidence unavailable")?;
    if info.scope != Scope::User
        || info.provider != config.provider
        || reservation.get("user_name") != Some(&serde_json::json!(info.name))
        || info.blob.is_empty()
        || info.blob.len() > 4096
    {
        return Err("user registration identity mismatch".into());
    }
    recovery::valid_name(&info.name)?;
    Ok(info)
}

fn check() -> Result<(), String> {
    let identity = effective_identity()?;
    ordinary(&identity)?;
    let (config, info, binding) = machine()?;
    if identity.sid != config.authorized_sid {
        return Err("unauthorized Windows principal".into());
    }
    let user_info = user_info(&config, binding)?;
    let machine = Key::open(&info)?;
    check_machine_acl(&machine, &config)?;
    let user = Key::open(&user_info)?;
    user.inspection(&identity)?;
    let expected = crate::identity_poc::Challenge {
        schema_version: 1,
        nonce: rand::random(),
        installation_key: hash(&info.blob)?,
        user_key: hash(&user_info.blob)?,
        windows_sid: hash(config.authorized_sid.as_bytes())?,
        installation_evidence: binding,
        synthetic_customer: "synthetic:machine-key-lab".into(),
        purpose: crate::identity_poc::PURPOSE.into(),
    };
    policy::validate_enrollment(
        &policy::EnrollmentEvidence {
            finalized: true,
            acl_accepted: true,
            ordinary_user: true,
            machine_key: Some(hash(&machine.public()?)?),
            user_key: Some(hash(&user.public()?)?),
            authorized_sid: expected.windows_sid,
            live_sid: identity.sid_reference,
        },
        &expected,
        &expected,
    )?;
    let digest = hash(&expected.canonical()?)?;
    let proof = Proof {
        installation: machine.sign(&digest)?,
        user: user.sign(&digest)?,
    };
    let mut verifier = Verifier::new(expected.clone(), info.blob, user_info.blob)?;
    verifier.verify(&expected, &proof)?;
    if verifier.verify(&expected, &proof).is_ok() {
        return Err("nonce replay accepted".into());
    }
    Ok(())
}

fn validate_recovery_registration(
    config: &Config,
    record: &recovery::Snapshot,
    protected_registration: Result<(Config, PublicKey, [u8; 32]), String>,
) -> Result<(), String> {
    // machine() independently validates the fixed registration's owner/DACL and planned key.
    // Even prior disposal must be attributed to this exact lab before reporting success.
    let (registered_config, registered_key, _) = protected_registration?;
    if config != &registered_config
        || record.key != registered_key
        || record.key.scope != Scope::Machine
        || record.key.provider != config.provider
        || record.key.blob.is_empty()
    {
        return Err(
            "exact lab registration relationship unavailable; retain receipt for independent recovery"
                .into(),
        );
    }
    if (record.cleanup == Some(recovery::CleanupOutcome::Succeeded)
        && record.native_status != Some(0))
        || (record.cleanup == Some(recovery::CleanupOutcome::Absent)
            && !matches!(record.native_status, Some(0 | 0x80090011)))
    {
        return Err("prior disposal lacks exact native evidence; retain receipt for review".into());
    }
    Ok(())
}

fn recover(config_path: &Path, receipt_path: &Path) -> Result<(), String> {
    let identity = effective_identity()?;
    identity.accept(ContextPurpose::Diagnostic)?;
    let config: Config = read_json(config_path)?;
    config.validate()?;
    if !identity.elevated
        || identity.sid != config.provisioner_sid
        || identity.logon != LogonContext::Interactive
    {
        return Err("elevated provisioning principal P required for machine recovery".into());
    }
    let receipt = recovery::Receipt::open(receipt_path, &identity)?;
    validate_recovery_registration(&config, &receipt.record, machine())?;
    if matches!(
        receipt.record.cleanup,
        Some(
            recovery::CleanupOutcome::Succeeded
                | recovery::CleanupOutcome::Absent
                | recovery::CleanupOutcome::NeverPersisted
        )
    ) {
        return Ok(());
    }
    let mut key = Key::open(&receipt.record.key)?;
    check_machine_acl(&key, &config)?;
    key.recovery = Some(receipt);
    key.delete()?;
    Ok(())
}

fn validate_observed_user(
    config: &Config,
    binding: [u8; 32],
    public_journal: (serde_json::Value, serde_json::Value),
    protected_reference: Result<(serde_json::Value, serde_json::Value), String>,
) -> Result<PublicKey, String> {
    let user = decode_user_journal(config, binding, public_journal)?;
    let original = decode_user_journal(config, binding, protected_reference?)?;
    if user != original {
        return Err("public copy is not the independently pinned original user key".into());
    }
    Ok(user)
}

fn trusted_user_reference(
    config: &Config,
) -> Result<(serde_json::Value, serde_json::Value), String> {
    let directory = machine_directory()?;
    let path = directory.join("original-user.jsonl");
    check_shared_path(&directory, config)?;
    check_shared_path(&path, config)?;
    policy::read_registration(&path)
}

fn validate_reference_copy(
    config: &Config,
    binding: [u8; 32],
    bytes: &[u8],
    independently_expected_digest: [u8; 32],
) -> Result<(), String> {
    if hash(bytes)? != independently_expected_digest {
        return Err("original A journal comparison failed; no trusted reference created".into());
    }
    decode_user_journal(config, binding, policy::decode_registration(bytes)?)?;
    Ok(())
}

/// P must obtain the expected digest from A's verified original journal through a separate
/// trusted local handoff, never by hashing B's supplied copy. No references enter reports.
fn pin_user(public_copy: &Path) -> Result<(), String> {
    let identity = effective_identity()?;
    identity.accept(ContextPurpose::Diagnostic)?;
    let (config, _, binding) = machine()?;
    if !identity.elevated
        || identity.sid != config.provisioner_sid
        || identity.logon != LogonContext::Interactive
        || identity.session_zero
    {
        return Err(
            "interactive elevated principal P required to pin A's original user reference".into(),
        );
    }
    eprintln!(
        "Enter original A journal SHA256 from the separate trusted local comparison (64 hex characters; never included in reports):"
    );
    let mut comparison = String::new();
    std::io::stdin()
        .read_line(&mut comparison)
        .map_err(|_| "independent local comparison unavailable")?;
    let comparison = comparison.trim();
    if comparison.len() != 64 || !comparison.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid independent local comparison".into());
    }
    let mut expected = [0u8; 32];
    for (index, value) in expected.iter_mut().enumerate() {
        *value = u8::from_str_radix(&comparison[index * 2..index * 2 + 2], 16)
            .map_err(|_| "invalid local digest")?;
    }
    let mut bytes = Vec::new();
    File::open(public_copy)
        .map_err(|_| "original journal copy unavailable")?
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|_| "original journal copy unreadable")?;
    validate_reference_copy(&config, binding, &bytes, expected)?;
    let path = machine_directory()?.join("original-user.jsonl");
    let mut file = protected_new(&path, &config.shared_descriptor(), false)?
        .ok_or("reference handle unavailable")?;
    use std::io::Write;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "partial original user reference retained; observations unverified")?;
    check_shared_path(&path, &config)
}

fn observe(public_user_state: &Path) -> Result<serde_json::Value, String> {
    let identity = effective_identity()?;
    identity.accept(ContextPurpose::Diagnostic)?;
    let (config, info, binding) = machine()?;
    let distinct = identity.sid == config.unauthorized_sid;
    if !distinct && identity.sid != config.authorized_sid {
        return Err("designated A or B token required".into());
    }
    let user = policy::read_registration(public_user_state)
        .and_then(|copy| {
            validate_observed_user(&config, binding, copy, trusted_user_reference(&config))
        })
        .ok();
    let mut observations = Vec::new();
    let mut targets = vec![&info];
    if let Some(user) = &user {
        targets.push(user);
    }
    for expected in targets {
        let opened = Key::open_raw(expected);
        let open = policy::uploadable_observation(
            opened.as_ref().map(|_| ()).map_err(Clone::clone),
            distinct,
            0,
        );
        let (sign, acl) = match opened {
            Ok(key) => {
                let result = (|| {
                    key.check_policy()?;
                    if key.public()? != expected.blob {
                        return Err("public copy/key mismatch".into());
                    }
                    let digest = hash(b"RoLauncher machine-key lab fixed access probe v1")?;
                    verify(&expected.blob, &digest, &key.sign(&digest)?)
                })();
                let acl = if expected.scope == Scope::Machine {
                    check_machine_acl(&key, &config).is_ok()
                } else {
                    key.inspection(&identity).is_ok()
                };
                (policy::uploadable_observation(result, distinct, 0), acl)
            }
            Err(error) => (
                policy::uploadable_observation(Err(error), distinct, 0),
                false,
            ),
        };
        observations.push(serde_json::json!({"scope":expected.scope,"open":open,"fixed_challenge_sign_and_verify":sign,"acl_accepted":acl}));
    }
    if user.is_none() {
        observations.push(serde_json::json!({"scope":"user", "original_user_evidence":"unverified",
            "native_private_use":"not_attempted", "reason":"original_reference_missing_invalid_or_mismatched"}));
    }
    Ok(
        serde_json::json!({"context": context()?, "standard_account": ordinary(&identity).is_ok(),
        "designated_user_b_live_token": distinct, "native_access":observations,
        "original_user_evidence":if user.is_some() {"matched_protected_independent_reference"} else {"unverified"},
        "enrollment_accepted":false, "customer_authenticated":false, "isolation_proven":false}),
    )
}

/// Sacrificial snapshot only: unexpected permission success can change/delete this PoC key.
/// This command never marks P's receipt deleted on A's behalf.
fn rights() -> Result<serde_json::Value, String> {
    use zeroize::Zeroize;
    let identity = effective_identity()?;
    ordinary(&identity)?;
    let (config, info, _) = machine()?;
    if identity.sid != config.authorized_sid {
        return Err("rights probe requires designated standard user A".into());
    }
    let mut key = Key::open(&info)?;
    check_machine_acl(&key, &config)?;
    let mut observations = serde_json::Map::new();
    let descriptor = property(
        key.handle.0,
        NCRYPT_SECURITY_DESCR_PROPERTY,
        OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
    )?;
    let mut violations = 0;
    let record = |status: i32| {
        policy::uploadable_observation(cng("fixed lab rights operation", status), false, 0)
    };
    let mut buffer = vec![0u8; 16384];
    let mut length = 0;
    let status = unsafe {
        NCryptExportKey(
            key.handle.0,
            0,
            BCRYPT_RSAPRIVATE_BLOB,
            null(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            &mut length,
            NCRYPT_SILENT_FLAG,
        )
    };
    buffer.zeroize();
    violations += usize::from(status == 0);
    observations.insert("private_export".into(), record(status));
    let status = unsafe {
        NCryptSetProperty(
            key.handle.0,
            NCRYPT_SECURITY_DESCR_PROPERTY,
            descriptor.as_ptr(),
            descriptor.len() as u32,
            DACL_SECURITY_INFORMATION | NCRYPT_PERSIST_FLAG,
        )
    };
    violations += usize::from(status == 0);
    observations.insert("write_same_dacl".into(), record(status));
    let text = wide(&format!("O:{}", config.authorized_sid));
    let mut pointer = null_mut();
    let mut size = 0;
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut pointer,
            &mut size,
        )
    } == 0
    {
        return Err("rights descriptor unavailable".into());
    }
    let allocation = LocalAllocation(pointer);
    let status = unsafe {
        NCryptSetProperty(
            key.handle.0,
            NCRYPT_SECURITY_DESCR_PROPERTY,
            pointer.cast(),
            size,
            OWNER_SECURITY_INFORMATION | NCRYPT_PERSIST_FLAG,
        )
    };
    violations += usize::from(status == 0);
    observations.insert("take_ownership".into(), record(status));
    drop(allocation);
    let status = unsafe { NCryptDeleteKey(key.handle.0, NCRYPT_SILENT_FLAG) };
    if status == 0 {
        key.handle.0 = 0;
    }
    violations += usize::from(status == 0);
    observations.insert("delete_exact_poc_machine_key".into(), record(status));
    Ok(
        serde_json::json!({"native_management_operations":observations,
        "unexpected_management_successes":violations,"enrollment_accepted":false,
        "complete_success":false,"independent_review":"pending",
        "recovery_receipts":"retained_for_provisioner_review"}),
    )
}

pub(super) fn run(args: &[String]) -> Result<(), String> {
    let result: Result<serde_json::Value, String> = (|| match args {
        [] => Ok(
            serde_json::json!({"context":context()?,"providers":capabilities(),"recovery":recovery::summary()?,"mutations":false}),
        ),
        [command] if command == "lab-inspect" => Ok(
            serde_json::json!({"context":context()?,"providers":capabilities(),"recovery":recovery::summary()?,"mutations":false}),
        ),
        [command] if command == "lab-check" => {
            check()?;
            let mut report = policy::uploadable_observation(Ok(()), false, 0);
            report["synthetic_two_key_proof_verified"] = true.into();
            report["pinned_registered_keys_reopened"] = true.into();
            Ok(report)
        }
        [command, public] if command == "lab-observe" => observe(Path::new(public)),
        [command, config, _flag] if command == "lab-provision" => {
            human_gate(args)?;
            provision(Path::new(config))?;
            Ok(policy::uploadable_observation(Ok(()), false, 0))
        }
        [command, _flag] if command == "lab-enroll" => {
            human_gate(args)?;
            enroll()?;
            Ok(policy::uploadable_observation(Ok(()), false, 0))
        }
        [command, config, receipt, _flag] if command == "lab-recover-machine" => {
            human_gate(args)?;
            recover(Path::new(config), Path::new(receipt))?;
            Ok(policy::uploadable_observation(Ok(()), false, 0))
        }
        [command, copy, _flag] if command == "lab-pin-user" => {
            human_gate(args)?;
            pin_user(Path::new(copy))?;
            Ok(policy::uploadable_observation(Ok(()), false, 0))
        }
        [command, receipt, _flag] if command == "lab-recover-user" => {
            human_gate(args)?;
            ordinary(&effective_identity()?)?;
            recovery::recover(Path::new(receipt))?;
            Ok(policy::uploadable_observation(Ok(()), false, 0))
        }
        [command, snapshot, _flag]
            if command == "lab-rights" && snapshot == "--sacrificial-snapshot" =>
        {
            human_gate(args)?;
            rights()
        }
        _ => Err("unsupported lab command; consult validation procedure".into()),
    })();
    let failed = result.is_err()
        || result.as_ref().is_ok_and(|report| {
            report["unexpected_management_successes"]
                .as_u64()
                .unwrap_or(0)
                != 0
        });
    let mut report =
        result.unwrap_or_else(|error| policy::uploadable_observation(Err(error), false, 0));
    report["enrollment_accepted"] = false.into();
    report["customer_authenticated"] = false.into();
    report["complete_success"] = false.into();
    report["operation"] = match args.first().map(String::as_str) {
        None | Some("lab-inspect") => "read_only_inspection",
        Some("lab-provision") => "machine_provisioning",
        Some("lab-enroll") => "ordinary_user_registration",
        Some("lab-check") => "synthetic_proof_check",
        Some("lab-observe") => "native_user_access_observation",
        Some("lab-rights") => "sacrificial_management_operations",
        Some("lab-recover-machine") => "exact_machine_recovery",
        Some("lab-recover-user") => "exact_user_recovery",
        Some("lab-pin-user") => "trusted_original_user_reference",
        _ => "unsupported",
    }
    .into();
    if !failed
        && args
            .first()
            .is_some_and(|command| command == "lab-provision")
    {
        report["machine_scope_policy_and_acl_accepted"] = true.into();
    }
    if args.first().is_some_and(|command| command == "lab-observe")
        && report.get("original_user_evidence").is_none()
    {
        report["original_user_evidence"] = "unverified".into();
    }
    match recovery::summary() {
        Ok(accounting) => {
            report["incomplete_cleanup"] = accounting["incomplete"].clone();
            report["recovery_accounting"] = accounting;
        }
        Err(_) => {
            report["recovery_accounting"] = "unavailable_retain_all_local_records".into();
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|_| "report encoding failed")?
    );
    if failed {
        Err("lab operation failed or unavailable; sanitized report only; retain local recovery evidence".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn review_config() -> Config {
        Config {
            lab_id: uuid::Uuid::from_u128(0x12345678123442348234123456789abc),
            provisioner_sid: "S-1-5-21-1-2-3-1001".into(),
            authorized_sid: "S-1-5-21-1-2-3-1002".into(),
            unauthorized_sid: "S-1-5-21-1-2-3-1003".into(),
            provider: Provider::Software,
        }
    }

    fn review_key() -> PublicKey {
        PublicKey {
            name: "RoLauncher.IdentityPoC.12345678-1234-4234-8234-123456789abc".into(),
            provider: Provider::Software,
            scope: Scope::Machine,
            blob: vec![1, 2, 3],
        }
    }

    fn review_receipt(key: PublicKey) -> recovery::Snapshot {
        recovery::Snapshot {
            schema_version: 1,
            key,
            owner_sid_reference: [1; 32],
            phase: recovery::Phase::Finalized,
            cleanup: Some(recovery::CleanupOutcome::Incomplete),
            native_status: Some(0x80090016),
        }
    }

    #[test]
    fn review_recovery_rejects_cross_lab_receipt_with_identical_principals_and_provider() {
        let config = review_config();
        let original = review_key();
        let mut other = original.clone();
        other.name = "RoLauncher.IdentityPoC.87654321-4321-4321-8321-cba987654321".into();
        other.blob = vec![4, 5, 6];
        assert!(
            validate_recovery_registration(
                &config,
                &review_receipt(other),
                Ok((config.clone(), original, [1; 32]))
            )
            .is_err()
        );
    }

    #[test]
    fn review_recovery_accepts_only_the_exact_registered_receipt_including_prior_disposal() {
        let config = review_config();
        let key = review_key();
        for cleanup in [
            Some(recovery::CleanupOutcome::Incomplete),
            Some(recovery::CleanupOutcome::Succeeded),
        ] {
            let mut record = review_receipt(key.clone());
            record.cleanup = cleanup;
            if cleanup == Some(recovery::CleanupOutcome::Succeeded) {
                record.native_status = Some(0);
            }
            validate_recovery_registration(
                &config,
                &record,
                Ok((config.clone(), key.clone(), [1; 32])),
            )
            .unwrap();
            let mut other = config.clone();
            other.lab_id = uuid::Uuid::from_u128(0x87654321432143218321cba987654321);
            assert!(
                validate_recovery_registration(
                    &other,
                    &record,
                    Ok((config.clone(), key.clone(), [1; 32]))
                )
                .is_err()
            );
        }
    }

    #[test]
    fn review_recovery_rejects_other_lab_uuid_even_with_the_same_registered_key() {
        let config = review_config();
        let key = review_key();
        let mut other = config.clone();
        other.lab_id = uuid::Uuid::from_u128(0x87654321432143218321cba987654321);
        assert!(
            validate_recovery_registration(
                &other,
                &review_receipt(key.clone()),
                Ok((config, key, [1; 32]))
            )
            .is_err()
        );
    }

    #[test]
    fn review_recovery_rejects_partial_and_torn_registration_without_changing_receipt() {
        let config = review_config();
        let record = review_receipt(review_key());
        let before = serde_json::to_vec(&record).unwrap();
        let directory =
            std::env::temp_dir().join(format!("rolauncher-review-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("synthetic-registration.jsonl");
        let reservation =
            Registration::reserve(&path, serde_json::json!({"synthetic":true})).unwrap();
        drop(reservation);
        for torn in [false, true] {
            if torn {
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap()
                    .write_all(b"{torn")
                    .unwrap();
            }
            let registration =
                policy::read_registration(&path).map(|_| (config.clone(), review_key(), [1; 32]));
            assert!(validate_recovery_registration(&config, &record, registration).is_err());
            assert_eq!(serde_json::to_vec(&record).unwrap(), before);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn review_user_journal() -> (serde_json::Value, serde_json::Value) {
        let mut user = review_key();
        user.scope = Scope::User;
        (
            serde_json::json!({"binding":vec![1;32],"user_name":user.name}),
            serde_json::to_value(user).unwrap(),
        )
    }

    #[test]
    fn review_observation_rejects_substituted_key_with_self_consistent_name_and_machine_binding() {
        let original = review_user_journal();
        let mut substituted = original.clone();
        substituted.0["user_name"] =
            "RoLauncher.IdentityPoC.87654321-4321-4321-8321-cba987654321".into();
        substituted.1["name"] = substituted.0["user_name"].clone();
        substituted.1["blob"] = serde_json::json!([9, 9, 9]);
        assert!(
            validate_observed_user(&review_config(), [1; 32], substituted, Ok(original)).is_err()
        );
    }

    #[test]
    fn review_observation_rejects_inconsistent_user_name_and_metadata() {
        let original = review_user_journal();
        let mut inconsistent = original.clone();
        inconsistent.0["user_name"] =
            "RoLauncher.IdentityPoC.87654321-4321-4321-8321-cba987654321".into();
        assert!(
            validate_observed_user(&review_config(), [1; 32], inconsistent, Ok(original)).is_err()
        );
    }

    #[test]
    fn review_observation_requires_separate_reference_and_accepts_authentic_copy() {
        let original = review_user_journal();
        validate_observed_user(
            &review_config(),
            [1; 32],
            original.clone(),
            Ok(original.clone()),
        )
        .unwrap();
        assert!(
            validate_observed_user(
                &review_config(),
                [1; 32],
                original,
                Err("trusted original unavailable".into())
            )
            .is_err()
        );
    }

    #[test]
    fn review_recovery_rejects_individual_name_provider_scope_and_public_key_substitution() {
        let config = review_config();
        let original = review_key();
        for field in 0..4 {
            let mut changed = original.clone();
            match field {
                0 => {
                    changed.name =
                        "RoLauncher.IdentityPoC.87654321-4321-4321-8321-cba987654321".into()
                }
                1 => changed.provider = Provider::Tpm,
                2 => changed.scope = Scope::User,
                _ => changed.blob = vec![9, 9, 9],
            }
            assert!(
                validate_recovery_registration(
                    &config,
                    &review_receipt(changed),
                    Ok((config.clone(), original.clone(), [1; 32]))
                )
                .is_err()
            );
        }
    }

    #[test]
    fn review_reference_comparison_rejects_a_changed_copy_and_preserves_original_bytes() {
        let config = review_config();
        let original = review_user_journal();
        let directory =
            std::env::temp_dir().join(format!("rolauncher-review-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("synthetic-user-journal.jsonl");
        Registration::reserve(&path, original.0)
            .unwrap()
            .commit(original.1)
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let independently_expected = hash(&bytes).unwrap();
        validate_reference_copy(&config, [1; 32], &bytes, independently_expected).unwrap();
        let mut changed = bytes.clone();
        changed.push(b'\n');
        assert!(
            validate_reference_copy(&config, [1; 32], &changed, independently_expected).is_err()
        );
        assert!(validate_reference_copy(&config, [2; 32], &bytes, independently_expected).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn review_recovery_never_accepts_ambiguous_status_as_prior_deletion_or_absence() {
        let config = review_config();
        let key = review_key();
        for cleanup in [
            recovery::CleanupOutcome::Succeeded,
            recovery::CleanupOutcome::Absent,
        ] {
            let mut record = review_receipt(key.clone());
            record.cleanup = Some(cleanup);
            assert!(
                validate_recovery_registration(
                    &config,
                    &record,
                    Ok((config.clone(), key.clone(), [1; 32]))
                )
                .is_err()
            );
        }
    }

    #[test]
    fn legacy_machine_creation_requires_lab_command_before_any_native_creation() {
        let identity = effective_identity().unwrap();
        let error = Key::create_named(
            Provider::Software,
            Scope::Machine,
            &identity,
            &format!("RoLauncher.IdentityPoC.{}", uuid::Uuid::new_v4()),
        )
        .err()
        .unwrap();
        assert!(error.contains("human-confirmed disposable lab command"));
    }

    #[test]
    fn synthetic_lab_configuration_rejects_shared_principals_groups_and_sddl_injection() {
        let mut config = Config {
            lab_id: uuid::Uuid::new_v4(),
            provisioner_sid: "S-1-5-21-1-2-3-1001".into(),
            authorized_sid: "S-1-5-21-1-2-3-1002".into(),
            unauthorized_sid: "S-1-5-21-1-2-3-1003".into(),
            provider: Provider::Software,
        };
        config.validate().unwrap();
        config.unauthorized_sid = config.authorized_sid.clone();
        assert!(config.validate().is_err());
        for input in [
            "S-1-1-0",
            "S-1-5-21-1-2-3-1003)(A;;GA;;;WD)",
            "S-1-5-21-01-2-3-1003",
        ] {
            config.unauthorized_sid = input.into();
            assert!(config.validate().is_err());
        }
    }
}
