// Read-only Windows resource sampling for verified managed processes and host metrics.
// Never opens processes with VM read/write or termination rights, and never reads
// arbitrary external process command lines or memory.

use std::collections::{HashMap, HashSet};
use std::mem::{size_of, zeroed};
use std::time::{Duration, Instant};

const GPU_COUNTER_PATH: &str = "\\GPU Engine(*)\\Utilization Percentage";
const GPU_POLL_MIN_INTERVAL: Duration = Duration::from_millis(1800);
const GPU_REBUILD_MIN_INTERVAL: Duration = Duration::from_secs(5);
const GPU_PERIODIC_REBUILD: Duration = Duration::from_secs(30);
const GPU_FAILURE_BACKOFF: Duration = Duration::from_secs(60);

struct PdhQuery(PDH_HQUERY);
unsafe impl Send for PdhQuery {}
impl Drop for PdhQuery {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                PdhCloseQuery(self.0);
            }
        }
    }
}

#[derive(Default)]
struct GpuPdhState {
    query: Option<PdhQuery>,
    counter: PDH_HCOUNTER,
    primed: bool,
    collects: u32,
    last_collect: Option<Instant>,
    last_rebuild: Option<Instant>,
    unavailable_until: Option<Instant>,
    known_pids: HashSet<u32>,
    cached: Option<(Option<f64>, HashMap<u32, f64>)>,
}
unsafe impl Send for GpuPdhState {}

static GPU_PDH: std::sync::LazyLock<std::sync::Mutex<GpuPdhState>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(GpuPdhState::default()));

pub(super) fn reset_gpu_sampler() {
    if let Ok(mut guard) = GPU_PDH.lock() {
        guard.query = None;
        guard.counter = std::ptr::null_mut();
        guard.primed = false;
        guard.collects = 0;
        guard.known_pids.clear();
        guard.cached = None;
    }
}

fn filetime_to_u64(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

fn system_now_100ns() -> u64 {
    let mut now: FILETIME = unsafe { zeroed() };
    unsafe {
        GetSystemTimeAsFileTime(&mut now);
    }
    filetime_to_u64(now)
}

pub(super) fn parse_gpu_engine_instance(instance: &str) -> Option<(u32, String)> {
    let rest = instance.strip_prefix("pid_")?;
    let sep = rest.find('_')?;
    if sep == 0 {
        return None;
    }
    let pid: u32 = rest[..sep].parse().ok()?;
    let after_pid = &rest[sep + 1..];
    let bucket = if let Some(idx) = after_pid.find("_engtype_") {
        &after_pid[..idx]
    } else {
        after_pid
    };
    Some((pid, bucket.to_string()))
}

fn wide_slice_to_string(ptr: *const u16) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // Bound UTF-16 scan so malformed counter buffers cannot overrun memory.
    let mut len = 0usize;
    while len < 512 {
        if unsafe { *ptr.add(len) } == 0 {
            break;
        }
        len += 1;
    }
    Some(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(ptr, len)
    }))
}

