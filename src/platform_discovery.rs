// Included in the Windows implementation. Native snapshots observe processes;
// Metadata is bound to pinned process objects, including direct-child handoffs.
#[derive(Default)]
pub struct ProcessScan {
    pub players: Vec<Player>,
    pub inaccessible: usize,
    pub rejected: usize,
    pub unowned: usize,
    pub uninspected: usize,
    pub metadata_pending: bool,
    pub metadata_failed: bool,
    pub bootstrapper_active: bool,
    pub uninspected_pids: std::collections::HashSet<u32>,
    pub inaccessible_processes: Vec<InaccessibleProcess>,
    pub bootstrappers: Vec<ObservedBootstrapper>,
    parents: std::collections::HashMap<u32, u32>,
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

    pub fn parent_pid(&self, pid: u32) -> Option<u32> {
        self.parents.get(&pid).copied()
    }

    #[cfg(test)]
    pub fn set_parent(&mut self, child_pid: u32, parent_pid: u32) {
        self.parents.insert(child_pid, parent_pid);
    }

    pub fn has_unowned_child(&self, parent_pid: u32) -> bool {
        self.players.iter().any(|p| {
            p.tracker.is_empty() && self.parents.get(&p.pid).copied() == Some(parent_pid)
        }) || self
            .inaccessible_processes
            .iter()
            .any(|ip| ip.parent_pid == parent_pid)
    }

    pub fn bootstrapper_relevant_for(
        &self,
        tracker: &str,
        target_pid: Option<u32>,
        other_owned_pids: &std::collections::HashSet<u32>,
        other_account_launching: bool,
    ) -> bool {
        if self.bootstrappers.is_empty() {
            return self.bootstrapper_active;
        }
        let other_bootstrapper_pids: std::collections::HashSet<u32> = self
            .bootstrappers
            .iter()
            .filter(|b| !b.tracker.is_empty() && b.tracker != tracker)
            .map(|b| b.pid)
            .collect();
        self.bootstrappers.iter().any(|b| {
            (!tracker.is_empty() && b.tracker == tracker)
                || target_pid.is_some_and(|pid| b.parent_pid == pid)
                || (b.tracker.is_empty()
                    && !other_owned_pids.contains(&b.parent_pid)
                    && !other_bootstrapper_pids.contains(&b.parent_pid)
                    && (b.inspected || self.metadata_pending || !other_account_launching))
        })
    }

    pub fn inaccessible_relevant_for(&self, target: &ProcessIdentity) -> usize {
        if self.inaccessible_processes.is_empty() {
            return self.inaccessible;
        }
        let relevant_bootstrappers: std::collections::HashSet<u32> = self
            .bootstrappers
            .iter()
            .filter(|b| {
                (!target.tracker.is_empty() && b.tracker == target.tracker)
                    || b.parent_pid == target.pid
            })
            .map(|b| b.pid)
            .collect();
        self.inaccessible_processes
            .iter()
            .filter(|ip| {
                ip.pid == target.pid
                    || ip.parent_pid == target.pid
                    || relevant_bootstrappers.contains(&ip.parent_pid)
            })
            .count()
    }

    pub fn uninspected_relevant_for(
        &self,
        target: &ProcessIdentity,
        other_owned_pids: &std::collections::HashSet<u32>,
    ) -> usize {
        if self.uninspected_pids.is_empty() {
            return self.uninspected;
        }
        let other_bootstrapper_pids: std::collections::HashSet<u32> = self
            .bootstrappers
            .iter()
            .filter(|b| !b.tracker.is_empty() && b.tracker != target.tracker)
            .map(|b| b.pid)
            .collect();
        let target_birth = target.creation_time.parse::<u64>().ok();
        self.uninspected_pids
            .iter()
            .filter(|pid| {
                if other_owned_pids.contains(pid) {
                    return false;
                }
                if self.parents.get(pid).is_some_and(|pp| {
                    other_owned_pids.contains(pp) || other_bootstrapper_pids.contains(pp)
                }) {
                    return false;
                }
                if let Some(tb) = target_birth
                    && let Some(p) = self.players.iter().find(|p| p.pid == **pid)
                    && let Ok(pb) = p.creation_time.parse::<u64>()
                    && pb.saturating_add(600_000_000) < tb
                {
                    return false;
                }
                true
            })
            .count()
    }

