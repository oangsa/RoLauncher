use crate::{
    engine::{AccountPatch, Engine},
    model::Snapshot,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{
        Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, patch, post},
};
use futures_util::StreamExt;
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;
use uuid::Uuid;

type ApiError = (StatusCode, Json<serde_json::Value>);
fn error(message: String) -> ApiError {
    (
        if message == "Account not found" {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        },
        Json(serde_json::json!({"error":message})),
    )
}
pub fn router(engine: Engine) -> Router {
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/accounts", get(accounts).post(import))
        .route("/v1/accounts/bulk", patch(bulk_update))
        .route("/v1/profiles", get(profiles).post(profile_save))
        .route("/v1/profiles/import", post(profile_import))
        .route("/v1/profiles/{id}", axum::routing::delete(profile_delete))
        .route("/v1/profiles/{id}/{action}", post(profile_action))
        .route("/v1/activity", get(activity))
        .route("/v1/diagnostics", get(diagnostics))
        .route("/v1/backups", get(backups).post(backup))
        .route("/v1/backups/{name}/restore", post(restore))
        .route("/v1/updates", patch(update_repository))
        .route("/v1/updates/check", post(update_check))
        .route("/v1/updates/official/check", post(official_update_check))
        .route("/v1/settings/updates", patch(update_settings))
        .route("/v1/login", post(browser_login))
        .route("/v1/accounts/{id}", patch(update).delete(remove))
        .route("/v1/accounts/{id}/repair", post(repair))
        .route("/v1/accounts/{id}/{action}", post(command))
        .route("/v1/operations/{id}", get(operation))
        .route("/v1/events", get(events))
        .route(
            "/v1/settings/discord",
            get(discord_settings).patch(discord_update),
        )
        .route("/v1/settings/discord/test", post(discord_test))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(middleware::from_fn_with_state(engine.clone(), authenticate))
        .with_state(engine)
}
async fn browser_login(State(engine): State<Engine>) -> Result<StatusCode, ApiError> {
    open_login(engine, None)
}
fn open_login(engine: Engine, expected: Option<String>) -> Result<StatusCode, ApiError> {
    #[cfg(any(windows, target_os = "linux"))]
    {
        let runtime = tokio::runtime::Handle::current();
        std::thread::Builder::new()
            .name("browser-login".into())
            .spawn(move || {
                if let Err(error) = crate::login::run_for(engine, runtime, expected) {
                    crate::ui::message(&error);
                }
            })
            .map_err(|_| error("Unable to open browser sign-in".into()))?;
        Ok(StatusCode::ACCEPTED)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (engine, expected);
        Err(error("Browser sign-in requires Windows".into()))
    }
}
async fn bulk_update(
    State(engine): State<Engine>,
    Json(input): Json<crate::engine::BulkPatch>,
) -> Result<Json<Vec<crate::model::Account>>, ApiError> {
    engine.bulk_patch(input).map(Json).map_err(error)
}
async fn update_repository(
    State(engine): State<Engine>,
    Json(patch): Json<crate::updates::RepositoryPatch>,
) -> Result<StatusCode, ApiError> {
    engine
        .set_update_repository(patch.repository)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(error)
}
async fn update_check(
    State(engine): State<Engine>,
) -> Result<Json<crate::updates::UpdateView>, ApiError> {
    let snapshot = engine.snapshot();
    crate::updates::check(&snapshot.update_repository, snapshot.include_beta_updates)
        .await
        .map(Json)
        .map_err(error)
}
async fn official_update_check(
    State(engine): State<Engine>,
) -> Result<Json<crate::updates::UpdateView>, ApiError> {
    crate::updates::check(
        crate::updates::DEFAULT_REPOSITORY,
        engine.snapshot().include_beta_updates,
    )
    .await
    .map(Json)
    .map_err(error)
}
#[derive(Deserialize)]
struct UpdateSettings {
    include_beta: bool,
}
async fn update_settings(
    State(engine): State<Engine>,
    Json(settings): Json<UpdateSettings>,
) -> Result<StatusCode, ApiError> {
    engine
        .set_beta_updates(settings.include_beta)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(error)
}
async fn profiles(State(engine): State<Engine>) -> Json<Vec<crate::model::LaunchProfile>> {
    Json(engine.snapshot().profiles)
}
async fn profile_save(
    State(engine): State<Engine>,
    Json(input): Json<crate::engine::ProfileSave>,
) -> Result<Json<crate::model::LaunchProfile>, ApiError> {
    engine.save_profile(input).map(Json).map_err(error)
}
async fn profile_import(
    State(engine): State<Engine>,
    Json(input): Json<Vec<crate::model::LaunchProfile>>,
) -> Result<Json<usize>, ApiError> {
    engine.import_profiles(input).map(Json).map_err(error)
}
async fn profile_delete(
    State(engine): State<Engine>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    engine
        .delete_profile(id)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(error)
}
async fn profile_action(
    State(engine): State<Engine>,
    Path((id, action)): Path<(Uuid, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    match action.as_str() {
        "apply" => engine
            .apply_profile(id)
            .map(|v| Json(serde_json::json!(v)))
            .map_err(error),
        "start" => engine
            .launch_profile(id)
            .map(|v| Json(serde_json::json!(v)))
            .map_err(error),
        _ => Err(error("Unknown profile action".into())),
    }
}
#[derive(Deserialize)]
struct ActivityQuery {
    account_id: Option<String>,
}
async fn activity(
    State(engine): State<Engine>,
    Query(q): Query<ActivityQuery>,
) -> Json<Vec<crate::model::Activity>> {
    Json(engine.activity(q.account_id.as_deref()))
}
async fn diagnostics(State(engine): State<Engine>) -> Json<serde_json::Value> {
    Json(engine.diagnostics())
}
async fn backups(State(engine): State<Engine>) -> Result<Json<Vec<String>>, ApiError> {
    engine.backups().map(Json).map_err(error)
}
async fn backup(State(engine): State<Engine>) -> Result<Json<String>, ApiError> {
    engine.backup().map(Json).map_err(error)
}
async fn restore(
    State(engine): State<Engine>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    engine
        .restore(&name)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(error)
}
async fn discord_settings(State(engine): State<Engine>) -> Json<crate::discord::DiscordView> {
    Json(engine.discord_settings())
}
async fn discord_update(
    State(engine): State<Engine>,
    Json(patch): Json<crate::discord::DiscordPatch>,
) -> Result<Json<crate::discord::DiscordView>, ApiError> {
    engine.set_discord(patch).map(Json).map_err(error)
}
async fn discord_test(State(engine): State<Engine>) -> Result<StatusCode, ApiError> {
    engine
        .test_discord()
        .map(|_| StatusCode::ACCEPTED)
        .map_err(error)
}
async fn authenticate(
    State(engine): State<Engine>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, ApiError> {
    // Native automation only. Browser origins are rejected even with a valid token.
    if request.headers().contains_key("origin")
        || request
            .headers()
            .get("sec-fetch-site")
            .is_some_and(|v| v != "none")
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error":"Cross-origin requests are disabled"})),
        ));
    }
    let expected = engine.token().as_bytes();
    let actual = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("")
        .as_bytes();
    let mut mismatch = expected.len() ^ actual.len();
    for (i, b) in expected.iter().enumerate() {
        mismatch |= (*b ^ actual.get(i).copied().unwrap_or(0)) as usize;
    }
    if mismatch != 0 {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error":"Bearer token required"})),
        ));
    }
    Ok(next.run(request).await)
}
async fn status(State(engine): State<Engine>) -> Json<Snapshot> {
    Json(engine.snapshot())
}
async fn accounts(State(engine): State<Engine>) -> Json<Vec<crate::model::Account>> {
    Json(engine.snapshot().accounts)
}
#[derive(Deserialize)]
struct Import {
    cookies: Vec<String>,
}
#[derive(serde::Serialize)]
struct ImportResult {
    accounts: Vec<crate::model::Account>,
    errors: Vec<ImportError>,
}
#[derive(serde::Serialize)]
struct ImportError {
    index: usize,
    error: String,
}
async fn import(
    State(engine): State<Engine>,
    Json(mut input): Json<Import>,
) -> Result<Json<ImportResult>, ApiError> {
    if input.cookies.is_empty() || input.cookies.len() > 50 {
        return Err(error("Import 1â€“50 cookies per request".into()));
    }
    let mut result = ImportResult {
        accounts: Vec::new(),
        errors: Vec::new(),
    };
    use zeroize::Zeroize;
    for (index, cookie) in input.cookies.iter_mut().enumerate() {
        match engine.import(cookie).await {
            Ok(a) => result.accounts.push(a),
            Err(e) => result.errors.push(ImportError { index, error: e }),
        };
        cookie.zeroize();
    }
    Ok(Json(result))
}
async fn update(
    State(engine): State<Engine>,
    Path(id): Path<String>,
    Json(patch): Json<AccountPatch>,
) -> Result<Json<crate::model::Account>, ApiError> {
    engine.patch(&id, patch).map(Json).map_err(error)
}
async fn remove(
    State(engine): State<Engine>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    engine
        .remove(&id)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(error)
}
async fn command(
    State(engine): State<Engine>,
    Path((id, action)): Path<(String, String)>,
) -> Result<(StatusCode, Json<crate::model::Operation>), ApiError> {
    engine
        .command(&id, &action)
        .map(|op| (StatusCode::ACCEPTED, Json(op)))
        .map_err(error)
}
async fn repair(
    State(engine): State<Engine>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if !engine.snapshot().accounts.iter().any(|a| a.id == id) {
        return Err(error("Account not found".into()));
    }
    open_login(engine, Some(id))
}
async fn operation(
    State(engine): State<Engine>,
    Path(id): Path<Uuid>,
) -> Result<Json<crate::model::Operation>, ApiError> {
    engine.operation(id).map(Json).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":"Operation not found or expired"})),
        )
    })
}
async fn events(
    State(engine): State<Engine>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let receiver = engine.subscribe();
    let initial = engine.snapshot();
    let recover = engine.clone();
    let stream = futures_util::stream::once(async move {
        Ok(Event::default()
            .event("snapshot")
            .json_data(initial)
            .unwrap())
    })
    .chain(BroadcastStream::new(receiver).map(move |item| {
        let snapshot = item.unwrap_or_else(|_| recover.snapshot());
        Ok(Event::default()
            .event("snapshot")
            .json_data(snapshot)
            .unwrap())
    }));
    Sse::new(stream).keep_alive(KeepAlive::default())
}
pub async fn serve(engine: Engine, port: u16) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|_| "Local API port is unavailable".to_string())?;
    serve_on(engine, listener).await
}
pub async fn serve_on(engine: Engine, listener: tokio::net::TcpListener) -> Result<(), String> {
    let shutdown = engine.clone();
    axum::serve(listener, router(engine))
        .with_graceful_shutdown(async move {
            while !shutdown.is_shutdown() {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        })
        .await
        .map_err(|_| "Local API stopped unexpectedly".into())
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn beta_update_preference_is_authenticated_and_persists() {
        let path = std::env::temp_dir().join(format!("rbx-update-settings-{}", Uuid::new_v4()));
        let engine = Engine::open(
            crate::store::Store::new(path.clone()).unwrap(),
            "fixture".into(),
        )
        .unwrap();
        assert!(!engine.snapshot().include_beta_updates);
        let app = router(engine.clone());
        for authenticated in [false, true] {
            let mut request = Request::builder()
                .method("PATCH")
                .uri("/v1/settings/updates")
                .header("content-type", "application/json");
            if authenticated {
                request = request.header("authorization", format!("Bearer {}", engine.token()));
            }
            let response = app
                .clone()
                .oneshot(
                    request
                        .body(axum::body::Body::from(r#"{"include_beta":true}"#))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if authenticated {
                    StatusCode::NO_CONTENT
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
            assert_eq!(engine.snapshot().include_beta_updates, authenticated);
        }
        drop(app);
        drop(engine);
        let reopened = Engine::open(
            crate::store::Store::new(path.clone()).unwrap(),
            "fixture".into(),
        )
        .unwrap();
        assert!(reopened.snapshot().include_beta_updates);
        reopened.set_beta_updates(false).unwrap();
        drop(reopened);
        let stable = Engine::open(
            crate::store::Store::new(path.clone()).unwrap(),
            "fixture".into(),
        )
        .unwrap();
        assert!(!stable.snapshot().include_beta_updates);
        drop(stable);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[tokio::test]
    async fn management_routes_require_authentication_and_bulk_commit_is_atomic() {
        let path = std::env::temp_dir().join(format!("rbx-management-api-{}", Uuid::new_v4()));
        let store = crate::store::Store::new(path.clone()).unwrap();
        let mut db = crate::model::Database {
            version: 2,
            ..Default::default()
        };
        for id in ["1", "2"] {
            let mut account = crate::model::Account::new(id.into(), format!("user{id}"));
            account.target = Some(crate::model::Target {
                place_id: 1,
                job_id: None,
                private_server_link: None,
            });
            db.accounts.push(crate::model::SavedAccount {
                account,
                encrypted_session: crate::platform::protect("TEST_ONLY_SESSION").unwrap(),
            });
        }
        store.save(&db).unwrap();
        let engine = Engine::open(store, "fixture".into()).unwrap();
        let app = router(engine.clone());
        for (method, route) in [
            ("PATCH", "/v1/accounts/bulk"),
            ("GET", "/v1/profiles"),
            ("POST", "/v1/profiles/import"),
            ("GET", "/v1/activity"),
            ("GET", "/v1/diagnostics"),
            ("GET", "/v1/backups"),
            ("POST", "/v1/accounts/1/repair"),
        ] {
            for origin in [false, true] {
                let mut request = Request::builder().method(method).uri(route);
                if origin {
                    request = request
                        .header("authorization", format!("Bearer {}", engine.token()))
                        .header("origin", "https://example.com");
                }
                let response = app
                    .clone()
                    .oneshot(request.body(axum::body::Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(
                    response.status(),
                    if origin {
                        StatusCode::FORBIDDEN
                    } else {
                        StatusCode::UNAUTHORIZED
                    }
                );
            }
        }
        for (ids, expected) in [
            (vec!["1", "missing"], StatusCode::NOT_FOUND),
            (vec!["1", "2"], StatusCode::OK),
        ] {
            let response = app.clone().oneshot(Request::builder().method("PATCH").uri("/v1/accounts/bulk").header("authorization", format!("Bearer {}", engine.token())).header("content-type", "application/json").body(axum::body::Body::from(serde_json::json!({"account_ids":ids,"patch":{"group":"Team","fallback_policy":"pause"}}).to_string())).unwrap()).await.unwrap();
            assert_eq!(response.status(), expected);
            if expected != StatusCode::OK {
                assert!(
                    engine
                        .snapshot()
                        .accounts
                        .iter()
                        .all(|a| a.group.is_empty())
                );
            }
        }
        assert!(
            engine
                .snapshot()
                .accounts
                .iter()
                .all(|a| a.group == "Team"
                    && a.fallback_policy == crate::model::FallbackPolicy::Pause)
        );
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/diagnostics")
                    .header("authorization", format!("Bearer {}", engine.token()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let text = String::from_utf8_lossy(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .to_string();
        assert!(!text.contains("TEST_ONLY_SESSION"));
        assert!(!text.contains(engine.token()));
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/accounts/missing/repair")
                    .header("authorization", format!("Bearer {}", engine.token()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        drop(app);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[tokio::test]
    async fn browser_sign_in_bridge_requires_authentication_and_rejects_origins() {
        let path = std::env::temp_dir().join(format!("rbx-login-api-test-{}", Uuid::new_v4()));
        let engine = Engine::open(
            crate::store::Store::new(path.clone()).unwrap(),
            "test".into(),
        )
        .unwrap();
        let app = router(engine.clone());
        for origin in [false, true] {
            let mut request = Request::builder().method("POST").uri("/v1/login");
            if origin {
                request = request
                    .header("authorization", format!("Bearer {}", engine.token()))
                    .header("origin", "https://example.com");
            }
            let response = app
                .clone()
                .oneshot(request.body(axum::body::Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                if origin {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::UNAUTHORIZED
                }
            );
            let body = axum::body::to_bytes(response.into_body(), 1024)
                .await
                .unwrap();
            assert!(!String::from_utf8_lossy(&body).contains(engine.token()));
        }
        drop(app);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[tokio::test]
    async fn discord_settings_api_is_authenticated_and_never_exports_webhook() {
        let path = std::env::temp_dir().join(format!("rbx-discord-api-test-{}", Uuid::new_v4()));
        let engine = Engine::open(
            crate::store::Store::new(path.clone()).unwrap(),
            "test".into(),
        )
        .unwrap();
        let app = router(engine.clone());
        for route in ["/v1/settings/discord", "/v1/settings/discord/test"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(if route.ends_with("test") {
                            "POST"
                        } else {
                            "GET"
                        })
                        .uri(route)
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        let patch = serde_json::json!({"enabled":true,"webhook_url":"https://discord.com/api/webhooks/123456/SECRET_WEBHOOK","notify_recovery":false});
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/v1/settings/discord")
                    .header("authorization", format!("Bearer {}", engine.token()))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(patch.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("SECRET"));
        assert!(engine.discord_settings().enabled);
        assert!(!engine.discord_settings().notify_recovery);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/settings/discord/test")
                    .header("authorization", format!("Bearer {}", engine.token()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/settings/discord")
                    .header("authorization", format!("Bearer {}", engine.token()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(!body.contains("SECRET"));
        assert!(!body.contains("encrypted_webhook"));
        assert!(body.contains("Notification queued"));
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[tokio::test]
    async fn api_authentication_origins_and_redaction() {
        let path = std::env::temp_dir().join(format!("rbx-api-test-{}", Uuid::new_v4()));
        let engine = Engine::open(
            crate::store::Store::new(path.clone()).unwrap(),
            "test".into(),
        )
        .unwrap();
        let app = router(engine.clone());
        let anonymous = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/status")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
        let origin = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/status")
                    .header("authorization", format!("Bearer {}", engine.token()))
                    .header("origin", "https://example.com")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(origin.status(), StatusCode::FORBIDDEN);
        let allowed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/status")
                    .header("authorization", format!("Bearer {}", engine.token()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(allowed.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let response = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!response.contains(engine.token()));
        assert!(!response.contains("encrypted_session"));
        let invalid = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/accounts")
                    .header("authorization", format!("Bearer {}", engine.token()))
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(r#"{"cookies":["SECRET; invalid"]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(invalid.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("SECRET"));
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
}
