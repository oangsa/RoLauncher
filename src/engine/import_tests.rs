use super::*;
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::get,
};
use std::sync::atomic::AtomicUsize;

#[derive(Clone, Default)]
struct ValidationState {
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

async fn validate(
    State(state): State<ValidationState>,
    headers: HeaderMap,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    let cookie = headers.get("cookie").unwrap().to_str().unwrap();
    let id: u64 = cookie
        .strip_prefix(".ROBLOSECURITY=fixture-")
        .unwrap()
        .parse()
        .unwrap();
    let active = state.active.fetch_add(1, Ordering::SeqCst) + 1;
    state.peak.fetch_max(active, Ordering::SeqCst);
    state.calls.fetch_add(1, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(if id == 1 { 80 } else { 15 })).await;
    state.active.fetch_sub(1, Ordering::SeqCst);
    if id == 3 {
        (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({"error":"expired fixture"})),
        )
    } else {
        (
            StatusCode::OK,
            axum::Json(serde_json::json!({"id":id,"name":format!("fixture-{id}")})),
        )
    }
}

#[tokio::test]
async fn authenticated_import_batches_overlap_four_checks_and_preserve_order_and_partial_success() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let state = ValidationState::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let fixture = Router::new()
        .route("/user", get(validate))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });
    let path = std::env::temp_dir().join(format!("rbx-import-fixture-{}", Uuid::new_v4()));
    let mut engine = Engine::open(Store::new(path.clone()).unwrap(), "fixture".into()).unwrap();
    Arc::get_mut(&mut engine.0).unwrap().roblox.user_url = Some(format!("http://{address}/user"));
    let app = crate::api::router(engine.clone());
    let cookies = [1, 2, 3, 4, 2, 5, 6, 7].map(|id| format!("fixture-{id}"));
    let request = |cookies: serde_json::Value| {
        Request::builder()
            .method("POST")
            .uri("/v1/accounts")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", engine.token()))
            .body(Body::from(
                serde_json::json!({"cookies":cookies}).to_string(),
            ))
            .unwrap()
    };
    let response = app
        .clone()
        .oneshot(request(serde_json::json!(cookies)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
            .unwrap();
    assert_eq!(state.peak.load(Ordering::SeqCst), 4);
    assert_eq!(state.active.load(Ordering::SeqCst), 0);
    assert_eq!(state.calls.load(Ordering::SeqCst), 8);
    assert_eq!(
        value["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["1", "2", "4", "2", "5", "6", "7"]
    );
    assert_eq!(value["errors"][0]["index"], 2);
    assert!(
        value["errors"][0]["error"]
            .as_str()
            .unwrap()
            .contains("Session expired")
    );
    assert_eq!(
        engine
            .snapshot()
            .accounts
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "4", "5", "6", "7"]
    );
    for size in [0, 51] {
        let response = app
            .clone()
            .oneshot(request(serde_json::json!(vec!["fixture-1"; size])))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert_eq!(state.calls.load(Ordering::SeqCst), 8);
    let mut invalid = vec!["bad;cookie".to_owned()];
    assert!(engine.import_batch(&mut invalid).await[0].is_err());
    assert!(invalid[0].is_empty());
    drop(app);
    drop(engine);
    let reopened = Engine::open(Store::new(path.clone()).unwrap(), "fixture".into()).unwrap();
    assert_eq!(reopened.snapshot().accounts.len(), 6);
    drop(reopened);
    server.abort();
    std::fs::remove_dir_all(path).unwrap();
}