impl GpuPdhState {
    fn open_query(&mut self) -> Result<(), ()> {
        self.query = None;
        self.counter = std::ptr::null_mut();
        self.primed = false;
        self.known_pids.clear();

        let mut query: PDH_HQUERY = std::ptr::null_mut();
        let status = unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) };
        if status != ERROR_SUCCESS || query.is_null() {
            return Err(());
        }
        let guard = PdhQuery(query);
        let mut counter: PDH_HCOUNTER = std::ptr::null_mut();
        let path = wide(GPU_COUNTER_PATH);
        let status = unsafe { PdhAddEnglishCounterW(guard.0, path.as_ptr(), 0, &mut counter) };
        if status != ERROR_SUCCESS || counter.is_null() {
            return Err(());
        }
        let collect = unsafe { PdhCollectQueryData(guard.0) };
        if collect != ERROR_SUCCESS {
            return Err(());
        }
        self.query = Some(guard);
        self.counter = counter;
        self.primed = true;
        self.collects = 1;
        self.last_collect = Some(Instant::now());
        self.last_rebuild = Some(Instant::now());
        Ok(())
    }

    fn sample(&mut self, verified_pids: &HashSet<u32>) -> NativeGpuReading {
        let now = Instant::now();
        if self.unavailable_until.is_some_and(|until| now < until) {
            return NativeGpuReading::Unavailable;
        }

        let has_unseen_pid = verified_pids
            .iter()
            .any(|pid| !self.known_pids.contains(pid));
        let periodic_refresh = self
            .last_rebuild
            .is_none_or(|t| now.saturating_duration_since(t) >= GPU_PERIODIC_REBUILD);
        let rebuild_allowed = self
            .last_rebuild
            .is_none_or(|t| now.saturating_duration_since(t) >= GPU_REBUILD_MIN_INTERVAL);

        if self.query.is_none() || ((has_unseen_pid || periodic_refresh) && rebuild_allowed) {
            if self.open_query().is_err() {
                self.unavailable_until = Some(now + GPU_FAILURE_BACKOFF);
                self.cached = None;
                return NativeGpuReading::Unavailable;
            }
            if let Some((sys, map)) = &self.cached {
                let filtered = map
                    .iter()
                    .filter(|(pid, _)| verified_pids.contains(pid))
                    .map(|(&pid, &v)| (pid, v))
                    .collect();
                return NativeGpuReading::Available {
                    system_percent: *sys,
                    per_pid_percent: filtered,
                };
            }
            return NativeGpuReading::Pending;
        }

        if let Some(last) = self.last_collect
            && now.saturating_duration_since(last) < GPU_POLL_MIN_INTERVAL
            && let Some((sys, map)) = &self.cached
        {
            let filtered = map
                .iter()
                .filter(|(pid, _)| verified_pids.contains(pid))
                .map(|(&pid, &v)| (pid, v))
                .collect();
            return NativeGpuReading::Available {
                system_percent: *sys,
                per_pid_percent: filtered,
            };
        }

        let Some(query) = &self.query else {
            return NativeGpuReading::Unavailable;
        };
        let collect = unsafe { PdhCollectQueryData(query.0) };
        if collect != ERROR_SUCCESS {
            self.unavailable_until = Some(now + GPU_FAILURE_BACKOFF);
            self.query = None;
            self.cached = None;
            return NativeGpuReading::Unavailable;
        }
        self.last_collect = Some(now);
        self.collects = self.collects.saturating_add(1);
        if !self.primed {
            self.primed = true;
            return NativeGpuReading::Pending;
        }

        let mut buffer_bytes: u32 = 0;
        let mut item_count: u32 = 0;
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut buffer_bytes,
                &mut item_count,
                std::ptr::null_mut(),
            )
        };
        if status == PDH_NO_DATA {
            // Valid counter query exists, but zero GPU engine instances are active right now.
            self.cached = Some((Some(0.0), HashMap::new()));
            return NativeGpuReading::Available {
                system_percent: Some(0.0),
                per_pid_percent: HashMap::new(),
            };
        }
        if status != PDH_MORE_DATA || buffer_bytes == 0 || item_count == 0 {
            self.unavailable_until = Some(now + GPU_FAILURE_BACKOFF);
            self.query = None;
            self.cached = None;
            return NativeGpuReading::Unavailable;
        }

        let word_count = (buffer_bytes as usize).div_ceil(size_of::<u64>());
        let mut buffer = vec![0u64; word_count];
        let items_ptr = buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        let status = unsafe {
            PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut buffer_bytes,
                &mut item_count,
                items_ptr,
            )
        };
        if status != ERROR_SUCCESS {
            return NativeGpuReading::Unavailable;
        }
        let needed_bytes = (item_count as usize) * size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        if (buffer_bytes as usize) < needed_bytes || needed_bytes > word_count * size_of::<u64>() {
            return NativeGpuReading::Unavailable;
        }

        let items = unsafe { std::slice::from_raw_parts(items_ptr, item_count as usize) };
        let mut observed_pids = HashSet::new();
        let mut per_pid_all: HashMap<u32, f64> = HashMap::new();
        let mut per_engine_bucket: HashMap<String, f64> = HashMap::new();
        let mut valid_items = 0usize;

        for item in items {
            let cstatus = item.FmtValue.CStatus;
            if cstatus != PDH_CSTATUS_VALID_DATA && cstatus != PDH_CSTATUS_NEW_DATA {
                continue;
            }
            let val = unsafe { item.FmtValue.Anonymous.doubleValue };
            if !val.is_finite() || val < 0.0 {
                continue;
            }
            let Some(name) = wide_slice_to_string(item.szName) else {
                continue;
            };
            let Some((pid, bucket)) = parse_gpu_engine_instance(&name) else {
                continue;
            };
            valid_items += 1;
            observed_pids.insert(pid);
            let entry = per_pid_all.entry(pid).or_insert(0.0);
            *entry = (*entry + val).clamp(0.0, 100.0);
            let eng = per_engine_bucket.entry(bucket).or_insert(0.0);
            *eng = (*eng + val).clamp(0.0, 100.0);
        }

        if valid_items == 0 {
            if self.cached.is_none() && self.collects < 3 {
                return NativeGpuReading::Pending;
            }
            self.unavailable_until = Some(now + GPU_FAILURE_BACKOFF);
            return NativeGpuReading::Unavailable;
        }

        self.known_pids = observed_pids;
        let system_percent = per_engine_bucket.into_values().reduce(f64::max);
        let filtered: HashMap<u32, f64> = per_pid_all
            .iter()
            .filter(|(pid, _)| verified_pids.contains(pid))
            .map(|(&pid, &v)| (pid, v))
            .collect();
        self.cached = Some((system_percent, per_pid_all));
        NativeGpuReading::Available {
            system_percent,
            per_pid_percent: filtered,
        }
    }
}

