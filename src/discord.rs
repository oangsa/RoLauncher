use crate::model::{Account, Status};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::Mutex, time::Duration};
use tokio::sync::{Notify, mpsc};
use zeroize::Zeroizing;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedDiscord {
    pub enabled: bool,
    pub encrypted_webhook: String,
    #[serde(default = "default_recovery")]
    pub notify_recovery: bool,
}
fn default_recovery() -> bool {
    true
}
impl Default for SavedDiscord {
    fn default() -> Self {
        Self {
            enabled: false,
            encrypted_webhook: String::new(),
            notify_recovery: true,
        }
    }
}

// Deliberately no Debug/Serialize: the incoming URL contains a credential.
#[derive(Deserialize)]
pub struct DiscordPatch {
    pub enabled: Option<bool>,
    pub webhook_url: Option<String>,
    pub notify_recovery: Option<bool>,
}
impl Drop for DiscordPatch {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        if let Some(url) = &mut self.webhook_url {
            url.zeroize();
        }
    }
}

/// Restrict outbound requests to Discord's incoming webhook endpoint.
pub fn validate_webhook(value: &str) -> Result<Zeroizing<String>, String> {
    let invalid = || {
        "Paste a Discord channel webhook URL (https://discord.com/api/webhooks/ID/TOKEN)"
            .to_string()
    };
    let url = url::Url::parse(value.trim()).map_err(|_| invalid())?;
    let parts: Vec<_> = url.path().split('/').collect();
    let tail = match parts.as_slice() {
        ["", "api", "webhooks", id, token] => Some((*id, *token)),
        ["", "api", "v10", "webhooks", id, token] => Some((*id, *token)),
        _ => None,
    };
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("discord.com" | "discordapp.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || value.len() > 512
        || !tail.is_some_and(|(id, token)| {
            !id.is_empty()
                && id.len() <= 20
                && id.bytes().all(|c| c.is_ascii_digit())
                && !token.is_empty()
                && token
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        })
    {
        return Err(invalid());
    }
    Ok(Zeroizing::new(url.to_string()))
}

#[derive(Clone, Debug, Serialize)]
pub struct Notice {
    pub timestamp: DateTime<Utc>,
    pub account: String,
    pub title: String,
    pub message: String,
    pub place_id: Option<u64>,
    pub recovery: bool,
    pub color: u32,
}
impl Notice {
    pub fn network(suspended: bool) -> Self {
        Self { timestamp: Utc::now(), account: "RoLauncher".into(),
            title: if suspended { "Connection unavailable" } else { "Connection restored" }.into(),
            message: if suspended { "New launches and automatic rejoins are waiting for your Internet connection. Existing clients are still monitored." } else { "The connection is available again. Pending automatic rejoins can continue." }.into(),
            place_id: None, recovery: !suspended, color: if suspended { 0xfee75c } else { 0x57f287 } }
    }
    pub fn test() -> Self {
        Self { timestamp: Utc::now(), account: "RoLauncher".into(), title: "Discord notifications are ready".into(),
            message: "Test message received. Account alerts will appear in this channel when notifications are enabled.".into(),
            place_id: None, recovery: false, color: 0x57f287 }
    }
    fn for_account(a: &Account, title: &str, message: String, recovery: bool, color: u32) -> Self {
        Self {
            timestamp: Utc::now(),
            account: format!("{} (@{})", a.alias, a.username),
            title: title.into(),
            message,
            place_id: a.target.as_ref().map(|t| t.place_id),
            recovery,
            color,
        }
    }
    fn payload(&self) -> serde_json::Value {
        // Escape user-selected aliases and bound even corrupted/imported account names.
        let account: String = self
            .account
            .chars()
            .filter(|c| !c.is_control())
            .take(160)
            .flat_map(|c| {
                if "\\`*_~|<>[]()".contains(c) {
                    vec!['\\', c]
                } else {
                    vec![c]
                }
            })
            .collect();
        let mut fields =
            vec![serde_json::json!({"name":"Account", "value":account, "inline":true})];
        if let Some(place) = self.place_id {
            fields.push(
                serde_json::json!({"name":"Place", "value":place.to_string(), "inline":true}),
            );
        }
        serde_json::json!({"username":"RoLauncher", "allowed_mentions":{"parse":[]}, "embeds":[{
            "title":self.title, "description":self.message, "color":self.color, "fields":fields,
            "timestamp":self.timestamp.to_rfc3339(), "footer":{"text":concat!("RoLauncher ", env!("CARGO_PKG_VERSION"))}
        }]})
    }
}

fn rejoin(a: &Account) -> &'static str {
    if a.auto_recovery {
        "Waiting up to 30 seconds for reconnection, then automatic rejoin will try again."
    } else {
        "Automatic rejoin is off. Use Start or Restart when you are ready."
    }
}

