//! Read-only host and managed-instance resource monitoring service.
//! Consumes verified `ProcessIdentity` associations without mutating account
//! lifecycle state or performing privileged process operations.

use crate::model::{Account, ProcessIdentity, Status};
use crate::platform::{
    NativeGpuReading, NativeProcessStatus, NativeResourceScan, reset_native_resources,
    sample_native_resources,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify, broadcast};

pub const ACTIVE_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
pub const HISTORY_CAPACITY: usize = 60;
pub const STALE_THRESHOLD_MS: u64 = 5_000;
pub const DEMAND_HOLD_WINDOW: Duration = Duration::from_secs(10);
pub const COLD_CACHE_FRESHNESS: Duration = Duration::from_millis(2_000);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricAvailability {
    Available,
    Pending,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessAvailability {
    Available,
    Pending,
    Exited,
    Inaccessible,
    OwnershipMismatch,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotAvailability {
    Available,
    Partial,
    Pending,
    Unavailable,
    Stale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingMode {
    Active,
    OnDemand,
    Suspended,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ResourceMetadata {
    pub cpu_normalization: &'static str,
    pub cpu_formula: &'static str,
    pub memory_units: &'static str,
    pub gpu_normalization: &'static str,
    pub sample_interval_ms: u64,
    pub history_capacity: usize,
    pub stale_threshold_ms: u64,
    pub sampling_mode: SamplingMode,
    pub last_sample_duration_ms: u64,
}

impl Default for ResourceMetadata {
    fn default() -> Self {
        Self {
            cpu_normalization: "machine_percent",
            cpu_formula: "delta_cpu_time / (elapsed_wall_time * logical_processors) * 100",
            memory_units: "bytes",
            gpu_normalization: "gpu_engine_percent_sum_capped_100",
            sample_interval_ms: ACTIVE_SAMPLE_INTERVAL.as_millis() as u64,
            history_capacity: HISTORY_CAPACITY,
            stale_threshold_ms: STALE_THRESHOLD_MS,
            sampling_mode: SamplingMode::Suspended,
            last_sample_duration_ms: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SystemResources {
    pub logical_processors: u32,
    pub cpu_percent: Option<f64>,
    pub cpu_availability: MetricAvailability,
    pub total_ram_bytes: Option<u64>,
    pub available_ram_bytes: Option<u64>,
    pub used_ram_bytes: Option<u64>,
    pub ram_utilization_percent: Option<f64>,
    pub ram_availability: MetricAvailability,
    pub gpu_percent: Option<f64>,
    pub gpu_availability: MetricAvailability,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ManagedAggregates {
    pub running_instances: usize,
    pub total_accounts: usize,
    pub cpu_percent: f64,
    pub cpu_availability: MetricAvailability,
    pub working_set_bytes: u64,
    pub private_memory_bytes: Option<u64>,
    pub gpu_percent: Option<f64>,
    pub gpu_availability: MetricAvailability,
    pub unmanaged_roblox_processes: Option<usize>,
    pub inaccessible_roblox_processes: Option<usize>,
    pub exited_or_stale_instances: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ManagedInstanceResource {
    pub account_id: String,
    pub display_name: String,
    pub username: String,
    pub account_status: Status,
    pub pid: Option<u32>,
    pub creation_time: Option<String>,
    pub cpu_percent: Option<f64>,
    pub working_set_bytes: Option<u64>,
    pub private_memory_bytes: Option<u64>,
    pub gpu_percent: Option<f64>,
    pub gpu_availability: MetricAvailability,
    pub uptime_seconds: Option<u64>,
    pub sampled_at: DateTime<Utc>,
    pub availability: ProcessAvailability,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ResourceHistoryPoint {
    pub timestamp: DateTime<Utc>,
    pub system_cpu_percent: Option<f64>,
    pub ram_utilization_percent: Option<f64>,
    pub managed_cpu_percent: f64,
    pub managed_ram_bytes: u64,
    pub managed_gpu_percent: Option<f64>,
    pub running_instances: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AccountHistoryPoint {
    pub timestamp: DateTime<Utc>,
    pub cpu_percent: Option<f64>,
    pub working_set_bytes: Option<u64>,
    pub private_memory_bytes: Option<u64>,
    pub gpu_percent: Option<f64>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ResourceSnapshot {
    pub timestamp: DateTime<Utc>,
    pub sample_age_ms: u64,
    pub stale: bool,
    pub availability: SnapshotAvailability,
    pub system: SystemResources,
    pub aggregates: ManagedAggregates,
    pub accounts: Vec<ManagedInstanceResource>,
    pub metadata: ResourceMetadata,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ResourceSnapshotResponse {
    #[serde(flatten)]
    pub snapshot: ResourceSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<ResourceHistoryPoint>>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct AccountResourceResponse {
    pub timestamp: DateTime<Utc>,
    pub sample_age_ms: u64,
    pub stale: bool,
    pub metadata: ResourceMetadata,
    pub account: ManagedInstanceResource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<AccountHistoryPoint>>,
}

pub(crate) trait MetricsBackend: Send + Sync {
    fn collect(&self, targets: &[ProcessIdentity], collect_gpu: bool) -> NativeResourceScan;
    fn reset(&self) {}
}

struct NativeMetricsBackend;
impl MetricsBackend for NativeMetricsBackend {
    fn collect(&self, targets: &[ProcessIdentity], collect_gpu: bool) -> NativeResourceScan {
        sample_native_resources(targets, collect_gpu)
    }
    fn reset(&self) {
        reset_native_resources();
    }
}

#[derive(Clone, Copy)]
struct SystemCpuBaseline {
    idle_100ns: u64,
    kernel_100ns: u64,
    user_100ns: u64,
}

#[derive(Clone, Copy)]
struct ProcessCpuBaseline {
    cpu_100ns: u64,
    at: Instant,
}

#[derive(Default)]
struct MonitorState {
    latest: Option<ResourceSnapshot>,
    system_history: VecDeque<ResourceHistoryPoint>,
    account_history: HashMap<String, VecDeque<AccountHistoryPoint>>,
    prev_system_cpu: Option<SystemCpuBaseline>,
    prev_process_cpu: HashMap<(u32, String), ProcessCpuBaseline>,
    last_sample_mono: Option<Instant>,
    last_demand_mono: Option<Instant>,
    active_leases: usize,
    paused: bool,
    shutdown: bool,
}

struct MonitorInner {
    backend: Arc<dyn MetricsBackend>,
    state: RwLock<MonitorState>,
    sample_lock: Mutex<()>,
    events: broadcast::Sender<ResourceSnapshot>,
    wake: Notify,
    cold_warmup: StdMutex<Duration>,
}

/// Keeps active ~1s sampling running while held (e.g. an open SSE stream or view).
pub struct SamplingLease {
    inner: Arc<MonitorInner>,
}

impl Drop for SamplingLease {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.inner.state.write() {
            guard.active_leases = guard.active_leases.saturating_sub(1);
            if guard.active_leases == 0 {
                guard.last_demand_mono = Some(Instant::now());
            }
        }
        self.inner.wake.notify_one();
    }
}

#[derive(Clone)]
pub struct ResourceMonitor {
    inner: Arc<MonitorInner>,
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceMonitor {
    pub fn new() -> Self {
        Self::with_backend(Arc::new(NativeMetricsBackend))
    }

    pub(crate) fn with_backend(backend: Arc<dyn MetricsBackend>) -> Self {
        let (events, _) = broadcast::channel(32);
        Self {
            inner: Arc::new(MonitorInner {
                backend,
                state: RwLock::new(MonitorState::default()),
                sample_lock: Mutex::new(()),
                events,
                wake: Notify::new(),
                cold_warmup: StdMutex::new(Duration::from_millis(200)),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn set_cold_warmup_for_test(&self, duration: Duration) {
        *self.inner.cold_warmup.lock().unwrap() = duration;
    }

    pub fn shutdown(&self) {
        if let Ok(mut state) = self.inner.state.write() {
            state.shutdown = true;
            state.prev_system_cpu = None;
            state.prev_process_cpu.clear();
        }
        self.inner.backend.reset();
        self.inner.wake.notify_waiters();
    }

    pub fn lease(&self) -> SamplingLease {
        if let Ok(mut state) = self.inner.state.write() {
            state.active_leases += 1;
            state.paused = false;
            state.last_demand_mono = Some(Instant::now());
        }
        self.inner.wake.notify_one();
        SamplingLease {
            inner: self.inner.clone(),
        }
    }

    pub fn subscribe(&self) -> (SamplingLease, broadcast::Receiver<ResourceSnapshot>) {
        let lease = self.lease();
        let receiver = self.inner.events.subscribe();
        (lease, receiver)
    }

    pub fn sampling_mode(&self) -> SamplingMode {
        let state = self.inner.state.read().unwrap();
        if state.active_leases > 0 {
            SamplingMode::Active
        } else if state
            .last_demand_mono
            .is_some_and(|t| t.elapsed() < DEMAND_HOLD_WINDOW)
        {
            SamplingMode::OnDemand
        } else {
            SamplingMode::Suspended
        }
    }

    #[cfg(test)]
    pub(crate) fn is_suspended(&self) -> bool {
        let state = self.inner.state.read().unwrap();
        state.paused
            || (state.active_leases == 0
                && state
                    .last_demand_mono
                    .is_none_or(|t| t.elapsed() >= DEMAND_HOLD_WINDOW))
    }

    pub fn latest(&self, sustain_demand: bool) -> Option<ResourceSnapshot> {
        let mut state = self.inner.state.write().unwrap();
        let was_idle = state.paused
            || (state.active_leases == 0
                && state
                    .last_demand_mono
                    .is_none_or(|t| t.elapsed() >= DEMAND_HOLD_WINDOW));
        if sustain_demand {
            state.last_demand_mono = Some(Instant::now());
            state.paused = false;
            if was_idle {
                self.inner.wake.notify_one();
            }
        }
        let mode = if state.active_leases > 0 {
            SamplingMode::Active
        } else if state
            .last_demand_mono
            .is_some_and(|t| t.elapsed() < DEMAND_HOLD_WINDOW)
        {
            SamplingMode::OnDemand
        } else {
            SamplingMode::Suspended
        };
        state.latest.as_ref().map(|snap| with_freshness(snap, mode))
    }

    pub fn history(&self, seconds: usize) -> Vec<ResourceHistoryPoint> {
        let limit = seconds.clamp(1, HISTORY_CAPACITY);
        let state = self.inner.state.read().unwrap();
        let len = state.system_history.len();
        state
            .system_history
            .iter()
            .skip(len.saturating_sub(limit))
            .cloned()
            .collect()
    }

    pub fn account_history(&self, account_id: &str, seconds: usize) -> Vec<AccountHistoryPoint> {
        let limit = seconds.clamp(1, HISTORY_CAPACITY);
        let state = self.inner.state.read().unwrap();
        let Some(deque) = state.account_history.get(account_id) else {
            return Vec::new();
        };
        let len = deque.len();
        deque
            .iter()
            .skip(len.saturating_sub(limit))
            .cloned()
            .collect()
    }

    /// Returns a fresh snapshot (within 2s) or performs a centralized single sample.
    /// Does NOT spin up a separate sampler; optionally sustains the shared 1s loop
    /// for callers that are actively polling (`sustain_demand = true`).
    pub async fn snapshot_or_refresh(
        &self,
        accounts: &[Account],
        sustain_demand: bool,
        collect_gpu: bool,
        warmup_cold_delta: bool,
    ) -> ResourceSnapshot {
        if let Some(existing) = self.latest(sustain_demand)
            && existing.sample_age_ms <= COLD_CACHE_FRESHNESS.as_millis() as u64
            && !accounts_changed_from_snapshot(accounts, &existing)
        {
            return existing;
        }
        let _guard = self.inner.sample_lock.lock().await;
        if let Some(existing) = self.latest(sustain_demand)
            && existing.sample_age_ms <= COLD_CACHE_FRESHNESS.as_millis() as u64
            && !accounts_changed_from_snapshot(accounts, &existing)
        {
            return existing;
        }
        let was_cold = {
            let state = self.inner.state.read().unwrap();
            state.prev_system_cpu.is_none() || state.paused
        };
        let mut snapshot = self
            .collect_under_lock(accounts, collect_gpu, sustain_demand)
            .await;
        if warmup_cold_delta
            && was_cold
            && snapshot.system.cpu_availability == MetricAvailability::Pending
        {
            let warmup = *self.inner.cold_warmup.lock().unwrap();
            if !warmup.is_zero() {
                tokio::time::sleep(warmup).await;
                snapshot = self
                    .collect_under_lock(accounts, collect_gpu, sustain_demand)
                    .await;
            }
        }
        snapshot
    }

    pub async fn sample_now(
        &self,
        accounts: &[Account],
        collect_gpu: bool,
        sustain_demand: bool,
    ) -> ResourceSnapshot {
        let _guard = self.inner.sample_lock.lock().await;
        self.collect_under_lock(accounts, collect_gpu, sustain_demand)
            .await
    }

    async fn collect_under_lock(
        &self,
        accounts: &[Account],
        collect_gpu: bool,
        sustain_demand: bool,
    ) -> ResourceSnapshot {
        if sustain_demand {
            let mut state = self.inner.state.write().unwrap();
            state.last_demand_mono = Some(Instant::now());
            state.paused = false;
        }
        let accounts_owned = accounts.to_vec();
        let backend = self.inner.backend.clone();
        let targets: Vec<ProcessIdentity> = accounts_owned
            .iter()
            .filter_map(|a| a.process.clone())
            .collect();
        let sample_accounts: Vec<Account> = accounts_owned
            .iter()
            .filter(|a| a.process.is_some())
            .cloned()
            .collect();
        let total_accounts = accounts_owned.len();

        let started = Instant::now();
        let scan = tokio::task::spawn_blocking(move || backend.collect(&targets, collect_gpu))
            .await
            .unwrap_or_else(|_| NativeResourceScan {
                system: crate::platform::NativeSystemCounters {
                    logical_processors: 1,
                    cpu_times: None,
                    total_ram_bytes: None,
                    available_ram_bytes: None,
                },
                processes: vec![
                    crate::platform::NativeProcessCounters {
                        status: NativeProcessStatus::Inaccessible,
                        cpu_100ns: None,
                        working_set_bytes: None,
                        private_memory_bytes: None,
                        uptime_seconds: None,
                    };
                    sample_accounts.len()
                ],
                gpu: NativeGpuReading::Unavailable,
                unmanaged_processes: None,
                inaccessible_processes: None,
            });
        let duration_ms = started.elapsed().as_millis() as u64;
        self.apply_scan(
            &sample_accounts,
            total_accounts,
            &scan,
            Utc::now(),
            Instant::now(),
            duration_ms,
        )
    }

    pub(crate) fn apply_scan(
        &self,
        sampled_accounts: &[Account],
        total_accounts: usize,
        scan: &NativeResourceScan,
        now_wall: DateTime<Utc>,
        now_mono: Instant,
        sample_duration_ms: u64,
    ) -> ResourceSnapshot {
        let mut state = self.inner.state.write().unwrap();
        state.paused = false;
        let logical_processors = scan.system.logical_processors.max(1);

        let (sys_cpu_percent, sys_cpu_avail) = match scan.system.cpu_times {
            Some((idle, kernel, user)) => {
                let prev = state.prev_system_cpu;
                state.prev_system_cpu = Some(SystemCpuBaseline {
                    idle_100ns: idle,
                    kernel_100ns: kernel,
                    user_100ns: user,
                });
                if let Some(prev) = prev {
                    let prev_total = prev.kernel_100ns.saturating_add(prev.user_100ns);
                    let cur_total = kernel.saturating_add(user);
                    let total_delta = cur_total.saturating_sub(prev_total);
                    let idle_delta = idle.saturating_sub(prev.idle_100ns);
                    if total_delta > 0 {
                        let busy_delta = total_delta.saturating_sub(idle_delta);
                        let pct = round2(
                            (busy_delta as f64 / total_delta as f64 * 100.0).clamp(0.0, 100.0),
                        );
                        (Some(pct), MetricAvailability::Available)
                    } else {
                        (None, MetricAvailability::Pending)
                    }
                } else {
                    (None, MetricAvailability::Pending)
                }
            }
            None => (None, MetricAvailability::Unavailable),
        };

        let (total_ram, avail_ram, used_ram, ram_pct, ram_avail) =
            match (scan.system.total_ram_bytes, scan.system.available_ram_bytes) {
                (Some(total), Some(avail)) if total > 0 => {
                    let used = total.saturating_sub(avail);
                    let pct = round2((used as f64 / total as f64 * 100.0).clamp(0.0, 100.0));
                    (
                        Some(total),
                        Some(avail),
                        Some(used),
                        Some(pct),
                        MetricAvailability::Available,
                    )
                }
                _ => (None, None, None, None, MetricAvailability::Unavailable),
            };

        let (sys_gpu_percent, sys_gpu_avail, gpu_map) = match &scan.gpu {
            NativeGpuReading::Available {
                system_percent,
                per_pid_percent,
            } => (
                system_percent.map(|v| round2(v.clamp(0.0, 100.0))),
                MetricAvailability::Available,
                Some(per_pid_percent),
            ),
            NativeGpuReading::Pending => (None, MetricAvailability::Pending, None),
            NativeGpuReading::Unavailable => (None, MetricAvailability::Unavailable, None),
        };

        let mut next_baselines: HashMap<(u32, String), ProcessCpuBaseline> = HashMap::new();
        let mut active_account_ids = HashSet::new();
        let mut rows = Vec::with_capacity(sampled_accounts.len());
        let mut running_instances = 0usize;
        let mut exited_or_stale_instances = 0usize;
        let mut managed_cpu_percent = 0.0f64;
        let mut cpu_available_count = 0usize;
        let mut working_set_bytes = 0u64;
        let mut private_sum = 0u64;
        let mut private_count = 0usize;
        let mut gpu_sum = 0.0f64;
        let mut gpu_count = 0usize;

        for (account, reading) in sampled_accounts.iter().zip(scan.processes.iter()) {
            let Some(identity) = account.process.as_ref() else {
                continue;
            };
            active_account_ids.insert(account.id.clone());
            let key = (identity.pid, identity.creation_time.clone());
            let display_name = if account.alias.trim().is_empty() {
                account.username.clone()
            } else {
                account.alias.clone()
            };

            match reading.status {
                NativeProcessStatus::Alive => {
                    running_instances += 1;
                    let cpu_percent = if let Some(cur_100ns) = reading.cpu_100ns {
                        let prev = state.prev_process_cpu.get(&key).copied();
                        next_baselines.insert(
                            key.clone(),
                            ProcessCpuBaseline {
                                cpu_100ns: cur_100ns,
                                at: now_mono,
                            },
                        );
                        prev.and_then(|p| {
                            let elapsed_secs =
                                now_mono.saturating_duration_since(p.at).as_secs_f64();
                            if elapsed_secs > 0.01 && cur_100ns >= p.cpu_100ns {
                                let delta_100ns = cur_100ns - p.cpu_100ns;
                                Some(calculate_machine_cpu_percent(
                                    delta_100ns,
                                    elapsed_secs,
                                    logical_processors,
                                ))
                            } else {
                                None
                            }
                        })
                    } else {
                        None
                    };

                    if let Some(cpu) = cpu_percent {
                        managed_cpu_percent += cpu;
                        cpu_available_count += 1;
                    }
                    if let Some(ws) = reading.working_set_bytes {
                        working_set_bytes = working_set_bytes.saturating_add(ws);
                    }
                    if let Some(priv_bytes) = reading.private_memory_bytes {
                        private_sum = private_sum.saturating_add(priv_bytes);
                        private_count += 1;
                    }

                    let (proc_gpu, proc_gpu_avail) = match (sys_gpu_avail, gpu_map) {
                        (MetricAvailability::Available, Some(map)) => {
                            let val = round2(
                                map.get(&identity.pid)
                                    .copied()
                                    .unwrap_or(0.0)
                                    .clamp(0.0, 100.0),
                            );
                            gpu_sum += val;
                            gpu_count += 1;
                            (Some(val), MetricAvailability::Available)
                        }
                        (MetricAvailability::Pending, _) => (None, MetricAvailability::Pending),
                        _ => (None, MetricAvailability::Unavailable),
                    };

                    let availability = if cpu_percent.is_some() {
                        ProcessAvailability::Available
                    } else {
                        ProcessAvailability::Pending
                    };

                    let deque = state.account_history.entry(account.id.clone()).or_default();
                    if deque.len() >= HISTORY_CAPACITY {
                        deque.pop_front();
                    }
                    deque.push_back(AccountHistoryPoint {
                        timestamp: now_wall,
                        cpu_percent,
                        working_set_bytes: reading.working_set_bytes,
                        private_memory_bytes: reading.private_memory_bytes,
                        gpu_percent: proc_gpu,
                    });

                    rows.push(ManagedInstanceResource {
                        account_id: account.id.clone(),
                        display_name,
                        username: account.username.clone(),
                        account_status: account.status.clone(),
                        pid: Some(identity.pid),
                        creation_time: Some(identity.creation_time.clone()),
                        cpu_percent,
                        working_set_bytes: reading.working_set_bytes,
                        private_memory_bytes: reading.private_memory_bytes,
                        gpu_percent: proc_gpu,
                        gpu_availability: proc_gpu_avail,
                        uptime_seconds: reading.uptime_seconds.or_else(|| {
                            account.running_since.map(|since| {
                                now_wall.signed_duration_since(since).num_seconds().max(0) as u64
                            })
                        }),
                        sampled_at: now_wall,
                        availability,
                    });
                }
                NativeProcessStatus::Exited
                | NativeProcessStatus::Inaccessible
                | NativeProcessStatus::OwnershipMismatch => {
                    exited_or_stale_instances += 1;
                    let availability = match reading.status {
                        NativeProcessStatus::Exited => ProcessAvailability::Exited,
                        NativeProcessStatus::Inaccessible => ProcessAvailability::Inaccessible,
                        NativeProcessStatus::OwnershipMismatch => {
                            ProcessAvailability::OwnershipMismatch
                        }
                        NativeProcessStatus::Alive => unreachable!(),
                    };
                    rows.push(ManagedInstanceResource {
                        account_id: account.id.clone(),
                        display_name,
                        username: account.username.clone(),
                        account_status: account.status.clone(),
                        pid: Some(identity.pid),
                        creation_time: Some(identity.creation_time.clone()),
                        cpu_percent: None,
                        working_set_bytes: None,
                        private_memory_bytes: None,
                        gpu_percent: None,
                        gpu_availability: MetricAvailability::Unavailable,
                        uptime_seconds: None,
                        sampled_at: now_wall,
                        availability,
                    });
                }
            }
        }

        rows.sort_by(|a, b| {
            a.display_name
                .to_lowercase()
                .cmp(&b.display_name.to_lowercase())
                .then_with(|| a.account_id.cmp(&b.account_id))
        });

        state.prev_process_cpu = next_baselines;
        state
            .account_history
            .retain(|id, _| active_account_ids.contains(id));

        let managed_cpu_avail = if running_instances == 0 || cpu_available_count > 0 {
            MetricAvailability::Available
        } else if sys_cpu_avail == MetricAvailability::Unavailable {
            MetricAvailability::Unavailable
        } else {
            MetricAvailability::Pending
        };

        let managed_private_ram_bytes = if running_instances == 0 {
            Some(0)
        } else if private_count > 0 {
            Some(private_sum)
        } else {
            None
        };

        let (managed_gpu_percent, managed_gpu_avail) = match sys_gpu_avail {
            MetricAvailability::Available if running_instances == 0 => {
                (Some(0.0), MetricAvailability::Available)
            }
            MetricAvailability::Available if gpu_count > 0 => {
                (Some(round2(gpu_sum)), MetricAvailability::Available)
            }
            MetricAvailability::Pending => (None, MetricAvailability::Pending),
            _ => (None, MetricAvailability::Unavailable),
        };

        let sampling_mode = if state.active_leases > 0 {
            SamplingMode::Active
        } else if state
            .last_demand_mono
            .is_some_and(|t| t.elapsed() < DEMAND_HOLD_WINDOW)
        {
            SamplingMode::OnDemand
        } else {
            SamplingMode::Suspended
        };

        let managed_cpu_percent = round2(managed_cpu_percent.clamp(0.0, 100.0));

        let availability = if sys_cpu_avail == MetricAvailability::Unavailable
            && ram_avail == MetricAvailability::Unavailable
        {
            SnapshotAvailability::Unavailable
        } else if sys_cpu_avail == MetricAvailability::Pending
            || managed_cpu_avail == MetricAvailability::Pending
        {
            if exited_or_stale_instances > 0 {
                SnapshotAvailability::Partial
            } else {
                SnapshotAvailability::Pending
            }
        } else if exited_or_stale_instances > 0
            || scan.inaccessible_processes.unwrap_or(0) > 0
            || sys_cpu_avail != MetricAvailability::Available
            || ram_avail != MetricAvailability::Available
        {
            SnapshotAvailability::Partial
        } else {
            SnapshotAvailability::Available
        };

        if state.system_history.len() >= HISTORY_CAPACITY {
            state.system_history.pop_front();
        }
        state.system_history.push_back(ResourceHistoryPoint {
            timestamp: now_wall,
            system_cpu_percent: sys_cpu_percent,
            ram_utilization_percent: ram_pct,
            managed_cpu_percent,
            managed_ram_bytes: working_set_bytes,
            managed_gpu_percent,
            running_instances,
        });

        let snapshot = ResourceSnapshot {
            timestamp: now_wall,
            sample_age_ms: 0,
            stale: false,
            availability,
            system: SystemResources {
                logical_processors,
                cpu_percent: sys_cpu_percent,
                cpu_availability: sys_cpu_avail,
                total_ram_bytes: total_ram,
                available_ram_bytes: avail_ram,
                used_ram_bytes: used_ram,
                ram_utilization_percent: ram_pct,
                ram_availability: ram_avail,
                gpu_percent: sys_gpu_percent,
                gpu_availability: sys_gpu_avail,
            },
            aggregates: ManagedAggregates {
                running_instances,
                total_accounts,
                cpu_percent: managed_cpu_percent,
                cpu_availability: managed_cpu_avail,
                working_set_bytes,
                private_memory_bytes: managed_private_ram_bytes,
                gpu_percent: managed_gpu_percent,
                gpu_availability: managed_gpu_avail,
                unmanaged_roblox_processes: scan.unmanaged_processes,
                inaccessible_roblox_processes: scan.inaccessible_processes,
                exited_or_stale_instances,
            },
            accounts: rows,
            metadata: ResourceMetadata {
                sampling_mode,
                last_sample_duration_ms: sample_duration_ms,
                ..Default::default()
            },
        };

        state.latest = Some(snapshot.clone());
        state.last_sample_mono = Some(now_mono);
        drop(state);

        let _ = self.inner.events.send(snapshot.clone());
        snapshot
    }

    pub fn spawn(&self, engine: crate::engine::Engine) {
        let monitor = self.clone();
        tokio::spawn(async move {
            monitor.run_loop(engine).await;
        });
    }

    async fn run_loop(&self, engine: crate::engine::Engine) {
        while !engine.is_shutdown() {
            let active = {
                let mut state = self.inner.state.write().unwrap();
                if state.shutdown {
                    return;
                }
                let active = state.active_leases > 0
                    || state
                        .last_demand_mono
                        .is_some_and(|t| t.elapsed() < DEMAND_HOLD_WINDOW);
                if !active && !state.paused {
                    state.paused = true;
                    state.prev_system_cpu = None;
                    state.prev_process_cpu.clear();
                    drop(state);
                    self.inner.backend.reset();
                    false
                } else {
                    active
                }
            };

            if !active {
                tokio::select! {
                    _ = self.inner.wake.notified() => {},
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {},
                }
                continue;
            }

            let accounts = engine.snapshot().accounts;
            let _ = self.sample_now(&accounts, true, false).await;

            tokio::select! {
                _ = self.inner.wake.notified() => {},
                _ = tokio::time::sleep(ACTIVE_SAMPLE_INTERVAL) => {},
            }
        }
    }

    pub fn account_view(
        &self,
        account: &Account,
        snapshot: &ResourceSnapshot,
        include_history: bool,
        history_seconds: usize,
    ) -> AccountResourceResponse {
        let existing = snapshot
            .accounts
            .iter()
            .find(|a| a.account_id == account.id)
            .cloned();
        let entry = existing.unwrap_or_else(|| {
            let display_name = if account.alias.trim().is_empty() {
                account.username.clone()
            } else {
                account.alias.clone()
            };
            ManagedInstanceResource {
                account_id: account.id.clone(),
                display_name,
                username: account.username.clone(),
                account_status: account.status.clone(),
                pid: account.process.as_ref().map(|p| p.pid),
                creation_time: account.process.as_ref().map(|p| p.creation_time.clone()),
                cpu_percent: None,
                working_set_bytes: None,
                private_memory_bytes: None,
                gpu_percent: None,
                gpu_availability: MetricAvailability::Unavailable,
                uptime_seconds: None,
                sampled_at: snapshot.timestamp,
                availability: if account.status == Status::Stopped && account.process.is_none() {
                    ProcessAvailability::Exited
                } else {
                    ProcessAvailability::Unavailable
                },
            }
        });
        AccountResourceResponse {
            timestamp: snapshot.timestamp,
            sample_age_ms: snapshot.sample_age_ms,
            stale: snapshot.stale,
            metadata: snapshot.metadata.clone(),
            account: entry,
            history: include_history.then(|| self.account_history(&account.id, history_seconds)),
        }
    }
}

pub fn calculate_machine_cpu_percent(
    cpu_delta_100ns: u64,
    elapsed_secs: f64,
    logical_processors: u32,
) -> f64 {
    if elapsed_secs <= 0.0 || !elapsed_secs.is_finite() {
        return 0.0;
    }
    let cpus = f64::from(logical_processors.max(1));
    let cpu_secs = cpu_delta_100ns as f64 / 10_000_000.0;
    let raw = (cpu_secs / elapsed_secs / cpus) * 100.0;
    if raw.is_finite() {
        round2(raw.clamp(0.0, 100.0))
    } else {
        0.0
    }
}

fn round2(value: f64) -> f64 {
    if value.is_finite() {
        (value * 100.0).round() / 100.0
    } else {
        0.0
    }
}

fn with_freshness(snapshot: &ResourceSnapshot, sampling_mode: SamplingMode) -> ResourceSnapshot {
    let mut clone = snapshot.clone();
    let age_ms = Utc::now()
        .signed_duration_since(clone.timestamp)
        .num_milliseconds()
        .max(0) as u64;
    clone.sample_age_ms = age_ms;
    clone.stale = age_ms > STALE_THRESHOLD_MS;
    if clone.stale {
        clone.availability = SnapshotAvailability::Stale;
    }
    clone.metadata.sampling_mode = sampling_mode;
    clone
}

fn accounts_changed_from_snapshot(accounts: &[Account], snapshot: &ResourceSnapshot) -> bool {
    if accounts.len() != snapshot.aggregates.total_accounts {
        return true;
    }
    let current_pids: Vec<(&str, Option<u32>, &str)> = accounts
        .iter()
        .filter(|a| a.process.is_some())
        .map(|a| {
            let p = a.process.as_ref().unwrap();
            (a.id.as_str(), Some(p.pid), p.creation_time.as_str())
        })
        .collect();
    if current_pids.len() != snapshot.accounts.len() {
        return true;
    }
    current_pids.iter().any(|(id, pid, created)| {
        !snapshot.accounts.iter().any(|row| {
            row.account_id == *id
                && row.pid == *pid
                && row.creation_time.as_deref() == Some(*created)
        })
    })
}

pub fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.0} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub fn format_uptime(seconds: u64) -> String {
    if seconds >= 86400 {
        format!(
            "{}d {}h {}m",
            seconds / 86400,
            seconds % 86400 / 3600,
            seconds % 3600 / 60
        )
    } else if seconds >= 3600 {
        format!(
            "{}h {}m {}s",
            seconds / 3600,
            seconds % 3600 / 60,
            seconds % 60
        )
    } else if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests;
