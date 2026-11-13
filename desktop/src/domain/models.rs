//! Serde models matching the RoLauncher Rust supervisor API contract.

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

fn default_policy() -> String {
    "allow_public".to_string()
}

fn default_true() -> bool {
    true
}

fn default_server_selection() -> String {
    "smart".to_string()
}

fn default_join_type() -> String {
    "random".to_string()
}

fn default_cpu_normalization() -> String {
    "machine_percent".to_string()
}

fn default_memory_units() -> String {
    "bytes".to_string()
}

fn default_sampling_mode() -> String {
    "suspended".to_string()
}

fn default_history_capacity() -> usize {
    60
}

fn default_interval_ms() -> u64 {
    1000
}

fn default_stale_ms() -> u64 {
    5000
}

fn default_connection_state() -> String {
    "offline".to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub place_id: u64,
    #[serde(default)]
    pub job_id: Option<String>,
    #[serde(default)]
    pub private_server_link: Option<String>,
}

impl Target {
    pub fn kind_label(&self) -> &'static str {
        format_destination_kind(Some(self))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub target: Option<Target>,
    #[serde(default)]
    pub auto_recovery: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub process: Option<ProcessInfo>,
    #[serde(default)]
    pub failures: u32,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub group: String,
    #[serde(default = "default_policy")]
    pub fallback_policy: String,
    #[serde(default)]
    pub next_retry: Option<DateTime<Utc>>,
    #[serde(default)]
    pub recovery_reason: String,
    #[serde(default)]
    pub public_fallback_active: bool,
    #[serde(default)]
    pub desired_running: bool,
    #[serde(default)]
    pub running_since: Option<DateTime<Utc>>,
    #[serde(default)]
    pub longest_streak_seconds: u64,
    #[serde(default)]
    pub game_profile_id: Option<String>,
}

