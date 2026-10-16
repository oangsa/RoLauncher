//! Append-only local recovery receipts, written before native key creation. Never upload.
use super::{Identity, Key, PublicKey};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupOutcome {
    Succeeded,
    NeverPersisted,
    Absent,
    Incomplete,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Reserved,
    Created,
    FinalizeAttempted,
    Finalized,
    RetainedForState,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u16,
    pub key: PublicKey,
    pub owner_sid_reference: [u8; 32],
    pub phase: Phase,
    pub cleanup: Option<CleanupOutcome>,
    pub native_status: Option<u32>,
}

pub(super) struct Receipt {
    file: File,
    path: PathBuf,
    pub record: Snapshot,
}

pub fn valid_name(name: &str) -> Result<uuid::Uuid, String> {
    let suffix = name
        .strip_prefix("RoLauncher.IdentityPoC.")
        .ok_or("not a PoC key")?;
    let id = uuid::Uuid::parse_str(suffix).map_err(|_| "invalid PoC key identifier")?;
    if id.get_version_num() != 4 || id.to_string() != suffix {
        return Err("invalid PoC key identifier".into());
    }
    Ok(id)
}

fn directory() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = FAULTS.with(|f| f.borrow().as_ref().map(|f| f.directory.clone())) {
        return path;
    }
    std::env::var_os("ROLAUNCHER_IDENTITY_POC_RECOVERY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/identity-poc-recovery"))
}

fn path_for(key: &PublicKey) -> Result<PathBuf, String> {
    let id = valid_name(&key.name)?;
    Ok(directory().join(format!("{:?}-{:?}-{id}.jsonl", key.provider, key.scope)))
}

