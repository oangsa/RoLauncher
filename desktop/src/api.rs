//! Authenticated loopback HTTP client and SSE resource stream for the RoLauncher supervisor.

use crate::domain::models::{
    Account, Activity, DiscordView, GameLookupResponse, GameProfile, LaunchProfile,
    ResourceSnapshotResponse, Snapshot, UpdateView,
};
use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use url::Url;
use zeroize::Zeroizing;

/// Queued command operation returned by `POST /v1/accounts/{id}/{action}`.
#[derive(Debug, Clone, Deserialize)]
pub struct OperationView {
    /// Operation state string.
    #[serde(default)]
    pub state: String,
    /// Optional error text when rejected.
    #[serde(default)]
    pub error: Option<String>,
}

/// Outcome returned by `POST /v1/profiles/{id}/apply` or `start`.
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileActionItem {
    /// Target account identifier (`id` on apply, `account_id` on start).
    #[serde(default, alias = "id")]
    pub account_id: String,
    /// Optional per-account error string.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct ApiErrorEnvelope {
    error: Option<String>,
}

#[derive(Deserialize)]
struct CloseAllResponse {
    closed: usize,
}

/// Loopback-only authenticated HTTP client communicating with the Rust supervisor.
#[derive(Clone)]
pub struct ApiClient {
    base_url: Url,
    token: Arc<Zeroizing<String>>,
    http: Client,
    sse_http: Client,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiClient")
            .field("base_url", &self.base_url.as_str())
            .field("token", &"[REDACTED]")
            .finish()
    }
}

impl ApiClient {
    /// Constructs an authenticated loopback client for `127.0.0.1:{port}` and owns a compact Tokio runtime.
    pub fn new(port: u16, token: String) -> Result<Self, String> {
        if port == 0 {
            return Err("Invalid API port".to_string());
        }
        let trimmed = token.trim().to_string();
        if trimmed.is_empty() {
            return Err("Missing API bearer token".to_string());
        }
        let base_url = Url::parse(&format!("http://127.0.0.1:{port}/v1/"))
            .map_err(|err| format!("Invalid loopback API URL: {err}"))?;

        let http = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|err| format!("Unable to initialize HTTP client: {err}"))?;