fn sample_process_with(
    identity: &ProcessIdentity,
    now_100ns: u64,
    verify: impl Fn(HANDLE) -> bool,
) -> NativeProcessCounters {
    let empty = |status| NativeProcessCounters {
        status,
        cpu_100ns: None,
        working_set_bytes: None,
        private_memory_bytes: None,
        uptime_seconds: None,
    };

    if identity.creation_time.is_empty() {
        return empty(NativeProcessStatus::OwnershipMismatch);
    }

    let handle = match process_handle(
        identity.pid,
        PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
    ) {
        Ok(h) => h,
        Err(_) => {
            let err = unsafe { GetLastError() };
            return empty(if err == ERROR_INVALID_PARAMETER {
                NativeProcessStatus::Exited
            } else {
                NativeProcessStatus::Inaccessible
            });
        }
    };

    match unsafe { WaitForSingleObject(handle.0, 0) } {
        WAIT_OBJECT_0 => return empty(NativeProcessStatus::Exited),
        WAIT_TIMEOUT => {}
        _ => return empty(NativeProcessStatus::Inaccessible),
    }

    let mut created: FILETIME = unsafe { zeroed() };
    let mut exited: FILETIME = unsafe { zeroed() };
    let mut kernel: FILETIME = unsafe { zeroed() };
    let mut user: FILETIME = unsafe { zeroed() };
    if unsafe { GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        let exited_now = unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0;
        return empty(if exited_now {
            NativeProcessStatus::Exited
        } else {
            NativeProcessStatus::Inaccessible
        });
    }

    let created_ticks = filetime_to_u64(created);
    if created_ticks.to_string() != identity.creation_time {
        return empty(NativeProcessStatus::OwnershipMismatch);
    }
    if !same_session(identity.pid) || !verify(handle.0) {
        let exited_now = unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0;
        return empty(if exited_now {
            NativeProcessStatus::Exited
        } else {
            NativeProcessStatus::OwnershipMismatch
        });
    }

    let cpu_100ns = Some(filetime_to_u64(kernel).saturating_add(filetime_to_u64(user)));
    let uptime_seconds = Some(now_100ns.saturating_sub(created_ticks) / 10_000_000);

    let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { zeroed() };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    let mem_ok = unsafe {
        K32GetProcessMemoryInfo(
            handle.0,
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
            counters.cb,
        )
    } != 0;

    if !mem_ok && unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0 {
        return empty(NativeProcessStatus::Exited);
    }

    NativeProcessCounters {
        status: NativeProcessStatus::Alive,
        cpu_100ns,
        working_set_bytes: mem_ok.then_some(counters.WorkingSetSize as u64),
        private_memory_bytes: mem_ok.then_some(counters.PrivateUsage as u64),
        uptime_seconds,
    }
}

