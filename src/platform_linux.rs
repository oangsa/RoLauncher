//! Linux uses Flatpak's installed Sober, /proc metadata, pidfds and existing logs.
//! Never read /proc/*/mem, environ, or modify any client/runtime files.
use super::{Player, tracker_from_command_line};
use crate::model::ProcessIdentity;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Payload},
};
#[cfg(not(test))]
use std::sync::Mutex;
use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::OnceLock,
};
use zeroize::Zeroizing;

const SOBER: &str = "org.vinegarhq.Sober";
const ENVELOPE: &str = "linux-v1:";
const AAD: &[u8] = b"RoLauncher credentials v1";
static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
#[cfg(not(test))]
static KEY: Mutex<Option<Zeroizing<[u8; 32]>>> = Mutex::new(None);

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| "HOME must be an absolute directory".into())
}
fn xdg(variable: &str, fallback: &str) -> Result<PathBuf, String> {
    Ok(std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or(home()?.join(fallback)))
}
pub fn data_directory() -> Result<PathBuf, String> {
    Ok(xdg("XDG_DATA_HOME", ".local/share")?.join("RoLauncher"))
}
pub fn private_directory(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|_| "Unable to create private application directory")?;
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "Cannot verify application directory")?;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::getuid() } {
        return Err(
            "Application directory must be owned by the current user and not a symlink".into(),
        );
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| "Unable to restrict application directory permissions".into())
}
pub fn initialize(directory: &Path) -> Result<(), String> {
    let directory = if directory.is_absolute() {
        directory.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|_| "Cannot locate working directory")?
            .join(directory)
    };
    private_directory(&directory)?;
    DIRECTORY
        .set(directory)
        .map_err(|_| "Linux platform was already initialized".into())
}

