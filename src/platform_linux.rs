//! Linux uses Flatpak's installed Sober, /proc metadata, pidfds and existing logs.
//! Never read /proc/*/mem, environ, or modify any client/runtime files.
use super::{Player, tracker_from_command_line};
use crate::model::ProcessIdentity;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, Payload},
};
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
    sync::{LazyLock, OnceLock},
};
use zeroize::Zeroizing;

const SOBER: &str = "org.vinegarhq.Sober";
const ENVELOPE: &str = "linux-v1:";
const AAD: &[u8] = b"RoLauncher credentials v1";
static DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
// Hold a launch reservation until Flatpak exits, including the startup interval
// before Sober appears in /proc. The engine's launches are serialized as well.
static LAUNCH_PENDING: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static LAUNCH_EXITS: LazyLock<Mutex<HashMap<String, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub struct ExitWatch(String);
impl ExitWatch {
    pub fn new(identity: &ProcessIdentity) -> Option<Self> {
        Some(Self(identity.tracker.clone()))
    }
    pub fn clean_exit(&self) -> Option<bool> {
        LAUNCH_EXITS.lock().ok()?.remove(&self.0)
    }
}
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
    let mut pending = LAUNCH_PENDING.lock().map_err(|_| "Sober launch is busy")?;
    if pending.contains(account_id) {
        return Err("This account's Sober launch is still active".into());
    }
    let directory = DIRECTORY.get().ok_or("Linux platform is not initialized")?;
    let instance = directory.join("sober-instances").join(account_id);
    private_directory(&instance)?;
    let mut child = sober_command(&instance)?
        .args([SOBER, uri])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Cannot launch Sober; install Flatpak and org.vinegarhq.Sober")?;
    pending.insert(account_id.to_owned());
    let tracker = tracker_from_command_line(uri).ok_or("Invalid launch identity")?;
    let account_id = account_id.to_owned();
    // Reap the Flatpak child; closing the supervisor deliberately leaves clients open.
    std::thread::spawn(move || {
        if let Ok(status) = child.wait()
            && let Ok(mut exits) = LAUNCH_EXITS.lock()
        {
            if exits.len() >= 1000 {
                exits.clear();
            }
            exits.insert(tracker, status.success());
        }
        if let Ok(mut pending) = LAUNCH_PENDING.lock() {
            pending.remove(&account_id);
        }
    });
    Ok(())
}
fn sober_command(instance: &Path) -> Result<Command, String> {
    // Isolation is part of every launch, including retries. Preserve the real
    // host XDG paths so Flatpak can locate the installed app/runtime; its inside
    // XDG paths and HOME are private to this account. No wrapper or opt-in is needed.
    let mut command = Command::new("flatpak");
    command
        .env("HOME", instance)
        .env("XDG_DATA_HOME", xdg("XDG_DATA_HOME", ".local/share")?)
        .env("XDG_CONFIG_HOME", xdg("XDG_CONFIG_HOME", ".config")?)
        .env("XDG_CACHE_HOME", xdg("XDG_CACHE_HOME", ".cache")?)
        .arg("run")
        .args(isolation_arguments(instance)?);
    Ok(command)
}
fn isolation_arguments(instance: &Path) -> Result<Vec<String>, String> {
    let instance = instance.to_str().ok_or("Sober account path is not UTF-8")?;
    if !Path::new(instance).is_absolute()
        || instance.chars().any(|c| matches!(c, ':' | '\n' | '\r'))
    {
        return Err("Sober account path is not valid for a Flatpak filesystem grant".into());
    }
    // --sandbox disables Flatpak's shared per-app /tmp and runtime directories.
    // Regrant normal rendering/network access, but keep SysV IPC private. X11
    // clients must fall back from MIT-SHM; Wayland remains available normally.
    // Sandbox mode does not mount the usual app data, so grant only this
    // account's private HOME and explicitly restore its existing XDG paths.
    let data = Path::new(instance).join(".var/app").join(SOBER);
    // These options are compiled into the app; no external script is needed.
    // The developer adapter for old beta binaries uses this same source file.
    let mut args: Vec<String> =
        serde_json::from_str(include_str!("../scripts/sober-isolation/options.json"))
            .map_err(|_| "Invalid built-in Sober sandbox options")?;
    args.push(format!("--filesystem={instance}"));
    // Flatpak reconstructs HOME from the passwd entry; the host environment
    // alone does not isolate Sober's HOME-relative instance lock.
    args.push(format!("--env=HOME={instance}"));
    for (variable, suffix) in [
        ("XDG_DATA_HOME", "data"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_STATE_HOME", ".local/state"),
    ] {
        args.push(format!("--env={variable}={}", data.join(suffix).display()));
    }
    Ok(args)
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
    let mut trackers: HashMap<Sandbox, HashSet<String>> = HashMap::new();
    let mut owners: HashMap<(u64, u64), HashSet<Sandbox>> = HashMap::new();
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
        let sandbox = sandbox(pid)?;
        if let Some(tracker) = process_tracker(pid) {
            trackers.entry(sandbox).or_default().insert(tracker);
        }
        let files = open_logs(pid)?;
        for (_, file) in &files {
            owners.entry(*file).or_default().insert(sandbox);
        }
        // Sober's helper and game processes can inherit the same log descriptor.
        // A descriptor shared inside one kernel PID namespace is not shared
        // between independent Flatpak launches. Keep all candidates so separate
        // logs still cause an ambiguity instead of choosing the newest one.
        let group: &mut Vec<LogFile> = candidates.entry(sandbox).or_default();
        for file in files {
            if !group.iter().any(|existing| existing.1 == file.1) {
                group.push(file);
            }
        }
    }
    for identity in identities {
        if !alive(identity)? {
            continue;
        }
        let sandbox = sandbox(identity.pid)?;
        // Helpers without a URI are allowed, but a different tracked launch in
        // the same sandbox makes attributing its connection signals ambiguous.
        if !trackers
            .get(&sandbox)
            .is_some_and(|group| group.len() == 1 && group.contains(&identity.tracker))
        {
            continue;
        }
        let Some(files) = candidates.get(&sandbox) else {
            continue;
        };
        // Multiple logs or shared ownership never become guessed connection signals.
        if let Some(path) = exclusive_log(sandbox, files, &owners)
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
type Sandbox = (u64, u64);
fn sandbox(pid: u32) -> Result<Sandbox, String> {
    let metadata = std::fs::metadata(format!("/proc/{pid}/ns/pid"))
        .map_err(|_| "Sober sandbox identity unavailable")?;
    Ok((metadata.dev(), metadata.ino()))
}
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
        if !is_sober_log(&path) {
            continue;
        }
        // Read the descriptor itself: Flatpak mount paths and deleted/rotated
        // log names need not resolve through /proc/PID/root from the host.
        let path = fd.path();
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
fn is_sober_log(path: &Path) -> bool {
    path.is_absolute()
        && path.extension().is_some_and(|e| e == "log")
        && (path.components().any(|c| c.as_os_str() == "sober_logs")
            || path.parent().is_some_and(|p| p.ends_with("sober/logs")))
}
fn exclusive_log<'a>(
    sandbox: Sandbox,
    files: &'a [LogFile],
    owners: &HashMap<(u64, u64), HashSet<Sandbox>>,
) -> Option<&'a Path> {
    let [(path, file)] = files else { return None };
    owners
        .get(file)
        .filter(|sandboxes| sandboxes.len() == 1 && sandboxes.contains(&sandbox))?;
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
        let version = Command::new("flatpak")
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        if !version.is_ok_and(|output| {
            output.status.success()
                && supports_input_device(&String::from_utf8_lossy(&output.stdout))
        }) {
            return Err("Flatpak 1.15.6 or newer is required for isolated Sober input access. Update Flatpak, then restart RoLauncher".into());
        }
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
fn supports_input_device(version: &str) -> bool {
    let Some(number) = version.split_whitespace().nth(1) else {
        return false;
    };
    let parts: Vec<_> = number
        .split('.')
        .take(3)
        .map(|v| v.parse::<u32>())
        .collect();
    matches!(parts.as_slice(), [Ok(major), Ok(minor), Ok(patch)] if (*major, *minor, *patch) >= (1, 15, 6))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sandbox_home_is_explicit_and_log_paths_are_restricted() {
        let args = isolation_arguments(Path::new("/tmp/test/sober-instances/123")).unwrap();
        assert!(args.contains(&"--env=HOME=/tmp/test/sober-instances/123".into()));
        assert!(is_sober_log(Path::new(
            "/home/user/sober/sober_logs/latest.log"
        )));
        assert!(is_sober_log(Path::new("/home/user/sober/logs/latest.log")));
        assert!(!is_sober_log(Path::new("/home/user/other/latest.log")));
        assert!(!is_sober_log(Path::new("relative/sober_logs/latest.log")));
    }
    #[test]
    fn input_device_grants_require_a_supported_flatpak_version() {
        for version in ["Flatpak 1.15.6", "Flatpak 1.16.3", "Flatpak 2.0.0"] {
            assert!(supports_input_device(version));
        }
        for version in [
            "Flatpak 1.14.10",
            "Flatpak 1.15.5",
            "unexpected",
            "Flatpak 1.x.0",
        ] {
            assert!(!supports_input_device(version));
        }
    }
    #[test]
    #[ignore = "Requires scripts/test-sober-isolation.py's disposable Flatpak fixture"]
    fn flatpak_runtime_isolation_separates_locks() {
        use std::io::{BufRead, BufReader, Write};
        struct Probe(std::process::Child);
        impl Drop for Probe {
            fn drop(&mut self) {
                if let Some(stdin) = self.0.stdin.as_mut() {
                    let _ = stdin.write_all(b"\n");
                }
                let _ = self.0.wait();
            }
        }
        let root = PathBuf::from(std::env::var_os("ROLAUNCHER_ISOLATION_FIXTURE_ROOT").unwrap());
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let launch = |slot: &str, isolated: bool| {
            let home = root.join(slot);
            private_directory(&home).unwrap();
            let mut command = if isolated {
                sober_command(&home).unwrap()
            } else {
                let mut command = Command::new("flatpak");
                command.arg("run").env("HOME", &home);
                command
            };
            command.args(["--user", "--branch=1"]);
            let mut child = Probe(
                command
                    // The fixture needs no desktop integration. Test only the
                    // actual filesystem, IPC, PID and network namespace behavior.
                    .args(["--no-session-bus", "--no-a11y-bus"])
                    .arg(format!("--env=ROLAUNCHER_PROBE_NAME={nonce}"))
                    .arg("org.rolauncher.IsolationProbe")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap(),
            );
            let stdout = child.0.stdout.take().unwrap();
            let mut fd = libc::pollfd {
                fd: stdout.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            assert!(
                unsafe { libc::poll(&mut fd, 1, 30_000) } > 0,
                "Flatpak probe timed out"
            );
            let mut line = String::new();
            BufReader::new(stdout).read_line(&mut line).unwrap();
            let report: serde_json::Value = serde_json::from_str(&line)
                .expect("Flatpak fixture did not produce a namespace report");
            (child, report)
        };
        let (_normal_a, normal_a) = launch("normal-a", false);
        let (_normal_b, normal_b) = launch("normal-b", false);
        assert_eq!(normal_a["locked"], true);
        assert_eq!(
            normal_b["locked"], false,
            "Control must reproduce a shared lock"
        );
        assert_eq!(normal_a["tmp"], normal_b["tmp"]);
        assert_eq!(normal_a["runtime"], normal_b["runtime"]);
        let (_isolated_a, isolated_a) = launch("isolated-a", true);
        let (_isolated_b, isolated_b) = launch("isolated-b", true);
        for report in [&isolated_a, &isolated_b] {
            assert_eq!(
                report["input_device"], true,
                "Isolated launches must advertise input access to Sober"
            );
            assert_eq!(report["locked"], true);
            for field in ["tmp", "runtime", "ipc", "pid", "net"] {
                assert!(report[field].as_u64().unwrap() > 0);
            }
        }
        // Different tmpfs mounts can reuse an inode number; compare devices too.
        for field in ["tmp", "runtime"] {
            let device = format!("{field}_dev");
            assert_ne!(
                (&isolated_a[&device], &isolated_a[field]),
                (&isolated_b[&device], &isolated_b[field]),
                "Shared {field}"
            );
        }
        for field in ["ipc", "pid"] {
            assert_ne!(isolated_a[field], isolated_b[field], "Shared {field}");
        }
        assert_eq!(
            isolated_a["net"], isolated_b["net"],
            "Networking must stay available"
        );
        println!(
            "Control shared a lock; two experimental sandboxes had independent temp/runtime/IPC/PID resources and retained host networking."
        );
    }
    #[test]
    fn log_ownership_rejects_shared_files_and_multiple_candidates() {
        let a = (PathBuf::from("/proc/10/root/log"), (1, 20));
        let b = (PathBuf::from("/proc/10/root/other-log"), (1, 21));
        let files = [a.clone()];
        let sandbox = (4, 10);
        let other = (4, 11);
        let mut owners = HashMap::from([(a.1, HashSet::from([sandbox]))]);
        // Another helper in the same sandbox inherits the log: still one owner.
        owners.get_mut(&a.1).unwrap().insert(sandbox);
        assert!(exclusive_log(sandbox, &files, &owners).is_some());
        assert!(exclusive_log(other, &files, &owners).is_none());
        assert!(exclusive_log(sandbox, &[a.clone(), b], &owners).is_none());
        owners.get_mut(&a.1).unwrap().insert(other);
        assert!(exclusive_log(sandbox, &files, &owners).is_none());
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
