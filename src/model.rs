use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Stopped,
    Queued,
    Launching,
    Running,
    Reconnecting,
    Backoff,
    Unknown,
    NeedsAttention,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackPolicy {
    #[default]
    AllowPublic,
    Stay,
    Pause,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub account_id: String,
    pub alias: String,
    pub target: Option<Target>,
    pub auto_recovery: bool,
    pub fallback_policy: FallbackPolicy,
    pub group: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaunchProfile {
    pub id: Uuid,
    pub name: String,
    pub entries: Vec<ProfileEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Activity {
    pub timestamp: DateTime<Utc>,
    pub account_id: String,
    pub place_id: Option<u64>,
    pub status: Status,
    pub failures: u32,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Target {
    pub place_id: u64,
    #[serde(default)]
    pub job_id: Option<Uuid>,
    #[serde(default)]
    pub private_server_link: Option<String>,
}
impl Target {
    pub fn validate(&self) -> Result<(), String> {
        if self.place_id == 0 {
            return Err("PlaceId must be greater than zero".into());
        }
        if self.job_id.is_some() && self.private_server_link.is_some() {
            return Err("Choose a JobId or a private-server link, not both".into());
        }
        if let Some(link) = &self.private_server_link {
            if link.len() > 2048 {
                return Err("Private-server link must be at most 2048 characters".into());
            }
            let u = url::Url::parse(link).map_err(|_| "Invalid private-server URL")?;
            if u.scheme() != "https"
                || !matches!(u.host_str(), Some("www.roblox.com" | "roblox.com"))
                || !u.username().is_empty()
                || u.password().is_some()
                || u.port().is_some()
            {
                return Err("Private links must use https://www.roblox.com".into());
            }
            let query: std::collections::HashMap<_, _> = u.query_pairs().collect();
            if query.contains_key("code") && query.get("type").map(|s| s.as_ref()) != Some("Server")
            {
                return Err("Share links must have type=Server".into());
            }
            if query.contains_key("privateServerLinkCode") {
                let id = u.path_segments().and_then(|mut s| {
                    if s.next() == Some("games") {
                        s.next().and_then(|s| s.parse::<u64>().ok())
                    } else {
                        None
                    }
                });
                if id != Some(self.place_id) {
                    return Err("Private-server link must match the configured PlaceId".into());
                }
            }
            let code = query
                .get("privateServerLinkCode")
                .or_else(|| query.get("code"));
            if !code.is_some_and(|c| {
                !c.is_empty()
                    && c.len() <= 512
                    && c.chars().all(|x| x.is_ascii_alphanumeric() || x == '-')
            }) {
                return Err("Link must contain a valid privateServerLinkCode or share code".into());
            }
        }
        Ok(())
    }
    pub fn public(&self) -> Self {
        Self {
            place_id: self.place_id,
            job_id: None,
            private_server_link: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Windows FILETIME or Linux boot ID/start ticks; text preserves integer precision.
    pub creation_time: String,
    pub tracker: String,
    pub generation: Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub username: String,
    pub alias: String,
    pub target: Option<Target>,
    pub desired_running: bool,
    pub auto_recovery: bool,
    pub status: Status,
    pub process: Option<ProcessIdentity>,
    pub generation: Uuid,
    pub tracker: Option<String>,
    #[serde(default)]
    pub ownership_deadline: Option<DateTime<Utc>>,
    pub failures: u32,
    pub next_retry: Option<DateTime<Utc>>,
    pub connected_since: Option<DateTime<Utc>>,
    pub disconnected_since: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub operation_id: Option<Uuid>,
    #[serde(default)]
    pub public_fallback_active: bool,
    #[serde(default)]
    pub fallback_policy: FallbackPolicy,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub recovery_reason: String,
}
impl Account {
    pub fn new(id: String, username: String) -> Self {
        Self {
            alias: username.clone(),
            username,
            id,
            target: None,
            desired_running: false,
            auto_recovery: true,
            status: Status::Stopped,
            process: None,
            generation: Uuid::new_v4(),
            tracker: None,
            ownership_deadline: None,
            failures: 0,
            next_retry: None,
            connected_since: None,
            disconnected_since: None,
            last_error: None,
            operation_id: None,
            public_fallback_active: false,
            fallback_policy: FallbackPolicy::default(),
            group: String::new(),
            recovery_reason: String::new(),
        }
    }
    pub fn effective_target(&self) -> Option<Target> {
        self.target.as_ref().map(|t| {
            if self.public_fallback_active {
                t.public()
            } else {
                t.clone()
            }
        })
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedAccount {
    pub account: Account,
    pub encrypted_session: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Database {
    #[serde(default = "schema_version")]
    pub version: u32,
    pub accounts: Vec<SavedAccount>,
    pub encrypted_api_token: String,
    /// Cancelled launch trackers stay durable so late instances can be identified safely.
    #[serde(default)]
    pub retired_launches: Vec<RetiredLaunch>,
    #[serde(default)]
    pub discord: crate::discord::SavedDiscord,
    #[serde(default)]
    pub profiles: Vec<LaunchProfile>,
    #[serde(default)]
    pub activity: Vec<Activity>,
    #[serde(default)]
    pub update_repository: String,
    #[serde(default)]
    pub include_beta_updates: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct RetiredLaunch {
    pub tracker: String,
    pub generation: Uuid,
    pub account_id: String,
    pub deadline: DateTime<Utc>,
}
fn schema_version() -> u32 {
    2
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Operation {
    pub id: Uuid,
    pub account_id: String,
    pub state: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub accounts: Vec<Account>,
    pub network_suspended: bool,
    pub compatibility: String,
    pub profiles: Vec<LaunchProfile>,
    pub update_repository: String,
    pub include_beta_updates: bool,
}

/// The fifth retry is attempted; its failure ends the retry sequence.
pub fn retry_seconds(failures: u32) -> Option<u64> {
    match failures {
        1..=5 => Some(10u64 << (failures - 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targets_reject_external_urls_and_conflicting_destinations() {
        let mut t = Target {
            place_id: 1,
            job_id: None,
            private_server_link: Some("https://evil.example/?code=abc".into()),
        };
        assert!(t.validate().is_err());
        t.private_server_link =
            Some("https://www.roblox.com/games/1?privateServerLinkCode=abc123".into());
        assert!(t.validate().is_ok());
        t.job_id = Some(Uuid::new_v4());
        assert!(t.validate().is_err());
    }
    #[test]
    fn backoff_is_bounded() {
        assert_eq!(
            (1..=6).map(retry_seconds).collect::<Vec<_>>(),
            vec![Some(10), Some(20), Some(40), Some(80), Some(160), None]
        );
    }
}