    pub fn inaccessible_relevant_for_retired(
        &self,
        tracker: &str,
        process: Option<&ProcessIdentity>,
    ) -> usize {
        if self.inaccessible_processes.is_empty() {
            return self.inaccessible;
        }
        let target_pid = process.map(|p| p.pid);
        let relevant_bootstrappers: std::collections::HashSet<u32> = self
            .bootstrappers
            .iter()
            .filter(|b| {
                (!tracker.is_empty() && b.tracker == tracker)
                    || target_pid.is_some_and(|pid| b.parent_pid == pid)
            })
            .map(|b| b.pid)
            .collect();
        self.inaccessible_processes
            .iter()
            .filter(|ip| {
                target_pid.is_some_and(|pid| ip.pid == pid || ip.parent_pid == pid)
                    || relevant_bootstrappers.contains(&ip.parent_pid)
            })
            .count()
    }

    pub fn uninspected_relevant_for_retired(
        &self,
        tracker: &str,
        other_owned_pids: &std::collections::HashSet<u32>,
    ) -> usize {
        if self.uninspected_pids.is_empty() {
            return self.uninspected;
        }
        let other_bootstrapper_pids: std::collections::HashSet<u32> = self
            .bootstrappers
            .iter()
            .filter(|b| !b.tracker.is_empty() && b.tracker != tracker)
            .map(|b| b.pid)
            .collect();
        self.uninspected_pids
            .iter()
            .filter(|pid| {
                !other_owned_pids.contains(pid)
                    && !self.parents.get(pid).is_some_and(|pp| {
                        other_owned_pids.contains(pp) || other_bootstrapper_pids.contains(pp)
                    })
            })
            .count()
    }

    pub fn verified_for_target(&self, target: &ProcessIdentity) -> Result<Vec<Player>, String> {
        if target.tracker.is_empty() {
            return Err("Target process has no verified tracker".into());
        }
        if self.inaccessible > 0
            || self.uninspected > 0
            || self.players.iter().any(|p| {
                p.pid == target.pid
                    && p.creation_time == target.creation_time
                    && p.tracker.is_empty()
                    && self.metadata_pending
            })
        {
            return Err(
                "Process observation incomplete; waiting for verified tracker metadata".into(),
            );
        }
        if self.has_unowned_child(target.pid) {
            return Err(
                "Target process has an unverified child process; waiting for handoff metadata"
                    .into(),
            );
        }
        Ok(self.players.clone())
    }

    pub fn verified_for_tracker(
        &self,
        tracker: &str,
        process: Option<&ProcessIdentity>,
    ) -> Result<Vec<Player>, String> {
        if tracker.is_empty() {
            return Err("Target tracker is empty".into());
        }
        if self.inaccessible > 0 || self.uninspected > 0 {
            return Err(
                "Process observation incomplete; waiting for verified tracker metadata".into(),
            );
        }
        if let Some(target) = process
            && self.has_unowned_child(target.pid)
        {
            return Err(
                "Target process has an unverified child process; waiting for handoff metadata"
                    .into(),
            );
        }
        Ok(self.players.clone())
    }
}