fn sample_resources_with(
    targets: &[ProcessIdentity],
    collect_gpu: bool,
    verify: impl Fn(HANDLE) -> bool + Copy,
) -> NativeResourceScan {
    let mut sys_info: SYSTEM_INFO = unsafe { zeroed() };
    unsafe {
        GetSystemInfo(&mut sys_info);
    }
    let logical_processors = sys_info.dwNumberOfProcessors.max(1);

    let mut idle: FILETIME = unsafe { zeroed() };
    let mut kernel: FILETIME = unsafe { zeroed() };
    let mut user: FILETIME = unsafe { zeroed() };
    let cpu_times = if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } != 0 {
        Some((
            filetime_to_u64(idle),
            filetime_to_u64(kernel),
            filetime_to_u64(user),
        ))
    } else {
        None
    };

    let mut mem_status: MEMORYSTATUSEX = unsafe { zeroed() };
    mem_status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    let (total_ram_bytes, available_ram_bytes) =
        if unsafe { GlobalMemoryStatusEx(&mut mem_status) } != 0 {
            (
                Some(mem_status.ullTotalPhys),
                Some(mem_status.ullAvailPhys),
            )
        } else {
            (None, None)
        };

    let now_100ns = system_now_100ns();
    let processes: Vec<NativeProcessCounters> = targets
        .iter()
        .map(|id| sample_process_with(id, now_100ns, verify))
        .collect();

    let (unmanaged_processes, inaccessible_processes) = match native_players(verify) {
        Ok(scan) => {
            let managed_keys: HashSet<(u32, &str)> = targets
                .iter()
                .map(|t| (t.pid, t.creation_time.as_str()))
                .collect();
            let unmanaged = scan
                .players
                .iter()
                .filter(|p| !managed_keys.contains(&(p.pid, p.creation_time.as_str())))
                .count();
            (Some(unmanaged), Some(scan.inaccessible))
        }
        Err(_) => (None, None),
    };

    let gpu = if collect_gpu {
        let verified_pids: HashSet<u32> = targets
            .iter()
            .zip(&processes)
            .filter(|(_, sample)| sample.status == NativeProcessStatus::Alive)
            .map(|(id, _)| id.pid)
            .collect();
        match GPU_PDH.lock() {
            Ok(mut guard) => guard.sample(&verified_pids),
            Err(_) => NativeGpuReading::Unavailable,
        }
    } else {
        reset_gpu_sampler();
        NativeGpuReading::Unavailable
    };

    NativeResourceScan {
        system: NativeSystemCounters {
            logical_processors,
            cpu_times,
            total_ram_bytes,
            available_ram_bytes,
        },
        processes,
        gpu,
        unmanaged_processes,
        inaccessible_processes,
    }
}

pub fn sample_native_resources(targets: &[ProcessIdentity], collect_gpu: bool) -> NativeResourceScan {
    sample_resources_with(targets, collect_gpu, valid_player_image)
}

#[cfg(test)]
pub(crate) fn sample_native_resources_fixture(
    targets: &[ProcessIdentity],
    collect_gpu: bool,
) -> NativeResourceScan {
    sample_resources_with(targets, collect_gpu, fixture_image)
}

#[cfg(test)]
pub(crate) fn reset_native_gpu_sampler_for_test() {
    reset_gpu_sampler();
}

