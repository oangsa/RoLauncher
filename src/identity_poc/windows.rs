use std::{
    ffi::c_void,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, Cryptography::*, *},
    System::{Registry::*, Threading::*},
};

#[path = "recovery.rs"]
pub mod recovery;

#[path = "lab.rs"]
mod lab;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn win_error(operation: &str) -> String {
    format!("{operation}: Windows error {}", unsafe { GetLastError() })
}

pub fn hash(bytes: &[u8]) -> Result<[u8; 32], String> {
    let input_length = u32::try_from(bytes.len()).map_err(|_| "hash input too large")?;
    let mut provider = null_mut();
    let mut output = [0; 32];
    let opened =
        unsafe { BCryptOpenAlgorithmProvider(&mut provider, BCRYPT_SHA256_ALGORITHM, null(), 0) };
    if opened != 0 {
        return Err(format!("SHA256 provider: 0x{:08x}", opened as u32));
    }
    let status = unsafe {
        BCryptHash(
            provider,
            null(),
            0,
            bytes.as_ptr(),
            input_length,
            output.as_mut_ptr(),
            32,
        )
    };
    unsafe {
        BCryptCloseAlgorithmProvider(provider, 0);
    }
    if status != 0 {
        return Err(format!("SHA256: 0x{:08x}", status as u32));
    }
    Ok(output)
}

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}

#[derive(Clone)]
pub struct Identity {
    pub sid_reference: [u8; 32],
    pub thread_token: bool,
    pub elevated: bool,
    pub restricted: bool,
    pub session_zero: bool,
    pub service: bool,
    pub logon: LogonContext,
    sid: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogonContext {
    Interactive,
    Batch,
    Service,
    Network,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPurpose {
    Enrollment,
    Diagnostic,
}

impl Identity {
    pub fn accept(&self, purpose: ContextPurpose) -> Result<(), String> {
        if self.thread_token || self.service || self.restricted {
            return Err("unsupported effective security token".into());
        }
        match self.logon {
            LogonContext::Unknown | LogonContext::Network => {
                return Err("unsupported noninteractive logon context".into());
            }
            LogonContext::Service if !self.session_zero => {
                return Err("unexpected service logon outside session zero".into());
            }
            _ => (),
        }
        if purpose == ContextPurpose::Enrollment
            && (self.elevated || self.session_zero || self.logon != LogonContext::Interactive)
        {
            return Err("enrollment requires an ordinary unelevated interactive user; use explicit diagnostics for capability measurements".into());
        }
        Ok(())
    }
}

pub fn effective_identity() -> Result<Identity, String> {
    let mut handle = null_mut();
    // Fall back only when no thread token exists, never on access denial/anonymous impersonation.
    let thread_token =
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut handle) } != 0;
    if !thread_token {
        if unsafe { GetLastError() } != ERROR_NO_TOKEN {
            return Err(win_error("effective thread token"));
        }
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
            return Err(win_error("process token"));
        }
    }
    let token = Token(handle);
    let mut required = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required);
    }
    if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER || !(16..=65536).contains(&required) {
        return Err(win_error("token user size"));
    }
    // usize storage gives TOKEN_USER its required pointer alignment.
    let mut buffer = vec![0usize; (required as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    } == 0
    {
        return Err(win_error("token user"));
    }
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    if unsafe { IsValidSid(user.User.Sid) } == 0 {
        return Err("invalid token SID".into());
    }
    if unsafe { IsWellKnownSid(user.User.Sid, WinAnonymousSid) } != 0 {
        return Err("unsupported anonymous effective token; no process fallback".into());
    }
    let mut sid_pointer = null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid_pointer) } == 0 {
        return Err(win_error("token SID conversion"));
    }
    let allocation = LocalAllocation(sid_pointer.cast());
    let mut length = 0;
    while unsafe { *sid_pointer.add(length) } != 0 {
        length += 1;
    }
    let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(sid_pointer, length) })
        .map_err(|_| "invalid SID encoding")?;
    drop(allocation);
    let mut elevation = TOKEN_ELEVATION::default();
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut required,
        )
    } == 0
    {
        return Err(win_error("token elevation"));
    }
    let mut session = 0u32;
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenSessionId,
            (&mut session as *mut u32).cast(),
            4,
            &mut required,
        )
    } == 0
    {
        return Err(win_error("token session"));
    }
    let service = [WinLocalSystemSid, WinLocalServiceSid, WinNetworkServiceSid]
        .iter()
        .any(|kind| unsafe { IsWellKnownSid(user.User.Sid, *kind) } != 0);
    Ok(Identity {
        sid_reference: hash(sid.as_bytes())?,
        sid,
        thread_token,
        elevated: elevation.TokenIsElevated != 0,
        restricted: unsafe { IsTokenRestricted(token.0) } != 0,
        session_zero: session == 0,
        service,
        logon: token_logon(token.0)?,
    })
}

