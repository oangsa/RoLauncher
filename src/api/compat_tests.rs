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