pub(super) fn private_new_file(path: &Path, identity: &Identity) -> Result<File, String> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
    };
    let text = super::wide(&format!("D:P(A;;FA;;;SY)(A;;FA;;;{})", identity.sid));
    let mut descriptor = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err("cannot protect local recovery receipt".into());
    }
    let allocation = super::LocalAllocation(descriptor);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let absolute = std::path::absolute(path).map_err(|_| "invalid local recovery directory")?;
    use std::os::windows::ffi::OsStrExt;
    let name: Vec<u16> = absolute.as_os_str().encode_wide().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            FILE_APPEND_DATA | FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    drop(allocation);
    if handle == INVALID_HANDLE_VALUE {
        return Err(
            "key collision or inaccessible existing local recovery receipt; no overwrite".into(),
        );
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

impl Receipt {
    pub fn prepare(key: PublicKey, identity: &Identity) -> Result<Self, String> {
        let path = path_for(&key)?;
        std::fs::create_dir_all(path.parent().ok_or("invalid recovery path")?)
            .map_err(|_| "cannot create local recovery directory")?;
        let file = private_new_file(&path, identity)?;
        let mut receipt = Self {
            file,
            path,
            record: Snapshot {
                schema_version: 1,
                key,
                owner_sid_reference: identity.sid_reference,
                phase: Phase::Reserved,
                cleanup: None,
                native_status: None,
            },
        };
        receipt.save()?;
        Ok(receipt)
    }

    pub fn open(path: &Path, identity: &Identity) -> Result<Self, String> {
        let record = snapshot(path)?;
        if record.owner_sid_reference != identity.sid_reference {
            return Err("local recovery owner mismatch".into());
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .append(true)
            .open(path)
            .map_err(|_| "cannot open local recovery receipt")?;
        Ok(Self {
            file,
            path: path.into(),
            record,
        })
    }

    pub fn for_existing(key: PublicKey, identity: &Identity) -> Result<Self, String> {
        let path = path_for(&key)?;
        if path.exists() {
            let receipt = Self::open(&path, identity)?;
            if !same_key(&receipt.record.key, &key) || receipt.record.key.blob != key.blob {
                return Err("local recovery key evidence mismatch".into());
            }
            Ok(receipt)
        } else {
            let mut receipt = Self::prepare(key, identity)?;
            receipt.record.phase = Phase::Finalized;
            receipt.save()?;
            Ok(receipt)
        }
    }

    pub fn save(&mut self) -> Result<(), String> {
        let mut bytes =
            serde_json::to_vec(&self.record).map_err(|_| "cannot encode local recovery receipt")?;
        bytes.push(b'\n');
        self.file
            .write_all(&bytes)
            .and_then(|_| self.file.sync_all())
            .map_err(|_| {
                "cannot synchronize local recovery receipt; prior recovery identity retained".into()
            })
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        self.record = snapshot(&self.path)?;
        Ok(())
    }

    pub fn finish(
        &mut self,
        outcome: CleanupOutcome,
        status: Option<u32>,
    ) -> Result<CleanupOutcome, String> {
        self.record.cleanup = Some(outcome);
        self.record.native_status = status;
        self.save()?;
        Ok(outcome)
    }
}

fn same_key(a: &PublicKey, b: &PublicKey) -> bool {
    a.name == b.name && a.provider == b.provider && a.scope == b.scope
}

pub fn snapshot(path: &Path) -> Result<Snapshot, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "cannot read local recovery receipt")?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read local recovery receipt")?;
    if bytes.len() > 65536 || !bytes.ends_with(b"\n") {
        return Err("incomplete local recovery receipt; retain for review".into());
    }
    let mut latest: Option<Snapshot> = None;
    for line in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let record: Snapshot = serde_json::from_slice(line)
            .map_err(|_| "corrupt local recovery receipt; retain for review")?;
        valid_name(&record.key.name)?;
        if record.schema_version != 1
            || record.owner_sid_reference == [0; 32]
            || record.key.blob.len() > 4096
        {
            return Err("invalid local recovery receipt".into());
        }
        if let Some(previous) = &latest
            && (!same_key(&previous.key, &record.key)
                || previous.owner_sid_reference != record.owner_sid_reference
                || (!previous.key.blob.is_empty() && previous.key.blob != record.key.blob))
        {
            return Err("local recovery identity changed; no deletion".into());
        }
        latest = Some(record);
    }
    latest.ok_or_else(|| "empty local recovery receipt; retain for review".into())
}