fn token_logon(token: HANDLE) -> Result<LogonContext, String> {
    let mut size = 0;
    unsafe {
        GetTokenInformation(token, TokenGroups, null_mut(), 0, &mut size);
    }
    if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER || !(16..=65536).contains(&size) {
        return Err("cannot measure token logon groups".into());
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token,
            TokenGroups,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    } == 0
    {
        return Err("cannot read token logon groups".into());
    }
    let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
    let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
    if offset + groups.GroupCount as usize * size_of::<SID_AND_ATTRIBUTES>() > size as usize {
        return Err("invalid token logon groups".into());
    }
    let entries =
        unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize) };
    // Winnt.h SE_GROUP_ENABLED (0x4), SE_GROUP_USE_FOR_DENY_ONLY (0x10).
    let has = |kind| {
        entries.iter().any(|group| {
            group.Attributes & 0x00000004 != 0
                && group.Attributes & 0x00000010 == 0
                && unsafe { IsWellKnownSid(group.Sid, kind) } != 0
        })
    };
    if !has(WinAuthenticatedUserSid) {
        return Ok(LogonContext::Unknown);
    }
    Ok(if has(WinNetworkSid) {
        LogonContext::Network
    } else if has(WinServiceSid) {
        LogonContext::Service
    } else if has(WinBatchSid) {
        LogonContext::Batch
    } else if has(WinInteractiveSid) {
        LogonContext::Interactive
    } else {
        LogonContext::Unknown
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Software,
    Tpm,
}
impl Provider {
    fn name(self) -> &'static str {
        match self {
            Self::Software => "Microsoft Software Key Storage Provider",
            Self::Tpm => "Microsoft Platform Crypto Provider",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    User,
    Machine,
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicKey {
    pub name: String,
    pub provider: Provider,
    pub scope: Scope,
    pub blob: Vec<u8>,
}

fn cng(operation: &str, status: i32) -> Result<(), String> {
    if status == 0 {
        return Ok(());
    }
    let kind = match status as u32 {
        0x8009000f => "collision",
        0x80090010 | 0x80070005 => "permission_denied",
        0x80090011 => "not_found",
        0x80090016 => "absent_or_unreadable_keyset",
        0x80090005 | 0x8009000b => "invalid_data_or_key_state",
        0x80090029 => "unsupported",
        0x80090013 | 0x80090017 | 0x80090030 | 0xc0000225 => "provider_unavailable",
        _ => "native_failure",
    };
    Err(format!("{operation}: {kind} (0x{:08x})", status as u32))
}

struct Object(usize);
impl Drop for Object {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                NCryptFreeObject(self.0);
            }
        }
    }
}

fn provider(kind: Provider) -> Result<Object, String> {
    provider_named(kind.name())
}

fn provider_named(name: &str) -> Result<Object, String> {
    let mut handle = 0;
    cng("open provider", unsafe {
        NCryptOpenStorageProvider(&mut handle, wide(name).as_ptr(), 0)
    })?;
    Ok(Object(handle))
}

fn property(handle: usize, name: *const u16, flags: u32) -> Result<Vec<u8>, String> {
    let mut length = 0;
    cng("property size", unsafe {
        NCryptGetProperty(handle, name, null_mut(), 0, &mut length, flags)
    })?;
    if length == 0 || length > 65536 {
        return Err("invalid property length".into());
    }
    let mut bytes = vec![0u8; length as usize];
    cng("read property", unsafe {
        NCryptGetProperty(handle, name, bytes.as_mut_ptr(), length, &mut length, flags)
    })?;
    if length as usize > bytes.len() {
        return Err("invalid property result length".into());
    }
    bytes.truncate(length as usize);
    Ok(bytes)
}

fn number(handle: usize, name: *const u16) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        property(handle, name, 0)?
            .try_into()
            .map_err(|_| "invalid numeric property")?,
    ))
}

fn set_number(handle: usize, name: *const u16, value: u32) -> Result<(), String> {
    cng("set policy", unsafe {
        NCryptSetProperty(handle, name, value.to_le_bytes().as_ptr(), 4, 0)
    })
}

pub struct Key {
    handle: Object,
    _provider: Object,
    pub name: String,
    pub kind: Provider,
    pub scope: Scope,
    delete_on_drop: bool,
    finalize_attempted: bool,
    recovery: Option<recovery::Receipt>,
    initial_acl_error: Option<String>,
}
impl Drop for Key {
    fn drop(&mut self) {
        if self.delete_on_drop && self.handle.0 != 0 && self.cleanup_recorded().is_err() {
            // The durable receipt, not this diagnostic, carries recovery identity.
            #[cfg(test)]
            if !std::thread::panicking() {
                panic!("unconfirmed PoC fixture cleanup; local recovery retained");
            }
            eprintln!("cleanup incomplete; local recovery retained");
        }
    }
}

impl Key {
    pub fn info(&self) -> Result<PublicKey, String> {
        Ok(PublicKey {
            name: self.name.clone(),
            provider: self.kind,
            scope: self.scope,
            blob: self.public()?,
        })
    }
    pub fn open(info: &PublicKey) -> Result<Self, String> {
        let key = Self::open_raw(info)?;
        if key.public()? != info.blob {
            return Err("public metadata mismatch; recovery required".into());
        }
        key.check_policy()?;
        Ok(key)
    }

    fn open_raw(info: &PublicKey) -> Result<Self, String> {
        let suffix = info
            .name
            .strip_prefix("RoLauncher.IdentityPoC.")
            .ok_or("not a PoC key")?;
        let id = uuid::Uuid::parse_str(suffix).map_err(|_| "invalid PoC name")?;
        if id.get_version_num() != 4 || id.to_string() != suffix {
            return Err("invalid PoC identifier".into());
        }
        let provider = provider(info.provider)?;
        let mut handle = 0;
        cng("reopen key; recovery required on failure", unsafe {
            NCryptOpenKey(
                provider.0,
                &mut handle,
                wide(&info.name).as_ptr(),
                0,
                NCRYPT_SILENT_FLAG
                    | if info.scope == Scope::Machine {
                        NCRYPT_MACHINE_KEY_FLAG
                    } else {
                        0
                    },
            )
        })?;
        Ok(Self {
            handle: Object(handle),
            _provider: provider,
            name: info.name.clone(),
            kind: info.provider,
            scope: info.scope,
            delete_on_drop: false,
            finalize_attempted: true,
            recovery: None,
            initial_acl_error: None,
        })
    }

