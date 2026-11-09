// Included in the Windows implementation. Native snapshots observe processes;
// only a tracker read while holding the exact live process handle can bind one.
#[derive(Default)]
pub struct ProcessScan {
    pub players: Vec<Player>,
    pub inaccessible: usize,
    pub rejected: usize,
    pub unowned: usize,
    pub metadata_pending: bool,
}
impl ProcessScan {
    pub fn verified(self) -> Result<Vec<Player>, String> {
        if self.inaccessible > 0 || self.unowned > 0 {
            return Err(
                "Process observation incomplete; waiting for verified tracker metadata".into(),
            );
        }
        Ok(self.players)
    }
}

struct TrackedProcess {
    handle: Handle,
    player: Player,
}
// Windows process handles support query/wait from another thread. Ownership is
// transferred, never duplicated without the kernel, and Drop closes each once.
unsafe impl Send for TrackedProcess {}
type MetadataResult = Result<Vec<TrackedProcess>, String>;
#[derive(Default)]
struct Discovery {
    tracked: Vec<TrackedProcess>,
    pending: Option<std::sync::mpsc::Receiver<MetadataResult>>,
    last_query: Option<std::time::Instant>,
}
static DISCOVERY: std::sync::LazyLock<std::sync::Mutex<Discovery>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(Discovery::default()));

fn native_players(verify: impl Fn(HANDLE) -> bool) -> Result<ProcessScan, String> {
    let snapshot = Handle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) });
    if snapshot.0 == INVALID_HANDLE_VALUE {
        return Err(last_error("Process snapshot unavailable"));
    }
    let mut session = 0;
    if unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) } == 0 {
        return Err(last_error("Windows session unavailable"));
    }
    let mut row: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    row.dwSize = std::mem::size_of_val(&row) as u32;
    let mut next = unsafe { Process32FirstW(snapshot.0, &mut row) } != 0;
    let mut scan = ProcessScan::default();
    while next {
        let len = row
            .szExeFile
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(row.szExeFile.len());
        if String::from_utf16_lossy(&row.szExeFile[..len])
            .eq_ignore_ascii_case("RobloxPlayerBeta.exe")
        {
            let mut owner_session = 0;
            if unsafe { ProcessIdToSessionId(row.th32ProcessID, &mut owner_session) } == 0 {
                if unsafe { GetLastError() } != ERROR_INVALID_PARAMETER {
                    scan.inaccessible += 1;
                }
            } else if owner_session == session {
                match process_handle(
                    row.th32ProcessID,
                    PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                ) {
                    Ok(handle) => {
                        if unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_TIMEOUT {
                            if image_path(handle.0).is_none() {
                                scan.inaccessible += 1;
                                next = unsafe { Process32NextW(snapshot.0, &mut row) } != 0;
                                continue;
                            }
                            match process_time(handle.0) {
                                Ok(creation_time) if verify(handle.0) => {
                                    scan.players.push(Player {
                                        pid: row.th32ProcessID,
                                        creation_time,
                                        tracker: String::new(),
                                    })
                                }
                                Ok(_) => scan.rejected += 1,
                                Err(_) => scan.inaccessible += 1,
                            }
                        }
                    }
                    Err(_) if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER => {}
                    Err(_) => scan.inaccessible += 1,
                }
            }
        }
        next = unsafe { Process32NextW(snapshot.0, &mut row) } != 0;
    }
    if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
        return Err(last_error("Process snapshot incomplete"));
    }
    Ok(scan)
}

fn query_trackers(mut candidates: Vec<TrackedProcess>) -> MetadataResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Row {
        process_id: u32,
        command_line: Option<String>,
    }
    let connection =
        wmi::WMIConnection::new().map_err(|_| "Windows process metadata unavailable")?;
    let rows: Vec<Row> = connection
        .raw_query(
            "SELECT ProcessId, CommandLine FROM Win32_Process WHERE Name = 'RobloxPlayerBeta.exe'",
        )
        .map_err(|_| "Windows process query failed")?;
    for row in rows {
        if let Some(candidate) = candidates
            .iter_mut()
            .find(|c| c.player.pid == row.process_id)
            && unsafe { WaitForSingleObject(candidate.handle.0, 0) } == WAIT_TIMEOUT
            && let Some(tracker) = row
                .command_line
                .as_deref()
                .and_then(tracker_from_command_line)
        {
            candidate.player.tracker = tracker;
        }
    }
    candidates.retain(|c| {
        !c.player.tracker.is_empty()
            && unsafe { WaitForSingleObject(c.handle.0, 0) } == WAIT_TIMEOUT
    });
    Ok(candidates)
}

impl Discovery {
    fn observe(&mut self, mut scan: ProcessScan) -> Result<ProcessScan, String> {
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    self.last_query = Some(std::time::Instant::now());
                    if let Ok(found) = result {
                        self.tracked.retain(|old| {
                            !found.iter().any(|new| {
                                old.player.pid == new.player.pid
                                    && old.player.creation_time == new.player.creation_time
                            })
                        });
                        self.tracked.extend(found);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        self.tracked.retain(|c| unsafe { WaitForSingleObject(c.handle.0, 0) } == WAIT_TIMEOUT && scan.players.iter().any(|p| p.pid == c.player.pid && p.creation_time == c.player.creation_time));
        for player in &mut scan.players {
            if let Some(cached) = self.tracked.iter().find(|c| {
                c.player.pid == player.pid && c.player.creation_time == player.creation_time
            }) {
                player.tracker.clone_from(&cached.player.tracker);
            }
        }
        scan.unowned = scan.players.iter().filter(|p| p.tracker.is_empty()).count();
        if self.pending.is_none()
            && !scan.players.is_empty()
            && self
                .last_query
                .is_none_or(|last| last.elapsed() >= std::time::Duration::from_secs(5))
        {
            if scan.players.len() > 512 {
                return Err("Process observation budget exceeded".into());
            }
            let candidates = scan
                .players
                .iter()
                .filter_map(|p| {
                    let handle =
                        process_handle(p.pid, PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)
                            .ok()?;
                    (process_time(handle.0).ok()? == p.creation_time
                        && unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_TIMEOUT)
                        .then(|| TrackedProcess {
                            handle,
                            player: Player {
                                tracker: String::new(),
                                ..p.clone()
                            },
                        })
                })
                .collect();
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            std::thread::Builder::new()
                .name("roblox-metadata".into())
                .spawn(move || {
                    let _ = sender.send(query_trackers(candidates));
                })
                .map_err(|_| "Process metadata worker unavailable")?;
            self.pending = Some(receiver);
            self.last_query = Some(std::time::Instant::now());
        }
        scan.metadata_pending = self.pending.is_some();
        Ok(scan)
    }
}

pub fn observe_players() -> Result<ProcessScan, String> {
    let scan = native_players(valid_player_image)?;
    DISCOVERY
        .lock()
        .map_err(|_| "Process observation unavailable")?
        .observe(scan)
}
pub fn players() -> Result<Vec<Player>, String> {
    observe_players()?.verified()
}
pub fn all_players() -> Result<Vec<Player>, String> {
    let scan = native_players(valid_player_image)?;
    if scan.inaccessible > 0 {
        return Err("Some Roblox processes could not be inspected".into());
    }
    Ok(scan.players)
}
pub fn observe_all_players() -> Result<ProcessScan, String> {
    native_players(valid_player_image)
}
