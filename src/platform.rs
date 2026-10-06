use crate::model::ProcessIdentity;
#[cfg(not(target_os = "linux"))]
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct Player {
    pub pid: u32,
    pub creation_time: String,
    pub tracker: String,
}
impl Player {
    pub fn identity(&self, generation: Uuid) -> ProcessIdentity {
        ProcessIdentity {
            pid: self.pid,
            creation_time: self.creation_time.clone(),
            tracker: self.tracker.clone(),
            generation,
        }
    }
}

/// Windows uses the installed protocol; Linux isolates Sober by saved account.
#[cfg(not(target_os = "linux"))]
pub fn launch_for(uri: &str, _account_id: &str) -> Result<(), String> {
    launch(uri)
}

pub fn tracker_from_command_line(cmd: &str) -> Option<String> {
    let args: Vec<_> = cmd.split_whitespace().collect();
    let mut trackers = Vec::new();
    for pair in args.windows(2) {
        if pair[0].trim_matches('"') == "-b" {
            trackers.push(pair[1].trim_matches('"'));
        }
    }
    // Current clients retain the protocol URI as one argument instead of using -b.
    // Only examine that argument's outer fields, never a nested URL or arbitrary text.
    for arg in &args {
        let arg = arg.trim_matches('"');
        if let Some(protocol) = arg.strip_prefix("roblox-player:") {
            for field in protocol.split('+') {
                if let Some((key, value)) = field.split_once(':')
                    && key.eq_ignore_ascii_case("browsertrackerid")
                {
                    trackers.push(value);
                }
            }
        }
    }
    if trackers.len() != 1 {
        return None;
    }
    let tracker = trackers[0];
    (tracker.len() >= 6 && tracker.len() <= 20 && tracker.bytes().all(|c| c.is_ascii_digit()))
        .then(|| tracker.to_owned())
}