    fn check_policy(&self) -> Result<(), String> {
        if number(self.handle.0, NCRYPT_EXPORT_POLICY_PROPERTY)? != 0
            || number(self.handle.0, NCRYPT_KEY_USAGE_PROPERTY)? != NCRYPT_ALLOW_SIGNING_FLAG
            || number(self.handle.0, NCRYPT_LENGTH_PROPERTY)? != 2048
            || (number(self.handle.0, NCRYPT_KEY_TYPE_PROPERTY)? & NCRYPT_MACHINE_KEY_FLAG != 0)
                != (self.scope == Scope::Machine)
        {
            return Err("key scope or policy mismatch".into());
        }
        let implementation = number(self._provider.0, NCRYPT_IMPL_TYPE_PROPERTY)?;
        let required = if self.kind == Provider::Tpm {
            NCRYPT_IMPL_HARDWARE_FLAG
        } else {
            NCRYPT_IMPL_SOFTWARE_FLAG
        };
        if implementation & required == 0 {
            return Err("provider capability mismatch; no fallback".into());
        }
        Ok(())
    }

    pub fn inspection(&self, identity: &Identity) -> Result<serde_json::Value, String> {
        self.check_policy()?;
        let bytes = property(
            self.handle.0,
            NCRYPT_SECURITY_DESCR_PROPERTY,
            DACL_SECURITY_INFORMATION,
        )?;
        let mut aligned = vec![0usize; bytes.len().div_ceil(size_of::<usize>())];
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                aligned.as_mut_ptr().cast::<u8>(),
                bytes.len(),
            );
        }
        let descriptor = aligned.as_mut_ptr().cast();
        if unsafe { IsValidSecurityDescriptor(descriptor) } == 0 {
            return Err("invalid key security descriptor".into());
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = null_mut();
        if unsafe { GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) }
            == 0
        {
            return Err(win_error("key DACL"));
        }
        if present == 0 || acl.is_null() || unsafe { IsValidAcl(acl) } == 0 {
            return Err("missing or invalid key DACL".into());
        }
        let mut principals = Vec::new();
        let mut access_masks = Vec::new();
        for index in 0..unsafe { (*acl).AceCount } as u32 {
            let mut ace = null_mut();
            if unsafe { GetAce(acl, index, &mut ace) } == 0 {
                return Err(win_error("key ACE"));
            }
            let header = unsafe { &*ace.cast::<ACE_HEADER>() };
            if header.AceType != 0 || header.AceFlags != 0 {
                return Err(format!(
                    "unexpected key ACE type/flags: type={} flags=0x{:02x}",
                    header.AceType, header.AceFlags
                ));
            }
            let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
            let sid = (&allowed.SidStart as *const u32).cast_mut().cast();
            if unsafe { IsValidSid(sid) } == 0 {
                return Err("invalid ACE SID".into());
            }
            let mut text = null_mut();
            if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
                return Err(win_error("ACE SID"));
            }
            let allocation = LocalAllocation(text.cast());
            let mut length = 0;
            while unsafe { *text.add(length) } != 0 {
                length += 1;
            }
            let text = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) })
                .map_err(|_| "invalid ACE encoding")?;
            drop(allocation);
            if text == identity.sid {
                principals.push("owner");
            } else if text == "S-1-5-18" {
                principals.push("system");
            } else {
                return Err("unexpected key ACL principal".into());
            }
            // Microsoft KSP can expand GA to include generic/file rights on readback.
            if allowed.Mask & 0x10000000 == 0 && allowed.Mask != 0x001f01ff {
                return Err(format!(
                    "missing PoC full-control rights: 0x{:08x}",
                    allowed.Mask
                ));
            }
            access_masks.push(allowed.Mask);
        }
        principals.sort();
        if principals != ["owner", "system"] {
            return Err("key ACL membership mismatch".into());
        }
        let mut control = 0;
        let mut revision = 0;
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0
            || control & SE_DACL_PROTECTED == 0
        {
            return Err("key DACL inheritance not protected".into());
        }
        Ok(
            serde_json::json!({"scope": self.scope, "provider": self.kind,
            "provider_implementation": number(self._provider.0, NCRYPT_IMPL_TYPE_PROPERTY)?,
            "security_descriptor_support": number(self._provider.0, NCRYPT_SECURITY_DESCR_SUPPORT_PROPERTY)?,
            "export_policy": number(self.handle.0, NCRYPT_EXPORT_POLICY_PROPERTY)?,
            "key_usage": number(self.handle.0, NCRYPT_KEY_USAGE_PROPERTY)?, "bits": 2048,
            "acl": {"only_owner_and_system": true, "protected": true, "access_masks": access_masks, "rights": "GENERIC_ALL for PoC owner and SYSTEM; not production least privilege"},
            "post_finalize_acl_repair": self.initial_acl_error,
            "attestation": "not performed"}),
        )
    }

    pub fn delete(mut self) -> Result<recovery::CleanupOutcome, String> {
        self.cleanup_recorded()
    }

    fn cleanup_recorded(&mut self) -> Result<recovery::CleanupOutcome, String> {
        use recovery::CleanupOutcome;
        self.delete_on_drop = false;
        if self.recovery.is_none() {
            let identity = effective_identity()?;
            self.inspection(&identity)?;
            self.recovery = Some(recovery::Receipt::for_existing(self.info()?, &identity)?);
        }
        let receipt = self
            .recovery
            .as_mut()
            .ok_or("missing local recovery receipt")?;
        receipt.refresh()?;
        if let Some(outcome) = receipt.record.cleanup
            && outcome != CleanupOutcome::Incomplete
        {
            return Ok(CleanupOutcome::Absent); // Prior confirmed disposal of this exact identity.
        }
        let (outcome, status) = if !self.finalize_attempted {
            let status = unsafe { NCryptFreeObject(self.handle.0) };
            if status == 0 {
                self.handle.0 = 0;
                (CleanupOutcome::NeverPersisted, 0)
            } else {
                (CleanupOutcome::Incomplete, status)
            }
        } else {
            let status = if recovery::delete_denied() {
                0x80090010u32 as i32
            } else {
                unsafe {
                    NCryptDeleteKey(
                        self.handle.0,
                        if self.kind == Provider::Tpm {
                            0
                        } else {
                            NCRYPT_SILENT_FLAG
                        },
                    )
                }
            };
            match status as u32 {
                0 => {
                    self.handle.0 = 0;
                    (CleanupOutcome::Succeeded, status)
                }
                0x80090011 => (CleanupOutcome::Absent, status),
                _ => (CleanupOutcome::Incomplete, status),
            }
        };
        receipt.finish(outcome, Some(status as u32))?;
        if outcome == CleanupOutcome::Incomplete {
            Err("cleanup incomplete; local recovery retained".into())
        } else {
            Ok(outcome)
        }
    }
    #[cfg(test)]
    pub fn create(kind: Provider, scope: Scope, identity: &Identity) -> Result<Self, String> {
        Self::create_named(
            kind,
            scope,
            identity,
            &format!("RoLauncher.IdentityPoC.{}", uuid::Uuid::new_v4()),
        )
    }

    #[cfg(test)]
    pub fn create_named(
        kind: Provider,
        scope: Scope,
        identity: &Identity,
        name: &str,
    ) -> Result<Self, String> {
        Self::create_measured(
            kind,
            scope,
            identity,
            name,
            true,
            ContextPurpose::Diagnostic,
        )
    }

    // Only the diagnostic key-probe may return an ACL-rejected key to its local, fixed
    // signing measurement. Enrollment always enforces ACLs; no such handle is persisted.
    fn create_measured(
        kind: Provider,
        scope: Scope,
        identity: &Identity,
        name: &str,
        enforce_acl: bool,
        purpose: ContextPurpose,
    ) -> Result<Self, String> {
        if scope == Scope::Machine {
            return Err(
                "machine-key provisioning requires the human-confirmed disposable lab command"
                    .into(),
            );
        }
        let suffix = name
            .strip_prefix("RoLauncher.IdentityPoC.")
            .ok_or("not a PoC name")?;
        let id = uuid::Uuid::parse_str(suffix).map_err(|_| "invalid PoC name")?;
        if id.get_version_num() != 4 || id.to_string() != suffix {
            return Err("invalid PoC name".into());
        }
        identity.accept(purpose)?;
        let provider = provider(kind)?;
        let planned = PublicKey {
            name: name.into(),
            provider: kind,
            scope,
            blob: Vec::new(),
        };
        let mut receipt = recovery::Receipt::prepare(planned, identity)?;
        let mut handle = 0;
        let created = unsafe {
            NCryptCreatePersistedKey(
                provider.0,
                &mut handle,
                NCRYPT_RSA_ALGORITHM,
                wide(name).as_ptr(),
                0,
                NCRYPT_SILENT_FLAG
                    | if scope == Scope::Machine {
                        NCRYPT_MACHINE_KEY_FLAG
                    } else {
                        0
                    },
            )
        };
        if created != 0 {
            receipt.finish(
                recovery::CleanupOutcome::NeverPersisted,
                Some(created as u32),
            )?;
            return cng("create persisted key", created)
                .and_then(|_| Err("unexpected creation result".into()));
        }
        let mut key = Self {
            handle: Object(handle),
            _provider: provider,
            name: name.into(),
            kind,
            scope,
            delete_on_drop: true,
            finalize_attempted: false,
            recovery: Some(receipt),
            initial_acl_error: None,
        };
        let result: Result<(), String> = (|| {
            key.recovery.as_mut().unwrap().record.phase = recovery::Phase::Created;
            key.recovery.as_mut().unwrap().save()?;
            recovery::checkpoint(recovery::FaultStage::BeforeFinalize)?;
            set_number(handle, NCRYPT_LENGTH_PROPERTY, 2048)?;
            set_number(handle, NCRYPT_EXPORT_POLICY_PROPERTY, 0)?;
            set_number(handle, NCRYPT_KEY_USAGE_PROPERTY, NCRYPT_ALLOW_SIGNING_FLAG)?;
            // Request protection before finalization; measure whether the provider honors it.
            let descriptor = wide(&format!("D:P(A;;GA;;;SY)(A;;GA;;;{})", identity.sid));
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
                return Err(win_error("PoC descriptor"));
            }
            let allocation = LocalAllocation(pointer);
            cng("set PoC DACL", unsafe {
                NCryptSetProperty(
                    handle,
                    NCRYPT_SECURITY_DESCR_PROPERTY,
                    pointer.cast(),
                    length,
                    DACL_SECURITY_INFORMATION | NCRYPT_PERSIST_FLAG,
                )
            })?;
            key.recovery.as_mut().unwrap().record.phase = recovery::Phase::FinalizeAttempted;
            key.recovery.as_mut().unwrap().save()?;
            key.finalize_attempted = true;
            cng("finalize key", unsafe {
                NCryptFinalizeKey(handle, NCRYPT_SILENT_FLAG)
            })?;
            let public = key.public()?;
            let receipt = key.recovery.as_mut().unwrap();
            receipt.record.phase = recovery::Phase::Finalized;
            receipt.record.key.blob = public;
            receipt.save()?;
            recovery::checkpoint(recovery::FaultStage::AfterFinalize)?;
            // PCP may materialize inherited ACLs at finalization despite accepting the earlier
            // property. Measure this explicitly; post-finalize repair is experimental only.
            key.initial_acl_error = key.inspection(identity).err();
            if enforce_acl && let Some(error) = &key.initial_acl_error {
                return Err(format!(
                    "provider did not retain requested initial key policy/ACL: {error}"
                ));
            }
            if key.initial_acl_error.is_some() {
                cng("reapply PoC DACL after finalize", unsafe {
                    NCryptSetProperty(
                        handle,
                        NCRYPT_SECURITY_DESCR_PROPERTY,
                        pointer.cast(),
                        length,
                        DACL_SECURITY_INFORMATION | NCRYPT_PERSIST_FLAG,
                    )
                })?;
            }
            drop(allocation);
            if enforce_acl {
                key.inspection(identity)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(key),
            Err(error) => match key.cleanup_recorded() {
                Ok(outcome) => Err(format!("{error}; cleanup outcome: {outcome:?}")),
                Err(_) => Err(format!(
                    "{error}; cleanup incomplete; local recovery retained"
                )),
            },
        }
    }

    pub fn public(&self) -> Result<Vec<u8>, String> {
        let mut length = 0;
        cng("public export size", unsafe {
            NCryptExportKey(
                self.handle.0,
                0,
                BCRYPT_RSAPUBLIC_BLOB,
                null(),
                null_mut(),
                0,
                &mut length,
                NCRYPT_SILENT_FLAG,
            )
        })?;
        if length > 4096 {
            return Err("invalid public key size".into());
        }
        let mut output = vec![0u8; length as usize];
        cng("public export", unsafe {
            NCryptExportKey(
                self.handle.0,
                0,
                BCRYPT_RSAPUBLIC_BLOB,
                null(),
                output.as_mut_ptr(),
                length,
                &mut length,
                NCRYPT_SILENT_FLAG,
            )
        })?;
        output.truncate(length as usize);
        Ok(output)
    }

    pub fn sign(&self, digest: &[u8; 32]) -> Result<Vec<u8>, String> {
        let padding = BCRYPT_PKCS1_PADDING_INFO {
            pszAlgId: BCRYPT_SHA256_ALGORITHM,
        };
        let mut length = 0;
        let mut signature = vec![0; 256];
        cng("sign synthetic challenge", unsafe {
            NCryptSignHash(
                self.handle.0,
                (&padding as *const BCRYPT_PKCS1_PADDING_INFO).cast(),
                digest.as_ptr(),
                32,
                signature.as_mut_ptr(),
                256,
                &mut length,
                NCRYPT_PAD_PKCS1_FLAG | NCRYPT_SILENT_FLAG,
            )
        })?;
        if length != 256 {
            return Err("unexpected signature size".into());
        }
        Ok(signature)
    }
}
pub fn verify(public: &[u8], digest: &[u8; 32], signature: &[u8]) -> Result<(), String> {
    if !(24..=4096).contains(&public.len()) || signature.len() != 256 {
        return Err("invalid proof sizes".into());
    }
    let provider = provider(Provider::Software)?;
    let mut handle = 0;
    cng("import public key", unsafe {
        NCryptImportKey(
            provider.0,
            0,
            BCRYPT_RSAPUBLIC_BLOB,
            null(),
            &mut handle,
            public.as_ptr(),
            public.len() as u32,
            NCRYPT_SILENT_FLAG,
        )
    })?;
    let key = Object(handle);
    let padding = BCRYPT_PKCS1_PADDING_INFO {
        pszAlgId: BCRYPT_SHA256_ALGORITHM,
    };
    cng("verify signature", unsafe {
        NCryptVerifySignature(
            key.0,
            (&padding as *const BCRYPT_PKCS1_PADDING_INFO).cast(),
            digest.as_ptr(),
            32,
            signature.as_ptr(),
            signature.len() as u32,
            NCRYPT_PAD_PKCS1_FLAG | NCRYPT_SILENT_FLAG,
        )
    })
}