        let sse_http = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|err| format!("Unable to initialize SSE client: {err}"))?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rolauncher-api")
            .enable_all()
            .build()
            .map_err(|err| format!("Unable to start desktop async runtime: {err}"))?;

        Ok(Self {
            base_url,
            token: Arc::new(Zeroizing::new(trimmed)),
            http,
            sse_http,
            runtime: Arc::new(runtime),
        })
    }

    /// Returns a cloneable Tokio runtime handle for running background API/update tasks.
    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    fn endpoint(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<Url, String> {
        let mut url = self.base_url.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| "Loopback API URL cannot have path segments".to_string())?;
            path.pop_if_empty();
            for segment in segments {
                path.push(segment);
            }
        }
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        Ok(url)
    }

    async fn send_json<B: Serialize, R: DeserializeOwned>(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, &str)],
        body: Option<&B>,
        timeout: Option<Duration>,
    ) -> Result<R, String> {
        let text = self
            .send_raw(method, segments, query, body, timeout)
            .await?;
        serde_json::from_str(&text)
            .map_err(|err| format!("Unexpected response from local engine: {err}"))
    }

    async fn send_empty<B: Serialize>(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, &str)],
        body: Option<&B>,
        timeout: Option<Duration>,
    ) -> Result<(), String> {
        let _ = self
            .send_raw(method, segments, query, body, timeout)
            .await?;
        Ok(())
    }

    async fn execute_request(
        &self,
        method: Method,
        url: Url,
        json_body: Option<serde_json::Value>,
        timeout: Option<Duration>,
    ) -> Result<Result<(StatusCode, String), reqwest::Error>, tokio::task::JoinError> {
        let http = self.http.clone();
        let token = self.token.clone();
        self.runtime
            .spawn(async move {
                let mut req = http
                    .request(method, url)
                    .header(AUTHORIZATION, format!("Bearer {}", token.as_str()));
                if let Some(duration) = timeout {
                    req = req.timeout(duration);
                }
                if let Some(payload) = json_body {
                    req = req.json(&payload);
                }
                let resp = req.send().await?;
                let status = resp.status();
                let text = resp.text().await?;
                Ok((status, text))
            })
            .await
    }

    async fn send_raw<B: Serialize>(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, &str)],
        body: Option<&B>,
        timeout: Option<Duration>,
    ) -> Result<String, String> {
        let url = self.endpoint(segments, query)?;
        let json_body = body
            .map(serde_json::to_value)
            .transpose()
            .map_err(|err| format!("Failed serializing request payload: {err}"))?;

        let (status, text) = self
            .execute_request(method, url, json_body, timeout)
            .await
            .map_err(|_| "Local engine request interrupted.".to_string())?
            .map_err(|err| format!("Unable to reach local engine: {err}"))?;

        if !status.is_success() {
            if let Ok(parsed) = serde_json::from_str::<ApiErrorEnvelope>(&text)
                && let Some(msg) = parsed.error.filter(|m| !m.trim().is_empty())
            {
                return Err(msg);
            }
            return Err(format!(
                "Local engine rejected request (HTTP {})",
                status.as_u16()
            ));
        }

        Ok(text)
    }

    /// Fetches the complete runtime snapshot (`GET /v1/status`).
    pub async fn status(&self) -> Result<Snapshot, String> {
        self.send_json::<(), Snapshot>(Method::GET, &["status"], &[], None, None)
            .await
    }

    /// Fetches activity log entries (`GET /v1/activity`).
    pub async fn activity(&self, account_id: Option<&str>) -> Result<Vec<Activity>, String> {
        let query: Vec<(&str, &str)> = match account_id {
            Some(id) if !id.is_empty() => vec![("account_id", id)],
            _ => Vec::new(),
        };
        self.send_json::<(), Vec<Activity>>(Method::GET, &["activity"], &query, None, None)
            .await
    }

    /// Fetches engine diagnostics (`GET /v1/diagnostics`).
    pub async fn diagnostics(&self) -> Result<serde_json::Value, String> {
        self.send_json::<(), serde_json::Value>(Method::GET, &["diagnostics"], &[], None, None)
            .await
    }

    /// Requests closing all managed Roblox instances (`POST /v1/instances/close-all`, 5 min timeout).
    pub async fn close_all_instances(&self) -> Result<usize, String> {
        let res: CloseAllResponse = self
            .send_json::<(), CloseAllResponse>(
                Method::POST,
                &["instances", "close-all"],
                &[],
                None,
                Some(Duration::from_secs(300)),
            )
            .await?;
        Ok(res.closed)
    }

    /// Opens the WebView2 browser login dialog owned by the supervisor (`POST /v1/login`).
    pub async fn browser_login(&self) -> Result<(), String> {
        self.send_empty::<()>(Method::POST, &["login"], &[], None, None)
            .await
    }

    /// Imports 1–50 `.ROBLOSECURITY` cookies via the supervisor (`POST /v1/accounts`, 10 min timeout).
    pub async fn import_cookies_batch(
        &self,
        cookies: Vec<String>,
    ) -> Result<serde_json::Value, crate::domain::cookie_batch::BatchTransportError> {
        use crate::domain::cookie_batch::BatchTransportError;

        let url = self
            .endpoint(&["accounts"], &[])
            .map_err(|_| BatchTransportError::Failed)?;
        let payload = serde_json::json!({ "cookies": cookies });
        let (status, text) = self
            .execute_request(
                Method::POST,
                url,
                Some(payload),
                Some(Duration::from_secs(600)),
            )
            .await
            .map_err(|_| BatchTransportError::Failed)?
            .map_err(|err| {
                if err.is_timeout() {
                    BatchTransportError::Timeout
                } else {
                    BatchTransportError::Failed
                }
            })?;
        if !status.is_success() {
            return Err(BatchTransportError::Failed);
        }
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|_| BatchTransportError::InvalidResponse)
    }

    /// Updates one account (`PATCH /v1/accounts/{id}`).
    pub async fn patch_account(
        &self,
        id: &str,
        patch: serde_json::Value,
    ) -> Result<Account, String> {
        self.send_json(Method::PATCH, &["accounts", id], &[], Some(&patch), None)
            .await
    }

    /// Updates multiple selected accounts (`PATCH /v1/accounts/bulk`).
    pub async fn bulk_patch(
        &self,
        account_ids: Vec<String>,
        patch: serde_json::Value,
    ) -> Result<Vec<Account>, String> {
        let body = serde_json::json!({
            "account_ids": account_ids,
            "patch": patch,
        });
        self.send_json(Method::PATCH, &["accounts", "bulk"], &[], Some(&body), None)
            .await
    }

    /// Deletes one account (`DELETE /v1/accounts/{id}`).
    pub async fn delete_account(&self, id: &str) -> Result<(), String> {
        self.send_empty::<()>(Method::DELETE, &["accounts", id], &[], None, None)
            .await
    }

    /// Invokes a lifecycle command (`start`, `stop`, `restart`, `retry`) on an account.
    pub async fn account_command(&self, id: &str, action: &str) -> Result<OperationView, String> {
        self.send_json::<(), OperationView>(
            Method::POST,
            &["accounts", id, action],
            &[],
            None,
            None,
        )
        .await
    }

    /// Opens WebView2 repair login for an existing account (`POST /v1/accounts/{id}/repair`).
    pub async fn repair_account(&self, id: &str) -> Result<(), String> {
        self.send_empty::<()>(Method::POST, &["accounts", id, "repair"], &[], None, None)
            .await
    }

    /// Looks up place name & thumbnail metadata (`GET /v1/games/{place_id}`).
    pub async fn game_details(&self, place_id: u64) -> Result<GameLookupResponse, String> {
        let id = place_id.to_string();
        self.send_json::<(), GameLookupResponse>(Method::GET, &["games", &id], &[], None, None)
            .await
    }

    /// Saves or updates a saved game entry (`POST /v1/game-profiles`).
    pub async fn save_game_profile(&self, profile: &GameProfile) -> Result<GameProfile, String> {
        self.send_json(Method::POST, &["game-profiles"], &[], Some(profile), None)
            .await
    }

    /// Deletes a saved game entry (`DELETE /v1/game-profiles/{id}`).
    pub async fn delete_game_profile(&self, id: &str) -> Result<(), String> {
        self.send_empty::<()>(Method::DELETE, &["game-profiles", id], &[], None, None)
            .await
    }

    /// Saves or updates an account preset (`POST /v1/profiles`).
    pub async fn save_profile(
        &self,
        id: Option<&str>,
        name: &str,
        account_ids: Vec<String>,
    ) -> Result<LaunchProfile, String> {
        let body = serde_json::json!({
            "id": id,
            "name": name,
            "account_ids": account_ids,
        });
        self.send_json(Method::POST, &["profiles"], &[], Some(&body), None)
            .await
    }

    /// Imports launch presets (`POST /v1/profiles/import`).
    pub async fn import_profiles(&self, profiles: &[LaunchProfile]) -> Result<usize, String> {
        self.send_json(
            Method::POST,
            &["profiles", "import"],
            &[],
            Some(&profiles),
            None,
        )
        .await
    }

    /// Deletes a launch preset (`DELETE /v1/profiles/{id}`).
    pub async fn delete_profile(&self, id: &str) -> Result<(), String> {
        self.send_empty::<()>(Method::DELETE, &["profiles", id], &[], None, None)
            .await
    }

    /// Applies or launches a preset (`POST /v1/profiles/{id}/{action}`).
    pub async fn profile_action(
        &self,
        id: &str,
        action: &str,
    ) -> Result<Vec<ProfileActionItem>, String> {
        self.send_json::<(), Vec<ProfileActionItem>>(
            Method::POST,
            &["profiles", id, action],
            &[],
            None,
            None,
        )
        .await
    }

    /// Persists close-to-tray behavior (`PATCH /v1/settings/window`).
    pub async fn set_window_settings(&self, close_to_tray: bool) -> Result<(), String> {
        let body = serde_json::json!({ "close_to_tray": close_to_tray });
        self.send_empty(
            Method::PATCH,
            &["settings", "window"],
            &[],
            Some(&body),
            None,
        )
        .await
    }

    /// Persists server selection and join type (`PATCH /v1/settings/game`).
    pub async fn set_game_settings(
        &self,
        server_selection: &str,
        join_type: &str,
    ) -> Result<(), String> {
        let body = serde_json::json!({
            "server_selection": server_selection,
            "join_type": join_type,
        });
        self.send_empty(Method::PATCH, &["settings", "game"], &[], Some(&body), None)
            .await
    }

    /// Persists multi-instance setting (`PATCH /v1/settings/instances`).
    pub async fn set_multi_instance(&self, enabled: bool) -> Result<(), String> {
        let body = serde_json::json!({ "multi_instance_enabled": enabled });
        self.send_empty(
            Method::PATCH,
            &["settings", "instances"],
            &[],
            Some(&body),
            None,
        )
        .await
    }

    /// Persists startup background cleanup setting (`PATCH /v1/settings/startup-cleanup`).
    pub async fn set_startup_cleanup(&self, enabled: bool) -> Result<(), String> {
        let body = serde_json::json!({ "external_background_cleanup_enabled": enabled });
        self.send_empty(
            Method::PATCH,
            &["settings", "startup-cleanup"],
            &[],
            Some(&body),
            None,
        )
        .await
    }

    /// Lists available encrypted backup filenames (`GET /v1/backups`).
    pub async fn backups(&self) -> Result<Vec<String>, String> {
        self.send_json::<(), Vec<String>>(Method::GET, &["backups"], &[], None, None)
            .await
    }

    /// Creates a new encrypted backup (`POST /v1/backups`).
    pub async fn create_backup(&self) -> Result<String, String> {
        self.send_json::<(), String>(Method::POST, &["backups"], &[], None, None)
            .await
    }

    /// Restores an encrypted backup (`POST /v1/backups/{name}/restore`).
    pub async fn restore_backup(&self, name: &str) -> Result<(), String> {
        self.send_empty::<()>(Method::POST, &["backups", name, "restore"], &[], None, None)
            .await
    }

    /// Checks official repository for updates (`POST /v1/updates/official/check`).
    pub async fn check_official_updates(&self) -> Result<UpdateView, String> {
        self.send_json::<(), UpdateView>(
            Method::POST,
            &["updates", "official", "check"],
            &[],
            None,
            None,
        )
        .await
    }

    /// Updates beta update opt-in setting (`PATCH /v1/settings/updates`).
    pub async fn set_beta_updates(&self, include_beta: bool) -> Result<(), String> {
        let body = serde_json::json!({ "include_beta": include_beta });
        self.send_empty(
            Method::PATCH,
            &["settings", "updates"],
            &[],
            Some(&body),
            None,
        )
        .await
    }

    /// Fetches sanitized Discord notification and bot settings (`GET /v1/settings/discord`).
    pub async fn discord_settings(&self) -> Result<DiscordView, String> {
        self.send_json::<(), DiscordView>(Method::GET, &["settings", "discord"], &[], None, None)
            .await
    }

    /// Updates Discord webhook/notification/bot settings (`PATCH /v1/settings/discord`).
    pub async fn update_discord(&self, patch: serde_json::Value) -> Result<DiscordView, String> {
        self.send_json(
            Method::PATCH,
            &["settings", "discord"],
            &[],
            Some(&patch),
            None,
        )
        .await
    }

    /// Sends a Discord test notification (`POST /v1/settings/discord/test`).
    pub async fn test_discord(&self) -> Result<(), String> {
        self.send_empty::<()>(
            Method::POST,
            &["settings", "discord", "test"],
            &[],
            None,
            None,
        )
        .await
    }

    /// Starts, stops, or restarts the Discord bot (`POST /v1/settings/discord/bot/{action}`).
    pub async fn bot_control(&self, action: &str) -> Result<DiscordView, String> {
        self.send_json::<(), DiscordView>(
            Method::POST,
            &["settings", "discord", "bot", action],
            &[],
            None,
            None,
        )
        .await
    }

    /// Fetches one-off resource monitor snapshot (`GET /v1/resources`).
    pub async fn resources(
        &self,
        history: bool,
        seconds: usize,
    ) -> Result<ResourceSnapshotResponse, String> {
        let hist_str = if history { "true" } else { "false" };
        let sec_str = seconds.to_string();
        self.send_json::<(), ResourceSnapshotResponse>(
            Method::GET,
            &["resources"],
            &[("history", hist_str), ("seconds", &sec_str)],
            None,
            None,
        )
        .await
    }

    /// Streams `event: resources` snapshots from `GET /v1/resources/events` until `stop_rx` is true.
    pub async fn stream_resources_events<F, E>(
        &self,
        mut stop_rx: watch::Receiver<bool>,
        mut on_snapshot: F,
        mut on_status: E,
    ) where
        F: FnMut(ResourceSnapshotResponse) + Send,
        E: FnMut(Option<String>) + Send,
    {
        let mut backoff = Duration::from_secs(1);

        loop {
            if *stop_rx.borrow() {
                break;
            }
            let url = match self.endpoint(&["resources", "events"], &[]) {
                Ok(u) => u,
                Err(err) => {
                    on_status(Some(err));
                    break;
                }
            };

            let connect = tokio::select! {
                _ = stop_rx.changed() => break,
                res = self.sse_http
                    .request(Method::GET, url)
                    .header(AUTHORIZATION, format!("Bearer {}", self.token.as_str()))
                    .header(ACCEPT, "text/event-stream")
                    .send() => res,
            };

            match connect {
                Ok(mut resp) if resp.status() == StatusCode::OK => {
                    backoff = Duration::from_secs(1);
                    on_status(None);
                    let mut parser = SseFrameParser::default();

                    loop {
                        let next_chunk = tokio::select! {
                            _ = stop_rx.changed() => {
                                if *stop_rx.borrow() {
                                    return;
                                }
                                continue;
                            }
                            chunk = resp.chunk() => chunk,
                        };

                        match next_chunk {
                            Ok(Some(bytes)) => {
                                for event in parser.push_bytes(&bytes) {
                                    if event.event == "resources"
                                        && let Ok(snap) =
                                            serde_json::from_str::<ResourceSnapshotResponse>(
                                                &event.data,
                                            )
                                    {
                                        on_snapshot(snap);
                                    }
                                }
                            }
                            Ok(None) => break,
                            Err(err) => {
                                on_status(Some(format!("Resource stream interrupted: {err}")));
                                break;
                            }
                        }
                    }
                }
                Ok(resp) => {
                    on_status(Some(format!(
                        "Resource stream unavailable (HTTP {})",
                        resp.status().as_u16()
                    )));
                }
                Err(err) => {
                    on_status(Some(format!("Resource stream unreachable: {err}")));
                }
            }

            if *stop_rx.borrow() {
                break;
            }
            tokio::select! {
                _ = stop_rx.changed() => break,
                _ = tokio::time::sleep(backoff) => {}
            }
            backoff = (backoff * 2).min(Duration::from_secs(5));
        }
    }

    /// Returns a cloned copy of the bearer token ONLY for the controlled Support reveal dialog.
    pub fn reveal_token_for_support(&self) -> Zeroizing<String> {
        Zeroizing::new(self.token.as_str().to_string())
    }
}