struct TrackedProcess {
    handle: Handle,
    player: Player,
    parent: Option<Player>,
    roblox_parent: bool,
}
// Windows process handles support query/wait from another thread. Ownership is
// transferred, never duplicated without the kernel, and Drop closes each once.
unsafe impl Send for TrackedProcess {}
struct TrackedBootstrapper {
    handle: Handle,
    bootstrapper: ObservedBootstrapper,
}
unsafe impl Send for TrackedBootstrapper {}
type MetadataResult = Result<(Vec<TrackedProcess>, Vec<TrackedBootstrapper>), String>;
#[derive(Default)]
struct Discovery {
    tracked: Vec<TrackedProcess>,
    tracked_bootstrappers: Vec<TrackedBootstrapper>,
    pending: Option<std::sync::mpsc::Receiver<MetadataResult>>,
    last_query: Option<std::time::Instant>,
    last_query_failed: bool,
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
        let exe_name = String::from_utf16_lossy(&row.szExeFile[..len]);
        if exe_name.eq_ignore_ascii_case("RobloxPlayerBeta.exe") {
            let mut owner_session = 0;
            if unsafe { ProcessIdToSessionId(row.th32ProcessID, &mut owner_session) } == 0 {
                if unsafe { GetLastError() } != ERROR_INVALID_PARAMETER {
                    scan.inaccessible += 1;
                    scan.inaccessible_processes.push(InaccessibleProcess {
                        pid: row.th32ProcessID,
                        parent_pid: row.th32ParentProcessID,
                    });
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
                                scan.inaccessible_processes.push(InaccessibleProcess {
                                    pid: row.th32ProcessID,
                                    parent_pid: row.th32ParentProcessID,
                                });
                                next = unsafe { Process32NextW(snapshot.0, &mut row) } != 0;
                                continue;
                            }
                            match process_time(handle.0) {
                                Ok(creation_time) if verify(handle.0) => {
                                    scan.parents
                                        .insert(row.th32ProcessID, row.th32ParentProcessID);
                                    scan.players.push(Player {
                                        pid: row.th32ProcessID,
                                        creation_time,
                                        tracker: String::new(),
                                    })
                                }
                                Ok(_) => scan.rejected += 1,
                                Err(_) => {
                                    scan.inaccessible += 1;
                                    scan.inaccessible_processes.push(InaccessibleProcess {
                                        pid: row.th32ProcessID,
                                        parent_pid: row.th32ParentProcessID,
                                    });
                                }
                            }
                        }
                    }
                    Err(_) if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER => {}
                    Err(_) => {
                        scan.inaccessible += 1;
                        scan.inaccessible_processes.push(InaccessibleProcess {
                            pid: row.th32ProcessID,
                            parent_pid: row.th32ParentProcessID,
                        });
                    }
                }
            }
        } else if exe_name.eq_ignore_ascii_case("RobloxPlayerLauncher.exe")
            || exe_name.eq_ignore_ascii_case("RobloxPlayerInstaller.exe")
        {
            let mut owner_session = 0;
            if unsafe { ProcessIdToSessionId(row.th32ProcessID, &mut owner_session) } != 0
                && owner_session == session
                && let Ok(handle) = process_handle(
                    row.th32ProcessID,
                    PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                )
                && unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_TIMEOUT
                && valid_bootstrapper_image(handle.0)
            {
                scan.bootstrapper_active = true;
                scan.bootstrappers.push(ObservedBootstrapper {
                    pid: row.th32ProcessID,
                    parent_pid: row.th32ParentProcessID,
                    creation_time: process_time(handle.0).unwrap_or_default(),
                    tracker: String::new(),
                    inspected: false,
                });
            }
        }
        next = unsafe { Process32NextW(snapshot.0, &mut row) } != 0;
    }
    if unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
        return Err(last_error("Process snapshot incomplete"));
    }
    Ok(scan)
}

fn query_trackers(
    mut candidates: Vec<TrackedProcess>,
    mut bootstrapper_candidates: Vec<TrackedBootstrapper>,
) -> MetadataResult {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Row {
        process_id: u32,
        parent_process_id: u32,
        command_line: Option<String>,
    }
    let connection =
        wmi::WMIConnection::new().map_err(|_| "Windows process metadata unavailable")?;
    let rows: Vec<Row> = connection
        .raw_query(
            "SELECT ProcessId, ParentProcessId, CommandLine FROM Win32_Process WHERE Name = 'RobloxPlayerBeta.exe' OR Name = 'RobloxPlayerLauncher.exe' OR Name = 'RobloxPlayerInstaller.exe'",
        )
        .map_err(|_| "Windows process query failed")?;
    let mut inspected = Vec::new();
    let mut inspected_bootstrappers = Vec::new();
    for row in rows {
        if let Some(pos) = candidates
            .iter()
            .position(|c| c.player.pid == row.process_id)
            && unsafe { WaitForSingleObject(candidates[pos].handle.0, 0) } == WAIT_TIMEOUT
            && let Some(cmd) = row.command_line.as_deref()
        {
            let mut candidate = candidates.swap_remove(pos);
            let no_tracker_args = command_arguments(cmd).is_some_and(|args| {
                args.iter().skip(1).all(|arg| {
                    !arg.eq_ignore_ascii_case("-b")
                        && !arg.to_ascii_lowercase().starts_with("roblox-player:")
                })
            });
            if let Some(tracker) = tracker_from_command_line(cmd) {
                candidate.player.tracker = tracker;
                candidate.parent = None;
                inspected.push(candidate);
            } else if let Some(parent) = &candidate.parent
                && parent.pid == row.parent_process_id
                && no_tracker_args
            {
                candidate.player.tracker.clone_from(&parent.tracker);
                inspected.push(candidate);
            } else if candidate.parent.is_none() && !candidate.roblox_parent && no_tracker_args {
                inspected.push(candidate);
            }
        } else if let Some(pos) = bootstrapper_candidates
            .iter()
            .position(|b| b.bootstrapper.pid == row.process_id)
            && unsafe { WaitForSingleObject(bootstrapper_candidates[pos].handle.0, 0) }
                == WAIT_TIMEOUT
            && let Some(cmd) = row.command_line.as_deref()
        {
            let mut b = bootstrapper_candidates.swap_remove(pos);
            b.bootstrapper.tracker = tracker_from_command_line(cmd).unwrap_or_default();
            b.bootstrapper.inspected = true;
            inspected_bootstrappers.push(b);
        }
    }
    inspected.retain(|c| unsafe { WaitForSingleObject(c.handle.0, 0) } == WAIT_TIMEOUT);
    inspected_bootstrappers
        .retain(|b| unsafe { WaitForSingleObject(b.handle.0, 0) } == WAIT_TIMEOUT);
    Ok((inspected, inspected_bootstrappers))
}