pub struct Proof {
    pub installation: Vec<u8>,
    pub user: Vec<u8>,
}
pub struct Verifier {
    expected: super::Challenge,
    installation: Vec<u8>,
    user: Vec<u8>,
    used: bool,
}
impl Verifier {
    pub fn new(
        expected: super::Challenge,
        installation: Vec<u8>,
        user: Vec<u8>,
    ) -> Result<Self, String> {
        expected.canonical()?;
        if hash(&installation)? != expected.installation_key
            || hash(&user)? != expected.user_key
            || installation == user
        {
            return Err("required distinct public-key evidence mismatched".into());
        }
        Ok(Self {
            expected,
            installation,
            user,
            used: false,
        })
    }
    pub fn verify(&mut self, challenge: &super::Challenge, proof: &Proof) -> Result<(), String> {
        if self.used || challenge != &self.expected {
            return Err("spent nonce or identity context mismatch".into());
        }
        let digest = hash(&challenge.canonical()?)?;
        verify(&self.installation, &digest, &proof.installation)?;
        verify(&self.user, &digest, &proof.user)?;
        self.used = true;
        Ok(())
    }
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum Candidate {
    A,
    B,
    C,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u16,
    candidate: Candidate,
    purpose: ContextPurpose,
    owner_sid_reference: [u8; 32],
    installation_evidence: [u8; 32],
    installation: PublicKey,
    user: PublicKey,
}

fn machine_evidence() -> Result<[u8; 32], String> {
    // Public, mutable installation label. Never claim this is hardware proof or upload it.
    let mut buffer = [0u16; 128];
    let mut bytes = size_of_val(&buffer) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide("SOFTWARE\\Microsoft\\Cryptography").as_ptr(),
            wide("MachineGuid").as_ptr(),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status != 0 {
        return Err(format!(
            "public installation evidence unavailable: Windows error {status}"
        ));
    }
    if bytes < 4 || bytes as usize > size_of_val(&buffer) || !bytes.is_multiple_of(2) {
        return Err("invalid installation evidence".into());
    }
    let length = bytes as usize / 2;
    if buffer[length - 1] != 0 {
        return Err("unterminated installation evidence".into());
    }
    let text = String::from_utf16(&buffer[..length - 1])
        .map_err(|_| "invalid installation evidence encoding")?;
    let guid = uuid::Uuid::parse_str(&text).map_err(|_| "invalid installation evidence format")?;
    hash(guid.as_bytes())
}

fn read_state(path: &std::path::Path) -> Result<State, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "cannot open PoC public state")?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read PoC state")?;
    if bytes.len() > 65536 {
        return Err("PoC public state too large".into());
    }
    let state: State = serde_json::from_slice(&bytes)
        .map_err(|_| "corrupt PoC public metadata; recovery required")?;
    let provider = if matches!(state.candidate, Candidate::C) {
        Provider::Tpm
    } else {
        Provider::Software
    };
    let scope = if matches!(state.candidate, Candidate::B) {
        Scope::User
    } else {
        Scope::Machine
    };
    if state.schema_version != 1
        || state.installation.provider != provider
        || state.user.provider != provider
        || state.installation.scope != scope
        || state.user.scope != Scope::User
        || state.owner_sid_reference == [0; 32]
        || state.installation_evidence == [0; 32]
        || state.installation.name == state.user.name
        || state.installation.blob == state.user.blob
        || state.installation.blob.len() > 4096
        || state.user.blob.len() > 4096
    {
        return Err("invalid PoC identity metadata".into());
    }
    Ok(state)
}

