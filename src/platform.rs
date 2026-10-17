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
        System::{
            Diagnostics::ToolHelp::*, RemoteDesktop::ProcessIdToSessionId, RestartManager::*,
            Threading::*,
        },
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
    pub struct ExitWatch(Handle);
    // A Windows process handle may be waited/query-checked from any thread.
    unsafe impl Send for ExitWatch {}
    impl ExitWatch {
        pub fn new(identity: &ProcessIdentity) -> Option<Self> {
            let handle = process_handle(
                identity.pid,
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
            )
            .ok()?;
            (process_time(handle.0).ok()? == identity.creation_time && valid_player_image(handle.0))
                .then_some(Self(handle))
        }
        pub fn clean_exit(&self) -> Option<bool> {
            let mut code = 0;
            (unsafe { WaitForSingleObject(self.0.0, 0) } == WAIT_OBJECT_0
                && unsafe { GetExitCodeProcess(self.0.0, &mut code) } != 0)
                .then_some(code == 0)
        }
        pub fn alive(&self, _identity: &ProcessIdentity) -> Result<bool, String> {
            match unsafe { WaitForSingleObject(self.0.0, 0) } {
                WAIT_TIMEOUT => Ok(true),
                WAIT_OBJECT_0 => Ok(false),
                _ => Err(last_error("Process exit wait failed")),
            }
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
    fn visible_processes() -> Result<std::collections::HashSet<u32>, String> {
        unsafe extern "system" fn collect_visible(hwnd: HWND, context: LPARAM) -> i32 {
            if unsafe { IsWindowVisible(hwnd) } != 0 {
                let mut pid = 0;
                unsafe {
                    GetWindowThreadProcessId(hwnd, &mut pid);
                }
                unsafe { &mut *(context as *mut std::collections::HashSet<u32>) }.insert(pid);
            }
            1
        }
        let mut visible = std::collections::HashSet::<u32>::new();
        if unsafe { EnumWindows(Some(collect_visible), &mut visible as *mut _ as isize) } == 0 {
            return Err(last_error("Visible client query unavailable"));
        }
        Ok(visible)
    }
    pub fn startup_launch_conflict() -> Result<bool, String> {
        startup_launch_conflict_named("ROBLOX_singletonMutex")
    }
    fn startup_launch_conflict_named(name: &str) -> Result<bool, String> {
        let handle = Handle(unsafe { CreateMutexW(std::ptr::null(), 0, wide(name).as_ptr()) });
        if handle.0.is_null() {
            return Err(last_error(
                "Cannot inspect startup conflict; quit conflicting Roblox clients and restart",
            ));
        }
        match unsafe { WaitForSingleObject(handle.0, 0) } {
            WAIT_TIMEOUT => Ok(true),
            WAIT_OBJECT_0 | WAIT_ABANDONED => {
                unsafe {
                    ReleaseMutex(handle.0);
                }
                Ok(false)
            }
            _ => Err(last_error(
                "Cannot inspect startup conflict; quit conflicting Roblox clients and restart",
            )),
        }
    }
    /// External cleanup requires separate persisted consent and an observed launch conflict.
    pub fn close_startup_background_players(
        protected: &[ProcessIdentity],
        authorized: bool,
        conflict: bool,
    ) -> Result<usize, String> {
        if !conflict {
            return Ok(0);
        }
        if !authorized {
            return Err("Roblox startup conflict. Quit conflicting background clients, or enable 'Automatically close conflicting background Roblox instances' in Settings and restart RoLauncher. No external clients closed.".into());
        }
        let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable")?;
        close_startup_background_players_at(
            protected,
            &PathBuf::from(local).join("Roblox").join("Versions"),
        ).and_then(|count| {
            if count == 0 {
                Err("No eligible hidden external clients found; visible and managed clients stay open".into())
            } else { Ok(count) }
        }).map_err(|error: String| format!("Startup cleanup incomplete: {error}. Quit conflicting Roblox clients and restart RoLauncher."))
    }
    fn close_startup_candidate(
        identity: &ProcessIdentity,
        root: &str,
        session: u32,
        force: bool,
    ) -> Result<bool, String> {
        // Keep this exact process handle through inspection and forced termination.
        let handle = process_handle(
            identity.pid,
            PROCESS_QUERY_LIMITED_INFORMATION
                | SYNCHRONIZE
                | if force { PROCESS_TERMINATE } else { 0 },
        )?;
        if unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0 {
            return Ok(false);
        }
        let mut actual_session = 0;
        let mut path = [0u16; 32768];
        let mut length = path.len() as u32;
        if process_time(handle.0)? != identity.creation_time
            || unsafe { ProcessIdToSessionId(identity.pid, &mut actual_session) } == 0
            || actual_session != session
            || unsafe { QueryFullProcessImageNameW(handle.0, 0, path.as_mut_ptr(), &mut length) }
                == 0
            || !String::from_utf16_lossy(&path[..length as usize])
                .to_lowercase()
                .starts_with(root)
            || !valid_player_image(handle.0)
        {
            return Err(
                "Background client identity changed or cannot be verified; left open".into(),
            );
        }
        if visible_processes()?.contains(&identity.pid) {
            return Ok(false);
        }
        if force {
            terminate_verified_handle(handle.0)?;
            if unsafe { WaitForSingleObject(handle.0, 1000) } != WAIT_OBJECT_0 {
                return Err("Background client did not exit; quit it manually and restart".into());
            }
        } else {
            unsafe extern "system" fn close_hidden(hwnd: HWND, context: LPARAM) -> i32 {
                let (pid, handle) = unsafe { &*(context as *const (u32, HANDLE)) };
                let mut owner = 0;
                unsafe {
                    GetWindowThreadProcessId(hwnd, &mut owner);
                }
                if owner == *pid
                    && unsafe { IsWindowVisible(hwnd) } == 0
                    && unsafe { WaitForSingleObject(*handle, 0) } == WAIT_TIMEOUT
                    && visible_processes().is_ok_and(|visible| !visible.contains(pid))
                {
                    unsafe {
                        PostMessageW(hwnd, WM_CLOSE, 0, 0);
                    }
                }
                1
            }
            let context = (identity.pid, handle.0);
            if unsafe { EnumWindows(Some(close_hidden), &context as *const _ as isize) } == 0 {
                return Err(last_error("Background client close unavailable"));
            }
        }
        Ok(true)
    }
    fn close_startup_background_players_at(
        protected: &[ProcessIdentity],
        root: &std::path::Path,
    ) -> Result<usize, String> {
        let root = root.to_string_lossy().to_lowercase() + "\\";
        let mut session = 0;
        if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0 {
            return Err(last_error("Windows session unavailable"));
        }
        let visible = visible_processes()?;
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(last_error("Startup process query failed"));
        }
        let snapshot = Handle(snapshot);
        let mut row: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        row.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut next = unsafe { Process32FirstW(snapshot.0, &mut row) } != 0;
        let mut candidates = Vec::new();
        let mut inspection_failed = false;
        while next {
            let name = String::from_utf16_lossy(
                &row.szExeFile[..row
                    .szExeFile
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(row.szExeFile.len())],
            );
            if name.eq_ignore_ascii_case("RobloxPlayerBeta.exe")
                && !visible.contains(&row.th32ProcessID)
            {
                let candidate = (|| -> Result<Option<ProcessIdentity>, String> {
                    let mut owner_session = 0;
                    if unsafe { ProcessIdToSessionId(row.th32ProcessID, &mut owner_session) } == 0 {
                        return Err(last_error("Background client session unavailable"));
                    }
                    if owner_session != session {
                        return Ok(None);
                    }
                    let handle = process_handle(
                        row.th32ProcessID,
                        PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                    )?;
                    let creation_time = process_time(handle.0)?;
                    if protected
                        .iter()
                        .any(|id| id.pid == row.th32ProcessID && id.creation_time == creation_time)
                    {
                        return Ok(None);
                    }
                    let mut path = [0u16; 32768];
                    let mut length = path.len() as u32;
                    if unsafe {
                        QueryFullProcessImageNameW(handle.0, 0, path.as_mut_ptr(), &mut length)
                    } == 0
                    {
                        return Err(last_error("Background client image unavailable"));
                    }
                    if !valid_player_image(handle.0)
                        || !String::from_utf16_lossy(&path[..length as usize])
                            .to_lowercase()
                            .starts_with(&root)
                    {
                        return Ok(None);
                    }
                    Ok(Some(
                        Player {
                            pid: row.th32ProcessID,
                            creation_time,
                            tracker: String::new(),
                        }
                        .identity(Uuid::nil()),
                    ))
                })();
                match candidate {
                    Ok(Some(identity)) => candidates.push(identity),
                    Ok(None) => {}
                    Err(_) => inspection_failed = true,
                }
            }
            next = unsafe { Process32NextW(snapshot.0, &mut row) } != 0;
        }
        let mut closing = Vec::new();
        for identity in &candidates {
            if close_startup_candidate(identity, &root, session, false)? {
                closing.push(identity);
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline
            && closing.iter().any(|id| alive(id).unwrap_or(true))
        {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let mut closed = 0;
        for identity in closing {
            if !alive(identity)? || close_startup_candidate(identity, &root, session, true)? {
                closed += 1;
            }
        }
        if inspection_failed {
            return Err("Some background clients could not be inspected; left open".into());
        }
        Ok(closed)
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
        if identities.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
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
        let earliest = identities
            .iter()
            .filter_map(|id| id.creation_time.parse::<u64>().ok())
            .filter_map(|time| time.checked_sub(116_444_736_000_000_000))
            .min()
            .map(|ticks| {
                std::time::UNIX_EPOCH + std::time::Duration::from_nanos(ticks.saturating_mul(100))
            });
        candidates.retain(|entry| {
            earliest.is_none_or(|start| {
                entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|modified| modified >= start)
            })
        });
        candidates.sort_by_cached_key(|x| {
            std::cmp::Reverse(x.metadata().and_then(|m| m.modified()).ok())
        });
        let mut matched: std::collections::HashMap<(u32, String), Vec<PathBuf>> =
            std::collections::HashMap::new();
        for candidate in candidates {
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
    /// Keep acquisition and release on one owner thread, including delayed acquisition.
    pub struct MultiInstanceGuard {
        ready: std::sync::Arc<std::sync::atomic::AtomicBool>,
        stop: std::sync::mpsc::Sender<()>,
        worker: Option<std::thread::JoinHandle<()>>,
    }
    impl MultiInstanceGuard {
        pub fn acquire() -> Result<Self, String> {
            Self::acquire_named("ROBLOX_singletonMutex")
        }
        pub fn launch_gate(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
            self.ready.clone()
        }
        fn acquire_named(name: &str) -> Result<Self, String> {
            use std::sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
                mpsc,
            };
            let ready = Arc::new(AtomicBool::new(false));
            let worker_ready = ready.clone();
            let name = name.to_owned();
            let (stop, stopped) = mpsc::channel();
            let (started, startup) = mpsc::sync_channel(1);
            let worker = std::thread::Builder::new()
                .name("roblox-coordination".into())
                .spawn(move || {
                    let h = unsafe { CreateMutexW(std::ptr::null(), 1, wide(&name).as_ptr()) };
                    if h.is_null() {
                        let _ = started
                            .send(Err(last_error("Multi-instance coordination unavailable")));
                        return;
                    }
                    let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
                    let handle = Handle(h);
                    let wait = if existing {
                        unsafe { WaitForSingleObject(h, 0) }
                    } else {
                        WAIT_OBJECT_0
                    };
                    let mut owned = matches!(wait, WAIT_OBJECT_0 | WAIT_ABANDONED);
                    if !owned && wait != WAIT_TIMEOUT {
                        let _ = started
                            .send(Err(last_error("Multi-instance coordination wait failed")));
                        return;
                    }
                    worker_ready.store(owned, Ordering::Release);
                    let _ = started.send(Ok(()));
                    if owned {
                        let _ = stopped.recv();
                    }
                    while let Err(mpsc::RecvTimeoutError::Timeout) = if owned {
                        Ok(())
                    } else {
                        stopped.recv_timeout(std::time::Duration::from_millis(100))
                    } {
                        if !owned {
                            match unsafe { WaitForSingleObject(handle.0, 0) } {
                                WAIT_OBJECT_0 | WAIT_ABANDONED => {
                                    owned = true;
                                    worker_ready.store(true, Ordering::Release);
                                    let _ = stopped.recv();
                                }
                                WAIT_TIMEOUT => {}
                                _ => break,
                            }
                        }
                    }
                    worker_ready.store(false, Ordering::Release);
                    if owned {
                        unsafe {
                            ReleaseMutex(handle.0);
                        }
                    }
                })
                .map_err(|_| "Unable to start Roblox coordination worker")?;
            let guard = Self {
                ready,
                stop,
                worker: Some(worker),
            };
            match startup.recv() {
                Ok(Ok(())) => Ok(guard),
                Ok(Err(error)) => Err(error),
                Err(_) => Err("Roblox coordination worker stopped during initialization".into()),
            }
        }
    }
    impl Drop for MultiInstanceGuard {
        fn drop(&mut self) {
            let _ = self.stop.send(());
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
    include!("platform_memory.rs");
    #[cfg(test)]
    mod startup_tests {
        include!("platform_startup_tests.rs");
    }
    #[cfg(test)]
    mod close_tests {
        use super::*;
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        use std::sync::atomic::Ordering;

        #[test]
        fn cached_watches_detect_fifty_exits_and_reject_non_player_identity() {
            let mut children = Vec::new();
            let mut watches = Vec::new();
            for _ in 0..50 {
                let child = std::process::Command::new("cmd.exe")
                    .args(["/C", "set /p fixture="])
                    .creation_flags(0x08000000)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap();
                let identity = Player {
                    pid: child.id(),
                    creation_time: process_time(child.as_raw_handle() as _).unwrap(),
                    tracker: String::new(),
                }
                .identity(Uuid::nil());
                assert!(ExitWatch::new(&identity).is_none());
                let watch = ExitWatch(
                    process_handle(child.id(), PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)
                        .unwrap(),
                );
                assert!(watch.alive(&identity).unwrap());
                watches.push((watch, identity));
                children.push(child);
            }
            for child in &mut children {
                child.kill().unwrap();
                child.wait().unwrap();
            }
            for (watch, identity) in watches {
                assert!(!watch.alive(&identity).unwrap());
                assert_eq!(watch.clean_exit(), Some(false));
            }
        }

        #[test]
        fn startup_cleanup_closes_only_hidden_unmanaged_fixture_in_exact_installation() {
            let root = std::env::temp_dir().join(format!("rbx-startup-{}", Uuid::new_v4()));
            let versions = root.join("Roblox").join("Versions");
            std::fs::create_dir_all(&versions).unwrap();
            let executable = versions.join("RobloxPlayerBeta.exe");
            std::fs::copy(
                std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                    .join("System32")
                    .join("cmd.exe"),
                &executable,
            )
            .unwrap();
            let spawn = || {
                std::process::Command::new(&executable)
                    .args(["/C", "set /p fixture="])
                    .creation_flags(0x08000000)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap()
            };
            let mut protected = spawn();
            let mut leftover = spawn();
            let identity = Player {
                pid: protected.id(),
                creation_time: process_time(protected.as_raw_handle() as _).unwrap(),
                tracker: String::new(),
            }
            .identity(Uuid::nil());
            let result =
                close_startup_background_players_at(std::slice::from_ref(&identity), &versions);
            let protected_alive = protected.try_wait().unwrap().is_none();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while leftover.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let leftover_exited = leftover.try_wait().unwrap().is_some();
            let _ = protected.kill();
            let _ = leftover.kill();
            protected.wait().unwrap();
            leftover.wait().unwrap();
            std::fs::remove_dir_all(root).unwrap();
            assert_eq!(result.unwrap(), 1);
            assert!(protected_alive);
            assert!(leftover_exited);
        }

        #[test]
        #[ignore = "release-build microbenchmark; no Roblox processes"]
        fn hotfix_benchmark_exit_polling() {
            let pid = unsafe { GetCurrentProcessId() };
            let identity = Player {
                pid,
                creation_time: process_time(unsafe { GetCurrentProcess() }).unwrap(),
                tracker: String::new(),
            }
            .identity(Uuid::nil());
            let watches: Vec<_> = (0..50)
                .map(|_| {
                    ExitWatch(
                        process_handle(pid, PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)
                            .unwrap(),
                    )
                })
                .collect();
            let mut reports = Vec::new();
            for old in [true, false] {
                let start = std::time::Instant::now();
                let mut rounds = 0u64;
                while start.elapsed().as_secs_f64() < 2.0 {
                    for watch in &watches {
                        if old {
                            let handle = process_handle(
                                pid,
                                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                            )
                            .unwrap();
                            std::hint::black_box(process_time(handle.0).unwrap());
                            std::hint::black_box(valid_player_image(handle.0));
                            std::hint::black_box(unsafe { WaitForSingleObject(handle.0, 0) });
                        } else {
                            std::hint::black_box(watch.alive(&identity).unwrap());
                        }
                    }
                    rounds += 1;
                }
                reports.push(serde_json::json!({"mode":if old {"baseline"} else {"new"},"mean_ns_per_50":start.elapsed().as_nanos() as f64 / rounds as f64,"rounds":rounds}));
            }
            println!(
                "HOTFIX_BENCH {}",
                serde_json::json!({"workload":"50 Windows process-handle polls against this fixture process; excludes Roblox, discovery, persistence and log parsing","reports":reports})
            );
        }

        fn wait_ready(guard: &MultiInstanceGuard) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while !guard.ready.load(Ordering::Acquire) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "coordination did not recover"
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        #[test]
        fn coordination_recovers_after_existing_owner_releases_and_releases_on_drop() {
            let name = format!("RoLauncher-test-{}", Uuid::new_v4());
            let existing =
                Handle(unsafe { CreateMutexW(std::ptr::null(), 1, wide(&name).as_ptr()) });
            assert!(!existing.0.is_null());
            let guard = MultiInstanceGuard::acquire_named(&name).unwrap();
            assert!(!guard.ready.load(Ordering::Acquire));
            assert_ne!(unsafe { ReleaseMutex(existing.0) }, 0);
            wait_ready(&guard);
            assert_eq!(unsafe { WaitForSingleObject(existing.0, 0) }, WAIT_TIMEOUT);
            let gate = guard.launch_gate();
            drop(guard);
            assert!(!gate.load(Ordering::Acquire));
            assert_eq!(unsafe { WaitForSingleObject(existing.0, 0) }, WAIT_OBJECT_0);
            assert_ne!(unsafe { ReleaseMutex(existing.0) }, 0);
        }

        #[test]
        fn pending_coordination_can_shut_down_without_releasing_another_owner() {
            let name = format!("RoLauncher-test-{}", Uuid::new_v4());
            let existing =
                Handle(unsafe { CreateMutexW(std::ptr::null(), 1, wide(&name).as_ptr()) });
            assert!(!existing.0.is_null());
            let guard = MultiInstanceGuard::acquire_named(&name).unwrap();
            assert!(!guard.ready.load(Ordering::Acquire));
            drop(guard);
            assert_ne!(unsafe { ReleaseMutex(existing.0) }, 0);
        }

        #[test]
        fn coordination_recovers_an_abandoned_mutex() {
            let name = format!("RoLauncher-test-{}", Uuid::new_v4());
            let name_for_owner = name.clone();
            let (created, started) = std::sync::mpsc::channel();
            let (exit, stopped) = std::sync::mpsc::channel();
            let owner = std::thread::spawn(move || {
                let handle = Handle(unsafe {
                    CreateMutexW(std::ptr::null(), 1, wide(&name_for_owner).as_ptr())
                });
                assert!(!handle.0.is_null());
                created.send(()).unwrap();
                stopped.recv().unwrap();
                // Exit without ReleaseMutex to exercise WAIT_ABANDONED.
            });
            started.recv().unwrap();
            let guard = MultiInstanceGuard::acquire_named(&name).unwrap();
            assert!(!guard.ready.load(Ordering::Acquire));
            exit.send(()).unwrap();
            owner.join().unwrap();
            wait_ready(&guard);
        }

        #[test]
        fn coordination_acquires_a_new_mutex_immediately() {
            let guard =
                MultiInstanceGuard::acquire_named(&format!("RoLauncher-test-{}", Uuid::new_v4()))
                    .unwrap();
            assert!(guard.ready.load(Ordering::Acquire));
        }

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