/// The boot ID prevents a persisted PID/start time matching a process after reboot.
fn stat_identity(stat: &str, boot: &str) -> Option<String> {
    let (_, fields) = stat.rsplit_once(") ")?;
    let start = fields.split_whitespace().nth(19)?;
    if !start.bytes().all(|c| c.is_ascii_digit()) || start.is_empty() {
        return None;
    }
    Some(format!("{}:{start}", boot.trim()))
}
fn creation_time(pid: u32) -> Result<Option<String>, String> {
    let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Linux process metadata is unavailable".into()),
    };
    let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|_| "Linux boot identity unavailable")?;
    stat_identity(&stat, &boot)
        .map(Some)
        .ok_or_else(|| "Invalid Linux process metadata".into())
}
fn valid_sober(pid: u32) -> bool {
    let root = PathBuf::from(format!("/proc/{pid}"));
    if !root
        .metadata()
        .is_ok_and(|m| m.uid() == unsafe { libc::getuid() })
    {
        return false;
    }
    let Ok(exe) = std::fs::read_link(root.join("exe")) else {
        return false;
    };
    if exe.file_name().is_none_or(|name| name != "sober") {
        return false;
    }
    // Verify the sandbox identity as well as the executable name.
    std::fs::read_to_string(root.join("root/.flatpak-info")).is_ok_and(|info| {
        let mut application = false;
        for line in info.lines() {
            if line.starts_with('[') {
                application = line == "[Application]";
            }
            if application && line == format!("name={SOBER}") {
                return true;
            }
        }
        false
    })
}
fn process_tracker(pid: u32) -> Option<String> {
    let cmd = Zeroizing::new(std::fs::read(format!("/proc/{pid}/cmdline")).ok()?);
    let cmd = Zeroizing::new(String::from_utf8(cmd.to_vec()).ok()?.replace('\0', " "));
    tracker_from_command_line(&cmd)
}
pub fn players() -> Result<Vec<Player>, String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir("/proc")
        .map_err(|_| "Linux process metadata unavailable")?
        .flatten()
    {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if !valid_sober(pid) {
            continue;
        }
        let Some(tracker) = process_tracker(pid) else {
            continue;
        };
        if let Some(creation_time) = creation_time(pid)? {
            out.push(Player {
                pid,
                creation_time,
                tracker,
            });
        }
    }
    Ok(out)
}
pub fn alive(identity: &ProcessIdentity) -> Result<bool, String> {
    Ok(
        creation_time(identity.pid)?.as_ref() == Some(&identity.creation_time)
            && valid_sober(identity.pid)
            && process_tracker(identity.pid).as_ref() == Some(&identity.tracker),
    )
}
pub fn close(identity: &ProcessIdentity, force: bool) -> Result<(), String> {
    // Pin the kernel process object before checking metadata, eliminating PID reuse races.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, identity.pid, 0) } as i32;
    if fd < 0 {
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err("Cannot open a Linux process exit handle".into());
    }
    use std::os::fd::FromRawFd;
    let handle = unsafe { File::from_raw_fd(fd) };
    if !alive(identity)? {
        if creation_time(identity.pid)?.is_none() {
            return Ok(());
        }
        return Err("Instance identity changed; refusing to close it".into());
    }
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            handle.as_raw_fd(),
            if force { libc::SIGKILL } else { libc::SIGTERM },
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    if result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err("Unable to close the verified Sober instance".into())
    }
}
pub fn launch_for(uri: &str, account_id: &str) -> Result<(), String> {
    if account_id.is_empty()
        || !account_id.bytes().all(|c| c.is_ascii_digit())
        || tracker_from_command_line(uri).is_none()
    {
        return Err("Invalid launch identity".into());
    }
    let directory = DIRECTORY.get().ok_or("Linux platform is not initialized")?;
    let instance = directory.join("sober-instances").join(account_id);
    private_directory(&instance)?;
    let mut child = Command::new("flatpak")
        // Isolate Sober's own lock and state through its normal Flatpak home, without
        // changing Roblox binaries, client files, or another installation's settings.
        .env("HOME", instance)
        .env("XDG_DATA_HOME", xdg("XDG_DATA_HOME", ".local/share")?)
        .env("XDG_CONFIG_HOME", xdg("XDG_CONFIG_HOME", ".config")?)
        .env("XDG_CACHE_HOME", xdg("XDG_CACHE_HOME", ".cache")?)
        .args(["run", SOBER, uri])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Cannot launch Sober; install Flatpak and org.vinegarhq.Sober")?;
    // Reap the Flatpak child; closing the supervisor deliberately leaves clients open.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
pub fn owned_log(identity: &ProcessIdentity) -> Result<Option<PathBuf>, String> {
    owned_logs(std::slice::from_ref(identity))
        .map(|mut logs| logs.remove(&(identity.pid, identity.creation_time.clone())))
}
pub fn owned_logs(
    identities: &[ProcessIdentity],
) -> Result<HashMap<(u32, String), PathBuf>, String> {
    let mut logs = HashMap::new();
    let mut candidates = HashMap::new();
    let mut owners: HashMap<(u64, u64), HashSet<u32>> = HashMap::new();
    // Include unmanaged Sober clients: a shared file is ambiguous even when only
    // one of its owners belongs to RoLauncher. Compare inodes across namespaces.
    for entry in std::fs::read_dir("/proc")
        .map_err(|_| "Linux process metadata unavailable")?
        .flatten()
    {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse().ok()) else {
            continue;
        };
        if !valid_sober(pid) {
            continue;
        }
        let files = open_logs(pid)?;
        for (_, file) in &files {
            owners.entry(*file).or_default().insert(pid);
        }
        candidates.insert(pid, files);
    }
    for identity in identities {
        if !alive(identity)? {
            continue;
        }
        let Some(files) = candidates.get(&identity.pid) else {
            continue;
        };
        // Multiple logs or shared ownership never become guessed connection signals.
        if let Some(path) = exclusive_log(identity.pid, files, &owners)
            && alive(identity)?
        {
            logs.insert(
                (identity.pid, identity.creation_time.clone()),
                path.to_owned(),
            );
        }
    }
    Ok(logs)
}