pub fn create_state(candidate: Candidate, path: &std::path::Path) -> Result<(), String> {
    create_state_for(candidate, path, ContextPurpose::Enrollment)
}

pub fn measure_state(candidate: Candidate, path: &std::path::Path) -> Result<(), String> {
    create_state_for(candidate, path, ContextPurpose::Diagnostic)
}

fn create_state_for(
    candidate: Candidate,
    path: &std::path::Path,
    purpose: ContextPurpose,
) -> Result<(), String> {
    use std::io::Write;
    let identity = effective_identity()?;
    identity.accept(purpose)?;
    // Reserve the state file first. No replacement, new identity or key deletion on collision.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| "PoC state already exists or is inaccessible")?;
    let mut owned = Vec::new();
    let result: Result<(), String> = (|| {
        let evidence = machine_evidence()?;
        let provider = if matches!(candidate, Candidate::C) {
            Provider::Tpm
        } else {
            Provider::Software
        };
        let scope = if matches!(candidate, Candidate::B) {
            Scope::User
        } else {
            Scope::Machine
        };
        owned.push(Key::create_measured(
            provider,
            scope,
            &identity,
            &format!("RoLauncher.IdentityPoC.{}", uuid::Uuid::new_v4()),
            true,
            purpose,
        )?);
        recovery::checkpoint(recovery::FaultStage::AfterFirstKey)?;
        owned.push(Key::create_measured(
            provider,
            Scope::User,
            &identity,
            &format!("RoLauncher.IdentityPoC.{}", uuid::Uuid::new_v4()),
            true,
            purpose,
        )?);
        let state = State {
            schema_version: 1,
            candidate,
            purpose,
            owner_sid_reference: identity.sid_reference,
            installation_evidence: evidence,
            installation: owned[0].info()?,
            user: owned[1].info()?,
        };
        let bytes =
            serde_json::to_vec_pretty(&state).map_err(|_| "cannot encode PoC public state")?;
        let split = bytes.len() / 2;
        file.write_all(&bytes[..split])
            .and_then(|_| file.sync_all())
            .map_err(|_| "cannot persist PoC public state")?;
        recovery::checkpoint(recovery::FaultStage::PersistState)?;
        file.write_all(&bytes[split..])
            .and_then(|_| file.sync_all())
            .map_err(|_| "cannot persist PoC public state")?;
        for key in &mut owned {
            let receipt = key.recovery.as_mut().unwrap();
            receipt.record.phase = recovery::Phase::RetainedForState;
            receipt.save()?;
        }
        for key in &mut owned {
            key.delete_on_drop = false;
        }
        Ok(())
    })();
    drop(file);
    if let Err(error) = result {
        // Failed state and every pre-creation receipt remain strictly local, including
        // partial writes. Explicit cleanup, never Drop logging, determines the outcome.
        return match cleanup_owned(owned) {
            Ok(outcomes) => Err(format!(
                "{error}; owned key cleanup: {outcomes:?}; failed state retained locally"
            )),
            Err(_) => Err(format!(
                "{error}; cleanup incomplete; local recovery retained"
            )),
        };
    }
    Ok(())
}

