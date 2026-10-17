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