#[cfg(windows)]
mod imp {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use serde::Deserialize;
    use windows_sys::Win32::{
        Foundation::*,
        Security::Cryptography::*,
        Storage::FileSystem::*,
        System::{RestartManager::*, Threading::*},
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::*},
    };
    use zeroize::Zeroizing;

    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn last_error(label: &str) -> String {
        format!("{label} (Windows error {})", unsafe { GetLastError() })
    }
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    fn process_handle(pid: u32, access: u32) -> Result<Handle, String> {
        let h = unsafe { OpenProcess(access, 0, pid) };
        if h.is_null() {
            Err(last_error("Process access unavailable"))
        } else {
            Ok(Handle(h))
        }
    }
    fn process_time(handle: HANDLE) -> Result<String, String> {
        let mut created: FILETIME = unsafe { std::mem::zeroed() };
        let mut exited = created;
        let mut kernel = created;
        let mut user = created;
        if unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) }
            == 0
        {
            return Err(last_error("Process creation time unavailable"));
        }
        Ok((((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64).to_string())
    }
    fn valid_player_image(handle: HANDLE) -> bool {
        let mut path = vec![0u16; 32768];
        let mut len = path.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut len) } == 0 {
            return false;
        }
        let s = String::from_utf16_lossy(&path[..len as usize]).to_lowercase();
        s.ends_with("\\robloxplayerbeta.exe") && s.contains("\\roblox\\versions\\")
    }
    pub fn players() -> Result<Vec<Player>, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Row {
            process_id: u32,
            command_line: Option<String>,
        }
        let connection =
            wmi::WMIConnection::new().map_err(|_| "Windows process metadata unavailable")?;
        let rows: Vec<Row> = connection.raw_query("SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name = 'RobloxPlayerBeta.exe'").map_err(|_| "Windows process query failed")?;
        let mut out = Vec::new();
        for row in rows {
            let Some(tracker) = row
                .command_line
                .as_deref()
                .and_then(tracker_from_command_line)
            else {
                continue;
            };
            let Ok(h) = process_handle(
                row.process_id,
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
            ) else {
                continue;
            };
            if !valid_player_image(h.0) {
                continue;
            }
            if let Ok(creation_time) = process_time(h.0) {
                out.push(Player {
                    pid: row.process_id,
                    creation_time,
                    tracker,
                });
            }
        }
        Ok(out)
    }
    pub fn alive(identity: &ProcessIdentity) -> Result<bool, String> {
        let h = match process_handle(
            identity.pid,
            PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
        ) {
            Ok(h) => h,
            Err(e) => {
                if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
                    return Ok(false);
                }
                return Err(e);
            }
        };
        if process_time(h.0)? != identity.creation_time || !valid_player_image(h.0) {
            return Ok(false);
        }
        match unsafe { WaitForSingleObject(h.0, 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 => Ok(false),
            _ => Err(last_error("Process exit wait failed")),
        }
    }
    pub fn close(identity: &ProcessIdentity, force: bool) -> Result<(), String> {
        let h = match process_handle(
            identity.pid,
            PROCESS_QUERY_LIMITED_INFORMATION
                | SYNCHRONIZE
                | if force { PROCESS_TERMINATE } else { 0 },
        ) {
            Ok(h) => h,
            Err(_) if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER => return Ok(()),
            Err(error) => return Err(error),
        };
        if process_time(h.0)? != identity.creation_time || !valid_player_image(h.0) {
            return Err("Instance identity changed; refusing to close it".into());
        }
        if force {
            terminate_verified_handle(h.0)?;
        } else {
            unsafe extern "system" fn callback(hwnd: HWND, pid: LPARAM) -> i32 {
                let mut owner = 0;
                unsafe {
                    GetWindowThreadProcessId(hwnd, &mut owner);
                }
                if owner == pid as u32 {
                    unsafe {
                        PostMessageW(hwnd, WM_CLOSE, 0, 0);
                    }
                }
                1
            }
            unsafe {
                EnumWindows(Some(callback), identity.pid as isize);
            }
        }
        Ok(())
    }
    fn terminate_verified_handle(handle: HANDLE) -> Result<(), String> {
        // Exit can race discovery or WM_CLOSE. A signaled process is already closed.
        match unsafe { WaitForSingleObject(handle, 0) } {
            WAIT_OBJECT_0 => return Ok(()),
            WAIT_TIMEOUT => {}
            _ => return Err(last_error("Process exit wait failed")),
        }
        if unsafe { TerminateProcess(handle, 0) } == 0 {
            let error = unsafe { GetLastError() };
            // TerminateProcess returns ERROR_ACCESS_DENIED for already-terminated processes.
            if unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0 {
                return Ok(());
            }
            return Err(format!(
                "Unable to close managed instance (Windows error {error})"
            ));
        }
        Ok(())
    }
    pub fn launch(uri: &str) -> Result<(), String> {
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide("open").as_ptr(),
                wide(uri).as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize <= 32 {
            Err("Roblox launch protocol unavailable; install the desktop Roblox client".into())
        } else {
            Ok(())
        }
    }
    pub fn protect(secret: &str) -> Result<String, String> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: secret.len() as u32,
            pbData: secret.as_ptr() as *mut u8,
        };
        let mut output = unsafe { std::mem::zeroed() };
        if unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } == 0
        {
            return Err(last_error("Unable to encrypt credentials"));
        }
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
        let encrypted = STANDARD.encode(bytes);
        unsafe {
            LocalFree(output.pbData as _);
        }
        Ok(encrypted)
    }
    pub fn unprotect(encrypted: &str) -> Result<Zeroizing<String>, String> {
        let bytes = STANDARD
            .decode(encrypted)
            .map_err(|_| "Invalid encrypted session")?;
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut output = unsafe { std::mem::zeroed() };
        if unsafe {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } == 0
        {
            return Err(
                "Credentials cannot be decrypted by this Windows user; sign in again".into(),
            );
        }
        let bytes =
            unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
        let result = String::from_utf8(bytes.to_vec())
            .map(Zeroizing::new)
            .map_err(|_| "Invalid decrypted session".into());
        use zeroize::Zeroize;
        bytes.zeroize();
        unsafe {
            LocalFree(output.pbData as _);
        }
        result
    }
    pub fn replace_file(
        temp: &std::path::Path,
        destination: &std::path::Path,
    ) -> Result<(), String> {
        if unsafe {
            MoveFileExW(
                wide(&temp.to_string_lossy()).as_ptr(),
                wide(&destination.to_string_lossy()).as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            Err(last_error("Unable to save account database"))
        } else {
            Ok(())
        }
    }
    pub fn owned_logs(
        identities: &[ProcessIdentity],
    ) -> Result<std::collections::HashMap<(u32, String), PathBuf>, String> {
        let Some(local) = std::env::var_os("LOCALAPPDATA") else {
            return Err("LOCALAPPDATA unavailable".into());
        };
        let root = PathBuf::from(local).join("Roblox").join("logs");
        let mut candidates: Vec<_> = std::fs::read_dir(root)
            .map_err(|_| "Roblox logs are unavailable")?
            .filter_map(Result::ok)
            .filter(|x| {
                x.file_name().to_string_lossy().contains("_Player_")
                    && x.path().extension().is_some_and(|e| e == "log")
            })
            .collect();
        candidates.sort_by_key(|x| std::cmp::Reverse(x.metadata().and_then(|m| m.modified()).ok()));
        let mut matched: std::collections::HashMap<(u32, String), Vec<PathBuf>> =
            std::collections::HashMap::new();
        for candidate in candidates.into_iter().take(100) {
            let mut session = 0;
            let mut key = [0u16; 33];
            if unsafe { RmStartSession(&mut session, 0, key.as_mut_ptr()) } != 0 {
                return Err("Windows log-ownership query unavailable".into());
            }
            struct Session(u32);
            impl Drop for Session {
                fn drop(&mut self) {
                    unsafe {
                        RmEndSession(self.0);
                    }
                }
            }
            let _session = Session(session);
            let path = wide(&candidate.path().to_string_lossy());
            let paths = [path.as_ptr()];
            if unsafe {
                RmRegisterResources(
                    session,
                    1,
                    paths.as_ptr(),
                    0,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                )
            } != 0
            {
                continue;
            }
            let mut needed = 0;
            let mut count = 0;
            let mut reason = 0;
            let rc = unsafe {
                RmGetList(
                    session,
                    &mut needed,
                    &mut count,
                    std::ptr::null_mut(),
                    &mut reason,
                )
            };
            if rc != ERROR_MORE_DATA || needed == 0 || needed > 1024 {
                continue;
            }
            let mut list: Vec<RM_PROCESS_INFO> =
                vec![unsafe { std::mem::zeroed() }; needed as usize];
            count = needed;
            if unsafe {
                RmGetList(
                    session,
                    &mut needed,
                    &mut count,
                    list.as_mut_ptr(),
                    &mut reason,
                )
            } != 0
            {
                continue;
            }
            let owners = &list[..count as usize];
            if owners.len() == 1 {
                let owner = &owners[0].Process;
                let creation_time = (((owner.ProcessStartTime.dwHighDateTime as u64) << 32)
                    | owner.ProcessStartTime.dwLowDateTime as u64)
                    .to_string();
                if identities
                    .iter()
                    .any(|id| id.pid == owner.dwProcessId && id.creation_time == creation_time)
                {
                    matched
                        .entry((owner.dwProcessId, creation_time))
                        .or_default()
                        .push(candidate.path());
                }
            }
        }
        Ok(matched
            .into_iter()
            .filter_map(|(key, mut paths)| {
                if paths.len() == 1 {
                    Some((key, paths.pop().unwrap()))
                } else {
                    None
                }
            })
            .collect())
    }
    pub fn owned_log(identity: &ProcessIdentity) -> Result<Option<PathBuf>, String> {
        owned_logs(std::slice::from_ref(identity))
            .map(|mut m| m.remove(&(identity.pid, identity.creation_time.clone())))
    }
    /// Own only the application's coordination mutex. Never open/close Roblox handles.
    pub struct InstanceGuard(Vec<Handle>);
    impl InstanceGuard {
        pub fn acquire() -> Result<Self, String> {
            let mut guard = Self(Vec::new());
            // Keep the legacy lock too: both brands can supervise the same saved accounts.
            for name in ["Local\\RoLauncherSupervisor", "Local\\RbxToolsSupervisor"] {
                let h = unsafe { CreateMutexW(std::ptr::null(), 1, wide(name).as_ptr()) };
                if h.is_null() {
                    return Err(last_error("Unable to create application mutex"));
                }
                let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
                let handle = Handle(h);
                if existing {
                    return Err("RoLauncher or an older installation is already running for this Windows session".into());
                }
                guard.0.push(handle);
            }
            Ok(guard)
        }
    }
    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            for handle in &self.0 {
                unsafe {
                    ReleaseMutex(handle.0);
                }
            }
        }
    }
    /// Must be acquired and dropped on the native UI thread that owns the mutex.
    pub struct MultiInstanceGuard(Handle);
    impl MultiInstanceGuard {
        pub fn acquire() -> Result<Self, String> {
            let h = unsafe {
                CreateMutexW(std::ptr::null(), 1, wide("ROBLOX_singletonMutex").as_ptr())
            };
            if h.is_null() {
                return Err(last_error("Multi-instance coordination unavailable"));
            }
            let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            let handle = Handle(h);
            if existing {
                let wait = unsafe { WaitForSingleObject(h, 0) };
                if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
                    return Err("Roblox singleton is already owned. Close clients yourself and restart the tool to initialize coordination".into());
                }
            }
            Ok(Self(handle))
        }
    }
    impl Drop for MultiInstanceGuard {
        fn drop(&mut self) {
            unsafe {
                ReleaseMutex(self.0.0);
            }
        }
    }
    #[cfg(test)]
    mod close_tests {
        use super::*;
        use std::os::windows::io::AsRawHandle;
        #[test]
        fn an_exited_process_is_successfully_closed_even_when_terminate_returns_five() {
            let mut child = std::process::Command::new("cmd.exe")
                .args(["/C", "exit", "0"])
                .spawn()
                .unwrap();
            child.wait().unwrap();
            let handle = child.as_raw_handle() as HANDLE;
            assert_eq!(unsafe { TerminateProcess(handle, 0) }, 0);
            assert_eq!(unsafe { GetLastError() }, ERROR_ACCESS_DENIED);
            assert!(terminate_verified_handle(handle).is_ok());
        }
    }
}
#[cfg(windows)]
pub use imp::*;