fn cleanup_owned(keys: Vec<Key>) -> Result<Vec<recovery::CleanupOutcome>, String> {
    let mut outcomes = Vec::new();
    let mut incomplete = false;
    for key in keys {
        match key.delete() {
            Ok(outcome) => outcomes.push(outcome),
            Err(_) => incomplete = true,
        }
    }
    if incomplete {
        Err("cleanup incomplete; local recovery retained".into())
    } else {
        Ok(outcomes)
    }
}

fn bound_challenge(
    state: &State,
    identity: &Identity,
    evidence: [u8; 32],
) -> Result<super::Challenge, String> {
    identity.accept(state.purpose)?;
    if identity.sid_reference != state.owner_sid_reference {
        return Err("Windows user mismatch; no automatic authorization".into());
    }
    if evidence != state.installation_evidence {
        return Err("installation evidence changed; recovery required".into());
    }
    Ok(super::Challenge {
        schema_version: 1,
        nonce: rand::random(),
        installation_key: hash(&state.installation.blob)?,
        user_key: hash(&state.user.blob)?,
        windows_sid: identity.sid_reference,
        installation_evidence: evidence,
        synthetic_customer: "synthetic:customer-a".into(),
        purpose: super::PURPOSE.into(),
    })
}

pub fn check_state(path: &std::path::Path) -> Result<(), String> {
    check_state_for(path, ContextPurpose::Enrollment)
}
pub fn measure_check(path: &std::path::Path) -> Result<(), String> {
    check_state_for(path, ContextPurpose::Diagnostic)
}
fn check_state_for(path: &std::path::Path, purpose: ContextPurpose) -> Result<(), String> {
    let state = read_state(path)?;
    if state.purpose != purpose {
        return Err("state context purpose mismatch; diagnostic evidence is not enrollment".into());
    }
    let identity = effective_identity()?;
    let challenge = bound_challenge(&state, &identity, machine_evidence()?)?;
    let installation = Key::open(&state.installation)?;
    let user = Key::open(&state.user)?;
    installation.inspection(&identity)?;
    user.inspection(&identity)?;
    let digest = hash(&challenge.canonical()?)?;
    let proof = Proof {
        installation: installation.sign(&digest)?,
        user: user.sign(&digest)?,
    };
    Verifier::new(challenge.clone(), state.installation.blob, state.user.blob)?
        .verify(&challenge, &proof)
}

