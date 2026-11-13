use super::*;
use crate::model::{Account, ProcessIdentity, Status};
use crate::platform::{
    NativeGpuReading, NativeProcessCounters, NativeProcessStatus, NativeSystemCounters,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

type ScriptedCollector =
    Box<dyn FnMut(&[ProcessIdentity], bool, usize) -> NativeResourceScan + Send>;

struct ScriptedBackend {
    calls: AtomicUsize,
    responder: StdMutex<ScriptedCollector>,
}

impl ScriptedBackend {
    fn new(
        f: impl FnMut(&[ProcessIdentity], bool, usize) -> NativeResourceScan + Send + 'static,
    ) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            responder: StdMutex::new(Box::new(f)),
        }
    }
}

impl MetricsBackend for ScriptedBackend {
    fn collect(&self, targets: &[ProcessIdentity], collect_gpu: bool) -> NativeResourceScan {
        let idx = self.calls.fetch_add(1, Ordering::AcqRel);
        (self.responder.lock().unwrap())(targets, collect_gpu, idx)
    }
}

fn running_account(id: &str, name: &str, pid: u32, creation_time: &str) -> Account {
    let mut acc = Account::new(id.into(), name.into());
    acc.status = Status::Running;
    acc.desired_running = true;
    acc.process = Some(ProcessIdentity {
        pid,
        creation_time: creation_time.into(),
        tracker: format!("9000{pid}"),
        generation: Uuid::new_v4(),
    });
    acc.running_since = Some(Utc::now() - chrono::Duration::seconds(120));
    acc
}

#[test]
fn cpu_normalization_divides_by_elapsed_and_logical_processors() {
    // 1.0 second elapsed, 1.0 CPU second consumed on an 8-logical-CPU machine => 12.50%
    assert_eq!(calculate_machine_cpu_percent(10_000_000, 1.0, 8), 12.5);
    // 4.0 CPU seconds in 1.0s across 8 logical CPUs => 50.0%
    assert_eq!(calculate_machine_cpu_percent(40_000_000, 1.0, 8), 50.0);
    // Zero/invalid elapsed or saturated delta safely clamped to 0..=100
    assert_eq!(calculate_machine_cpu_percent(10_000_000, 0.0, 8), 0.0);
    assert_eq!(calculate_machine_cpu_percent(u64::MAX, 1.0, 8), 100.0);
}