type LogFile = (PathBuf, (u64, u64));
fn open_logs(pid: u32) -> Result<Vec<LogFile>, String> {
    let root = PathBuf::from(format!("/proc/{pid}"));
    let entries = match std::fs::read_dir(root.join("fd")) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Sober log ownership unavailable".into()),
    };
    let mut files = Vec::new();
    for fd in entries.flatten() {
        let Ok(path) = std::fs::read_link(fd.path()) else {
            continue;
        };
        if !path.is_absolute()
            || path.extension().is_none_or(|e| e != "log")
            || !path.components().any(|c| c.as_os_str() == "sober_logs")
        {
            continue;
        }
        let path = root.join("root").join(path.strip_prefix("/").unwrap());
        if let Ok(metadata) = path.metadata()
            && metadata.is_file()
        {
            let file = (metadata.dev(), metadata.ino());
            if !files.iter().any(|(_, existing)| *existing == file) {
                files.push((path, file));
            }
        }
    }
    Ok(files)
}
fn exclusive_log<'a>(
    pid: u32,
    files: &'a [LogFile],
    owners: &HashMap<(u64, u64), HashSet<u32>>,
) -> Option<&'a Path> {
    let [(path, file)] = files else { return None };
    owners
        .get(file)
        .filter(|pids| pids.len() == 1 && pids.contains(&pid))?;
    Some(path)
}