impl Account {
    pub fn display_name(&self) -> &str {
        if self.alias.trim().is_empty() {
            &self.username
        } else {
            &self.alias
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub account_id: String,
    pub alias: String,
    #[serde(default)]
    pub target: Option<Target>,
    pub auto_recovery: bool,
    #[serde(default = "default_policy")]
    pub fallback_policy: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub game_profile_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub entries: Vec<ProfileEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameProfile {
    pub id: String,
    pub name: String,
    pub game_name: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    pub target: Target,
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub network_suspended: bool,
    #[serde(default)]
    pub compatibility: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub profiles: Vec<LaunchProfile>,
    #[serde(default)]
    pub update_repository: String,
    #[serde(default)]
    pub include_beta_updates: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub game_profiles: Vec<GameProfile>,
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    #[serde(default = "default_server_selection")]
    pub server_selection: String,
    #[serde(default = "default_join_type")]
    pub join_type: String,
    #[serde(default = "default_true")]
    pub multi_instance_enabled: bool,
    #[serde(default = "default_true")]
    pub active_multi_instance: bool,
    #[serde(default)]
    pub external_background_cleanup_enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateView {
    pub available: bool,
    pub version: String,
    pub release_url: String,
    pub download_url: String,
    pub checksums_url: String,
    #[serde(default)]
    pub installer_url: String,
    #[serde(default)]
    pub prerelease: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub timestamp: DateTime<Utc>,
    pub account_id: String,
    #[serde(default)]
    pub place_id: Option<u64>,
    pub status: String,
    pub failures: u32,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub timestamp: DateTime<Utc>,
    pub account: String,
    pub title: String,
    pub message: String,
    #[serde(default)]
    pub place_id: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotView {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub guild_id: String,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default = "default_connection_state")]
    pub connection_state: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscordView {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub notify_recovery: bool,
    #[serde(default)]
    pub delivery_status: String,
    #[serde(default)]
    pub recent: Vec<Notice>,
    #[serde(default)]
    pub bot: Option<BotView>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceMetadata {
    #[serde(default = "default_cpu_normalization")]
    pub cpu_normalization: String,
    #[serde(default)]
    pub cpu_formula: String,
    #[serde(default = "default_memory_units")]
    pub memory_units: String,
    #[serde(default)]
    pub gpu_normalization: String,
    #[serde(default = "default_interval_ms")]
    pub sample_interval_ms: u64,
    #[serde(default = "default_history_capacity")]
    pub history_capacity: usize,
    #[serde(default = "default_stale_ms")]
    pub stale_threshold_ms: u64,
    #[serde(default = "default_sampling_mode")]
    pub sampling_mode: String,
    #[serde(default)]
    pub last_sample_duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SystemResources {
    pub logical_processors: u32,
    #[serde(default)]
    pub cpu_percent: Option<f64>,
    pub cpu_availability: String,
    #[serde(default)]
    pub total_ram_bytes: Option<u64>,
    #[serde(default)]
    pub available_ram_bytes: Option<u64>,
    #[serde(default)]
    pub used_ram_bytes: Option<u64>,
    #[serde(default)]
    pub ram_utilization_percent: Option<f64>,
    pub ram_availability: String,
    #[serde(default)]
    pub gpu_percent: Option<f64>,
    pub gpu_availability: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManagedAggregates {
    pub running_instances: usize,
    pub total_accounts: usize,
    pub cpu_percent: f64,
    pub cpu_availability: String,
    pub working_set_bytes: u64,
    #[serde(default)]
    pub private_memory_bytes: Option<u64>,
    #[serde(default)]
    pub gpu_percent: Option<f64>,
    pub gpu_availability: String,
    #[serde(default)]
    pub unmanaged_roblox_processes: Option<usize>,
    #[serde(default)]
    pub inaccessible_roblox_processes: Option<usize>,
    pub exited_or_stale_instances: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManagedInstanceResource {
    pub account_id: String,
    pub display_name: String,
    pub username: String,
    pub account_status: String,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub creation_time: Option<String>,
    #[serde(default)]
    pub cpu_percent: Option<f64>,
    #[serde(default)]
    pub working_set_bytes: Option<u64>,
    #[serde(default)]
    pub private_memory_bytes: Option<u64>,
    #[serde(default)]
    pub gpu_percent: Option<f64>,
    pub gpu_availability: String,
    #[serde(default)]
    pub uptime_seconds: Option<u64>,
    pub sampled_at: DateTime<Utc>,
    pub availability: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceHistoryPoint {
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub system_cpu_percent: Option<f64>,
    #[serde(default)]
    pub ram_utilization_percent: Option<f64>,
    pub managed_cpu_percent: f64,
    pub managed_ram_bytes: u64,
    #[serde(default)]
    pub managed_gpu_percent: Option<f64>,
    pub running_instances: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResourceSnapshotResponse {
    pub timestamp: DateTime<Utc>,
    pub sample_age_ms: u64,
    pub stale: bool,
    pub availability: String,
    pub system: SystemResources,
    pub aggregates: ManagedAggregates,
    #[serde(default)]
    pub accounts: Vec<ManagedInstanceResource>,
    pub metadata: ResourceMetadata,
    #[serde(default)]
    pub history: Option<Vec<ResourceHistoryPoint>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameLookupResponse {
    pub name: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

/// Formats elapsed durations matching the desktop UI (`Xd Yh Zm`, `Xh Ym Zs`, `Xm Ys`, `Xs`).
pub fn format_duration(seconds: u64) -> String {
    if seconds >= 86_400 {
        format!(
            "{}d {}h {}m",
            seconds / 86_400,
            seconds % 86_400 / 3_600,
            seconds % 3_600 / 60
        )
    } else if seconds >= 3_600 {
        format!(
            "{}h {}m {}s",
            seconds / 3_600,
            seconds % 3_600 / 60,
            seconds % 60
        )
    } else if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}

pub fn format_account_status(status: &str) -> &'static str {
    match status {
        "needs_attention" => "Needs attention",
        "stopped" => "Stopped",
        "stopping" => "Stopping",
        "queued" => "Queued",
        "launching" => "Launching",
        "running" => "Running",
        "reconnecting" => "Reconnecting",
        "backoff" => "Backoff",
        _ => "Unknown",
    }
}

pub fn format_destination_kind(target: Option<&Target>) -> &'static str {
    match target {
        Some(t) if t.private_server_link.is_some() => "Private server",
        Some(t) if t.job_id.is_some() => "Specific server",
        _ => "Public server",
    }
}

pub fn format_group(group: &str) -> &str {
    if group.is_empty() { "Ungrouped" } else { group }
}

/// Formats a fallback policy string (`"stay"`, `"pause"`, or default allow).
pub fn format_fallback_policy(policy: &str) -> &'static str {
    match policy {
        "stay" => "Stay on destination",
        "pause" => "Pause and notify",
        _ => "Public fallback allowed",
    }
}

pub fn format_fallback(account: &Account) -> &'static str {
    if account.public_fallback_active {
        "Public fallback active"
    } else {
        format_fallback_policy(account.fallback_policy.as_str())
    }
}

pub fn format_error(last_error: Option<&str>) -> String {
    last_error
        .unwrap_or("—")
        .replace("recovery", "rejoin")
        .replace("Recovery", "Rejoin")
}

pub fn format_recovery_reason(account: &Account) -> String {
    match account.recovery_reason.as_str() {
        "Auth" => "Session expired · sign in again".to_string(),
        "Permission" => "Experience denied access".to_string(),
        "TargetUnavailable" => "Destination unavailable".to_string(),
        "Unsupported" => "Integration needs attention".to_string(),
        "RateLimit" => "Rate limited · retry pending".to_string(),
        "Network" => "Connection lost".to_string(),
        _ => format_error(account.last_error.as_deref()),
    }
}

pub fn format_retry(account: &Account, now: DateTime<Utc>) -> String {
    if let Some(next) = account.next_retry {
        let remaining = (next - now).num_seconds().max(0);
        let local = next.with_timezone(&Local);
        format!("Retry in {remaining}s · {}", local.format("%H:%M:%S"))
    } else if account.status == "reconnecting" {
        "Waiting up to 30 seconds to reconnect".to_string()
    } else {
        "No retry scheduled".to_string()
    }
}

pub fn can_retry(account: &Account) -> bool {
    account.status == "backoff"
        && account.next_retry.is_some()
        && account.auto_recovery
        && account.desired_running
        && account.recovery_reason != "RateLimit"
}

pub fn running_seconds(account: &Account, now: DateTime<Utc>) -> u64 {
    account
        .running_since
        .map(|since| (now - since).num_seconds().max(0) as u64)
        .unwrap_or(0)
}

pub fn longest_seconds(account: &Account, now: DateTime<Utc>) -> u64 {
    account
        .longest_streak_seconds
        .max(running_seconds(account, now))
}