/// Only known summaries are exported; raw errors/logs/targets never leave the app.
pub fn account_notice(old: &Account, new: &Account) -> Option<Notice> {
    let error = new.last_error.as_deref().unwrap_or("");
    if new.status == Status::Reconnecting
        && (old.status != Status::Reconnecting
            || (error.starts_with("Session lost (code 267)")
                && !old
                    .last_error
                    .as_deref()
                    .unwrap_or("")
                    .starts_with("Session lost (code 267)")))
    {
        let (title, reason) = if error.starts_with("Session lost (code 267)") {
            ("Account kicked", "Roblox reported a kick (code 267).")
        } else if error.starts_with("Connection failed:") {
            (
                "Unable to connect",
                "The game server did not respond (code 279).",
            )
        } else {
            (
                "Account disconnected",
                "The game connection was lost or the join failed.",
            )
        };
        return Some(Notice::for_account(
            new,
            title,
            format!("{reason} {}", rejoin(new)),
            false,
            0xfee75c,
        ));
    }
    if old.process.is_some()
        && new.process.is_none()
        && old.desired_running
        && !matches!(
            old.status,
            Status::Queued | Status::Backoff | Status::NeedsAttention
        )
        && new.status != Status::NeedsAttention
        && old.generation == new.generation
    {
        return Some(Notice::for_account(
            new,
            "Roblox client closed",
            if new.auto_recovery {
                "The managed Roblox client exited. Automatic rejoin will try again.".into()
            } else {
                "The managed Roblox client exited. Automatic rejoin is off; use Start to play again.".into()
            },
            false,
            0xfee75c,
        ));
    }
    if new.status == Status::NeedsAttention && old.status != Status::NeedsAttention {
        if error == "Automatic rejoin disabled; use Start to launch manually" {
            return None;
        }
        let advice = if error.contains("Session expired")
            || error.to_ascii_lowercase().contains("sign in")
            || error.contains("HTTP 401")
        {
            "Sign in to this account again, then use Start."
        } else if error.contains("permission") || error.contains("denied access") {
            "Check this account's access to the game, then use Start."
        } else if new.failures >= 5 {
            "Five attempts failed. Check the game and your connection, then use Start."
        } else {
            "Open RoLauncher and check this account's status before restarting it."
        };
        return Some(Notice::for_account(
            new,
            "Account needs attention",
            format!("Automatic rejoin is paused. {advice}"),
            false,
            0xed4245,
        ));
    }
    if new.status == Status::Unknown
        && old.status != Status::Unknown
        && (error.starts_with("Log unavailable")
            || error.starts_with("Process metadata inaccessible"))
    {
        return Some(Notice::for_account(new, "Account monitoring interrupted",
            "RoLauncher cannot verify this client's connection right now. Automatic replacement is paused; check the account's status in RoLauncher.".into(), false, 0xfee75c));
    }
    if new.status == Status::Backoff
        && let Some(next_retry) = new.next_retry
        && (old.status != Status::Backoff || old.failures != new.failures)
    {
        let message = format!(
            "Automatic rejoin attempt {} is scheduled for {}.{}",
            new.failures,
            next_retry.format("%H:%M:%S UTC"),
            if new.public_fallback_active {
                " The configured server is unavailable; a public server will be used."
            } else {
                ""
            }
        );
        return Some(Notice::for_account(
            new,
            "Rejoin scheduled",
            message,
            true,
            0x5865f2,
        ));
    }
    if new.status == Status::Running
        && old.status != Status::Running
        && (old.status == Status::Reconnecting || new.failures > 0)
    {
        return Some(Notice::for_account(
            new,
            "Account is back online",
            "The game connection is confirmed. You are back in the game.".into(),
            true,
            0x57f287,
        ));
    }
    None
}

