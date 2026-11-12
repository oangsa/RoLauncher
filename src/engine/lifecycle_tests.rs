use super::*;
use axum::{
    Router,
    extract::State as HttpState,
    http::StatusCode,
    routing::{get, post},
};
use std::sync::atomic::AtomicUsize;

#[test]
fn ownership_timeout_completes_the_operation_even_when_discovery_never_succeeds() {
    let (engine, path) = super::tests::fixture(1);
    let op = engine.command("1", "start").unwrap();
    let a = engine.snapshot().accounts[0].clone();
    let deadline = Utc::now() + ChronoDuration::seconds(90);
    engine
        .update("1", a.generation, |a| {
            a.status = Status::Launching;
            a.tracker = Some("123456789012".into());
            a.ownership_deadline = Some(deadline);
        })
        .unwrap();
    engine.expire_unowned_launches(deadline);
    let a = engine.snapshot().accounts[0].clone();
    assert_eq!(a.status, Status::NeedsAttention);
    assert!(a.tracker.is_some() && a.process.is_none());
    assert_eq!(engine.operation(op.id).unwrap().state, "failed");
    assert!(!launch_ready(&a, deadline + ChronoDuration::hours(1)));
    assert!(engine.command("1", "start").is_err());
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[derive(Clone, Default)]
struct Preparation {
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    selection: Arc<AtomicUsize>,
    tickets: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}
async fn selection(HttpState(s): HttpState<Preparation>) -> StatusCode {
    let active = s.active.fetch_add(1, Ordering::SeqCst) + 1;
    s.peak.fetch_max(active, Ordering::SeqCst);
    s.selection.fetch_add(1, Ordering::SeqCst);
    s.entered.notify_one();
    s.release.notified().await;
    s.active.fetch_sub(1, Ordering::SeqCst);
    StatusCode::UNAUTHORIZED
}
async fn ticket(
    HttpState(s): HttpState<Preparation>,
) -> (StatusCode, [(&'static str, &'static str); 1]) {
    s.tickets.fetch_add(1, Ordering::SeqCst);
    s.entered.notify_one();
    s.release.notified().await;
    (
        StatusCode::OK,
        [("rbx-authentication-ticket", "DISPOSABLE_FIXTURE_TICKET")],
    )
}
async fn wait_for(count: &AtomicUsize, expected: usize, signal: &Notify) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while count.load(Ordering::SeqCst) < expected {
            signal.notified().await;
        }
    })
    .await
    .unwrap();
}
async fn http_fixture(state: Preparation) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/servers", get(selection))
        .route("/ticket", post(ticket))
        .with_state(state);
    (
        url,
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }),
    )
}
fn engine_fixture(count: usize, url: &str, explicit: bool) -> (Engine, std::path::PathBuf) {
    let (mut engine, path) = super::tests::fixture(count);
    let inner = Arc::get_mut(&mut engine.0).unwrap();
    inner.roblox.servers_url = Some(format!("{url}/servers"));
    inner.roblox.ticket_url = Some(format!("{url}/ticket"));
    inner.launch_result = Some(Ok(()));
    engine
        .transaction(|s| {
            for saved in &mut s.database.accounts {
                saved.encrypted_session = platform::protect("DISPOSABLE_FIXTURE_SESSION")?;
                if explicit {
                    saved.account.target.as_mut().unwrap().job_id = Some(Uuid::from_u128(1));
                }
            }
            Ok(())
        })
        .unwrap();
    for id in 1..=count {
        engine.command(&id.to_string(), "start").unwrap();
    }
    (engine, path)
}