pub fn cleanup_state(path: &std::path::Path) -> Result<Vec<recovery::CleanupOutcome>, String> {
    let state = read_state(path)?;
    let identity = effective_identity()?;
    if identity.sid_reference != state.owner_sid_reference
        || identity.thread_token
        || identity.service
        || identity.restricted
    {
        return Err("PoC cleanup requires original unrestricted user context".into());
    }
    // Each deletion independently requires namespace, public-key and ACL verification.
    // Keep trying other owned keys after loss/denial; never misreport partial cleanup as complete.
    let mut errors = Vec::new();
    let mut outcomes = Vec::new();
    for info in [&state.installation, &state.user] {
        let result = (|| {
            if recovery::confirmed_disposal(info, &identity)? {
                return Ok(recovery::CleanupOutcome::Absent);
            }
            let key = Key::open(info)?;
            key.inspection(&identity)?;
            key.delete()
        })();
        match result {
            Ok(outcome) => outcomes.push(outcome),
            Err(error) => errors.push(error),
        }
    }
    if errors.is_empty() {
        Ok(outcomes)
    } else {
        Err(format!(
            "PoC cleanup incomplete or keyset absent/unreadable; retain public state: {}",
            errors.join("; ")
        ))
    }
}

fn context() -> Result<serde_json::Value, String> {
    let identity = effective_identity()?;
    Ok(
        serde_json::json!({"effective_token": if identity.thread_token {"thread"} else {"process"},
        "elevated": identity.elevated, "restricted": identity.restricted, "session_zero": identity.session_zero,
        "service_identity": identity.service, "logon_context": identity.logon, "diagnostic_context_supported": identity.accept(ContextPurpose::Diagnostic).is_ok(), "raw_sid_output": false,
        "ordinary_interactive_user_evidence": identity.accept(ContextPurpose::Enrollment).is_ok()}),
    )
}

pub fn inspect_provider_name(name: &str) -> Result<serde_json::Value, String> {
    let provider = provider_named(name)?;
    let implementation = number(provider.0, NCRYPT_IMPL_TYPE_PROPERTY)?;
    cng("RSA support", unsafe {
        NCryptIsAlgSupported(provider.0, NCRYPT_RSA_ALGORITHM, 0)
    })?;
    Ok(
        serde_json::json!({"status": "provider available; key creation not implied", "implementation": implementation,
        "descriptor_support": number(provider.0, NCRYPT_SECURITY_DESCR_SUPPORT_PROPERTY)?,
        "version": number(provider.0, NCRYPT_VERSION_PROPERTY)?, "rsa_supported": true, "attestation": "not tested"}),
    )
}

fn capabilities() -> serde_json::Value {
    let mut results = Vec::new();
    for kind in [Provider::Software, Provider::Tpm] {
        let observation = inspect_provider_name(kind.name());
        results.push(match observation {
            Ok(mut value) => { value["provider"] = serde_json::json!(kind); value["provider_name"] = kind.name().into(); value },
            Err(error) => serde_json::json!({"provider": kind, "status": "unavailable or unsupported", "error": error, "fallback": false})
        });
    }
    serde_json::json!({"providers": results, "context": context().unwrap_or_else(|error| serde_json::json!({"error":error})), "cross_user": "not tested by this command", "reboot": "not tested by this command"})
}