pub fn recover(path: &Path) -> Result<CleanupOutcome, String> {
    let identity = super::effective_identity()?;
    if identity.thread_token || identity.service || identity.restricted {
        return Err("unsupported local recovery token".into());
    }
    let receipt = Receipt::open(path, &identity)?;
    if receipt.record.key.scope == super::Scope::Machine {
        return Err(
            "machine recovery requires separately authorized disposable-lab recovery".into(),
        );
    }
    if let Some(outcome) = receipt.record.cleanup
        && outcome != CleanupOutcome::Incomplete
    {
        return Ok(outcome);
    }
    // No deletion by name alone. Unknown public evidence needs manual review, never enumeration.
    if receipt.record.key.blob.is_empty() {
        return Err(
            "cleanup incomplete; public-key evidence unavailable; local recovery retained".into(),
        );
    }
    let mut key = Key::open(&receipt.record.key)?;
    key.inspection(&identity)?;
    key.recovery = Some(receipt);
    key.delete()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FaultStage {
    BeforeFinalize,
    AfterFirstKey,
    AfterFinalize,
    PersistState,
    ProbeOperation,
}

pub(super) fn checkpoint(stage: FaultStage) -> Result<(), String> {
    #[cfg(test)]
    if FAULTS.with(|f| {
        let mut f = f.borrow_mut();
        if let Some(f) = f.as_mut()
            && f.stage == Some(stage)
        {
            f.stage = None;
            true
        } else {
            false
        }
    }) {
        return Err("injected PoC operation failure".into());
    }
    let _ = stage;
    Ok(())
}

pub(super) fn delete_denied() -> bool {
    #[cfg(test)]
    return FAULTS.with(|f| f.borrow().as_ref().is_some_and(|f| f.deny_delete));
    #[cfg(not(test))]
    false
}

#[cfg(test)]
struct Faults {
    directory: PathBuf,
    stage: Option<FaultStage>,
    deny_delete: bool,
}
#[cfg(test)]
thread_local! { static FAULTS: std::cell::RefCell<Option<Faults>> = const { std::cell::RefCell::new(None) }; }

#[cfg(test)]
pub struct FaultGuard {
    pub directory: PathBuf,
    previous: Option<Faults>,
}
#[cfg(test)]
impl FaultGuard {
    pub fn new(stage: Option<FaultStage>, deny_delete: bool) -> Self {
        let directory =
            std::env::temp_dir().join(format!("rolauncher-poc-recovery-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let previous = FAULTS.with(|f| {
            f.replace(Some(Faults {
                directory: directory.clone(),
                stage,
                deny_delete,
            }))
        });
        Self {
            directory,
            previous,
        }
    }
    pub fn allow_cleanup(&self) {
        FAULTS.with(|f| {
            let mut f = f.borrow_mut();
            let f = f.as_mut().unwrap();
            f.deny_delete = false;
            f.stage = None;
        });
    }
    pub fn records(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.directory)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
            .collect()
    }
}
#[cfg(test)]
impl Drop for FaultGuard {
    fn drop(&mut self) {
        let incomplete = self.records().iter().any(|path| {
            snapshot(path).map_or(true, |r| {
                !matches!(
                    r.cleanup,
                    Some(
                        CleanupOutcome::Succeeded
                            | CleanupOutcome::NeverPersisted
                            | CleanupOutcome::Absent
                    )
                )
            })
        });
        FAULTS.with(|f| {
            f.replace(self.previous.take());
        });
        if incomplete && !std::thread::panicking() {
            panic!("fault fixture retained unconfirmed key cleanup; local recovery preserved");
        }
    }
}

pub(super) fn confirmed_disposal(key: &PublicKey, identity: &Identity) -> Result<bool, String> {
    let path = path_for(key)?;
    if !path.exists() {
        return Ok(false);
    }
    let receipt = Receipt::open(&path, identity)?;
    if !same_key(&receipt.record.key, key) || receipt.record.key.blob != key.blob {
        return Err("local recovery key evidence mismatch".into());
    }
    Ok(matches!(
        receipt.record.cleanup,
        Some(CleanupOutcome::Succeeded | CleanupOutcome::Absent)
    ))
}

/// Enumerates only local receipts, never native key containers; output is counts only.
pub fn summary() -> Result<serde_json::Value, String> {
    let directory = directory();
    let mut counts = [0usize; 5];
    if directory.exists() {
        for entry in
            std::fs::read_dir(directory).map_err(|_| "cannot inspect local recovery accounting")?
        {
            let entry = entry.map_err(|_| "cannot inspect local recovery accounting")?;
            if entry.path().extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }
            let index = match snapshot(&entry.path()) {
                Ok(record) => match record.cleanup {
                    Some(CleanupOutcome::Succeeded) => 0,
                    Some(CleanupOutcome::NeverPersisted) => 1,
                    Some(CleanupOutcome::Absent) => 2,
                    None if record.phase == Phase::RetainedForState => 3,
                    _ => 4,
                },
                Err(_) => 4,
            };
            counts[index] += 1;
        }
    }
    Ok(
        serde_json::json!({"cleanup_succeeded": counts[0], "never_persisted": counts[1], "absent": counts[2], "intentionally_retained": counts[3], "incomplete": counts[4], "identifiers_omitted": true}),
    )
}

#[cfg(test)]
pub fn set_delete_denied(denied: bool) {
    FAULTS.with(|f| f.borrow_mut().as_mut().unwrap().deny_delete = denied);
}