#[derive(Clone, Serialize)]
pub struct DiscordView {
    pub enabled: bool,
    pub configured: bool,
    pub notify_recovery: bool,
    pub delivery_status: String,
    pub recent: Vec<Notice>,
}
struct Config {
    saved: SavedDiscord,
    revision: u64,
    status: String,
    recent: VecDeque<Notice>,
    stopped: bool,
    rejected: bool,
}
struct Queued {
    notice: Notice,
    revision: u64,
    test: bool,
}
pub struct Discord {
    config: Mutex<Config>,
    sender: mpsc::Sender<Queued>,
    receiver: Mutex<Option<mpsc::Receiver<Queued>>>,
    changed: Notify,
    client: reqwest::Client,
}
impl Discord {
    pub fn new(saved: SavedDiscord) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel(128);
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "Unable to prepare Discord notifications")?;
        Ok(Self {
            config: Mutex::new(Config {
                saved,
                revision: 0,
                status: "No messages sent this session.".into(),
                recent: VecDeque::new(),
                stopped: false,
                rejected: false,
            }),
            sender,
            receiver: Mutex::new(Some(receiver)),
            changed: Notify::new(),
            client,
        })
    }
    pub fn configure(&self, saved: SavedDiscord) {
        let mut c = self.config.lock().unwrap();
        c.saved = saved;
        c.revision += 1;
        c.rejected = false;
        c.status = "Settings saved. Use Send test to check the channel.".into();
        self.changed.notify_one();
    }
    pub fn view(&self) -> DiscordView {
        let c = self.config.lock().unwrap();
        DiscordView {
            enabled: c.saved.enabled,
            configured: !c.saved.encrypted_webhook.is_empty(),
            notify_recovery: c.saved.notify_recovery,
            delivery_status: c.status.clone(),
            recent: c.recent.iter().cloned().collect(),
        }
    }
    pub fn record(&self, notice: Notice, test: bool) -> Result<(), String> {
        let mut c = self.config.lock().unwrap();
        if c.stopped {
            return Err("Notifications stopped; reopen RoLauncher to resume".into());
        }
        if test && c.saved.encrypted_webhook.is_empty() {
            return Err("Save a Discord webhook before sending a test".into());
        }
        c.recent.push_front(notice.clone());
        c.recent.truncate(100);
        if !test
            && (!c.saved.enabled || c.rejected || (notice.recovery && !c.saved.notify_recovery))
        {
            return Ok(());
        }
        let revision = c.revision;
        self.sender
            .try_send(Queued {
                notice,
                revision,
                test,
            })
            .map_err(|_| {
                c.status =
                    "Notification queue full; this alert was kept in Recent activity only.".into();
                c.status.clone()
            })?;
        c.status = "Notification queued.".into();
        Ok(())
    }
    pub fn stop(&self) {
        self.config.lock().unwrap().stopped = true;
        self.changed.notify_one();
    }
    fn current(&self, q: &Queued) -> Option<SavedDiscord> {
        let c = self.config.lock().unwrap();
        if c.stopped || c.revision != q.revision || (!q.test && (!c.saved.enabled || c.rejected)) {
            None
        } else {
            Some(c.saved.clone())
        }
    }
    fn status(&self, q: &Queued, value: &str) {
        let mut c = self.config.lock().unwrap();
        if c.revision == q.revision {
            c.status = value.into();
        }
    }
    pub async fn run(&self) {
        let Some(mut receiver) = self.receiver.lock().unwrap().take() else {
            return;
        };
        let mut cooldown = tokio::time::Instant::now();
        loop {
            if self.config.lock().unwrap().stopped {
                return;
            }
            let q = tokio::select! {
                q = receiver.recv() => match q { Some(q) => q, None => return },
                _ = self.changed.notified() => continue,
            };
            for attempt in 0..3 {
                // Changes or Exit cancel queued/retrying alerts. An issued request may finish.
                while cooldown > tokio::time::Instant::now() && self.current(&q).is_some() {
                    tokio::select! { _ = tokio::time::sleep_until(cooldown) => {}, _ = self.changed.notified() => {} }
                }
                let Some(saved) = self.current(&q) else {
                    break;
                };
                let hook = crate::platform::unprotect(&saved.encrypted_webhook)
                    .and_then(|value| validate_webhook(&value));
                let Ok(hook) = hook else {
                    self.status(
                        &q,
                        "Saved webhook could not be read. Paste and save it again.",
                    );
                    break;
                };
                match send_attempt(&self.client, &hook, &q.notice.payload()).await {
                    Outcome::Sent(delay) => {
                        let mut c = self.config.lock().unwrap();
                        if c.revision == q.revision {
                            c.rejected = false;
                        }
                        drop(c);
                        cooldown = tokio::time::Instant::now() + delay.max(Duration::from_secs(1));
                        self.status(
                            &q,
                            if q.test {
                                "Test delivered. Check your Discord channel."
                            } else {
                                "Last notification delivered to Discord."
                            },
                        );
                        break;
                    }
                    Outcome::Retry(delay) => {
                        cooldown = tokio::time::Instant::now() + delay;
                        self.status(&q, if attempt == 2 { "Discord is busy; alert could not be delivered. See Recent activity." } else { "Discord is busy; waiting before retrying." });
                    }
                    Outcome::Failed(message) => {
                        cooldown = tokio::time::Instant::now() + Duration::from_secs(30);
                        self.status(&q, message);
                        break;
                    }
                    Outcome::Rejected(message) => {
                        let mut c = self.config.lock().unwrap();
                        if c.revision == q.revision {
                            c.rejected = true;
                            c.status = message.into();
                        }
                        break;
                    }
                }
            }
        }
    }
}