#[test]
fn managed_aggregation_memory_units_and_ownership_rejection() {
    let backend = Arc::new(ScriptedBackend::new(|_, _, _| unreachable!()));
    let monitor = ResourceMonitor::with_backend(backend);

    let a1 = running_account("1", "alpha", 101, "1000");
    let a2 = running_account("2", "bravo", 102, "2000");
    let reused = running_account("3", "charlie", 103, "3000");
    let exited = running_account("4", "delta", 104, "4000");
    let inaccessible = running_account("5", "echo", 105, "5000");
    let accounts = vec![
        a1.clone(),
        a2.clone(),
        reused.clone(),
        exited.clone(),
        inaccessible.clone(),
    ];

    let t0 = Instant::now();
    let t1 = t0 + Duration::from_secs(2);
    let wall = Utc::now();

    let scan0 = NativeResourceScan {
        system: NativeSystemCounters {
            logical_processors: 8,
            cpu_times: Some((100_000_000, 150_000_000, 50_000_000)),
            total_ram_bytes: Some(16 * 1024 * 1024 * 1024),
            available_ram_bytes: Some(8 * 1024 * 1024 * 1024),
        },
        processes: vec![
            NativeProcessCounters {
                status: NativeProcessStatus::Alive,
                cpu_100ns: Some(20_000_000),
                working_set_bytes: Some(512 * 1024 * 1024),
                private_memory_bytes: Some(400 * 1024 * 1024),
                uptime_seconds: Some(300),
            },
            NativeProcessCounters {
                status: NativeProcessStatus::Alive,
                cpu_100ns: Some(10_000_000),
                working_set_bytes: Some(256 * 1024 * 1024),
                private_memory_bytes: Some(200 * 1024 * 1024),
                uptime_seconds: Some(180),
            },
            NativeProcessCounters {
                status: NativeProcessStatus::OwnershipMismatch,
                cpu_100ns: Some(99_999_999), // Must never leak into aggregates
                working_set_bytes: Some(9_999_999),
                private_memory_bytes: Some(9_999_999),
                uptime_seconds: Some(99),
            },
            NativeProcessCounters {
                status: NativeProcessStatus::Exited,
                cpu_100ns: None,
                working_set_bytes: None,
                private_memory_bytes: None,
                uptime_seconds: None,
            },
            NativeProcessCounters {
                status: NativeProcessStatus::Inaccessible,
                cpu_100ns: None,
                working_set_bytes: None,
                private_memory_bytes: None,
                uptime_seconds: None,
            },
        ],
        gpu: NativeGpuReading::Pending,
        unmanaged_processes: Some(2),
        inaccessible_processes: Some(1),
    };

    let first = monitor.apply_scan(&accounts, 6, &scan0, wall, t0, 3);
    assert_eq!(first.system.cpu_availability, MetricAvailability::Pending);
    assert_eq!(first.system.ram_utilization_percent, Some(50.0));
    assert_eq!(first.aggregates.running_instances, 2);
    assert_eq!(first.aggregates.total_accounts, 6);
    assert_eq!(first.aggregates.exited_or_stale_instances, 3);
    assert_eq!(first.aggregates.unmanaged_roblox_processes, Some(2));
    assert_eq!(first.aggregates.inaccessible_roblox_processes, Some(1));
    assert_eq!(first.aggregates.working_set_bytes, 768 * 1024 * 1024);
    assert_eq!(
        first.aggregates.private_memory_bytes,
        Some(600 * 1024 * 1024)
    );
    assert_eq!(
        first.aggregates.cpu_availability,
        MetricAvailability::Pending
    );
    assert_eq!(
        first.accounts[2].availability,
        ProcessAvailability::OwnershipMismatch
    );
    assert_eq!(first.accounts[2].cpu_percent, None);
    assert_eq!(first.accounts[2].working_set_bytes, None);

    // Second tick (2.0s elapsed across 8 logical CPUs):
    // System: total delta = 160M, idle delta = 80M => 50.0%
    // Process 1: +16M 100ns (1.6s CPU / 2.0s / 8 = 10.0%)
    // Process 2: +8M 100ns (0.8s CPU / 2.0s / 8 = 5.0%)
    let mut gpu_map = HashMap::new();
    gpu_map.insert(101, 12.5);
    gpu_map.insert(102, 7.5);
    gpu_map.insert(9999, 80.0); // Unmanaged process must NOT affect managed GPU sum

    let scan1 = NativeResourceScan {
        system: NativeSystemCounters {
            logical_processors: 8,
            cpu_times: Some((180_000_000, 260_000_000, 100_000_000)),
            total_ram_bytes: Some(16 * 1024 * 1024 * 1024),
            available_ram_bytes: Some(4 * 1024 * 1024 * 1024),
        },
        processes: vec![
            NativeProcessCounters {
                status: NativeProcessStatus::Alive,
                cpu_100ns: Some(36_000_000),
                working_set_bytes: Some(500 * 1024 * 1024),
                private_memory_bytes: Some(400 * 1024 * 1024),
                uptime_seconds: Some(302),
            },
            NativeProcessCounters {
                status: NativeProcessStatus::Alive,
                cpu_100ns: Some(18_000_000),
                working_set_bytes: Some(300 * 1024 * 1024),
                private_memory_bytes: Some(250 * 1024 * 1024),
                uptime_seconds: Some(182),
            },
            NativeProcessCounters {
                status: NativeProcessStatus::OwnershipMismatch,
                cpu_100ns: None,
                working_set_bytes: None,
                private_memory_bytes: None,
                uptime_seconds: None,
            },
            NativeProcessCounters {
                status: NativeProcessStatus::Exited,
                cpu_100ns: None,
                working_set_bytes: None,
                private_memory_bytes: None,
                uptime_seconds: None,
            },
            NativeProcessCounters {
                status: NativeProcessStatus::Inaccessible,
                cpu_100ns: None,
                working_set_bytes: None,
                private_memory_bytes: None,
                uptime_seconds: None,
            },
        ],
        gpu: NativeGpuReading::Available {
            system_percent: Some(90.0),
            per_pid_percent: gpu_map,
        },
        unmanaged_processes: Some(1),
        inaccessible_processes: Some(1),
    };

    let second = monitor.apply_scan(
        &accounts,
        6,
        &scan1,
        wall + chrono::Duration::seconds(2),
        t1,
        4,
    );
    assert_eq!(second.system.cpu_percent, Some(50.0));
    assert_eq!(
        second.system.cpu_availability,
        MetricAvailability::Available
    );
    assert_eq!(second.system.ram_utilization_percent, Some(75.0));
    assert_eq!(second.system.gpu_percent, Some(90.0));
    assert_eq!(second.aggregates.cpu_percent, 15.0);
    assert_eq!(second.aggregates.working_set_bytes, 800 * 1024 * 1024);
    assert_eq!(second.aggregates.gpu_percent, Some(20.0));
    assert_eq!(second.accounts[0].cpu_percent, Some(10.0));
    assert_eq!(second.accounts[0].gpu_percent, Some(12.5));
    assert_eq!(second.accounts[1].cpu_percent, Some(5.0));
    assert_eq!(second.accounts[1].gpu_percent, Some(7.5));
}