#[cfg(test)]
mod resource_tests {
    use super::*;
    use std::os::windows::process::CommandExt;

    #[test]
    fn gpu_instance_name_parser_extracts_pid_and_engine_bucket() {
        assert_eq!(
            parse_gpu_engine_instance(
                "pid_4812_luid_0x00000000_0x00010A4B_phys_0_eng_0_engtype_3D"
            ),
            Some((4812, "luid_0x00000000_0x00010A4B_phys_0_eng_0".into()))
        );
        assert_eq!(parse_gpu_engine_instance("pid_abc_luid_0_eng_0"), None);
        assert_eq!(parse_gpu_engine_instance("other_1234"), None);
    }

    #[test]
    fn native_sampler_reads_limited_process_metrics_rejects_creation_mismatch_and_detects_exit() {
        struct Cleanup(Option<std::process::Child>, PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if let Some(child) = &mut self.0 {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                let _ = std::fs::remove_dir_all(&self.1);
            }
        }
        let root = std::env::temp_dir().join(format!("rbx-res-native-{}", Uuid::new_v4()));
        let versions = root.join("Roblox").join("Versions");
        std::fs::create_dir_all(&versions).unwrap();
        let exe = versions.join("RobloxPlayerBeta.exe");
        std::fs::copy(
            PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32")
                .join("cmd.exe"),
            &exe,
        )
        .unwrap();

        let child = std::process::Command::new(&exe)
            .args(["/C", "set /p fixture="])
            .creation_flags(0x08000000)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = child.id();
        let mut cleanup = Cleanup(Some(child), root);

        let handle = process_handle(pid, PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE).unwrap();
        let created = process_time(handle.0).unwrap();
        drop(handle);

        let valid = ProcessIdentity {
            pid,
            creation_time: created.clone(),
            tracker: "123456789012".into(),
            generation: Uuid::new_v4(),
        };
        let mismatched = ProcessIdentity {
            pid,
            creation_time: "999999999999999999".into(),
            tracker: "123456789012".into(),
            generation: Uuid::new_v4(),
        };

        let scan = sample_native_resources_fixture(&[valid.clone(), mismatched], true);
        assert!(scan.system.logical_processors >= 1);
        assert!(scan.system.cpu_times.is_some());
        assert!(scan.system.total_ram_bytes.unwrap_or(0) > 0);
        assert!(scan.system.available_ram_bytes.is_some());
        assert_eq!(scan.processes[0].status, NativeProcessStatus::Alive);
        assert!(scan.processes[0].cpu_100ns.is_some());
        assert!(scan.processes[0].working_set_bytes.unwrap_or(0) > 0);
        assert!(scan.processes[0].private_memory_bytes.unwrap_or(0) > 0);
        assert!(scan.processes[0].uptime_seconds.is_some());
        assert_eq!(
            scan.processes[1].status,
            NativeProcessStatus::OwnershipMismatch
        );
        assert!(scan.processes[1].working_set_bytes.is_none());

        // Real installation validator must reject temp fixture binary.
        let prod_scan = sample_native_resources(std::slice::from_ref(&valid), false);
        assert_eq!(
            prod_scan.processes[0].status,
            NativeProcessStatus::OwnershipMismatch
        );

        // Process exits -> must report Exited immediately.
        let mut child = cleanup.0.take().unwrap();
        let _ = child.kill();
        let _ = child.wait();
        let exited_scan = sample_native_resources_fixture(&[valid], false);
        assert_eq!(exited_scan.processes[0].status, NativeProcessStatus::Exited);
        assert!(exited_scan.processes[0].cpu_100ns.is_none());
        assert!(exited_scan.processes[0].working_set_bytes.is_none());
        reset_native_gpu_sampler_for_test();
    }