impl Discovery {
    fn observe(&mut self, mut scan: ProcessScan) -> Result<ProcessScan, String> {
        if let Some(pending) = &self.pending {
            match pending.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    self.last_query = Some(std::time::Instant::now());
                    match result {
                        Ok((found, found_bootstrappers)) => {
                            self.last_query_failed = false;
                            self.tracked.retain(|old| {
                                !found.iter().any(|new| {
                                    old.player.pid == new.player.pid
                                        && old.player.creation_time == new.player.creation_time
                                })
                            });
                            self.tracked.extend(found);
                            self.tracked_bootstrappers.retain(|old| {
                                !found_bootstrappers.iter().any(|new| {
                                    old.bootstrapper.pid == new.bootstrapper.pid
                                        && old.bootstrapper.creation_time
                                            == new.bootstrapper.creation_time
                                })
                            });
                            self.tracked_bootstrappers.extend(found_bootstrappers);
                        }
                        Err(_) => {
                            self.last_query_failed = true;
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        // Keep the exited parent's original handle while a potential direct child exists.
        let tracked_parents: std::collections::HashSet<u32> = self
            .tracked
            .iter()
            .filter(|c| !c.player.tracker.is_empty())
            .map(|c| c.player.pid)
            .chain(scan.players.iter().map(|p| p.pid))
            .collect();
        self.tracked
            .retain(|c| match unsafe { WaitForSingleObject(c.handle.0, 0) } {
                WAIT_TIMEOUT => {
                    scan.players.iter().any(|p| {
                        p.pid == c.player.pid && p.creation_time == c.player.creation_time
                    }) && (!c.player.tracker.is_empty()
                        || !scan
                            .parents
                            .get(&c.player.pid)
                            .is_some_and(|parent| tracked_parents.contains(parent)))
                }
                WAIT_OBJECT_0 => {
                    !c.player.tracker.is_empty()
                        && c.parent.is_none()
                        && scan.parents.values().any(|pid| *pid == c.player.pid)
                }
                _ => false,
            });
        self.tracked_bootstrappers.retain(|b| {
            let alive = unsafe { WaitForSingleObject(b.handle.0, 0) } == WAIT_TIMEOUT;
            alive
                && scan.bootstrappers.iter().any(|obs| {
                    obs.pid == b.bootstrapper.pid
                        && obs.creation_time == b.bootstrapper.creation_time
                })
        });
        for obs in &mut scan.bootstrappers {
            if let Some(cached) = self.tracked_bootstrappers.iter().find(|b| {
                b.bootstrapper.pid == obs.pid && b.bootstrapper.creation_time == obs.creation_time
            }) {
                obs.tracker.clone_from(&cached.bootstrapper.tracker);
                obs.inspected = cached.bootstrapper.inspected;
            }
        }
        let mut uninspected = 0;
        for player in &mut scan.players {
            if let Some(cached) = self.tracked.iter().find(|c| {
                c.player.pid == player.pid
                    && c.player.creation_time == player.creation_time
                    && unsafe { WaitForSingleObject(c.handle.0, 0) } == WAIT_TIMEOUT
            }) {
                player.tracker.clone_from(&cached.player.tracker);
            } else if player.tracker.is_empty() {
                uninspected += 1;
                scan.uninspected_pids.insert(player.pid);
            }
        }
        scan.unowned = scan.players.iter().filter(|p| p.tracker.is_empty()).count();
        scan.uninspected = uninspected;
        scan.metadata_failed = self.last_query_failed;
        scan.bootstrapper_active = scan.bootstrapper_active || !scan.bootstrappers.is_empty();
        let has_uninspected_bootstrappers = scan.bootstrappers.iter().any(|b| !b.inspected);
        if self.pending.is_none()
            && (!scan.players.is_empty() || !scan.bootstrappers.is_empty())
            && self.last_query.is_none_or(|last| {
                last.elapsed()
                    >= std::time::Duration::from_secs(
                        if scan.uninspected > 0 || has_uninspected_bootstrappers {
                            1
                        } else {
                            5
                        },
                    )
            })
        {
            if scan.players.len() + scan.bootstrappers.len() > 512 {
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
                            parent: self
                                .tracked
                                .iter()
                                .find(|c| {
                                    !c.player.tracker.is_empty()
                                        && c.parent.is_none()
                                        && scan.parents.get(&p.pid) == Some(&c.player.pid)
                                        && child_born_before_exit(c, p)
                                })
                                .map(|c| c.player.clone()),
                            roblox_parent: scan
                                .parents
                                .get(&p.pid)
                                .is_some_and(|parent| tracked_parents.contains(parent)),
                            player: Player {
                                tracker: String::new(),
                                ..p.clone()
                            },
                        })
                })
                .collect();
            let bootstrapper_candidates = scan
                .bootstrappers
                .iter()
                .filter_map(|b| {
                    let handle =
                        process_handle(b.pid, PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)
                            .ok()?;
                    (process_time(handle.0).ok()? == b.creation_time
                        && unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_TIMEOUT)
                        .then(|| TrackedBootstrapper {
                            handle,
                            bootstrapper: ObservedBootstrapper {
                                tracker: String::new(),
                                inspected: false,
                                ..b.clone()
                            },
                        })
                })
                .collect();
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            std::thread::Builder::new()
                .name("roblox-metadata".into())
                .spawn(move || {
                    let _ = sender.send(query_trackers(candidates, bootstrapper_candidates));
                })
                .map_err(|_| "Process metadata worker unavailable")?;
            self.pending = Some(receiver);
            self.last_query = Some(std::time::Instant::now());
        }
        scan.metadata_pending = self.pending.is_some();
        Ok(scan)
    }
}