#[test]
fn bounded_history_caps_at_60_and_prunes_inactive_accounts() {
    let backend = Arc::new(ScriptedBackend::new(|_, _, _| unreachable!()));
    let monitor = ResourceMonitor::with_backend(backend);

    let a1 = running_account("1", "alpha", 201, "100");
    let base_t = Instant::now();
    let wall = Utc::now();

    for i in 0..100u64 {
        let accounts = if i < 80 { vec![a1.clone()] } else { vec![] };
        let procs = if i < 80 {
            vec![NativeProcessCounters {
                status: NativeProcessStatus::Alive,
                cpu_100ns: Some(1000 + i * 100),
                working_set_bytes: Some(100_000_000),
                private_memory_bytes: Some(80_000_000),
                uptime_seconds: Some(i),
            }]
        } else {
            vec![]
        };
        let scan = NativeResourceScan {
            system: NativeSystemCounters {
                logical_processors: 4,
                cpu_times: Some((1000 + i * 10, 2000 + i * 20, 1000 + i * 10)),
                total_ram_bytes: Some(8_000_000_000),
                available_ram_bytes: Some(4_000_000_000),
            },
            processes: procs,
            gpu: NativeGpuReading::Unavailable,
            unmanaged_processes: Some(0),
            inaccessible_processes: Some(0),
        };
        monitor.apply_scan(
            &accounts,
            1,
            &scan,
            wall + chrono::Duration::seconds(i as i64),
            base_t + Duration::from_secs(i),
            1,
        );
    }

    assert_eq!(monitor.history(120).len(), HISTORY_CAPACITY);
    assert!(monitor.account_history("1", 60).is_empty());
}

#[test]
fn gpu_unavailable_and_empty_snapshots_remain_consistent() {
    let backend = Arc::new(ScriptedBackend::new(|_, _, _| unreachable!()));
    let monitor = ResourceMonitor::with_backend(backend);
    let scan = NativeResourceScan {
        system: NativeSystemCounters {
            logical_processors: 4,
            cpu_times: Some((100, 200, 100)),
            total_ram_bytes: Some(4_000_000_000),
            available_ram_bytes: Some(2_000_000_000),
        },
        processes: vec![],
        gpu: NativeGpuReading::Unavailable,
        unmanaged_processes: Some(0),
        inaccessible_processes: Some(0),
    };
    let snap = monitor.apply_scan(&[], 0, &scan, Utc::now(), Instant::now(), 1);
    assert_eq!(snap.aggregates.running_instances, 0);
    assert_eq!(snap.aggregates.cpu_percent, 0.0);
    assert_eq!(snap.aggregates.working_set_bytes, 0);
    assert_eq!(
        snap.aggregates.gpu_availability,
        MetricAvailability::Unavailable
    );
    assert_eq!(snap.aggregates.gpu_percent, None);
    assert!(snap.accounts.is_empty());
}

#[test]
fn pid_reuse_clears_baseline_so_reattached_account_starts_cleanly() {
    let backend = Arc::new(ScriptedBackend::new(|_, _, _| unreachable!()));
    let monitor = ResourceMonitor::with_backend(backend);
    let t0 = Instant::now();
    let wall = Utc::now();

    let orig = running_account("1", "alpha", 777, "created-1");
    let reused_same_pid = running_account("1", "alpha", 777, "created-2");

    let scan0 = NativeResourceScan {
        system: NativeSystemCounters {
            logical_processors: 4,
            cpu_times: Some((100, 200, 100)),
            total_ram_bytes: Some(8_000_000_000),
            available_ram_bytes: Some(4_000_000_000),
        },
        processes: vec![NativeProcessCounters {
            status: NativeProcessStatus::Alive,
            cpu_100ns: Some(50_000_000),
            working_set_bytes: Some(100),
            private_memory_bytes: Some(80),
            uptime_seconds: Some(10),
        }],
        gpu: NativeGpuReading::Unavailable,
        unmanaged_processes: Some(0),
        inaccessible_processes: Some(0),
    };
    monitor.apply_scan(&[orig], 1, &scan0, wall, t0, 1);

    // Same PID, new creation_time => baseline keyed by (pid, creation_time) MUST be treated as fresh Pending.
    let scan1 = NativeResourceScan {
        processes: vec![NativeProcessCounters {
            status: NativeProcessStatus::Alive,
            cpu_100ns: Some(10_000_000),
            working_set_bytes: Some(200),
            private_memory_bytes: Some(160),
            uptime_seconds: Some(1),
        }],
        ..scan0
    };
    let after = monitor.apply_scan(
        &[reused_same_pid],
        1,
        &scan1,
        wall + chrono::Duration::seconds(1),
        t0 + Duration::from_secs(1),
        1,
    );
    assert_eq!(after.accounts[0].cpu_percent, None);
    assert_eq!(after.accounts[0].availability, ProcessAvailability::Pending);
    assert_eq!(
        after.accounts[0].creation_time.as_deref(),
        Some("created-2")
    );
}