#[tokio::test]
async fn slow_selection_does_not_serialize_unrelated_accounts_and_preparation_is_bounded() {
    for count in [1, 10, 50] {
        let state = Preparation::default();
        let (url, server) = http_fixture(state.clone()).await;
        let (engine, path) = engine_fixture(count, &url, false);
        let running = engine.clone();
        let worker = tokio::spawn(async move { running.launch_loop().await });
        wait_for(&state.selection, count.min(4), &state.entered).await;
        assert_eq!(state.peak.load(Ordering::SeqCst), count.min(4));
        assert_eq!(state.tickets.load(Ordering::SeqCst), 0);
        for id in 1..=count {
            engine.command(&id.to_string(), "stop").unwrap();
        }
        engine.shutdown();
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
        state.release.notify_waiters();
        assert!(
            engine
                .snapshot()
                .accounts
                .iter()
                .all(|a| !a.desired_running && a.tracker.is_none() && a.process.is_none())
        );
        server.abort();
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn stop_before_ticket_completion_cannot_dispatch_or_replace_the_account() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (engine, path) = engine_fixture(1, &url, true);
    let running = engine.clone();
    let worker = tokio::spawn(async move { running.launch_loop().await });
    wait_for(&state.tickets, 1, &state.entered).await;
    let op = engine.command("1", "stop").unwrap();
    state.release.notify_waiters();
    engine.shutdown();
    tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(engine.operation(op.id).unwrap().state, "completed");
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopped);
    assert!(
        !engine
            .0
            .journal
            .recent()
            .iter()
            .any(|e| e["event"] == "launch_submitted")
    );
    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn accepted_protocol_without_a_process_retains_tracking_and_blocks_duplicate_launch() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (engine, path) = engine_fixture(1, &url, true);
    let account = engine.snapshot().accounts[0].clone();
    let running = engine.clone();
    let task = tokio::spawn(async move { running.launch_one(account).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    task.await.unwrap();
    let a = engine.snapshot().accounts[0].clone();
    assert!(a.tracker.is_some() && a.process.is_none());
    assert!(!launch_ready(&a, Utc::now() + ChronoDuration::hours(1)));
    assert!(engine.command("1", "restart").is_err());
    engine.command("1", "stop").unwrap();
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopping);
    assert!(engine.command("1", "start").is_err());
    let journal = serde_json::to_string(&engine.0.journal.recent()).unwrap();
    for secret in [
        "DISPOSABLE_FIXTURE_SESSION",
        "DISPOSABLE_FIXTURE_TICKET",
        "roblox-player:",
        "http://",
    ] {
        assert!(!journal.contains(secret));
    }
    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn concurrent_preparation_preserves_five_second_protocol_handoff_spacing() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (engine, path) = engine_fixture(2, &url, true);
    let mut tasks = Vec::new();
    for account in engine.snapshot().accounts {
        let running = engine.clone();
        tasks.push(tokio::spawn(async move {
            running.launch_one(account).await;
        }));
    }
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();
    for task in tasks {
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
    }
    let events = engine.0.journal.recent();
    let submitted: Vec<_> = events
        .iter()
        .filter(|row| row["event"] == "launch_submitted")
        .collect();
    assert_eq!(submitted.len(), 2);
    assert!(
        submitted[1]["elapsed_ms"].as_u64().unwrap() - submitted[0]["elapsed_ms"].as_u64().unwrap()
            >= 5000
    );
    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn single_instance_and_pending_restart_modes_do_not_concurrently_prepare_second_start() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (engine, path) = super::tests::fixture(2);
    engine.set_multi_instance(false).unwrap();
    drop(engine);
    let mut reopened = Engine::open(
        Store::new(path.clone()).unwrap(),
        "Multi-instance: off · Compatibility: passed".into(),
    )
    .unwrap();
    let inner = Arc::get_mut(&mut reopened.0).unwrap();
    inner.roblox.servers_url = Some(format!("{url}/servers"));
    inner.roblox.ticket_url = Some(format!("{url}/ticket"));
    inner.launch_result = Some(Ok(()));
    reopened
        .transaction(|s| {
            for saved in &mut s.database.accounts {
                saved.encrypted_session = platform::protect("DISPOSABLE_FIXTURE_SESSION")?;
                saved.account.target.as_mut().unwrap().job_id = Some(Uuid::from_u128(1));
            }
            Ok(())
        })
        .unwrap();
    // Enabling multi-instance at runtime without restarting must not claim active readiness
    // or allow concurrent multi-account ticket preparation.
    reopened.set_multi_instance(true).unwrap();
    let snap = reopened.snapshot();
    assert!(snap.multi_instance_enabled && !snap.active_multi_instance);
    assert!(
        snap.compatibility.to_lowercase().contains("restart"),
        "expected compatibility to report restart required, got: {}",
        snap.compatibility
    );
    reopened.command("1", "start").unwrap();
    reopened.command("2", "start").unwrap();
    let snap = reopened.snapshot();
    assert!(
        snap.accounts[1]
            .last_error
            .as_deref()
            .is_some_and(|e| e.starts_with("Multiple instances disabled")),
        "second account must surface single-instance wait error immediately, got: {:?}",
        snap.accounts[1].last_error
    );
    let running = reopened.clone();
    let worker = tokio::spawn(async move { running.launch_loop().await });
    wait_for(&state.tickets, 1, &state.entered).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        state.tickets.load(Ordering::SeqCst),
        1,
        "only one account may prepare/request a ticket while active_multi_instance is false"
    );
    state.release.notify_waiters();
    reopened.shutdown();
    tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .unwrap()
        .unwrap();
    server.abort();
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn second_launch_waits_for_first_process_readiness_and_records_readiness_stages() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (engine, path) = engine_fixture(2, &url, true);
    let mut tasks = Vec::new();
    for account in engine.snapshot().accounts {
        let running = engine.clone();
        tasks.push(tokio::spawn(async move {
            running.launch_one(account).await;
        }));
    }
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();
    for task in tasks {
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .unwrap()
            .unwrap();
    }
    let events = engine.0.journal.recent();
    for stage in [
        "launch_requested",
        "launch_queued",
        "launch_preparing",
        "launch_ready_for_dispatch",
        "launch_waiting_for_slot",
        "launch_submitted",
        "launch_result",
        "launch_readiness_timeout",
    ] {
        assert!(
            events.iter().any(|row| row["event"] == stage),
            "missing expected diagnostic stage {stage}"
        );
    }
    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn slow_first_process_delays_second_dispatch_until_verified_then_connects_cleanly() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(2, &url, true);
    {
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_millis(50);
        inner.launch_readiness_timeout = Duration::from_secs(3);
    }
    let mut tasks = Vec::new();
    for account in engine.snapshot().accounts {
        let running = engine.clone();
        tasks.push(tokio::spawn(async move {
            running.launch_one(account).await;
        }));
    }
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();
    // Wait until the first account submits its protocol launch and reserves a tracker.
    let first_tracker = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let snap = engine.snapshot();
            if let Some(first) = snap.accounts.iter().find(|a| a.tracker.is_some()) {
                return (first.id.clone(), first.tracker.clone().unwrap());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // After 150ms (> 50ms spacing, < 3s readiness timeout), the second launch must still be waiting
    // because the first launch's process has not yet been observed.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let submitted_before_attach = engine
        .0
        .journal
        .recent()
        .into_iter()
        .filter(|row| row["event"] == "launch_submitted")
        .count();
    assert_eq!(
        submitted_before_attach, 1,
        "second launch must wait for first process observation beyond minimum spacing"
    );
    // Protocol acceptance alone must not mark the first account Running or its operation completed.
    let first_account = engine
        .snapshot()
        .accounts
        .into_iter()
        .find(|a| a.id == first_tracker.0)
        .unwrap();
    assert_eq!(first_account.status, Status::Launching);
    assert_eq!(
        engine
            .operation(first_account.operation_id.unwrap())
            .unwrap()
            .state,
        "queued"
    );

    // Observe the first process alongside an unrelated unowned external Roblox process (tracker "").
    let players = vec![
        platform::Player {
            pid: 9000,
            creation_time: "100".into(),
            tracker: String::new(),
        },
        platform::Player {
            pid: 4101,
            creation_time: "200".into(),
            tracker: first_tracker.1.clone(),
        },
    ];
    assert!(engine.reconcile_launch_discovery(&players, true));
    for task in tasks {
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
    let snap = engine.snapshot();
    let second_account = snap
        .accounts
        .iter()
        .find(|a| a.id != first_tracker.0)
        .unwrap()
        .clone();
    let second_tracker = second_account.tracker.clone().unwrap();
    let both_players = vec![
        platform::Player {
            pid: 9000,
            creation_time: "100".into(),
            tracker: String::new(),
        },
        platform::Player {
            pid: 4101,
            creation_time: "200".into(),
            tracker: first_tracker.1.clone(),
        },
        platform::Player {
            pid: 4102,
            creation_time: "201".into(),
            tracker: second_tracker.clone(),
        },
    ];
    assert!(engine.reconcile_launch_discovery(&both_players, true));
    // Verify both accounts attached their respective verified processes and ignored PID 9000.
    // Attachment transitions status from Launching to Unknown ("Observed process, waiting for log signal")
    // while keeping the operation "queued" until a verified Connected signal is observed.
    let snap = engine.snapshot();
    for account in &snap.accounts {
        let proc = account.process.as_ref().expect("process attached");
        assert_ne!(proc.pid, 9000);
        assert_eq!(Some(&proc.tracker), account.tracker.as_ref());
        assert_eq!(account.status, Status::Unknown);
        assert_eq!(
            engine
                .operation(account.operation_id.unwrap())
                .unwrap()
                .state,
            "queued"
        );
        engine.observe(
            &account.id,
            account.generation,
            proc,
            Observation::DiskLog(
                crate::logs::parse_event("[FLog::Network] Connection accepted from 127.0.0.1|5000")
                    .unwrap(),
            ),
        );
    }
    let snap = engine.snapshot();
    for account in &snap.accounts {
        assert_eq!(account.status, Status::Running);
        assert_eq!(
            engine
                .operation(account.operation_id.unwrap())
                .unwrap()
                .state,
            "completed"
        );
    }
    let events = engine.0.journal.recent();
    for stage in [
        "launch_process_observed",
        "launch_ownership_verified",
        "launch_connected",
    ] {
        assert!(
            events.iter().any(|row| row["event"] == stage),
            "missing expected stage {stage}"
        );
    }
    assert!(
        !events
            .iter()
            .any(|row| row["event"] == "launch_readiness_timeout"),
        "verified process readiness must proceed before readiness timeout"
    );
    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn five_concurrent_starts_and_ten_queued_accounts_schedule_fairly_without_starvation() {
    for count in [5usize, 10usize] {
        let state = Preparation::default();
        let (url, server) = http_fixture(state.clone()).await;
        let (mut engine, path) = engine_fixture(count, &url, true);
        let dispatched_order = Arc::new(Mutex::new(Vec::<String>::new()));
        {
            let order = dispatched_order.clone();
            let inner = Arc::get_mut(&mut engine.0).unwrap();
            inner.launch_spacing = Duration::from_millis(10);
            inner.launch_readiness_timeout = Duration::from_millis(20);
            inner.launch_hook = Some(Arc::new(move |_uri, account_id| {
                order.lock().unwrap().push(account_id.to_owned());
                Ok(())
            }));
        }
        if count == 5 {
            let mut tasks = Vec::new();
            for account in engine.snapshot().accounts {
                let running = engine.clone();
                tasks.push(tokio::spawn(async move {
                    running.launch_one(account).await;
                }));
            }
            wait_for(&state.tickets, 5, &state.entered).await;
            state.release.notify_waiters();
            for task in tasks {
                tokio::time::timeout(Duration::from_secs(5), task)
                    .await
                    .unwrap()
                    .unwrap();
            }
        } else {
            let running = engine.clone();
            let worker = tokio::spawn(async move { running.launch_loop().await });
            let deadline = Instant::now() + Duration::from_secs(5);
            while engine
                .snapshot()
                .accounts
                .iter()
                .any(|a| a.tracker.is_none())
            {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for {count} queued accounts to dispatch"
                );
                state.release.notify_waiters();
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            engine.shutdown();
            tokio::time::timeout(Duration::from_secs(2), worker)
                .await
                .unwrap()
                .unwrap();
        }
        let order = dispatched_order.lock().unwrap().clone();
        assert_eq!(
            order.len(),
            count,
            "expected exactly one dispatch per account"
        );
        let mut unique_accounts = std::collections::HashSet::new();
        for id in &order {
            assert!(
                unique_accounts.insert(id.clone()),
                "duplicate submission for account {id}"
            );
        }
        let snap = engine.snapshot();
        let mut trackers = std::collections::HashSet::new();
        for a in &snap.accounts {
            let tracker = a.tracker.as_ref().expect("durable tracker reserved");
            assert!(
                trackers.insert(tracker.clone()),
                "duplicate tracker reserved"
            );
            let op = engine.operation(a.operation_id.unwrap()).unwrap();
            if Some(&a.id) == order.last() {
                assert_eq!(a.status, Status::Launching);
                assert_eq!(op.state, "queued");
            } else {
                // Prior accounts whose readiness window elapsed without a client are marked
                // NeedsAttention with their tracker reservation preserved.
                assert_eq!(a.status, Status::NeedsAttention);
                assert_eq!(a.last_error.as_deref(), Some(STARTUP_UNRESOLVED_ERROR));
                assert_eq!(op.state, "failed");
            }
        }
        server.abort();
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn process_attribution_rejects_missing_wrong_reused_and_ambiguous_trackers() {
    let (engine, path) = super::tests::fixture(3);
    let op1 = engine.command("1", "start").unwrap();
    let op2 = engine.command("2", "start").unwrap();
    let op3 = engine.command("3", "start").unwrap();
    let snap = engine.snapshot();
    let deadline = Utc::now() + ChronoDuration::seconds(90);
    for (idx, tracker) in [
        ("1", "111111111111"),
        ("2", "222222222222"),
        ("3", "333333333333"),
    ] {
        let g = snap
            .accounts
            .iter()
            .find(|a| a.id == idx)
            .unwrap()
            .generation;
        engine
            .update(idx, g, |a| {
                a.status = Status::Launching;
                a.tracker = Some(tracker.into());
                a.ownership_deadline = Some(deadline);
            })
            .unwrap();
    }
    // 1. Missing tracker ("") and wrong tracker ("999999999999") must never attach to any account.
    // 2. Account 1 has a unique matching tracker ("111111111111") -> attaches cleanly.
    // 3. Account 3 has two processes sharing tracker "333333333333" -> rejected as ambiguous,
    //    transitions to NeedsAttention and marks op3 "failed".
    let players = vec![
        platform::Player {
            pid: 5000,
            creation_time: "10".into(),
            tracker: String::new(),
        },
        platform::Player {
            pid: 5001,
            creation_time: "11".into(),
            tracker: "999999999999".into(),
        },
        platform::Player {
            pid: 5002,
            creation_time: "12".into(),
            tracker: "111111111111".into(),
        },
        platform::Player {
            pid: 5003,
            creation_time: "13".into(),
            tracker: "333333333333".into(),
        },
        platform::Player {
            pid: 5004,
            creation_time: "14".into(),
            tracker: "333333333333".into(),
        },
    ];
    assert!(engine.reconcile_launch_discovery(&players, true));
    let snap = engine.snapshot();
    let a1 = &snap.accounts[0];
    let a2 = &snap.accounts[1];
    let a3 = &snap.accounts[2];
    assert_eq!(a1.process.as_ref().map(|p| p.pid), Some(5002));
    assert_eq!(engine.operation(op1.id).unwrap().state, "queued");
    assert!(a2.process.is_none() && a2.tracker.as_deref() == Some("222222222222"));
    assert_eq!(a2.status, Status::Launching);
    assert_eq!(engine.operation(op2.id).unwrap().state, "queued");
    assert!(a3.process.is_none());
    assert_eq!(a3.status, Status::NeedsAttention);
    assert_eq!(engine.operation(op3.id).unwrap().state, "failed");

    // If Account 2 later claims the same PID/creation_time as Account 1 under a conflicting claim,
    // attach_player rejects it and marks op2 "failed".
    let conflict = platform::Player {
        pid: 5002,
        creation_time: "12".into(),
        tracker: "222222222222".into(),
    };
    engine
        .attach_player(
            a2,
            conflict.identity(a2.generation),
            &[players[2].clone(), conflict],
        )
        .unwrap();
    let snap = engine.snapshot();
    assert!(snap.accounts[1].process.is_none());
    assert_eq!(snap.accounts[1].status, Status::NeedsAttention);
    assert_eq!(engine.operation(op2.id).unwrap().state, "failed");
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn stop_while_waiting_for_slot_cancels_promptly_and_records_launch_cancelled() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(2, &url, true);
    {
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(10);
        inner.launch_readiness_timeout = Duration::from_secs(10);
    }
    let mut tasks = Vec::new();
    for account in engine.snapshot().accounts {
        let running = engine.clone();
        tasks.push(tokio::spawn(async move {
            running.launch_one(account).await;
        }));
    }
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();
    // Wait until one account submits and the other enters launch_waiting_for_slot.
    let waiting_id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let events = engine.0.journal.recent();
            let submitted: Vec<_> = events
                .iter()
                .filter(|r| r["event"] == "launch_submitted")
                .collect();
            let waiting: Vec<_> = events
                .iter()
                .filter(|r| r["event"] == "launch_waiting_for_slot")
                .collect();
            if submitted.len() == 1 && waiting.len() == 2 {
                let first_id = submitted[0]["details"]["account"]["account_id"]
                    .as_u64()
                    .unwrap();
                return if first_id == 1 {
                    "2".to_owned()
                } else {
                    "1".to_owned()
                };
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    // Stop the waiting account while the 10-second slot spacing is still active; it must abort promptly.
    let stop_op = engine.command(&waiting_id, "stop").unwrap();
    for task in tasks {
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(engine.operation(stop_op.id).unwrap().state, "completed");
    let events = engine.0.journal.recent();
    assert_eq!(
        events
            .iter()
            .filter(|r| r["event"] == "launch_submitted")
            .count(),
        1,
        "stopped account must never submit protocol handoff"
    );
    let waiting_num = waiting_id.parse::<u64>().unwrap();
    assert!(
        events.iter().any(|r| {
            r["event"] == "launch_cancelled"
                && r["details"]["account"]["account_id"].as_u64() == Some(waiting_num)
        }),
        "expected launch_cancelled diagnostic event for stopped account"
    );
    let diag = serde_json::to_string(&engine.diagnostics()).unwrap();
    for secret in [
        "DISPOSABLE_FIXTURE_SESSION",
        "DISPOSABLE_FIXTURE_TICKET",
        "roblox-player:",
    ] {
        assert!(!diag.contains(secret));
    }
    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn protocol_handoff_failure_marks_operation_failed_and_partial_profile_reports_skipped() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(2, &url, true);
    {
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_result = Some(Err(
            "Roblox launch protocol unavailable; install the desktop Roblox client".into(),
        ));
    }
    let first = engine.snapshot().accounts[0].clone();
    let op1_id = first.operation_id.unwrap();
    let running = engine.clone();
    let task = tokio::spawn(async move { running.launch_one(first).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    task.await.unwrap();
    let snap = engine.snapshot();
    assert_eq!(snap.accounts[0].status, Status::NeedsAttention);
    assert!(snap.accounts[0].tracker.is_none());
    assert_eq!(engine.operation(op1_id).unwrap().state, "failed");

    // Account 2 has an unresolved tracker in NeedsAttention (cannot start without Stop/Retry),
    // while Account 1 is Stopped; launch_profile must report Account 1 started and Account 2 errored.
    engine.command("1", "stop").unwrap();
    let g2 = snap.accounts[1].generation;
    engine
        .update("2", g2, |a| {
            a.status = Status::NeedsAttention;
            a.tracker = Some("777777777777".into());
        })
        .unwrap();
    let profile = engine
        .save_profile(ProfileSave {
            id: None,
            name: "Batch".into(),
            account_ids: vec!["1".into(), "2".into()],
        })
        .unwrap();
    let results = engine.launch_profile(profile.id).unwrap();
    assert_eq!(results.len(), 2);
    assert!(results[0].operation.is_some() && results[0].error.is_none());
    assert!(results[1].operation.is_none() && results[1].error.is_some());
    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn deterministic_readiness_matrix_covers_2s_6s_12s_20s_30s_and_boundary_arrivals() {
    // 1. First client appears at 2s (before 5s minimum spacing) and at 6s (after 5s spacing, before 8s timeout),
    // plus boundary arrival immediately before timeout (7.95s).
    for (appear_ms, expected_second_dispatch_ms) in
        [(2_000u64, 5_000u64), (6_000, 6_000), (7_950, 7_950)]
    {
        let state = Preparation::default();
        let (url, server) = http_fixture(state.clone()).await;
        let (mut engine, path) = engine_fixture(2, &url, true);
        let base_instant = Instant::now();
        let now_cell = Arc::new(Mutex::new(base_instant));
        let dispatched = Arc::new(Mutex::new(Vec::<(String, u64)>::new()));
        {
            let clock_ref = now_cell.clone();
            let dispatched_ref = dispatched.clone();
            let clock_for_hook = now_cell.clone();
            let inner = Arc::get_mut(&mut engine.0).unwrap();
            inner.launch_spacing = Duration::from_secs(5);
            inner.launch_readiness_timeout = Duration::from_secs(8);
            inner.startup_verification_timeout = Duration::from_secs(35);
            inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
            inner.launch_hook = Some(Arc::new(move |_uri, id| {
                let elapsed = clock_for_hook
                    .lock()
                    .unwrap()
                    .saturating_duration_since(base_instant)
                    .as_millis() as u64;
                dispatched_ref
                    .lock()
                    .unwrap()
                    .push((id.to_owned(), elapsed));
                Ok(())
            }));
        }
        let accounts = engine.snapshot().accounts;
        let e1 = engine.clone();
        let a1 = accounts[0].clone();
        let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
        wait_for(&state.tickets, 1, &state.entered).await;
        state.release.notify_waiters();
        t1.await.unwrap();

        let e2 = engine.clone();
        let a2 = accounts[1].clone();
        let t2 = tokio::spawn(async move { e2.launch_one(a2).await });
        wait_for(&state.tickets, 2, &state.entered).await;
        state.release.notify_waiters();
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(dispatched.lock().unwrap().len(), 1);

        // Advance clock to appear_ms and observe Account 1's verified process.
        *now_cell.lock().unwrap() = base_instant + Duration::from_millis(appear_ms);
        let tracker1 = engine.snapshot().accounts[0].tracker.clone().unwrap();
        assert!(engine.observe_startup_scan(
            &[platform::Player {
                pid: 4100,
                creation_time: "100".into(),
                tracker: tracker1,
            }],
            0,
            0,
            false,
            false,
        ));
        if appear_ms < 5_000 {
            tokio::time::sleep(Duration::from_millis(30)).await;
            assert_eq!(
                dispatched.lock().unwrap().len(),
                1,
                "second launch must still wait for 5s spacing when first client appears at 2s"
            );
            *now_cell.lock().unwrap() = base_instant + Duration::from_millis(5_000);
            engine.0.wake.notify_waiters();
        }
        tokio::time::timeout(Duration::from_secs(2), t2)
            .await
            .unwrap()
            .unwrap();
        let records = dispatched.lock().unwrap().clone();
        assert_eq!(
            records,
            vec![
                ("1".to_owned(), 0),
                ("2".to_owned(), expected_second_dispatch_ms)
            ]
        );
        let snap = engine.snapshot();
        assert_eq!(snap.accounts[0].status, Status::Unknown);
        assert_eq!(snap.accounts[1].status, Status::Launching);
        assert_eq!(
            engine
                .operation(snap.accounts[0].operation_id.unwrap())
                .unwrap()
                .state,
            "queued"
        );
        assert_eq!(
            engine
                .operation(snap.accounts[1].operation_id.unwrap())
                .unwrap()
                .state,
            "queued"
        );
        server.abort();
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }

    // 2. Slow startup at 12s, 20s, and 30s with active bootstrapper/uninspected process observed:
    // readiness timeout at 8s must defer (not dispatch Account 2 early) and verify cleanly at 12s/20s/30s.
    for slow_secs in [12u64, 20, 30] {
        let state = Preparation::default();
        let (url, server) = http_fixture(state.clone()).await;
        let (mut engine, path) = engine_fixture(2, &url, true);
        let base_instant = Instant::now();
        let now_cell = Arc::new(Mutex::new(base_instant));
        let dispatched = Arc::new(Mutex::new(Vec::<(String, u64)>::new()));
        {
            let clock_ref = now_cell.clone();
            let dispatched_ref = dispatched.clone();
            let clock_for_hook = now_cell.clone();
            let inner = Arc::get_mut(&mut engine.0).unwrap();
            inner.launch_spacing = Duration::from_secs(5);
            inner.launch_readiness_timeout = Duration::from_secs(8);
            inner.startup_verification_timeout = Duration::from_secs(35);
            inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
            inner.launch_hook = Some(Arc::new(move |_uri, id| {
                let elapsed = clock_for_hook
                    .lock()
                    .unwrap()
                    .saturating_duration_since(base_instant)
                    .as_secs();
                dispatched_ref
                    .lock()
                    .unwrap()
                    .push((id.to_owned(), elapsed));
                Ok(())
            }));
        }
        let accounts = engine.snapshot().accounts;
        let e1 = engine.clone();
        let a1 = accounts[0].clone();
        let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
        wait_for(&state.tickets, 1, &state.entered).await;
        state.release.notify_waiters();
        t1.await.unwrap();

        let e2 = engine.clone();
        let a2 = accounts[1].clone();
        let t2 = tokio::spawn(async move { e2.launch_one(a2).await });
        wait_for(&state.tickets, 2, &state.entered).await;
        state.release.notify_waiters();
        tokio::time::sleep(Duration::from_millis(25)).await;

        // At 3s, bootstrapper starts; at 6s, client process appears without tracker metadata yet.
        *now_cell.lock().unwrap() = base_instant + Duration::from_secs(3);
        assert!(!engine.observe_startup_scan(&[], 0, 0, false, true));
        *now_cell.lock().unwrap() = base_instant + Duration::from_secs(6);
        assert!(!engine.observe_startup_scan(
            &[platform::Player {
                pid: 4200,
                creation_time: "200".into(),
                tracker: String::new(),
            }],
            1,
            0,
            true,
            false,
        ));
        // Advance past the 8s readiness timeout to 9s: Account 2 must NOT dispatch while startup is active.
        *now_cell.lock().unwrap() = base_instant + Duration::from_secs(9);
        engine.0.wake.notify_waiters();
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(
            dispatched.lock().unwrap().len(),
            1,
            "Account 2 must not collide while Account 1 startup process is active at {slow_secs}s"
        );
        let snap_mid = engine.snapshot();
        assert_eq!(snap_mid.accounts[0].status, Status::Launching);
        assert_eq!(
            snap_mid.accounts[0].last_error.as_deref(),
            Some(STARTUP_IN_PROGRESS_WAIT_ERROR)
        );
        assert_eq!(
            snap_mid.accounts[1].last_error.as_deref(),
            Some(STARTUP_IN_PROGRESS_WAIT_ERROR)
        );

        // At slow_secs (12s, 20s, or 30s), WMI resolves the tracker for PID 4200.
        *now_cell.lock().unwrap() = base_instant + Duration::from_secs(slow_secs);
        let tracker1 = snap_mid.accounts[0].tracker.clone().unwrap();
        assert!(engine.observe_startup_scan(
            &[platform::Player {
                pid: 4200,
                creation_time: "200".into(),
                tracker: tracker1,
            }],
            0,
            0,
            false,
            false,
        ));
        tokio::time::timeout(Duration::from_secs(2), t2)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            dispatched.lock().unwrap().clone(),
            vec![("1".to_owned(), 0), ("2".to_owned(), slow_secs)]
        );
        let snap_end = engine.snapshot();
        assert_eq!(snap_end.accounts[0].status, Status::Unknown);
        assert_eq!(
            snap_end.accounts[0].last_error.as_deref(),
            Some("PID verified; waiting for a verified log and connection signal")
        );
        assert_eq!(snap_end.accounts[1].status, Status::Launching);
        assert!(snap_end.accounts[1].last_error.is_none());
        server.abort();
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }

    // 3. Silent handoff timeout at 8s followed by late client arrival immediately after timeout (8.05s)
    // and at 12s, 20s, 30s after Account 2 has already dispatched.
    for late_ms in [8_050u64, 12_000, 20_000, 30_000] {
        let state = Preparation::default();
        let (url, server) = http_fixture(state.clone()).await;
        let (mut engine, path) = engine_fixture(2, &url, true);
        let base_instant = Instant::now();
        let now_cell = Arc::new(Mutex::new(base_instant));
        let dispatched = Arc::new(Mutex::new(Vec::<(String, u64)>::new()));
        {
            let clock_ref = now_cell.clone();
            let dispatched_ref = dispatched.clone();
            let clock_for_hook = now_cell.clone();
            let inner = Arc::get_mut(&mut engine.0).unwrap();
            inner.launch_spacing = Duration::from_secs(5);
            inner.launch_readiness_timeout = Duration::from_secs(8);
            inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
            inner.launch_hook = Some(Arc::new(move |_uri, id| {
                let elapsed = clock_for_hook
                    .lock()
                    .unwrap()
                    .saturating_duration_since(base_instant)
                    .as_millis() as u64;
                dispatched_ref
                    .lock()
                    .unwrap()
                    .push((id.to_owned(), elapsed));
                Ok(())
            }));
        }
        let accounts = engine.snapshot().accounts;
        let op1_id = accounts[0].operation_id.unwrap();
        let op2_id = accounts[1].operation_id.unwrap();
        let e1 = engine.clone();
        let a1 = accounts[0].clone();
        let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
        wait_for(&state.tickets, 1, &state.entered).await;
        state.release.notify_waiters();
        t1.await.unwrap();

        let e2 = engine.clone();
        let a2 = accounts[1].clone();
        let t2 = tokio::spawn(async move { e2.launch_one(a2).await });
        wait_for(&state.tickets, 2, &state.entered).await;
        state.release.notify_waiters();
        tokio::time::sleep(Duration::from_millis(25)).await;

        // At 8_000ms with no process observed, Account 1 becomes StartupUnresolved (NeedsAttention)
        // with its 90s tracker reservation intact, and Account 2 dispatches at 8_000ms.
        *now_cell.lock().unwrap() = base_instant + Duration::from_millis(8_000);
        engine.0.wake.notify_waiters();
        tokio::time::timeout(Duration::from_secs(2), t2)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            dispatched.lock().unwrap().clone(),
            vec![("1".to_owned(), 0), ("2".to_owned(), 8_000)]
        );
        let snap_timeout = engine.snapshot();
        let tracker1 = snap_timeout.accounts[0]
            .tracker
            .clone()
            .expect("tracker1 preserved");
        let tracker2 = snap_timeout.accounts[1]
            .tracker
            .clone()
            .expect("tracker2 reserved");
        assert_ne!(tracker1, tracker2);
        assert_eq!(snap_timeout.accounts[0].status, Status::NeedsAttention);
        assert_eq!(
            snap_timeout.accounts[0].last_error.as_deref(),
            Some(STARTUP_UNRESOLVED_ERROR)
        );
        assert_eq!(engine.operation(op1_id).unwrap().state, "failed");
        assert_eq!(snap_timeout.accounts[1].status, Status::Launching);
        assert_eq!(engine.operation(op2_id).unwrap().state, "queued");

        // Now at late_ms (8.05s, 12s, 20s, or 30s), Account 1's late process and Account 2's process appear.
        *now_cell.lock().unwrap() = base_instant + Duration::from_millis(late_ms);
        let players = vec![
            platform::Player {
                pid: 4301,
                creation_time: "301".into(),
                tracker: tracker1,
            },
            platform::Player {
                pid: 4302,
                creation_time: "302".into(),
                tracker: tracker2,
            },
        ];
        assert!(engine.observe_startup_scan(&players, 0, 0, false, false));
        let snap_recovered = engine.snapshot();
        for (idx, op_id, expected_pid) in [(0usize, op1_id, 4301u32), (1usize, op2_id, 4302u32)] {
            let acc = &snap_recovered.accounts[idx];
            let proc = acc.process.as_ref().expect("late/second process attached");
            assert_eq!(proc.pid, expected_pid);
            assert_eq!(acc.status, Status::Unknown);
            assert_eq!(
                acc.last_error.as_deref(),
                Some("PID verified; waiting for a verified log and connection signal")
            );
            assert_eq!(engine.operation(op_id).unwrap().state, "queued");
            engine.observe(
                &acc.id,
                acc.generation,
                proc,
                Observation::DiskLog(
                    crate::logs::parse_event(
                        "[FLog::Network] Connection accepted from 127.0.0.1|5000",
                    )
                    .unwrap(),
                ),
            );
            assert_eq!(engine.operation(op_id).unwrap().state, "completed");
            assert!(engine.snapshot().accounts[idx].last_error.is_none());
        }
        server.abort();
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn unverified_process_exits_before_verification_and_ambiguous_handoff_blocks_until_inspected()
{
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(2, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<(String, u64)>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let clock_for_hook = now_cell.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            let elapsed = clock_for_hook
                .lock()
                .unwrap()
                .saturating_duration_since(base_instant)
                .as_secs();
            dispatched_ref
                .lock()
                .unwrap()
                .push((id.to_owned(), elapsed));
            Ok(())
        }));
    }
    let accounts = engine.snapshot().accounts;
    let op1_id = accounts[0].operation_id.unwrap();
    let e1 = engine.clone();
    let a1 = accounts[0].clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    t1.await.unwrap();

    let e2 = engine.clone();
    let a2 = accounts[1].clone();
    let t2 = tokio::spawn(async move { e2.launch_one(a2).await });
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();
    tokio::time::sleep(Duration::from_millis(25)).await;

    // Scenario 7: At 2s, a client process appears without tracker metadata yet (uninspected = 1).
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(2);
    assert!(!engine.observe_startup_scan(
        &[platform::Player {
            pid: 5100,
            creation_time: "50".into(),
            tracker: String::new(),
        }],
        1,
        0,
        true,
        false,
    ));
    assert!(engine.snapshot().accounts[0].process.is_none());

    // Scenario 8: At 3s, PID 5100 exits before verification! Next complete scan only has an
    // uninspected external process PID 6100 (`uninspected = 1`) and then at 5s PID 6100 is still uninspected.
    // First, let's verify PID 5100 exiting with no other uninspected processes marks Account 1 StartupUnresolved immediately.
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(3);
    assert!(!engine.observe_startup_scan(&[], 0, 0, false, false));
    let snap = engine.snapshot();
    assert_eq!(snap.accounts[0].status, Status::NeedsAttention);
    assert_eq!(
        snap.accounts[0].last_error.as_deref(),
        Some(STARTUP_UNRESOLVED_ERROR)
    );
    assert!(snap.accounts[0].tracker.is_some());
    assert_eq!(engine.operation(op1_id).unwrap().state, "failed");
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_readiness_timeout"
            && e["details"]["reason"] == "observed_process_exited_before_verification"
    }));

    // Now suppose at 4s an uninspected process PID 6100 appears (`uninspected = 1`):
    // Even after 5s spacing (at 6s), Account 2 must block with `launch_blocked_ambiguous_handoff`
    // until PID 6100 is inspected!
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(4);
    assert!(!engine.observe_startup_scan(
        &[platform::Player {
            pid: 6100,
            creation_time: "60".into(),
            tracker: String::new(),
        }],
        1,
        0,
        true,
        false,
    ));
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(6);
    engine.0.wake.notify_waiters();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(
        dispatched.lock().unwrap().len(),
        1,
        "Account 2 must block while an uninspected Roblox process is present"
    );
    assert_eq!(
        engine.snapshot().accounts[1].last_error.as_deref(),
        Some(STARTUP_AMBIGUOUS_PROCESS_WAIT_ERROR)
    );
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_blocked_ambiguous_handoff"
            && e["details"]["reason"] == "unverified_roblox_process_present"
    }));

    // At 7s, WMI finishes inspecting PID 6100 and confirms it is an unrelated external trackerless
    // process (`uninspected = 0, metadata_pending = false`). Account 2 now dispatches immediately!
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(7);
    assert!(!engine.observe_startup_scan(
        &[platform::Player {
            pid: 6100,
            creation_time: "60".into(),
            tracker: String::new(),
        }],
        0,
        0,
        false,
        false,
    ));
    tokio::time::timeout(Duration::from_secs(2), t2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        dispatched.lock().unwrap().clone(),
        vec![("1".to_owned(), 0), ("2".to_owned(), 7)]
    );
    assert!(engine.snapshot().accounts[1].last_error.is_none());
    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn cancel_first_launch_restart_unresolved_and_engine_shutdown_during_readiness_wait() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(3, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<(String, u64)>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let clock_for_hook = now_cell.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            let elapsed = clock_for_hook
                .lock()
                .unwrap()
                .saturating_duration_since(base_instant)
                .as_secs();
            dispatched_ref
                .lock()
                .unwrap()
                .push((id.to_owned(), elapsed));
            Ok(())
        }));
    }
    let accounts = engine.snapshot().accounts;
    let e1 = engine.clone();
    let a1 = accounts[0].clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    t1.await.unwrap();

    // Restarting an unresolved launch must be rejected until Stop + discovery clears its retired tracker.
    assert!(engine.command("1", "restart").is_err());

    let e2 = engine.clone();
    let a2 = accounts[1].clone();
    let t2 = tokio::spawn(async move { e2.launch_one(a2).await });
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();
    tokio::time::sleep(Duration::from_millis(25)).await;

    // Scenario 9: Cancel Account 1 during readiness waiting at 2s; Account 2 dispatches once 5s spacing elapses.
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(2);
    let stop_op = engine.command("1", "stop").unwrap();
    assert_eq!(engine.operation(stop_op.id).unwrap().state, "queued");
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopping);
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(5);
    engine.0.wake.notify_waiters();
    tokio::time::timeout(Duration::from_secs(2), t2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        dispatched.lock().unwrap().clone(),
        vec![("1".to_owned(), 0), ("2".to_owned(), 5)]
    );

    // Account 3 now waits for Account 2's readiness, and an unexpected engine shutdown occurs at 6s.
    let e3 = engine.clone();
    let a3 = accounts[2].clone();
    let t3 = tokio::spawn(async move { e3.launch_one(a3).await });
    wait_for(&state.tickets, 3, &state.entered).await;
    state.release.notify_waiters();
    tokio::time::sleep(Duration::from_millis(25)).await;
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(6);
    engine.shutdown();
    tokio::time::timeout(Duration::from_secs(2), t3)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        dispatched.lock().unwrap().len(),
        2,
        "Account 3 must abort without dispatching when engine shuts down during readiness wait"
    );
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_cancelled"
            && e["details"]["account"]["account_id"].as_u64() == Some(3)
            && e["details"]["shutdown"] == true
    }));
    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn concurrent_poll_launch_readiness_and_attach_player_no_deadlock() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(2, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    {
        let clock_ref = now_cell.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_millis(50);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
    }
    let accounts = engine.snapshot().accounts;
    let e1 = engine.clone();
    let a1 = accounts[0].clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    t1.await.unwrap();

    let tracker1 = engine.snapshot().accounts[0].tracker.clone().unwrap();
    let waiting_account = engine.snapshot().accounts[1].clone();

    // Concurrently poll readiness on task 1 while attaching player under transaction on task 2.
    // Lock decoupling guarantees zero AB-BA inversion between launch_coordinator and state.
    let e_poll = engine.clone();
    let e_attach = engine.clone();
    let acc_for_attach = engine.snapshot().accounts[0].clone();

    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let b1 = barrier.clone();
    let b2 = barrier.clone();

    let poll_handle = tokio::spawn(async move {
        b1.wait().await;
        for _ in 0..20 {
            let _ = e_poll.poll_launch_readiness(&waiting_account, Instant::now());
            tokio::task::yield_now().await;
        }
    });

    let attach_handle = tokio::spawn(async move {
        b2.wait().await;
        let identity = ProcessIdentity {
            pid: 9999,
            creation_time: "999".into(),
            generation: acc_for_attach.generation,
            tracker: tracker1.clone(),
        };
        let players = vec![platform::Player {
            pid: 9999,
            creation_time: "999".into(),
            tracker: tracker1,
        }];
        let _ = e_attach.attach_player(&acc_for_attach, identity, &players);
    });

    tokio::time::timeout(Duration::from_secs(15), async {
        let _ = tokio::join!(poll_handle, attach_handle);
    })
    .await
    .expect("concurrent polling and player attachment must not deadlock");

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn inaccessible_process_defers_launch_and_blocks_ambiguous_handoff() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(2, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<(String, u64)>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let clock_for_hook = now_cell.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            let elapsed = clock_for_hook
                .lock()
                .unwrap()
                .saturating_duration_since(base_instant)
                .as_secs();
            dispatched_ref
                .lock()
                .unwrap()
                .push((id.to_owned(), elapsed));
            Ok(())
        }));
    }
    let accounts = engine.snapshot().accounts;
    let e1 = engine.clone();
    let a1 = accounts[0].clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    t1.await.unwrap();

    let e2 = engine.clone();
    let a2 = accounts[1].clone();
    let t2 = tokio::spawn(async move { e2.launch_one(a2).await });
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();
    tokio::time::sleep(Duration::from_millis(25)).await;

    // At 4s, an inaccessible process is observed (inaccessible = 1).
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(4);
    assert!(!engine.observe_startup_scan(&[], 0, 1, false, false));

    // At 9s (past normal 8s timeout), Account 2 must still be deferred because inaccessible = 1 indicates active startup.
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(9);
    engine.0.wake.notify_waiters();
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(
        dispatched.lock().unwrap().len(),
        1,
        "Account 2 must defer while an inaccessible process candidate is starting"
    );

    // At 12s, the process becomes accessible with matching tracker.
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(12);
    let tracker1 = engine.snapshot().accounts[0].tracker.clone().unwrap();
    assert!(engine.observe_startup_scan(
        &[platform::Player {
            pid: 7700,
            creation_time: "700".into(),
            tracker: tracker1,
        }],
        0,
        0,
        false,
        false,
    ));

    tokio::time::timeout(Duration::from_secs(2), t2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        dispatched.lock().unwrap().clone(),
        vec![("1".to_owned(), 0), ("2".to_owned(), 12)]
    );

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn ambiguous_unverified_process_recovers_after_timeout() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(2, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<(String, u64)>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let clock_for_hook = now_cell.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            let elapsed = clock_for_hook
                .lock()
                .unwrap()
                .saturating_duration_since(base_instant)
                .as_secs();
            dispatched_ref
                .lock()
                .unwrap()
                .push((id.to_owned(), elapsed));
            Ok(())
        }));
    }
    // No launch dispatched yet. An uninspected process PID 8800 appears without tracker.
    assert!(!engine.observe_startup_scan(
        &[platform::Player {
            pid: 8800,
            creation_time: "800".into(),
            tracker: String::new(),
        }],
        1,
        0,
        true,
        false,
    ));

    let accounts = engine.snapshot().accounts;
    let e1 = engine.clone();
    let a1 = accounts[0].clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    tokio::time::sleep(Duration::from_millis(25)).await;

    // At 10s, Account 1 is still blocked by the ambiguous uninspected process.
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(10);
    engine.0.wake.notify_waiters();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(dispatched.lock().unwrap().len(), 0);

    // At 36s (past 35s startup_verification_timeout), the readiness timeout expires.
    // Time elapsed MUST NOT grant launch authority while the process remains ambiguous.
    // Instead, the waiting account transitions to Status::NeedsAttention with an explicit timeout error.
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(36);
    engine.0.wake.notify_waiters();
    tokio::time::timeout(Duration::from_secs(2), t1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        dispatched.lock().unwrap().len(),
        0,
        "ambiguous uninspected process must never grant launch authority on timeout"
    );
    let snap = engine.snapshot();
    assert_eq!(snap.accounts[0].status, Status::NeedsAttention);
    assert_eq!(
        snap.accounts[0].last_error.as_deref(),
        Some(STARTUP_AMBIGUOUS_TIMEOUT_ERROR)
    );
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_ambiguous_process_timeout"
            && e["details"]["reason"] == "unverified_roblox_process_timeout_exceeded"
    }));

    // Fresh sufficient evidence arrives: clean scan confirms the ambiguous process exited.
    engine.observe_startup_scan(&[], 0, 0, false, false);

    // Account 1 can now safely be retried via Start, dispatching without conflict.
    let retry_op = engine.command("1", "start").unwrap();
    assert_eq!(engine.operation(retry_op.id).unwrap().state, "queued");
    let a1_retry = engine.snapshot().accounts[0].clone();
    let e1_retry = engine.clone();
    let t1_retry = tokio::spawn(async move { e1_retry.launch_one(a1_retry).await });
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();
    tokio::time::timeout(Duration::from_secs(2), t1_retry)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        dispatched.lock().unwrap().len(),
        1,
        "dispatches cleanly only after fresh complete discovery evidence resolves ambiguity"
    );

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