fn child_born_before_exit(parent: &TrackedProcess, child: &Player) -> bool {
    if unsafe { WaitForSingleObject(parent.handle.0, 0) } != WAIT_OBJECT_0 {
        return false;
    }
    let Ok((start, exit)) = process_times(parent.handle.0) else {
        return false;
    };
    child
        .creation_time
        .parse::<u64>()
        .is_ok_and(|birth| start < birth && birth <= exit)
}

pub fn direct_successor(parent: &ProcessIdentity, child: &ProcessIdentity) -> bool {
    DISCOVERY.lock().is_ok_and(|d| {
        d.tracked.iter().any(|c| {
            c.player.pid == child.pid
                && c.player.creation_time == child.creation_time
                && c.player.tracker == child.tracker
                && unsafe { WaitForSingleObject(c.handle.0, 0) } == WAIT_TIMEOUT
                && c.parent.as_ref().is_some_and(|p| {
                    p.pid == parent.pid
                        && p.creation_time == parent.creation_time
                        && p.tracker == parent.tracker
                })
        })
    })
}

pub fn successor_close_grace(identity: &ProcessIdentity) -> std::time::Duration {
    let verified_child = DISCOVERY.lock().is_ok_and(|d| {
        d.tracked.iter().any(|c| {
            c.player.pid == identity.pid
                && c.player.creation_time == identity.creation_time
                && c.player.tracker == identity.tracker
                && c.parent.is_some()
                && unsafe { WaitForSingleObject(c.handle.0, 0) } == WAIT_TIMEOUT
        })
    });
    std::time::Duration::from_secs(if verified_child { 1 } else { 5 })
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
