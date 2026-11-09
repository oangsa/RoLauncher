use super::*;

fn current_player() -> Player {
    Player {
        pid: unsafe { GetCurrentProcessId() },
        creation_time: process_time(unsafe { GetCurrentProcess() }).unwrap(),
        tracker: "123456789012".into(),
    }
}
fn tracked(player: &Player) -> TrackedProcess {
    TrackedProcess {
        handle: process_handle(player.pid, PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE)
            .unwrap(),
        player: player.clone(),
    }
}

#[test]
fn verified_handle_evidence_survives_wmi_outage_but_new_unknown_processes_block_authority() {
    let player = current_player();
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut discovery = Discovery {
        tracked: vec![tracked(&player)],
        pending: Some(receiver),
        ..Default::default()
    };
    let observed = Player {
        tracker: String::new(),
        ..player.clone()
    };
    let report = discovery
        .observe(ProcessScan {
            players: vec![observed.clone()],
            ..Default::default()
        })
        .unwrap();
    assert!(report.metadata_pending);
    assert_eq!(report.verified().unwrap()[0].tracker, player.tracker);
    let unknown = Player {
        pid: player.pid + 1,
        ..observed.clone()
    };
    let report = discovery
        .observe(ProcessScan {
            players: vec![observed.clone(), unknown],
            ..Default::default()
        })
        .unwrap();
    assert!(report.verified().is_err());
    // A reused PID or changed creation time can never recover cached ownership.
    let reused = Player {
        creation_time: "different-start".into(),
        ..observed
    };
    let report = discovery
        .observe(ProcessScan {
            players: vec![reused],
            ..Default::default()
        })
        .unwrap();
    assert!(report.verified().is_err());
    assert!(discovery.tracked.is_empty());
    drop(sender);
}

#[test]
fn metadata_recovery_binds_only_the_process_observed_with_the_same_creation_time() {
    let player = current_player();
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(Ok(vec![tracked(&player)])).unwrap();
    let mut discovery = Discovery {
        tracked: Vec::new(),
        pending: Some(receiver),
        ..Default::default()
    };
    let report = discovery
        .observe(ProcessScan {
            players: vec![Player {
                tracker: String::new(),
                ..player.clone()
            }],
            ..Default::default()
        })
        .unwrap();
    assert!(!report.metadata_pending);
    assert_eq!(
        report.verified().unwrap()[0].creation_time,
        player.creation_time
    );
    let identity = player.identity(Uuid::new_v4());
    let watch = ExitWatch(
        process_handle(player.pid, PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE).unwrap(),
        identity.clone(),
    );
    assert!(watch.alive(&identity).unwrap());
    let mut stale = identity.clone();
    stale.generation = Uuid::new_v4();
    assert!(watch.alive(&stale).is_err());
    stale = identity;
    stale.tracker = "different".into();
    assert!(watch.alive(&stale).is_err());
}

#[test]
fn refreshed_tracker_conflict_revokes_cached_attribution_without_rebinding_an_account() {
    let old = current_player();
    let mut updated = old.clone();
    updated.tracker = "987654321012".into();
    let (sender, receiver) = std::sync::mpsc::channel();
    sender.send(Ok(vec![tracked(&updated)])).unwrap();
    let mut discovery = Discovery {
        tracked: vec![tracked(&old)],
        pending: Some(receiver),
        ..Default::default()
    };
    let report = discovery
        .observe(ProcessScan {
            players: vec![Player {
                tracker: String::new(),
                ..old.clone()
            }],
            ..Default::default()
        })
        .unwrap();
    let found = report.verified().unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].tracker, updated.tracker);
    assert_ne!(found[0].tracker, old.tracker);
}

#[test]
fn native_observation_of_the_fixture_never_confers_tracker_ownership() {
    let scan = native_players(fixture_image).unwrap();
    assert!(scan.players.iter().all(|p| p.tracker.is_empty()));
    let unknown = ProcessScan {
        inaccessible: 1,
        ..Default::default()
    };
    assert!(unknown.verified().is_err());
}

#[test]
#[ignore = "explicit native snapshot benchmark using only disposable test processes"]
fn lifecycle_native_discovery_latency() {
    use std::os::windows::process::CommandExt;
    struct Children(Vec<std::process::Child>, PathBuf);
    impl Drop for Children {
        fn drop(&mut self) {
            for child in &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }
    let root = std::env::temp_dir().join(format!("rbx-discovery-bench-{}", Uuid::new_v4()));
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
    let mut children = Children(Vec::new(), root);
    let mut reports = Vec::new();
    for count in [1, 10, 50] {
        while children.0.len() < count {
            children.0.push(
                std::process::Command::new(&exe)
                    .args(["/C", "set /p fixture="])
                    .creation_flags(0x08000000)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .spawn()
                    .unwrap(),
            );
        }
        let mut samples = Vec::new();
        for _ in 0..20 {
            let start = std::time::Instant::now();
            let scan = native_players(fixture_image).unwrap();
            assert!(
                children
                    .0
                    .iter()
                    .all(|child| scan.players.iter().any(|p| p.pid == child.id()))
            );
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        reports.push(
            serde_json::json!({"clients": count, "p50_ms": samples[10], "p95_ms": samples[18]}),
        );
    }
    println!("LIFECYCLE_NATIVE_DISCOVERY {}", serde_json::json!(reports));
}