enum Outcome {
    Sent(Duration),
    Retry(Duration),
    Failed(&'static str),
    Rejected(&'static str),
}
fn seconds(value: f64) -> Option<Duration> {
    (value.is_finite() && (0.0..=3600.0).contains(&value))
        .then(|| Duration::from_secs_f64(value.max(1.0)))
}
async fn send_attempt(
    client: &reqwest::Client,
    hook: &str,
    payload: &serde_json::Value,
) -> Outcome {
    // Never return reqwest errors: they can include the secret webhook URL.
    let response = client
        .post(hook)
        .query(&[("wait", "true")])
        .json(payload)
        .send()
        .await;
    let Ok(mut response) = response else {
        return Outcome::Failed(
            "Cannot reach Discord. Check your connection; see Recent activity for the alert.",
        );
    };
    let code = response.status().as_u16();
    let header_seconds = |name| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<f64>().ok())
            .and_then(seconds)
    };
    let cooldown = if response
        .headers()
        .get("x-ratelimit-remaining")
        .is_some_and(|v| v == "0")
    {
        header_seconds("x-ratelimit-reset-after").unwrap_or(Duration::from_secs(3600))
    } else {
        Duration::ZERO
    };
    if (200..300).contains(&code) {
        return Outcome::Sent(cooldown);
    }
    if code == 429 {
        // An invalid or unusually long server delay pauses delivery instead of retrying early.
        if response.headers().contains_key("retry-after") && header_seconds("retry-after").is_none()
        {
            return Outcome::Rejected(
                "Discord rate limited notifications for an extended delay. Try again later by saving settings.",
            );
        }
        let header = header_seconds("retry-after");
        let mut bytes = Vec::new();
        while let Ok(Some(chunk)) = response.chunk().await {
            if bytes.len() + chunk.len() > 8192 {
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v.get("retry_after").and_then(|n| n.as_f64()))
            .and_then(seconds);
        let delay = match (header, body) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        return match delay {
            Some(delay) => Outcome::Retry(delay.max(cooldown)),
            None => Outcome::Rejected(
                "Discord rate limited notifications. Try again later; see Recent activity.",
            ),
        };
    }
    match code {
        401 | 403 | 404 => Outcome::Rejected(
            "Discord rejected the webhook. Check its permissions or paste a new webhook.",
        ),
        500..=599 => Outcome::Retry(Duration::from_secs(5)),
        _ => Outcome::Rejected(
            "Discord could not accept the message. Use a webhook for a regular text channel.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProcessIdentity, Target};
    use uuid::Uuid;

    const HOOK: &str = "https://discord.com/api/webhooks/123456/SECRET_token";
    #[test]
    fn webhook_destination_validation_and_redaction() {
        assert!(validate_webhook(HOOK).is_ok());
        assert!(validate_webhook("https://discordapp.com/api/v10/webhooks/123/token").is_ok());
        for url in [
            "http://discord.com/api/webhooks/123/SECRET",
            "https://discord.com.evil.test/api/webhooks/123/SECRET",
            "https://discord.com@evil.test/api/webhooks/123/SECRET",
            "https://127.0.0.1/api/webhooks/123/SECRET",
            "https://discord.com/api/webhooks/123/SECRET?thread_id=1",
            "https://discord.com/api/webhooks/123/SECRET#fragment",
            "https://discord.com/api/webhooks/123/SECRET/slack",
            "https://discord.com/api/webhooks/123/%53ECRET",
        ] {
            let error = validate_webhook(url).unwrap_err();
            assert!(!error.contains("SECRET"));
        }
        let old: crate::model::Database =
            serde_json::from_str(r#"{"accounts":[],"encrypted_api_token":""}"#).unwrap();
        assert!(!old.discord.enabled);
        assert!(old.discord.notify_recovery);
    }

    #[test]
    fn alerts_describe_incidents_recovery_and_attention_without_raw_errors() {
        let mut old = Account::new("1".into(), "user".into());
        old.desired_running = true;
        old.status = Status::Running;
        old.target = Some(Target {
            place_id: 123,
            job_id: None,
            private_server_link: Some("SECRET_LINK".into()),
        });
        old.process = Some(ProcessIdentity {
            pid: 1,
            creation_time: "1".into(),
            tracker: "SECRET_TRACKER".into(),
            generation: old.generation,
        });
        let mut new = old.clone();
        new.status = Status::Reconnecting;
        new.last_error = Some("Session lost (code 267); SECRET_RAW_ERROR".into());
        let kick = account_notice(&old, &new).unwrap();
        assert_eq!(kick.title, "Account kicked");
        assert!(kick.message.contains("30 seconds"));
        assert!(account_notice(&new, &new).is_none());
        let mut generic = new.clone();
        generic.last_error = Some("Game connection lost".into());
        assert_eq!(
            account_notice(&generic, &new).unwrap().title,
            "Account kicked"
        );
        let mut offline = new.clone();
        offline.auto_recovery = false;
        assert!(
            account_notice(&old, &offline)
                .unwrap()
                .message
                .contains("is off")
        );
        let mut exit = old.clone();
        exit.process = None;
        exit.status = Status::Backoff;
        exit.failures = 1;
        exit.next_retry = Some(Utc::now());
        assert_eq!(
            account_notice(&old, &exit).unwrap().title,
            "Roblox client closed"
        );
        let mut stopped = exit.clone();
        stopped.generation = Uuid::new_v4();
        stopped.desired_running = false;
        stopped.status = Status::Stopped;
        assert!(account_notice(&old, &stopped).is_none());
        let mut retry = new.clone();
        retry.status = Status::Backoff;
        retry.failures = 1;
        retry.next_retry = Some(Utc::now());
        assert!(account_notice(&new, &retry).unwrap().recovery);
        assert!(account_notice(&retry, &retry).is_none());
        let mut recovered = retry.clone();
        recovered.status = Status::Running;
        assert_eq!(
            account_notice(&retry, &recovered).unwrap().title,
            "Account is back online"
        );
        let mut attention = retry.clone();
        attention.status = Status::NeedsAttention;
        attention.last_error = Some("Session expired; SECRET_COOKIE".into());
        let alert = account_notice(&retry, &attention).unwrap();
        assert!(alert.message.contains("Sign in"));
        let mut payload_notice = kick;
        payload_notice.account = "@everyone **user** <@123>".into();
        let payload = payload_notice.payload();
        assert_eq!(payload["allowed_mentions"]["parse"], serde_json::json!([]));
        assert!(
            payload["embeds"][0]["fields"][0]["value"]
                .as_str()
                .unwrap()
                .contains("\\*")
        );
        for notice in [payload_notice, alert] {
            assert!(!notice.payload().to_string().contains("SECRET"));
        }
    }

    #[tokio::test]
    async fn queue_is_bounded_and_old_settings_cancel_pending_alerts() {
        let service = Discord::new(SavedDiscord::default()).unwrap();
        service.record(Notice::test(), false).unwrap();
        assert_eq!(service.view().recent.len(), 1);
        assert!(
            service
                .receiver
                .lock()
                .unwrap()
                .as_mut()
                .unwrap()
                .try_recv()
                .is_err()
        );
        assert!(service.record(Notice::test(), true).is_err());
        service.configure(SavedDiscord {
            enabled: true,
            encrypted_webhook: "encrypted-marker".into(),
            notify_recovery: false,
        });
        let mut recovery = Notice::test();
        recovery.recovery = true;
        service.record(recovery, false).unwrap();
        assert!(
            service
                .receiver
                .lock()
                .unwrap()
                .as_mut()
                .unwrap()
                .try_recv()
                .is_err()
        );
        for _ in 0..128 {
            service.record(Notice::test(), false).unwrap();
        }
        assert!(service.record(Notice::test(), false).is_err());
        assert_eq!(service.view().recent.len(), 100);
        let q = service
            .receiver
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .try_recv()
            .unwrap();
        assert!(service.current(&q).is_some());
        service.configure(SavedDiscord::default());
        assert!(service.current(&q).is_none());
        let worker = service.run();
        let stop = async {
            tokio::task::yield_now().await;
            service.stop();
        };
        tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(worker, stop);
        })
        .await
        .unwrap();
        assert!(!service.view().delivery_status.contains("could not be read"));
    }

    #[tokio::test]
    async fn transport_payload_rate_limits_redirects_and_errors_are_safe() {
        use axum::{
            Json, Router,
            http::{HeaderMap, StatusCode},
            routing::post,
        };
        let received = std::sync::Arc::new(Mutex::new(None));
        let capture = received.clone();
        let app = Router::new()
            .route(
                "/ok",
                post(
                    move |headers: HeaderMap,
                          uri: axum::http::Uri,
                          Json(body): Json<serde_json::Value>| async move {
                        assert!(headers.get("authorization").is_none());
                        assert_eq!(uri.query(), Some("wait=true"));
                        *capture.lock().unwrap() = Some(body);
                        (
                            StatusCode::OK,
                            [
                                ("x-ratelimit-remaining", "0"),
                                ("x-ratelimit-reset-after", "2.5"),
                            ],
                            "{}",
                        )
                    },
                ),
            )
            .route(
                "/limited",
                post(|| async {
                    (
                        StatusCode::TOO_MANY_REQUESTS,
                        [("retry-after", "1")],
                        r#"{"retry_after":2.5,"message":"SECRET_RESPONSE"}"#,
                    )
                }),
            )
            .route(
                "/bad",
                post(|| async { (StatusCode::NOT_FOUND, "SECRET_RESPONSE") }),
            )
            .route("/busy", post(|| async { StatusCode::SERVICE_UNAVAILABLE }))
            .route(
                "/redirect",
                post(|| async {
                    (
                        StatusCode::TEMPORARY_REDIRECT,
                        [("location", "http://127.0.0.1:1/SECRET")],
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let service = Discord::new(SavedDiscord::default()).unwrap();
        let payload = Notice::test().payload();
        assert!(
            matches!(send_attempt(&service.client, &format!("{base}/ok"), &payload).await, Outcome::Sent(d) if d == Duration::from_secs_f64(2.5))
        );
        assert_eq!(received.lock().unwrap().as_ref().unwrap(), &payload);
        assert!(
            matches!(send_attempt(&service.client, &format!("{base}/limited"), &payload).await, Outcome::Retry(d) if d == Duration::from_secs_f64(2.5))
        );
        assert!(matches!(
            send_attempt(&service.client, &format!("{base}/busy"), &payload).await,
            Outcome::Retry(_)
        ));
        for path in ["bad", "redirect"] {
            match send_attempt(&service.client, &format!("{base}/{path}"), &payload).await {
                Outcome::Rejected(message) => assert!(!message.contains("SECRET")),
                _ => panic!("unexpected result"),
            }
        }
        task.abort();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let unavailable = format!("http://{}/SECRET_TOKEN", listener.local_addr().unwrap());
        drop(listener);
        assert!(
            matches!(send_attempt(&service.client, &unavailable, &payload).await, Outcome::Failed(message) if !message.contains("SECRET"))
        );
    }
}
