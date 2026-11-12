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
        assert!(!engine.observe_startup_scan(&[], 0, false, true));
        *now_cell.lock().unwrap() = base_instant + Duration::from_secs(6);
        assert!(!engine.observe_startup_scan(
            &[platform::Player {
                pid: 4200,
                creation_time: "200".into(),
                tracker: String::new(),
            }],
            1,
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
        assert!(engine.observe_startup_scan(&players, 0, false, false));
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
        true,
        false,
    ));
    assert!(engine.snapshot().accounts[0].process.is_none());

    // Scenario 8: At 3s, PID 5100 exits before verification! Next complete scan only has an
    // uninspected external process PID 6100 (`uninspected = 1`) and then at 5s PID 6100 is still uninspected.
    // First, let's verify PID 5100 exiting with no other uninspected processes marks Account 1 StartupUnresolved immediately.
    *now_cell.lock().unwrap() = base_instant + Duration::from_secs(3);
    assert!(!engine.observe_startup_scan(&[], 0, false, false));
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