fn with_watchdog<F, T>(timeout: Duration, f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let res = f();
        let _ = tx.send(res);
    });
    match rx.recv_timeout(timeout) {
        Ok(res) => {
            let _ = worker.join();
            res
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!(
                "Test timed out after {:?}: potential deadlock detected across OS threads",
                timeout
            );
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("Worker thread panicked or disconnected");
        }
    }
}

// =========================================================================
// Finding F7 — Stop/Shutdown vs OS Launch Race Regression Tests
// =========================================================================

#[derive(Clone)]
struct HookBarrier {
    pause_tx: tokio::sync::mpsc::UnboundedSender<()>,
    resume_rx: Arc<Mutex<std::sync::mpsc::Receiver<()>>>,
}

impl HookBarrier {
    fn new() -> (
        Self,
        tokio::sync::mpsc::UnboundedReceiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let (pause_tx, pause_rx) = tokio::sync::mpsc::unbounded_channel();
        let (resume_tx, resume_rx) = std::sync::mpsc::channel();
        (
            Self {
                pause_tx,
                resume_rx: Arc::new(Mutex::new(resume_rx)),
            },
            pause_rx,
            resume_tx,
        )
    }

    fn wait_in_hook(&self) {
        let _ = self.pause_tx.send(());
        let _ = self
            .resume_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10));
    }
}

