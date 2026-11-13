#![cfg(any(windows, target_os = "linux"))]

use crate::{
    api,
    engine::Engine,
    model::{JoinType, ServerSelection},
    store::Store,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[tokio::test]
async fn accepted_stop_reports_stopping_and_a_pending_operation_with_the_process_retained() {
    let path = std::env::temp_dir().join(format!("rbx-api-stop-{}", uuid::Uuid::new_v4()));
    let store = Store::new(path.clone()).unwrap();
    let mut database = crate::model::Database {
        version: 2,
        ..Default::default()
    };
    let mut account = crate::model::Account::new("1".into(), "fixture".into());
    account.desired_running = true;
    account.tracker = Some("123456789012".into());
    account.process = Some(crate::model::ProcessIdentity {
        pid: 4242,
        creation_time: "fixture-start".into(),
        tracker: account.tracker.clone().unwrap(),
        generation: account.generation,
    });
    database.accounts.push(crate::model::SavedAccount {
        account,
        encrypted_session: String::new(),
    });
    store.save(&database).unwrap();
    let engine = Engine::open(store, "fixture".into()).unwrap();
    let app = api::router(engine.clone());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/accounts/1/stop")
                .header("authorization", format!("Bearer {}", engine.token()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let operation: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(operation["state"], "queued");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/status")
                .header("authorization", format!("Bearer {}", engine.token()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let snapshot: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(snapshot["accounts"][0]["status"], "stopping");
    assert_eq!(snapshot["accounts"][0]["desired_running"], false);
    assert_eq!(snapshot["accounts"][0]["process"]["pid"], 4242);
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn external_cleanup_requires_separate_persisted_opt_in() {
    let path = std::env::temp_dir().join(format!("rbx-cleanup-setting-{}", uuid::Uuid::new_v4()));
    let store = Store::new(path.clone()).unwrap();
    let mut legacy = serde_json::to_value(crate::model::Database::default()).unwrap();
    legacy["version"] = serde_json::json!(2);
    legacy
        .as_object_mut()
        .unwrap()
        .remove("external_background_cleanup_enabled");
    store
        .save(&serde_json::from_value(legacy).unwrap())
        .unwrap();
    let engine = Engine::open(store, "fixture".into()).unwrap();
    assert!(!engine.snapshot().external_background_cleanup_enabled);
    engine.set_multi_instance(false).unwrap();
    assert!(!engine.snapshot().external_background_cleanup_enabled);
    let app = api::router(engine.clone());
    for (uri, auth, body, expected) in [
        (
            "startup-cleanup",
            false,
            r#"{"external_background_cleanup_enabled":true}"#,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "startup-cleanup",
            true,
            r#"{"external_background_cleanup_enabled":"true"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "game",
            true,
            r#"{"external_background_cleanup_enabled":true,"multi_instance_enabled":true}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "game",
            true,
            r#"{"external_background_cleanup_enabled":true}"#,
            StatusCode::NO_CONTENT,
        ),
    ] {
        let mut request = Request::builder()
            .method("PATCH")
            .uri(format!("/v1/settings/{uri}"))
            .header("content-type", "application/json");
        if auth {
            request = request.header("authorization", format!("Bearer {}", engine.token()));
        }
        assert_eq!(
            app.clone()
                .oneshot(request.body(Body::from(body)).unwrap())
                .await
                .unwrap()
                .status(),
            expected
        );
        assert_eq!(
            engine.snapshot().external_background_cleanup_enabled,
            expected == StatusCode::NO_CONTENT
        );
        assert!(!engine.snapshot().multi_instance_enabled);
    }
    drop(app);
    drop(engine);
    let reopened = Engine::open(Store::new(path.clone()).unwrap(), "fixture".into()).unwrap();
    assert!(reopened.snapshot().external_background_cleanup_enabled);
    assert!(!reopened.snapshot().active_multi_instance);
    reopened.set_external_background_cleanup(false).unwrap();
    drop(reopened);
    let reopened = Engine::open(Store::new(path.clone()).unwrap(), "fixture".into()).unwrap();
    assert!(!reopened.snapshot().external_background_cleanup_enabled);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn instance_preference_works_through_game_settings_and_existing_alias() {
    let path = std::env::temp_dir().join(format!("rbx-settings-compat-{}", uuid::Uuid::new_v4()));
    let engine = Engine::open(Store::new(path.clone()).unwrap(), "fixture".into()).unwrap();
    let app = api::router(engine.clone());
    for (uri, auth, body, expected) in [
        (
            "game",
            false,
            r#"{"multi_instance_enabled":false}"#,
            StatusCode::UNAUTHORIZED,
        ),
        (
            "game",
            true,
            r#"{"multi_instance_enabled":"false"}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "game",
            true,
            r#"{"server_selection":"invalid","join_type":"friend","multi_instance_enabled":false}"#,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "game",
            true,
            r#"{"multi_instance_enabled":false}"#,
            StatusCode::NO_CONTENT,
        ),
        (
            "instances",
            true,
            r#"{"multi_instance_enabled":true}"#,
            StatusCode::NO_CONTENT,
        ),
        (
            "game",
            true,
            r#"{"multi_instance_enabled":false}"#,
            StatusCode::NO_CONTENT,
        ),
    ] {
        let before = engine.snapshot().multi_instance_enabled;
        let mut request = Request::builder()
            .method("PATCH")
            .uri(format!("/v1/settings/{uri}"))
            .header("content-type", "application/json");
        if auth {
            request = request.header("authorization", format!("Bearer {}", engine.token()));
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        let state = engine.snapshot();
        assert_eq!(
            state.multi_instance_enabled,
            if expected == StatusCode::NO_CONTENT {
                uri == "instances"
            } else {
                before
            }
        );
        assert!(state.active_multi_instance);
        assert_eq!(state.server_selection, ServerSelection::Smart);
        assert_eq!(state.join_type, JoinType::Random);
    }
    drop(app);
    drop(engine);
    let reopened = Engine::open(Store::new(path.clone()).unwrap(), "fixture".into()).unwrap();
    assert!(!reopened.snapshot().multi_instance_enabled);
    assert!(!reopened.snapshot().active_multi_instance);
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn resources_endpoints_enforce_auth_redact_secrets_and_serve_concurrent_readers() {
    let path = std::env::temp_dir().join(format!("rbx-api-resources-{}", uuid::Uuid::new_v4()));
    let store = Store::new(path.clone()).unwrap();
    let mut database = crate::model::Database {
        version: 2,
        ..Default::default()
    };
    let mut account = crate::model::Account::new("acc-1".into(), "resource_user".into());
    account.alias = "Farm One".into();
    let secret_job = uuid::Uuid::new_v4();
    account.target = Some(crate::model::Target {
        place_id: 1818,
        job_id: Some(secret_job),
        private_server_link: Some(
            "https://www.roblox.com/games/1818?privateServerLinkCode=SECRET_LINK".into(),
        ),
    });
    database.accounts.push(crate::model::SavedAccount {
        account,
        encrypted_session: "SECRET_ENCRYPTED_COOKIE".into(),
    });
    store.save(&database).unwrap();

    let engine = Engine::open(store, "fixture".into()).unwrap();
    let app = api::router(engine.clone());

    for uri in [
        "/v1/resources",
        "/v1/resources/accounts/acc-1",
        "/v1/resources/events",
    ] {
        let unauth = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED, "{uri}");

        let cross_origin = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .header("authorization", format!("Bearer {}", engine.token()))
                    .header("origin", "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN, "{uri}");
    }

    // Concurrent readers safely share one snapshot without corrupting or panicking.
    let req_a = app.clone().oneshot(
        Request::builder()
            .uri("/v1/resources?history=true&seconds=15")
            .header("authorization", format!("Bearer {}", engine.token()))
            .body(Body::empty())
            .unwrap(),
    );
    let req_b = app.clone().oneshot(
        Request::builder()
            .uri("/v1/resources?history=true&seconds=5")
            .header("authorization", format!("Bearer {}", engine.token()))
            .body(Body::empty())
            .unwrap(),
    );
    let (res_a, res_b) = tokio::join!(req_a, req_b);
    let res_a = res_a.unwrap();
    let res_b = res_b.unwrap();
    assert_eq!(res_a.status(), StatusCode::OK);
    assert_eq!(res_b.status(), StatusCode::OK);
    let bytes_a = axum::body::to_bytes(res_a.into_body(), 256 * 1024)
        .await
        .unwrap();
    let text_a = String::from_utf8(bytes_a.to_vec()).unwrap();
    assert!(!text_a.contains("SECRET_ENCRYPTED_COOKIE"));
    assert!(!text_a.contains("SECRET_LINK"));
    assert!(!text_a.contains(&secret_job.to_string()));
    assert!(!text_a.contains(engine.token()));

    let json_a: serde_json::Value = serde_json::from_str(&text_a).unwrap();
    assert_eq!(json_a["metadata"]["cpu_normalization"], "machine_percent");
    assert_eq!(json_a["metadata"]["memory_units"], "bytes");
    assert!(json_a["timestamp"].as_str().is_some());
    assert!(json_a["sample_age_ms"].as_u64().is_some());
    assert_eq!(json_a["aggregates"]["running_instances"], 0);
    assert_eq!(json_a["aggregates"]["total_accounts"], 1);
    assert!(json_a["history"].as_array().is_some_and(|h| !h.is_empty()));

    // Account endpoint returns known account & 404 on missing account.
    let acc_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/resources/accounts/acc-1?history=true")
                .header("authorization", format!("Bearer {}", engine.token()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(acc_res.status(), StatusCode::OK);
    let acc_body = axum::body::to_bytes(acc_res.into_body(), 128 * 1024)
        .await
        .unwrap();
    let acc_text = String::from_utf8(acc_body.to_vec()).unwrap();
    assert!(!acc_text.contains("SECRET"));
    let acc_json: serde_json::Value = serde_json::from_str(&acc_text).unwrap();
    assert_eq!(acc_json["account"]["account_id"], "acc-1");
    assert_eq!(acc_json["account"]["display_name"], "Farm One");
    assert_eq!(acc_json["account"]["availability"], "exited");
    assert!(acc_json["history"].as_array().is_some());

    let missing_res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/resources/accounts/nonexistent")
                .header("authorization", format!("Bearer {}", engine.token()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_res.status(), StatusCode::NOT_FOUND);

    // SSE endpoint returns OK text/event-stream and emits an initial `resources` event.
    let sse_res = app
        .oneshot(
            Request::builder()
                .uri("/v1/resources/events")
                .header("authorization", format!("Bearer {}", engine.token()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sse_res.status(), StatusCode::OK);
    use futures_util::StreamExt;
    let mut stream = sse_res.into_body().into_data_stream();
    let first_chunk = stream.next().await.unwrap().unwrap();
    let frame_text = String::from_utf8_lossy(&first_chunk);
    assert!(frame_text.contains("event: resources"), "{frame_text}");
    assert!(frame_text.contains("machine_percent"), "{frame_text}");

    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}
