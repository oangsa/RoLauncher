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