#[tokio::test]
async fn suspended_cold_demand_shares_single_sampler_across_concurrent_readers() {
    let backend = Arc::new(ScriptedBackend::new(|targets, _, idx| NativeResourceScan {
        system: NativeSystemCounters {
            logical_processors: 4,
            cpu_times: Some((
                100 + idx as u64 * 10,
                300 + idx as u64 * 40,
                100 + idx as u64 * 10,
            )),
            total_ram_bytes: Some(8_000_000_000),
            available_ram_bytes: Some(4_000_000_000),
        },
        processes: targets
            .iter()
            .map(|_| NativeProcessCounters {
                status: NativeProcessStatus::Alive,
                cpu_100ns: Some(1_000_000 + idx as u64 * 500_000),
                working_set_bytes: Some(256 * 1024 * 1024),
                private_memory_bytes: Some(200 * 1024 * 1024),
                uptime_seconds: Some(60),
            })
            .collect(),
        gpu: NativeGpuReading::Unavailable,
        unmanaged_processes: Some(0),
        inaccessible_processes: Some(0),
    }));
    let monitor = ResourceMonitor::with_backend(backend.clone());
    monitor.set_cold_warmup_for_test(Duration::from_millis(20));

    assert!(monitor.is_suspended());
    assert_eq!(monitor.sampling_mode(), SamplingMode::Suspended);

    let account = running_account("1", "alpha", 3001, "1000");
    let readers: Vec<_> = (0..10)
        .map(|_| {
            let m = monitor.clone();
            let a = account.clone();
            tokio::spawn(async move { m.snapshot_or_refresh(&[a], false, true, true).await })
        })
        .collect();
    for reader in readers {
        let snap = reader.await.unwrap();
        assert_eq!(snap.aggregates.running_instances, 1);
    }
    // Single-flight cache + cold warmup should trigger at most 2 backend collects across 10 readers.
    assert!(backend.calls.load(Ordering::Acquire) <= 2);

    // Lease toggles Active -> Suspended cleanly on Drop.
    let lease = monitor.lease();
    assert_eq!(monitor.sampling_mode(), SamplingMode::Active);
    drop(lease);
    assert_eq!(monitor.sampling_mode(), SamplingMode::OnDemand);
}

#[tokio::test]
async fn benchmark_0_10_50_simulated_verified_instances() {
    let backend = Arc::new(ScriptedBackend::new(|targets, _, idx| NativeResourceScan {
        system: NativeSystemCounters {
            logical_processors: 16,
            cpu_times: Some((
                1_000_000 + idx as u64 * 500_000,
                3_000_000 + idx as u64 * 1_500_000,
                1_000_000 + idx as u64 * 500_000,
            )),
            total_ram_bytes: Some(32 * 1024 * 1024 * 1024),
            available_ram_bytes: Some(16 * 1024 * 1024 * 1024),
        },
        processes: targets
            .iter()
            .enumerate()
            .map(|(i, _)| NativeProcessCounters {
                status: NativeProcessStatus::Alive,
                cpu_100ns: Some(10_000_000 + (idx as u64) * 1_000_000 + i as u64 * 10_000),
                working_set_bytes: Some(500 * 1024 * 1024),
                private_memory_bytes: Some(420 * 1024 * 1024),
                uptime_seconds: Some(120 + idx as u64),
            })
            .collect(),
        gpu: NativeGpuReading::Unavailable,
        unmanaged_processes: Some(0),
        inaccessible_processes: Some(0),
    }));
    let monitor = ResourceMonitor::with_backend(backend);

    for count in [0usize, 10, 50] {
        let accounts: Vec<_> = (0..count)
            .map(|i| {
                running_account(
                    &i.to_string(),
                    &format!("acct_{i}"),
                    10_000 + i as u32,
                    &format!("ts_{i}"),
                )
            })
            .collect();
        let start = Instant::now();
        let snap1 = monitor.sample_now(&accounts, false, true).await;
        tokio::time::sleep(Duration::from_millis(25)).await;
        let snap2 = monitor.sample_now(&accounts, false, true).await;
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(snap1.aggregates.running_instances, count);
        assert_eq!(snap2.aggregates.running_instances, count);
        assert_eq!(
            snap2.aggregates.working_set_bytes,
            (count as u64) * 500 * 1024 * 1024
        );
        println!(
            "SIMULATED_RESOURCE_BENCH count={count} total_ms={elapsed_ms:.2} sample_ms={}",
            snap2.metadata.last_sample_duration_ms
        );
    }
}