pub fn probe_key(kind: Provider, scope: Scope) -> Result<serde_json::Value, String> {
    let identity = effective_identity()?;
    let key = Key::create_measured(
        kind,
        scope,
        &identity,
        &format!("RoLauncher.IdentityPoC.{}", uuid::Uuid::new_v4()),
        false,
        ContextPurpose::Diagnostic,
    )?;
    let result: Result<serde_json::Value, String> = (|| {
        recovery::checkpoint(recovery::FaultStage::ProbeOperation)?;
        let info = key.info()?;
        let reopened = Key::open_raw(&info)?;
        let report = reopened.inspection(&identity);
        let provisioning = key.inspection(&identity);
        let accepted = report.is_ok() && provisioning.is_ok() && key.initial_acl_error.is_none();
        let implementation = number(key._provider.0, NCRYPT_IMPL_TYPE_PROPERTY)?;
        let digest = hash(b"RoLauncher-Identity-PoC-provider-probe-v1")?;
        verify(&info.blob, &digest, &reopened.sign(&digest)?)?;
        drop(reopened);
        Ok(
            serde_json::json!({"context": context()?, "execution_purpose": "diagnostic", "enrollment_accepted": false, "provider": kind, "scope": scope, "key_acl_result": report, "provisioning_acl_result": provisioning,
        "provider_implementation": implementation,
        "local_key_policy_accepted": accepted, "sign_verify_reopen": "proven by real Windows test; does not imply acceptable ACLs", "cleanup": "deleted", "cross_user": "not tested", "reboot": "not tested", "attestation": "not tested"}),
        )
    })();
    let cleanup = key.delete();
    match (result, cleanup) {
        (Ok(report), Ok(_)) => Ok(report),
        (Err(error), Ok(outcome)) => Err(format!("{error}; cleanup outcome: {outcome:?}")),
        (_, Err(_)) => Err("cleanup incomplete; local recovery retained".into()),
    }
}

fn inspect_state(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let state = read_state(path)?;
    let identity = effective_identity()?;
    Ok(
        serde_json::json!({"candidate": state.candidate, "execution_purpose": state.purpose, "enrollment_accepted": state.purpose == ContextPurpose::Enrollment && identity.accept(ContextPurpose::Enrollment).is_ok(), "context": context()?,
        "installation": Key::open(&state.installation)?.inspection(&identity)?,
        "user": Key::open(&state.user)?.inspection(&identity)?, "public_identifiers_omitted": true}),
    )
}

fn probe_state(path: &std::path::Path) -> Result<serde_json::Value, String> {
    let state = read_state(path)?;
    let identity = effective_identity()?;
    identity.accept(ContextPurpose::Diagnostic)?;
    let mut results = Vec::new();
    // Deliberately bypass application SID/ACL policy checks. This measures OS key-use enforcement.
    // Fixed synthetic digest only; no caller-supplied arbitrary signing input.
    let digest = hash(b"RoLauncher-Identity-PoC-native-access-probe-v1")?;
    for (role, info) in [("installation", &state.installation), ("user", &state.user)] {
        let attempt = Key::open_raw(info);
        let value = match attempt {
            Err(error) => {
                serde_json::json!({"role": role, "open": "failed", "private_key_exercised": false, "error": error})
            }
            Ok(key) => match key
                .sign(&digest)
                .and_then(|signature| verify(&info.blob, &digest, &signature))
            {
                Ok(()) => {
                    serde_json::json!({"role": role, "open": "succeeded", "private_key_exercised": true})
                }
                Err(error) => {
                    serde_json::json!({"role": role, "open": "succeeded", "private_key_exercised": false, "error": error})
                }
            },
        };
        results.push(value);
    }
    Ok(
        serde_json::json!({"context": context()?, "owner_sid_matches": identity.sid_reference == state.owner_sid_reference, "native_access": results, "authorization_granted": false}),
    )
}

pub fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0].starts_with("lab-") {
        return lab::run(&args);
    }
    let report = match args.as_slice() {
        [command] if command == "context" => context()?,
        [command] if command == "capabilities" => capabilities(),
        [command, kind, scope] if command == "key-probe" => {
            let kind = match kind.as_str() { "software" => Provider::Software, "tpm" => Provider::Tpm, _ => return Err("provider must be software or tpm".into()) };
            let scope = match scope.as_str() { "user" => Scope::User, "machine" => Scope::Machine, _ => return Err("scope must be user or machine".into()) };
            probe_key(kind, scope)?
        },
        [command, candidate, path] if command == "create" || command == "measure" => {
            let candidate = match candidate.as_str() { "A" => Candidate::A, "B" => Candidate::B, "C" => Candidate::C, _ => return Err("candidate must be A, B or C".into()) };
            if command == "create" { create_state(candidate, std::path::Path::new(path))?; } else { measure_state(candidate, std::path::Path::new(path))?; }
            serde_json::json!({"state_persisted": true, "execution_purpose": if command == "create" {"enrollment"} else {"diagnostic"}, "enrollment_accepted": command == "create", "context": context()?})
        },
        [command, path] if command == "check" || command == "measure-check" => { if command == "check" { check_state(std::path::Path::new(path))?; } else { measure_check(std::path::Path::new(path))?; } serde_json::json!({"synthetic_proof": "verified", "execution_purpose": if command == "check" {"enrollment"} else {"diagnostic"}, "enrollment_accepted": command == "check", "context": context()?, "customer_authenticated": false}) },
        [command, path] if command == "recover" => serde_json::json!({"cleanup_outcome": recovery::recover(std::path::Path::new(path))?}),
        [command] if command == "recovery-summary" => recovery::summary()? ,
        [command, path] if command == "inspect" => inspect_state(std::path::Path::new(path))?,
        [command, path] if command == "probe" => probe_state(std::path::Path::new(path))?,
        [command, path] if command == "cleanup" => { let outcomes = cleanup_state(std::path::Path::new(path))?; serde_json::json!({"cleanup_outcomes": outcomes, "poc_key_cleanup": "complete", "public_state": "retained; remove manually"}) },
        _ => return Err("Usage: context | capabilities | key-probe software|tpm user|machine | create|measure A|B|C PUBLIC_STATE.json | check|measure-check|inspect|probe|cleanup PUBLIC_STATE.json | recover LOCAL_RECEIPT.jsonl | recovery-summary".into()),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report).map_err(|_| "cannot encode sanitized report")?
    );
    if report.get("local_key_policy_accepted") == Some(&serde_json::Value::Bool(false)) {
        return Err("provider operations measured; PoC ACL requirements unmet".into());
    }
    Ok(())
}