    #[test]
    #[ignore = "explicit native resource sampler benchmark for 0, 10, and 50 real Windows processes"]
    fn native_benchmark_0_10_50_real_fixture_processes() {
        struct Cleanup(Vec<std::process::Child>, PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for mut child in self.0.drain(..) {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                let _ = std::fs::remove_dir_all(&self.1);
            }
        }
        let root = std::env::temp_dir().join(format!("rbx-res-bench-{}", Uuid::new_v4()));
        let versions = root.join("Roblox").join("Versions");
        std::fs::create_dir_all(&versions).unwrap();
        let exe = versions.join("RobloxPlayerBeta.exe");
        std::fs::copy(
            PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32")
                .join("cmd.exe"),
            &exe,
        )
        .unwrap();

        let self_handle = unsafe { GetCurrentProcess() };
        let logical_cpus = sample_native_resources_fixture(&[], false)
            .system
            .logical_processors
            .max(1);
        let mut children = Vec::new();
        let mut identities = Vec::new();
        let mut cleanup = Cleanup(Vec::new(), root);

        for target_count in [0usize, 10, 50] {
            while children.len() < target_count {
                let child = std::process::Command::new(&exe)
                    .args(["/C", "set /p fixture="])
                    .creation_flags(0x08000000)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap();
                let pid = child.id();
                let handle =
                    process_handle(pid, PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE).unwrap();
                let created = process_time(handle.0).unwrap();
                identities.push(ProcessIdentity {
                    pid,
                    creation_time: created,
                    tracker: format!("1000000000{:02}", identities.len()),
                    generation: Uuid::new_v4(),
                });
                children.push(child);
            }

            let mut c0 = zeroed_filetime();
            let mut e0 = zeroed_filetime();
            let mut k0 = zeroed_filetime();
            let mut u0 = zeroed_filetime();
            unsafe {
                GetProcessTimes(self_handle, &mut c0, &mut e0, &mut k0, &mut u0);
            }
            let wall_start = Instant::now();
            const TICKS: usize = 5;
            for tick in 0..TICKS {
                let scan = sample_native_resources_fixture(&identities, tick == 0 || tick == TICKS - 1);
                assert_eq!(scan.processes.len(), target_count);
                for proc_scan in &scan.processes {
                    assert_eq!(proc_scan.status, NativeProcessStatus::Alive);
                    assert!(proc_scan.working_set_bytes.unwrap_or(0) > 0);
                }
            }
            let elapsed = wall_start.elapsed();
            let mut k1 = zeroed_filetime();
            let mut u1 = zeroed_filetime();
            unsafe {
                GetProcessTimes(self_handle, &mut c0, &mut e0, &mut k1, &mut u1);
            }
            let mut mem: PROCESS_MEMORY_COUNTERS_EX = unsafe { zeroed() };
            mem.cb = size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
            unsafe {
                K32GetProcessMemoryInfo(
                    self_handle,
                    (&mut mem as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
                    mem.cb,
                );
            }
            let cpu_100ns = (filetime_to_u64(k1) + filetime_to_u64(u1))
                .saturating_sub(filetime_to_u64(k0) + filetime_to_u64(u0));
            let self_cpu_pct = if elapsed.as_secs_f64() > 0.0 {
                (cpu_100ns as f64 / 10_000_000.0) / elapsed.as_secs_f64() / f64::from(logical_cpus)
                    * 100.0
            } else {
                0.0
            };
            let avg_tick_ms = (elapsed.as_secs_f64() * 1000.0) / TICKS as f64;
            let self_ws_mb = mem.WorkingSetSize as f64 / (1024.0 * 1024.0);
            println!(
                "NATIVE_RESOURCE_BENCH count={target_count} ticks={TICKS} avg_tick_ms={avg_tick_ms:.2} self_cpu_pct={self_cpu_pct:.3} self_ws_mb={self_ws_mb:.2}"
            );
            assert!(avg_tick_ms < 250.0, "sampling {target_count} processes exceeded budget: {avg_tick_ms:.2}ms");
        }
        cleanup.0 = children;
        reset_native_gpu_sampler_for_test();
    }
}

#[cfg(test)]
fn zeroed_filetime() -> FILETIME {
    unsafe { zeroed() }
}