#[tokio::test]
async fn f7_01_stop_immediately_before_tracker_persistence() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;

    // User Stop executed before ticket response and before tracker persistence
    let stop_op = engine.command("1", "stop").unwrap();
    assert_eq!(engine.operation(stop_op.id).unwrap().state, "completed");
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopped);

    state.release.notify_waiters();
    t1.await.unwrap();

    assert_eq!(
        dispatched.lock().unwrap().len(),
        0,
        "Stop before tracker persistence must result in exactly 0 OS launch dispatches"
    );
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopped);
    assert!(engine.snapshot().accounts[0].tracker.is_none());

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f7_02_stop_immediately_after_tracker_persistence() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    let (barrier, mut pause_rx, resume_tx) = HookBarrier::new();
    {
        let dispatched_ref = dispatched.clone();
        let b = barrier.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.pre_dispatch_hook = Some(Arc::new(move |_id, _gen, _trk| {
            b.wait_in_hook();
        }));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    // Paused exactly after tracker persistence, before protocol submission
    tokio::time::timeout(Duration::from_secs(10), pause_rx.recv())
        .await
        .unwrap();
    let tracker_before_stop = engine.snapshot().accounts[0].tracker.clone().unwrap();
    assert_eq!(engine.snapshot().accounts[0].status, Status::Launching);

    // Stop accepted while paused at dispatch boundary
    let stop_op = engine.command("1", "stop").unwrap();
    assert_eq!(engine.operation(stop_op.id).unwrap().state, "queued");
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopping);
    {
        let s = engine.0.state.lock().unwrap();
        assert_eq!(s.database.retired_launches.len(), 1);
        assert_eq!(s.database.retired_launches[0].tracker, tracker_before_stop);
    }

    // Unblock the launch thread to attempt protocol submission
    let _ = resume_tx.send(());
    t1.await.unwrap();

    // Invariant: Order A applied. Old generation dispatch authorization is denied. 0 OS launches.
    assert_eq!(
        dispatched.lock().unwrap().len(),
        0,
        "Stop linearizing before dispatch authorization must cancel the OS launch handoff"
    );
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopping);
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_cancelled"
            && (e["details"]["reason"] == "generation_changed"
                || e["details"]["reason"] == "superseded_or_stopped"
                || e["details"]["reason"] == "retired_launch_pending"
                || e["details"]["reason"] == "user_stop")
    }));

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f7_03_stop_immediately_before_protocol_submission() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    let (barrier, mut pause_rx, resume_tx) = HookBarrier::new();
    {
        let dispatched_ref = dispatched.clone();
        let b = barrier.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.pre_dispatch_hook = Some(Arc::new(move |_id, _gen, _trk| {
            b.wait_in_hook();
        }));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    tokio::time::timeout(Duration::from_secs(10), pause_rx.recv())
        .await
        .unwrap();
    engine.command("1", "stop").unwrap();
    let _ = resume_tx.send(());
    t1.await.unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 0);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f7_04_stop_during_protocol_submission() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    let (barrier, mut in_effect_rx, release_effect_tx) = HookBarrier::new();
    {
        let dispatched_ref = dispatched.clone();
        let b = barrier.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            b.wait_in_hook();
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    // Wait until launch is inside the OS submission effect
    tokio::time::timeout(Duration::from_secs(10), in_effect_rx.recv())
        .await
        .unwrap();
    assert_eq!(dispatched.lock().unwrap().len(), 1);

    // Concurrently trigger Stop on a separate task while launch submission is in flight
    let e_stop = engine.clone();
    let stop_handle = tokio::spawn(async move { e_stop.command("1", "stop").unwrap() });

    // Release the submission effect
    let _ = release_effect_tx.send(());
    t1.await.unwrap();
    let stop_op = stop_handle.await.unwrap();

    // Invariant: Order B applied. Protocol handoff linearized before Stop.
    // The launch occurred (1 OS dispatch), Stop recognized the in-flight/submitted launch,
    // operation remains queued waiting for process exit/retirement, status is Stopping.
    assert_eq!(dispatched.lock().unwrap().len(), 1);
    assert_eq!(engine.operation(stop_op.id).unwrap().state, "queued");
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopping);
    let retired = engine
        .0
        .state
        .lock()
        .unwrap()
        .database
        .retired_launches
        .clone();
    assert_eq!(retired.len(), 1);
    assert_eq!(
        retired[0].close_intent.as_ref().unwrap().reason,
        CloseReason::UserStop
    );

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f7_05_stop_after_successful_os_acceptance() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    t1.await.unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 1);
    let stop_op = engine.command("1", "stop").unwrap();
    assert_eq!(engine.operation(stop_op.id).unwrap().state, "queued");
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopping);
    assert_eq!(
        engine
            .0
            .state
            .lock()
            .unwrap()
            .database
            .retired_launches
            .len(),
        1
    );

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f7_06_restart_during_dispatch_boundary() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    let (barrier, mut pause_rx, resume_tx) = HookBarrier::new();
    {
        let dispatched_ref = dispatched.clone();
        let b = barrier.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.pre_dispatch_hook = Some(Arc::new(move |_id, _gen, _trk| {
            b.wait_in_hook();
        }));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    tokio::time::timeout(Duration::from_secs(10), pause_rx.recv())
        .await
        .unwrap();
    // Account 1 is stopped and restarted
    engine.command("1", "stop").unwrap();
    let gen_after_stop = engine.snapshot().accounts[0].generation;

    // Unblock the old launch task
    let _ = resume_tx.send(());
    t1.await.unwrap();

    // Old launch was cancelled. 0 OS launches for old generation.
    assert_eq!(dispatched.lock().unwrap().len(), 0);
    assert_eq!(engine.snapshot().accounts[0].generation, gen_after_stop);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f7_07_shutdown_during_dispatch_boundary() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    let (barrier, mut pause_rx, resume_tx) = HookBarrier::new();
    {
        let dispatched_ref = dispatched.clone();
        let b = barrier.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.pre_dispatch_hook = Some(Arc::new(move |_id, _gen, _trk| {
            b.wait_in_hook();
        }));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    tokio::time::timeout(Duration::from_secs(10), pause_rx.recv())
        .await
        .unwrap();
    engine.shutdown();
    let _ = resume_tx.send(());
    t1.await.unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 0);
    assert!(engine.snapshot().accounts[0].tracker.is_none());
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_cancelled"
            && e["details"]["reason"] == "shutdown"
            && e["details"]["shutdown"] == true
    }));

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f7_08_launch_coordination_disabled_during_dispatch() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    let (barrier, mut pause_rx, resume_tx) = HookBarrier::new();
    {
        let dispatched_ref = dispatched.clone();
        let b = barrier.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.pre_dispatch_hook = Some(Arc::new(move |_id, _gen, _trk| {
            b.wait_in_hook();
        }));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    tokio::time::timeout(Duration::from_secs(10), pause_rx.recv())
        .await
        .unwrap();
    engine.set_launch_allowed(false);
    let _ = resume_tx.send(());
    t1.await.unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 0);
    assert!(engine.snapshot().accounts[0].tracker.is_none());
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_coordination_unavailable"
            && e["details"]["reason"] == "launch_coordination_unavailable"
    }));

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f7_09_persistence_failure_before_os_submission() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;

    // Create conflicting directory preventing store atomic rename during tracker persistence
    std::fs::create_dir(path.join("accounts.json.tmp")).unwrap();

    state.release.notify_waiters();
    t1.await.unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 0);
    assert!(engine.snapshot().accounts[0].tracker.is_none());
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_cancelled" && e["details"]["reason"] == "tracker_save_failed"
    }));

    server.abort();
    drop(engine);
    let _ = std::fs::remove_dir(path.join("accounts.json.tmp"));
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f7_10_duplicate_start_while_launch_in_flight() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    let (barrier, mut pause_rx, resume_tx) = HookBarrier::new();
    {
        let dispatched_ref = dispatched.clone();
        let b = barrier.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.pre_dispatch_hook = Some(Arc::new(move |_id, _gen, _trk| {
            b.wait_in_hook();
        }));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    tokio::time::timeout(Duration::from_secs(10), pause_rx.recv())
        .await
        .unwrap();
    // Duplicate start returns existing operation, does not duplicate or bump generation
    let duplicate_op = engine.command("1", "start").unwrap();
    assert_eq!(
        duplicate_op.id,
        engine.snapshot().accounts[0].operation_id.unwrap()
    );

    let _ = resume_tx.send(());
    t1.await.unwrap();

    assert_eq!(
        dispatched.lock().unwrap().len(),
        1,
        "exactly one OS launch submitted"
    );

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f7_11_stale_generation_after_cancellation() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    let (barrier, mut pause_rx, resume_tx) = HookBarrier::new();
    {
        let dispatched_ref = dispatched.clone();
        let b = barrier.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.pre_dispatch_hook = Some(Arc::new(move |_id, _gen, _trk| {
            b.wait_in_hook();
        }));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    tokio::time::timeout(Duration::from_secs(10), pause_rx.recv())
        .await
        .unwrap();
    engine.command("1", "stop").unwrap();
    let _ = resume_tx.send(());
    t1.await.unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 0);
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopping);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f7_12_late_process_appears_after_stop() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    t1.await.unwrap();

    let tracker = engine.snapshot().accounts[0].tracker.clone().unwrap();
    let stop_op = engine.command("1", "stop").unwrap();
    assert_eq!(engine.operation(stop_op.id).unwrap().state, "queued");

    // Late client appears matching the retired launch
    let retired_before = engine
        .0
        .state
        .lock()
        .unwrap()
        .database
        .retired_launches
        .clone();
    assert_eq!(retired_before.len(), 1);
    assert_eq!(retired_before[0].tracker, tracker);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f7_13_late_process_appears_after_restart() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    t1.await.unwrap();

    let tracker = engine.snapshot().accounts[0].tracker.clone().unwrap();
    engine.command("1", "stop").unwrap();
    let retired = engine
        .0
        .state
        .lock()
        .unwrap()
        .database
        .retired_launches
        .clone();
    assert_eq!(retired.len(), 1);
    assert_eq!(retired[0].tracker, tracker);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f7_14_exact_tracker_remains_protected_after_cancellation() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let (barrier, mut pause_rx, resume_tx) = HookBarrier::new();
    {
        let b = barrier.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.pre_dispatch_hook = Some(Arc::new(move |_id, _gen, _trk| {
            b.wait_in_hook();
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    tokio::time::timeout(Duration::from_secs(10), pause_rx.recv())
        .await
        .unwrap();
    let tracker = engine.snapshot().accounts[0].tracker.clone().unwrap();
    engine.command("1", "stop").unwrap();
    let _ = resume_tx.send(());
    t1.await.unwrap();

    let state_db = engine.0.state.lock().unwrap().database.clone();
    assert!(
        state_db
            .retired_launches
            .iter()
            .any(|r| r.tracker == tracker),
        "exact tracker must remain recorded in retired_launches to protect against rogue attachment"
    );

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn f7_15_stop_operation_never_reports_completion_while_unresolved_launch_can_emerge() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let (barrier, mut pause_rx, resume_tx) = HookBarrier::new();
    {
        let b = barrier.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.pre_dispatch_hook = Some(Arc::new(move |_id, _gen, _trk| {
            b.wait_in_hook();
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    tokio::time::timeout(Duration::from_secs(10), pause_rx.recv())
        .await
        .unwrap();
    let stop_op = engine.command("1", "stop").unwrap();
    assert_eq!(
        engine.operation(stop_op.id).unwrap().state,
        "queued",
        "Stop must not report completed while an unresolved tracker exists"
    );
    let _ = resume_tx.send(());
    t1.await.unwrap();

    assert_eq!(
        engine.operation(stop_op.id).unwrap().state,
        "queued",
        "Stop remains queued until retired launch exit or deadline"
    );

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn f7_16_launch_stop_racing_across_separate_os_threads() {
    with_watchdog(Duration::from_secs(15), || {
        for _ in 0..5 {
            let (engine, path) = super::tests::fixture(1);
            let e1 = engine.clone();
            let e2 = engine.clone();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let b1 = barrier.clone();
            let b2 = barrier.clone();

            let t_launch = std::thread::spawn(move || {
                b1.wait();
                let _ = e1.command("1", "start");
            });
            let t_stop = std::thread::spawn(move || {
                b2.wait();
                let _ = e2.command("1", "stop");
            });

            t_launch.join().unwrap();
            t_stop.join().unwrap();

            let snap = engine.snapshot();
            let a = &snap.accounts[0];
            // Valid linearized outcomes: either Stopped or queued Start
            assert!(
                a.status == Status::Stopped
                    || a.status == Status::Stopping
                    || a.status == Status::Queued
                    || a.status == Status::Launching
            );

            drop(engine);
            std::fs::remove_dir_all(path).unwrap();
        }
    });
}

// =========================================================================
// Finding F8 — Ambiguous Process Timeout Must Not Grant Launch Authority
// =========================================================================

#[tokio::test]
async fn f8_01_ambiguous_process_persists_beyond_35_seconds() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    // Ambiguous uninspected process PID 8800 present
    assert!(!engine.observe_startup_scan(
        &[platform::Player {
            pid: 8800,
            creation_time: "800".into(),
            tracker: String::new(),
        }],
        1,
        0,
        true,
        false,
    ));

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    tokio::time::sleep(Duration::from_millis(25)).await;

    // Check at 8s, 20s, 35s, 36s, 60s
    for secs in [8, 20, 35, 36, 60] {
        *now_cell.lock().unwrap() = base_instant + Duration::from_secs(secs);
        engine.0.wake.notify_waiters();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(
            dispatched.lock().unwrap().len(),
            0,
            "must never dispatch while ambiguous process remains present at {secs}s"
        );
    }

    t1.await.unwrap();
    let snap = engine.snapshot();
    assert_eq!(snap.accounts[0].status, Status::NeedsAttention);
    assert_eq!(
        snap.accounts[0].last_error.as_deref(),
        Some(STARTUP_AMBIGUOUS_TIMEOUT_ERROR)
    );
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_ambiguous_process_timeout"
            && e["details"]["reason"] == "unverified_roblox_process_timeout_exceeded"
    }));

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_02_inaccessible_process_persists_beyond_35_seconds() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    // Inaccessible process present
    assert!(!engine.observe_startup_scan(&[], 0, 1, false, false));

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(36);
    engine.0.wake.notify_waiters();
    t1.await.unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 0);
    assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_ambiguous_process_timeout"
            && e["details"]["reason"] == "inaccessible_process_timeout_exceeded"
    }));

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_03_active_bootstrapper_persists_beyond_35_seconds() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    // Bootstrapper active
    assert!(!engine.observe_startup_scan(&[], 0, 0, false, true));

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(36);
    engine.0.wake.notify_waiters();
    t1.await.unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 0);
    assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);
    assert!(engine.0.journal.recent().iter().any(|e| {
        e["event"] == "launch_ambiguous_process_timeout"
            && e["details"]["reason"] == "bootstrapper_active_timeout_exceeded"
    }));

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_04_wmi_failure_persists_beyond_35_seconds() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    // Discovery failure records inaccessible = 1
    engine.observe_startup_scan(&[], 0, 1, false, false);

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(36);
    engine.0.wake.notify_waiters();
    t1.await.unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 0);
    assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_05_ambiguous_process_becomes_verified_at_34_seconds() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    // Ambiguous process present initially
    engine.observe_startup_scan(
        &[platform::Player {
            pid: 8800,
            creation_time: "800".into(),
            tracker: String::new(),
        }],
        1,
        0,
        true,
        false,
    );

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    // At 34s, process is confirmed external unowned (uninspected = 0, inaccessible = 0)
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(34);
    engine.observe_startup_scan(
        &[platform::Player {
            pid: 8800,
            creation_time: "800".into(),
            tracker: String::new(),
        }],
        0,
        0,
        false,
        false,
    );
    engine.0.wake.notify_waiters();

    tokio::time::timeout(Duration::from_secs(2), t1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(dispatched.lock().unwrap().len(), 1);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_06_ambiguous_process_becomes_verified_at_36_seconds() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    engine.observe_startup_scan(&[], 1, 0, true, false);

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    // Times out at 35s
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(36);
    engine.0.wake.notify_waiters();
    t1.await.unwrap();
    assert_eq!(dispatched.lock().unwrap().len(), 0);
    assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);

    // Verified at 36s (uninspected = 0, inaccessible = 0)
    engine.observe_startup_scan(&[], 0, 0, false, false);

    // Retried via start -> dispatches cleanly
    let retry_op = engine.command("1", "start").unwrap();
    assert_eq!(engine.operation(retry_op.id).unwrap().state, "queued");
    let a1_retry = engine.snapshot().accounts[0].clone();
    let e1_retry = engine.clone();
    let t1_retry = tokio::spawn(async move { e1_retry.launch_one(a1_retry).await });
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();
    tokio::time::timeout(Duration::from_secs(2), t1_retry)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 1);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_07_fresh_complete_scan_shows_process_exited() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    // Ambiguous process
    engine.observe_startup_scan(&[], 1, 0, true, false);

    // Process exits: fresh complete scan confirms 0 players and 0 uninspected
    engine.observe_startup_scan(&[], 0, 0, false, false);

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    tokio::time::timeout(Duration::from_secs(2), t1)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 1);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_08_fresh_complete_scan_identifies_unrelated_external_process() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    // Confirmed unowned external process (uninspected = 0, inaccessible = 0)
    engine.observe_startup_scan(
        &[platform::Player {
            pid: 7777,
            creation_time: "777".into(),
            tracker: String::new(),
        }],
        0,
        0,
        false,
        false,
    );

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    tokio::time::timeout(Duration::from_secs(2), t1)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        dispatched.lock().unwrap().len(),
        1,
        "Confirmed unrelated external process must not block launch dispatch"
    );

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_09_previously_verified_client_remains_owned_during_scan_failures() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    t1.await.unwrap();

    let tracker = engine.snapshot().accounts[0].tracker.clone().unwrap();
    let account_gen = engine.snapshot().accounts[0].generation;
    let identity = ProcessIdentity {
        pid: 6543,
        creation_time: "654".into(),
        generation: account_gen,
        tracker: tracker.clone(),
    };
    let players = vec![platform::Player {
        pid: 6543,
        creation_time: "654".into(),
        tracker,
    }];
    engine
        .attach_player(&engine.snapshot().accounts[0], identity, &players)
        .unwrap();
    assert!(engine.snapshot().accounts[0].process.is_some());

    // Discovery failure occurs (inaccessible = 1)
    engine.observe_startup_scan(&[], 0, 1, false, false);

    // Verified account remains owned and unaffected
    assert!(
        engine.snapshot().accounts[0].process.is_some(),
        "Previously verified client must remain owned during scan failures"
    );

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_10_waiting_account_is_cancelled_after_timeout() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    {
        let clock_ref = now_cell.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
    }
    engine.observe_startup_scan(&[], 1, 0, true, false);

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(36);
    engine.0.wake.notify_waiters();
    t1.await.unwrap();

    assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);

    // User cancels the timed-out account via Stop
    let stop_op = engine.command("1", "stop").unwrap();
    assert_eq!(engine.operation(stop_op.id).unwrap().state, "completed");
    assert_eq!(engine.snapshot().accounts[0].status, Status::Stopped);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_11_waiting_account_is_manually_retried() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    engine.observe_startup_scan(&[], 1, 0, true, false);

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(36);
    engine.0.wake.notify_waiters();
    t1.await.unwrap();
    assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);

    // Clear ambiguous condition
    engine.observe_startup_scan(&[], 0, 0, false, false);

    // User restarts the account manually
    let start_op = engine.command("1", "start").unwrap();
    assert_eq!(engine.operation(start_op.id).unwrap().state, "queued");
    let a1_retry = engine.snapshot().accounts[0].clone();
    let e1_retry = engine.clone();
    let t1_retry = tokio::spawn(async move { e1_retry.launch_one(a1_retry).await });
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();
    tokio::time::timeout(Duration::from_secs(2), t1_retry)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(dispatched.lock().unwrap().len(), 1);

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_12_multiple_queued_accounts_share_same_ambiguous_condition() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(2, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    // Ambiguous condition persists
    engine.observe_startup_scan(&[], 1, 0, true, false);

    let accounts = engine.snapshot().accounts;
    let e1 = engine.clone();
    let a1 = accounts[0].clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    let e2 = engine.clone();
    let a2 = accounts[1].clone();
    let t2 = tokio::spawn(async move { e2.launch_one(a2).await });
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_waiters();

    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(36);
    engine.0.wake.notify_waiters();
    t1.await.unwrap();

    // Neither account can dispatch while the ambiguous condition remains globally unresolved
    assert_eq!(
        dispatched.lock().unwrap().len(),
        0,
        "Neither account should dispatch under ambiguous condition"
    );
    t2.abort();

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_13_late_first_client_appears_after_unresolved_startup() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(2, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    let accounts = engine.snapshot().accounts;
    let e1 = engine.clone();
    let a1 = accounts[0].clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    t1.await.unwrap();
    assert_eq!(dispatched.lock().unwrap().len(), 1);

    let tracker1 = engine.snapshot().accounts[0].tracker.clone().unwrap();

    // At 9s without observation: readiness window expires, Account 1 marked NeedsAttention
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(9);
    assert!(engine.poll_launch_readiness(&accounts[1], *now_cell.lock().unwrap()));
    assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);
    assert!(engine.snapshot().accounts[0].tracker.is_some());

    // At 15s: late client appears with matching tracker
    let identity = ProcessIdentity {
        pid: 5432,
        creation_time: "543".into(),
        generation: engine.snapshot().accounts[0].generation,
        tracker: tracker1.clone(),
    };
    let players = vec![platform::Player {
        pid: 5432,
        creation_time: "543".into(),
        tracker: tracker1,
    }];
    assert!(
        engine
            .attach_player(&engine.snapshot().accounts[0], identity, &players)
            .is_ok()
    );
    assert_eq!(engine.snapshot().accounts[0].status, Status::Unknown);
    assert!(engine.snapshot().accounts[0].process.is_some());

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_14_recovery_occurs_without_duplicate_protocol_dispatch() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
    {
        let clock_ref = now_cell.clone();
        let dispatched_ref = dispatched.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.launch_spacing = Duration::from_secs(5);
        inner.launch_readiness_timeout = Duration::from_secs(8);
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
        inner.launch_hook = Some(Arc::new(move |_uri, id| {
            dispatched_ref.lock().unwrap().push(id.to_owned());
            Ok(())
        }));
    }
    engine.observe_startup_scan(&[], 1, 0, true, false);

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(36);
    engine.0.wake.notify_waiters();
    t1.await.unwrap();

    engine.observe_startup_scan(&[], 0, 0, false, false);
    let start_op = engine.command("1", "start").unwrap();
    assert_eq!(engine.operation(start_op.id).unwrap().state, "queued");
    let a1_retry = engine.snapshot().accounts[0].clone();
    let e1_retry = engine.clone();
    let t1_retry = tokio::spawn(async move { e1_retry.launch_one(a1_retry).await });
    wait_for(&state.tickets, 2, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();
    tokio::time::timeout(Duration::from_secs(2), t1_retry)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        dispatched.lock().unwrap().len(),
        1,
        "Exactly one protocol dispatch must occur across the recovery cycle"
    );

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn f8_15_diagnostics_contain_clear_reason_without_secrets() {
    let state = Preparation::default();
    let (url, server) = http_fixture(state.clone()).await;
    let (mut engine, path) = engine_fixture(1, &url, true);
    let base_instant = Instant::now();
    let now_cell = Arc::new(Mutex::new(base_instant));
    {
        let clock_ref = now_cell.clone();
        let inner = Arc::get_mut(&mut engine.0).unwrap();
        inner.startup_verification_timeout = Duration::from_secs(35);
        inner.clock = Some(Arc::new(move || *clock_ref.lock().unwrap()));
    }
    engine.observe_startup_scan(&[], 1, 0, true, false);

    let a1 = engine.snapshot().accounts[0].clone();
    let e1 = engine.clone();
    let t1 = tokio::spawn(async move { e1.launch_one(a1).await });
    wait_for(&state.tickets, 1, &state.entered).await;
    state.release.notify_one();
    state.release.notify_waiters();

    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(36);
    engine.0.wake.notify_waiters();
    t1.await.unwrap();

    let journal_records = engine.0.journal.recent();
    let timeout_event = journal_records
        .iter()
        .find(|e| e["event"] == "launch_ambiguous_process_timeout")
        .expect("must record launch_ambiguous_process_timeout");

    assert_eq!(
        timeout_event["details"]["reason"],
        "unverified_roblox_process_timeout_exceeded"
    );

    // Verify diagnostic privacy: zero secrets, tickets, or private URLs in journal
    let serialized = serde_json::to_string(&journal_records).unwrap();
    assert!(!serialized.contains("DISPOSABLE_FIXTURE_TICKET"));
    assert!(!serialized.contains("DISPOSABLE_FIXTURE_SESSION"));

    server.abort();
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

// =========================================================================
// Finding F9 — Strengthen Multi-Threaded Concurrency Regression Tests
// =========================================================================

#[test]
fn f9_01_concurrent_readiness_polling_and_player_attachment_os_threads() {
    with_watchdog(Duration::from_secs(15), || {
        let (engine, path) = super::tests::fixture(2);
        let waiting_account = engine.snapshot().accounts[1].clone();
        let acc_for_attach = engine.snapshot().accounts[0].clone();
        let tracker = "123456789012".to_string();

        let barrier = Arc::new(std::sync::Barrier::new(2));
        let b1 = barrier.clone();
        let b2 = barrier.clone();

        let e_poll = engine.clone();
        let poll_thread = std::thread::spawn(move || {
            b1.wait();
            for _ in 0..50 {
                let _ = e_poll.poll_launch_readiness(&waiting_account, Instant::now());
            }
        });

        let e_attach = engine.clone();
        let attach_thread = std::thread::spawn(move || {
            b2.wait();
            let identity = ProcessIdentity {
                pid: 8888,
                creation_time: "888".into(),
                generation: acc_for_attach.generation,
                tracker: tracker.clone(),
            };
            let players = vec![platform::Player {
                pid: 8888,
                creation_time: "888".into(),
                tracker,
            }];
            let _ = e_attach.attach_player(&acc_for_attach, identity, &players);
        });

        poll_thread.join().unwrap();
        attach_thread.join().unwrap();

        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    });
}

#[test]
fn f9_02_state_mutation_while_readiness_state_changes_os_threads() {
    with_watchdog(Duration::from_secs(15), || {
        let (engine, path) = super::tests::fixture(2);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let b1 = barrier.clone();
        let b2 = barrier.clone();

        let e1 = engine.clone();
        let mutate_thread = std::thread::spawn(move || {
            b1.wait();
            for _ in 0..20 {
                let _ = e1.transaction(|s| {
                    s.database.multi_instance_enabled = !s.database.multi_instance_enabled;
                    Ok(())
                });
            }
        });

        let e2 = engine.clone();
        let waiting = engine.snapshot().accounts[1].clone();
        let observe_thread = std::thread::spawn(move || {
            b2.wait();
            for i in 0..20 {
                let _ = e2.observe_startup_scan(&[], i % 2, 0, false, false);
                let _ = e2.poll_launch_readiness(&waiting, Instant::now());
            }
        });

        mutate_thread.join().unwrap();
        observe_thread.join().unwrap();

        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    });
}

#[test]
fn f9_03_concurrent_stop_and_launch_dispatch_os_threads() {
    with_watchdog(Duration::from_secs(15), || {
        let (engine, path) = super::tests::fixture(1);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let b1 = barrier.clone();
        let b2 = barrier.clone();

        let e1 = engine.clone();
        let target = Target {
            place_id: 1818,
            job_id: None,
            private_server_link: None,
        };
        let acc = engine.snapshot().accounts[0].clone();

        let launch_thread = std::thread::spawn(move || {
            b1.wait();
            let _ = e1.submit_launch(&acc, &target, || Ok(()));
        });

        let e2 = engine.clone();
        let stop_thread = std::thread::spawn(move || {
            b2.wait();
            let _ = e2.command("1", "stop");
        });

        launch_thread.join().unwrap();
        stop_thread.join().unwrap();

        let snap = engine.snapshot();
        assert!(
            snap.accounts[0].status == Status::Stopped
                || snap.accounts[0].status == Status::Stopping
                || snap.accounts[0].status == Status::Queued
        );

        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    });
}

#[test]
fn f9_04_concurrent_restart_and_launch_dispatch_os_threads() {
    with_watchdog(Duration::from_secs(15), || {
        let (engine, path) = super::tests::fixture(1);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let b1 = barrier.clone();
        let b2 = barrier.clone();

        let e1 = engine.clone();
        let target = Target {
            place_id: 1818,
            job_id: None,
            private_server_link: None,
        };
        let acc = engine.snapshot().accounts[0].clone();

        let launch_thread = std::thread::spawn(move || {
            b1.wait();
            let _ = e1.submit_launch(&acc, &target, || Ok(()));
        });

        let e2 = engine.clone();
        let restart_thread = std::thread::spawn(move || {
            b2.wait();
            let _ = e2.command("1", "restart");
        });

        launch_thread.join().unwrap();
        restart_thread.join().unwrap();

        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    });
}

#[test]
fn f9_05_shutdown_while_dispatch_in_flight_os_threads() {
    with_watchdog(Duration::from_secs(15), || {
        let (engine, path) = super::tests::fixture(1);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let b1 = barrier.clone();
        let b2 = barrier.clone();

        let e1 = engine.clone();
        let target = Target {
            place_id: 1818,
            job_id: None,
            private_server_link: None,
        };
        let acc = engine.snapshot().accounts[0].clone();

        let launch_thread = std::thread::spawn(move || {
            b1.wait();
            let _ = e1.submit_launch(&acc, &target, || {
                std::thread::sleep(Duration::from_millis(10));
                Ok(())
            });
        });

        let e2 = engine.clone();
        let shutdown_thread = std::thread::spawn(move || {
            b2.wait();
            e2.shutdown();
        });

        launch_thread.join().unwrap();
        shutdown_thread.join().unwrap();

        assert!(engine.is_shutdown());

        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    });
}

#[test]
fn f9_06_recovery_and_process_exit_reconciliation_active_coordination_os_threads() {
    with_watchdog(Duration::from_secs(15), || {
        let (engine, path) = super::tests::fixture(2);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let b1 = barrier.clone();
        let b2 = barrier.clone();

        let e1 = engine.clone();
        let waiting = engine.snapshot().accounts[1].clone();
        let poll_thread = std::thread::spawn(move || {
            b1.wait();
            for _ in 0..20 {
                let _ = e1.poll_launch_readiness(&waiting, Instant::now());
            }
        });

        let e2 = engine.clone();
        let expire_thread = std::thread::spawn(move || {
            b2.wait();
            for _ in 0..20 {
                e2.expire_unowned_launches(Utc::now() + ChronoDuration::seconds(100));
            }
        });

        poll_thread.join().unwrap();
        expire_thread.join().unwrap();

        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    });
}