#[cfg(target_os = "linux")]
#[path = "platform_linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(not(any(windows, target_os = "linux")))]
mod imp {
    use super::*;
    pub fn players() -> Result<Vec<Player>, String> {
        Err("Windows only".into())
    }
    pub fn alive(_: &ProcessIdentity) -> Result<bool, String> {
        Err("Windows only".into())
    }
    pub fn close(_: &ProcessIdentity, _: bool) -> Result<(), String> {
        Err("Windows only".into())
    }
    pub fn launch(_: &str) -> Result<(), String> {
        Err("Windows only".into())
    }
    pub fn owned_log(_: &ProcessIdentity) -> Result<Option<PathBuf>, String> {
        Err("Windows only".into())
    }
    pub fn owned_logs(
        _: &[ProcessIdentity],
    ) -> Result<std::collections::HashMap<(u32, String), PathBuf>, String> {
        Err("Windows only".into())
    }
    pub fn protect(_: &str) -> Result<String, String> {
        Err("Windows DPAPI required".into())
    }
    pub fn unprotect(_: &str) -> Result<zeroize::Zeroizing<String>, String> {
        Err("Windows DPAPI required".into())
    }
    pub fn replace_file(temp: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
        std::fs::rename(temp, dest).map_err(|_| "Unable to save database".into())
    }
}
#[cfg(not(any(windows, target_os = "linux")))]
pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn match_tracker_argument_only() {
        assert_eq!(
            tracker_from_command_line("RobloxPlayerBeta.exe -b 123456789 -t SECRET"),
            Some("123456789".into())
        );
        assert_eq!(
            tracker_from_command_line("auxiliary.exe --other 123456789"),
            None
        );
        assert_eq!(tracker_from_command_line("player.exe -b 123oops"), None);
    }
    #[test]
    fn current_protocol_tracker_is_matched_without_using_ticket_or_nested_url() {
        assert_eq!(
            tracker_from_command_line(
                "\"C:\\Path With Spaces\\RobloxPlayerBeta.exe\" \"roblox-player:1+launchmode:play+gameinfo:SECRET+placelauncherurl:https%3A%2F%2Fexample.invalid+browsertrackerid:123456789+channel:\""
            ),
            Some("123456789".into())
        );
        for cmd in [
            "player.exe roblox-player:1+browsertrackerid:123oops",
            "player.exe roblox-player:1+browsertrackerid:123456789+browsertrackerid:987654321",
            "player.exe -b 123456789 roblox-player:1+browsertrackerid:123456789",
            "player.exe https://example.invalid/+browsertrackerid:123456789",
            "player.exe roblox-player:1+placelauncherurl:https%3A%2F%2Fexample.invalid%2Bbrowsertrackerid%3A123456789",
            "player.exe -b 123456789 -b 987654321",
        ] {
            assert!(tracker_from_command_line(cmd).is_none());
        }
    }
    #[cfg(windows)]
    #[test]
    fn dpapi_round_trip_and_corruption() {
        let encrypted = protect("sample-session").unwrap();
        assert!(!encrypted.contains("sample-session"));
        assert_eq!(&*unprotect(&encrypted).unwrap(), "sample-session");
        assert!(unprotect("bad-data").is_err());
    }
}