/// Incremental UTF-8 line parser for Server-Sent Events (`event:` / `data:` frames).
#[derive(Default)]
pub struct SseFrameParser {
    buffer: String,
    current_event: String,
    data_lines: Vec<String>,
}

/// Single parsed SSE frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEventFrame {
    /// SSE event name (`"message"` when omitted).
    pub event: String,
    /// Joined `data:` lines for the event.
    pub data: String,
}

impl SseFrameParser {
    /// Feeds raw UTF-8 chunk bytes and returns any completed SSE frames.
    pub fn push_bytes(&mut self, bytes: &[u8]) -> Vec<SseEventFrame> {
        self.buffer.push_str(&String::from_utf8_lossy(bytes));
        let mut out = Vec::new();

        while let Some(pos) = self.buffer.find('\n') {
            let mut line = self.buffer[..pos].to_string();
            self.buffer.drain(..=pos);
            if line.ends_with('\r') {
                line.pop();
            }

            if line.is_empty() {
                if !self.data_lines.is_empty() {
                    let event = if self.current_event.is_empty() {
                        "message".to_string()
                    } else {
                        std::mem::take(&mut self.current_event)
                    };
                    let data = self.data_lines.join("\n");
                    self.data_lines.clear();
                    out.push(SseEventFrame { event, data });
                } else {
                    self.current_event.clear();
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("event:") {
                self.current_event = rest.trim_start().to_string();
            } else if let Some(rest) = line.strip_prefix("data:") {
                self.data_lines.push(rest.trim_start().to_string());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn sse_parser_handles_split_chunks_and_crlf() {
        let mut parser = SseFrameParser::default();
        assert!(parser.push_bytes(b"event: resou").is_empty());
        assert!(parser.push_bytes(b"rces\r\ndata: {\"ok\":\r\n").is_empty());
        let events = parser.push_bytes(b"data: true}\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "resources");
        assert_eq!(events[0].data, "{\"ok\":\ntrue}");
    }

    #[test]
    fn api_client_redacts_token_and_builds_encoded_loopback_urls() {
        let api = ApiClient::new(45123, "secret-token-value".into()).unwrap();
        let debug_str = format!("{api:?}");
        assert!(!debug_str.contains("secret-token-value"));
        let url = api
            .endpoint(&["backups", "snap 1/evil.enc", "restore"], &[("x", "1")])
            .unwrap();
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert!(url.as_str().contains("snap%201%2Fevil.enc"));
    }

    #[test]
    fn api_client_sends_bearer_token_and_parses_error_envelope() {
        let api = ApiClient::new(1, "tok-123".into()).unwrap();
        let handle = api.runtime_handle();
        let port = handle.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]);
                assert!(req.contains("GET /v1/backups HTTP/1.1"));
                assert!(req.to_ascii_lowercase().contains("authorization: bearer tok-123"));
                let body = r#"{"error":"Sample rejection"}"#;
                let resp = format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket.write_all(resp.as_bytes()).await.unwrap();
            });
            port
        });

        let api_live = ApiClient::new(port, "tok-123".into()).unwrap();
        let err = api_live
            .runtime_handle()
            .block_on(api_live.backups())
            .expect_err("HTTP 400 should surface engine error text");
        assert_eq!(err, "Sample rejection");
    }
}