#[cfg(test)]
fn key(_create: bool) -> Result<Zeroizing<[u8; 32]>, String> {
    // Unit tests exercise real AEAD without touching the developer's desktop keyring.
    static TEST_KEY: OnceLock<[u8; 32]> = OnceLock::new();
    Ok(Zeroizing::new(*TEST_KEY.get_or_init(rand::random)))
}
#[cfg(not(test))]
fn key(create: bool) -> Result<Zeroizing<[u8; 32]>, String> {
    let mut cached = KEY.lock().map_err(|_| "Credential store is busy")?;
    if let Some(key) = &*cached {
        return Ok(Zeroizing::new(**key));
    }
    let entry = keyring::Entry::new("RoLauncher", "vault-v1")
        .map_err(|_| "Secret Service is unavailable; unlock GNOME Keyring or KWallet")?;
    let key = match entry.get_password() {
        Ok(value) => {
            let value = Zeroizing::new(value);
            let bytes = Zeroizing::new(
                STANDARD
                    .decode(value.as_bytes())
                    .map_err(|_| "Invalid vault key")?,
            );
            let key: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| "Invalid vault key")?;
            Zeroizing::new(key)
        }
        Err(keyring::Error::NoEntry) if create => {
            let key = Zeroizing::new(rand::random::<[u8; 32]>());
            let encoded = Zeroizing::new(STANDARD.encode(key.as_slice()));
            entry
                .set_password(&encoded)
                .map_err(|_| "Cannot save vault key; unlock GNOME Keyring or KWallet")?;
            key
        }
        Err(_) => {
            return Err(
                "Credentials cannot be unlocked; unlock this user's GNOME Keyring or KWallet"
                    .into(),
            );
        }
    };
    *cached = Some(Zeroizing::new(*key));
    Ok(key)
}
fn encrypt(secret: &str, key: &[u8; 32]) -> Result<String, String> {
    let nonce = rand::random::<[u8; 12]>();
    let cipher = ChaCha20Poly1305::new(key.into());
    let encrypted = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: secret.as_bytes(),
                aad: AAD,
            },
        )
        .map_err(|_| "Unable to encrypt credentials")?;
    let mut envelope = nonce.to_vec();
    envelope.extend(encrypted);
    Ok(format!("{ENVELOPE}{}", STANDARD.encode(envelope)))
}
fn decrypt(encrypted: &str, key: &[u8; 32]) -> Result<Zeroizing<String>, String> {
    let encoded = encrypted
        .strip_prefix(ENVELOPE)
        .ok_or("These credentials belong to another operating system; sign in again")?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "Invalid encrypted session")?;
    if bytes.len() < 28 {
        return Err("Invalid encrypted session".into());
    }
    let cipher = ChaCha20Poly1305::new(key.into());
    let decrypted = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&bytes[..12]),
                Payload {
                    msg: &bytes[12..],
                    aad: AAD,
                },
            )
            .map_err(|_| "Credentials could not be authenticated")?,
    );
    String::from_utf8(decrypted.to_vec())
        .map(Zeroizing::new)
        .map_err(|_| "Invalid decrypted session".into())
}
pub fn protect(secret: &str) -> Result<String, String> {
    encrypt(secret, &*key(true)?)
}
pub fn unprotect(encrypted: &str) -> Result<Zeroizing<String>, String> {
    decrypt(encrypted, &*key(false)?)
}
pub fn replace_file(temp: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(temp, destination).map_err(|_| "Unable to save account database")?;
    File::open(destination.parent().ok_or("Missing database directory")?)
        .and_then(|f| f.sync_all())
        .map_err(|_| "Unable to flush database directory".into())
}
pub struct InstanceGuard(File);
impl InstanceGuard {
    pub fn acquire() -> Result<Self, String> {
        let directory = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or(
                std::env::temp_dir().join(format!("rolauncher-{}", unsafe { libc::getuid() })),
            );
        private_directory(&directory)?;
        let directory = directory.join("RoLauncher");
        private_directory(&directory)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(directory.join("supervisor.lock"))
            .map_err(|_| "Cannot open supervisor lock")?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("RoLauncher is already running for this Linux user".into());
        }
        Ok(Self(lock))
    }
}
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
pub struct MultiInstanceGuard;
impl MultiInstanceGuard {
    pub fn acquire() -> Result<Self, String> {
        let installed = Command::new("flatpak")
            .args(["info", "--show-ref", SOBER])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if installed.is_ok_and(|s| s.success()) {
            Ok(Self)
        } else {
            Err("Sober unavailable. Install Flatpak and org.vinegarhq.Sober, then restart RoLauncher".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_ownership_rejects_shared_files_and_multiple_candidates() {
        let a = (PathBuf::from("/proc/10/root/log"), (1, 20));
        let b = (PathBuf::from("/proc/10/root/other-log"), (1, 21));
        let files = [a.clone()];
        let mut owners = HashMap::from([(a.1, HashSet::from([10]))]);
        assert!(exclusive_log(10, &files, &owners).is_some());
        assert!(exclusive_log(11, &files, &owners).is_none());
        assert!(exclusive_log(10, &[a.clone(), b], &owners).is_none());
        owners.get_mut(&a.1).unwrap().insert(11);
        assert!(exclusive_log(10, &files, &owners).is_none());
    }
    #[test]
    fn ordinary_processes_and_stale_birth_identities_cannot_be_closed() {
        let identity = ProcessIdentity {
            pid: std::process::id(),
            creation_time: creation_time(std::process::id()).unwrap().unwrap(),
            tracker: "fixture".into(),
            generation: uuid::Uuid::nil(),
        };
        assert!(!alive(&identity).unwrap());
        assert!(close(&identity, false).is_err());
        let stale = ProcessIdentity {
            creation_time: "previous-boot:0".into(),
            ..identity
        };
        assert!(close(&stale, true).is_err());
    }
    #[test]
    fn linux_vault_authenticates_and_never_accepts_windows_or_corrupt_envelopes() {
        let key = [7; 32];
        let encrypted = encrypt("sample-session", &key).unwrap();
        assert!(!encrypted.contains("sample-session"));
        assert_eq!(&*decrypt(&encrypted, &key).unwrap(), "sample-session");
        assert_ne!(encrypted, encrypt("sample-session", &key).unwrap());
        assert!(decrypt(&encrypted, &[8; 32]).is_err());
        assert!(decrypt("windows-dpapi", &key).is_err());
        assert!(decrypt("linux-v1:AAAA", &key).is_err());
    }
    #[test]
    fn birth_identity_handles_spaces_and_parentheses_and_distinguishes_reboots() {
        let stat = format!("123 (sober (main)) S {} 42 0", "0 ".repeat(18));
        assert_eq!(
            stat_identity(&stat, "boot-a\n").as_deref(),
            Some("boot-a:42")
        );
        assert_ne!(
            stat_identity(&stat, "boot-a"),
            stat_identity(&stat, "boot-b")
        );
        assert!(stat_identity("truncated", "boot").is_none());
    }
}
