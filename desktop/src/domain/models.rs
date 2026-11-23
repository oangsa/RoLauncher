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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountLifecycleView {
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub process_status: String,
    #[serde(default)]
    pub process_exit_observed: bool,
    #[serde(default)]
    pub exit_reconciliation_pending: bool,
    #[serde(default)]
    pub pending_reason: Option<String>,
    #[serde(default)]
    pub handoff_deadline: Option<DateTime<Utc>>,
    #[serde(default)]
    pub outstanding_launch_attempt: bool,
    #[serde(default)]
    pub outstanding_retired_launches: usize,
    #[serde(default)]
    pub next_retry_eligible_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub current_retry_blocker: Option<String>,
    #[serde(default)]
    pub last_verified_launch_stage: Option<String>,
    #[serde(default)]
    pub last_confirmed_connection_state: String,
    #[serde(default)]
    pub last_exit_code: Option<u32>,
    #[serde(default)]
    pub last_process_lifetime_ms: Option<u64>,
    #[serde(default)]
    pub last_termination_source: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub accounts: Vec<Account>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub account_lifecycle: std::collections::BTreeMap<String, AccountLifecycleView>,
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
        "Auth" => "Action required: sign in again".to_string(),
        "Permission" => "Action required: access restriction".to_string(),
        "TargetUnavailable" => "Destination unavailable".to_string(),
        "Unsupported" => "Action required: update or client check".to_string(),
        "RateLimit" => "Rate limited · cooldown active".to_string(),
        "SessionConflict" => "Cooldown for duplicate session".to_string(),
        "NetworkDisconnect" => "Network disconnect · rejoining".to_string(),
        "ConnectionFailed" => "Server did not respond (279)".to_string(),
        "AfkKick" => "Idle disconnect (278) · rejoining".to_string(),
        "Network" => "Waiting for internet / Roblox service".to_string(),
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

pub fn format_waiting_detail(
    account: &Account,
    network_suspended: bool,
    now: DateTime<Utc>,
) -> String {
    if network_suspended
        && account.desired_running
        && matches!(
            account.status.as_str(),
            "queued" | "backoff" | "reconnecting"
        )
    {
        return "Waiting for internet / Roblox service".to_string();
    }
    if account.recovery_reason == "SessionConflict"
        && let Some(next) = account.next_retry.filter(|t| *t > now)
    {
        let remaining = (next - now).num_seconds().max(0);
        return format!("Cooldown for duplicate session · {remaining}s");
    }
    if account.recovery_reason == "RateLimit"
        && let Some(next) = account.next_retry.filter(|t| *t > now)
    {
        let remaining = (next - now).num_seconds().max(0);
        return format!("Rate-limit cooldown · {remaining}s");
    }
    if account
        .last_error
        .as_deref()
        .is_some_and(|e| e.starts_with("Restart pending"))
    {
        return "Restart pending".to_string();
    }
    if account.process.is_some()
        && matches!(account.status.as_str(), "queued" | "stopping" | "backoff")
    {
        return "Waiting for old process to exit".to_string();
    }
    if account.last_error.as_deref().is_some_and(|e| {
        e.contains("surviving Roblox client") || e.contains("checking for a surviving")
    }) {
        return "Checking for process handoff".to_string();
    }
    if account.last_error.as_deref().is_some_and(|e| {
        e.contains("unverified Roblox process")
            || e.contains("startup is still in progress")
            || e.contains("watching for a late client")
            || e.contains("Launch tracker has no verified instance")
    }) {
        return "Inspecting late Roblox launch".to_string();
    }
    if account.status == "needs_attention" {
        return format_error(account.last_error.as_deref());
    }
    if account.next_retry.is_some() || account.status == "reconnecting" {
        format_retry(account, now)
    } else {
        format_error(account.last_error.as_deref())
    }
}

pub fn format_waiting_detail_with_lifecycle(
    account: &Account,
    lifecycle: Option<&AccountLifecycleView>,
    network_suspended: bool,
    now: DateTime<Utc>,
) -> String {
    if account.recovery_reason == "SessionConflict"
        && let Some(next) = account.next_retry.filter(|t| *t > now)
    {
        let remaining = (next - now).num_seconds().max(0);
        return format!("Cooldown for duplicate session · {remaining}s");
    }
    if account.recovery_reason == "RateLimit"
        && let Some(next) = account.next_retry.filter(|t| *t > now)
    {
        let remaining = (next - now).num_seconds().max(0);
        return format!("Rate-limit cooldown · {remaining}s");
    }
    if let Some(lc) = lifecycle {
        match lc.phase.as_str() {
            "waiting_for_startup" => return "Waiting for Roblox startup".to_string(),
            "waiting_for_ownership" => {
                return "Waiting for verified process ownership".to_string();
            }
            "checking_for_handoff" => return "Checking for process handoff".to_string(),
            "old_process_exited" => return "Old Roblox process exited".to_string(),
            "checking_for_surviving_client" => {
                return "Checking for surviving client".to_string();
            }
            "preparing_replacement" => {
                return if lc.outstanding_retired_launches > 0 {
                    "Restart pending · waiting for previous launch to clear".to_string()
                } else {
                    "Preparing replacement launch".to_string()
                };
            }
            "waiting_for_network" => return "Waiting for internet / Roblox service".to_string(),
            "launch_failed_before_connection" => {
                return if account.next_retry.is_some() {
                    format!(
                        "Launch failed before connection · {}",
                        format_retry(account, now)
                    )
                } else {
                    "Launch failed before connection".to_string()
                };
            }
            "retry_scheduled" => {
                return if account.next_retry.is_some() {
                    format!("Recovery retry scheduled · {}", format_retry(account, now))
                } else {
                    "Recovery retry scheduled".to_string()
                };
            }
            "action_required" => {
                return account
                    .last_error
                    .as_deref()
                    .map(|_| format_error(account.last_error.as_deref()))
                    .unwrap_or_else(|| "Action required".to_string());
            }
            _ => {}
        }
    }
    format_waiting_detail(account, network_suspended, now)
}

pub fn coordination_warning(compatibility: &str) -> Option<&str> {
    [
        "Waiting for launch coordination",
        "Startup conflict",
        "Roblox startup conflict",
        "Startup inspection unavailable",
        "launch coordination blocked",
    ]
    .iter()
    .any(|pattern| compatibility.contains(pattern))
    .then_some(compatibility)
}

pub fn can_retry(account: &Account) -> bool {
    account.status == "backoff"
        && account.next_retry.is_some()
        && account.auto_recovery
        && account.desired_running
        && !matches!(
            account.recovery_reason.as_str(),
            "RateLimit" | "SessionConflict"
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    #[test]
    fn retry_gate_blocks_rate_limit_and_duplicate_session_cooldowns() {
        let now = Utc::now();
        let mut account = Account {
            status: "backoff".into(),
            next_retry: Some(now + ChronoDuration::seconds(30)),
            auto_recovery: true,
            desired_running: true,
            ..Default::default()
        };
        assert!(can_retry(&account));
        for reason in ["RateLimit", "SessionConflict"] {
            account.recovery_reason = reason.into();
            assert!(!can_retry(&account), "{reason}");
        }
    }

    #[test]
    fn waiting_detail_and_coordination_warning_surface_specific_reasons() {
        let now = Utc::now();
        let account = Account {
            status: "backoff".into(),
            desired_running: true,
            auto_recovery: true,
            next_retry: Some(now + ChronoDuration::seconds(45)),
            recovery_reason: "SessionConflict".into(),
            ..Default::default()
        };
        assert_eq!(
            format_waiting_detail(&account, true, now),
            "Waiting for internet / Roblox service"
        );
        assert!(
            format_waiting_detail(&account, false, now)
                .starts_with("Cooldown for duplicate session")
        );

        let old_proc = Account {
            status: "queued".into(),
            desired_running: true,
            process: Some(ProcessInfo { pid: 123 }),
            ..Default::default()
        };
        assert_eq!(
            format_waiting_detail(&old_proc, false, now),
            "Waiting for old process to exit"
        );

        let late = Account {
            status: "needs_attention".into(),
            last_error: Some("Launch tracker has no verified instance".into()),
            ..Default::default()
        };
        assert_eq!(
            format_waiting_detail(&late, false, now),
            "Inspecting late Roblox launch"
        );

        assert_eq!(
            coordination_warning("UI fixture · real-client validation remains outstanding"),
            None
        );
        assert!(
            coordination_warning(
                "OS multi-instance coordination initialized; Waiting for launch coordination"
            )
            .is_some()
        );
    }

    #[test]
    fn lifecycle_view_formats_all_phases_accurately_and_avoids_stale_late_launch_label() {
        let now = Utc::now();
        let mut account = Account {
            status: "unknown".into(),
            desired_running: true,
            last_error: Some(
                "Managed process exited; checking for a surviving Roblox client before rejoin"
                    .into(),
            ),
            ..Default::default()
        };
        assert_eq!(
            format_waiting_detail(&account, false, now),
            "Checking for process handoff"
        );
        for (phase, expected) in [
            ("waiting_for_startup", "Waiting for Roblox startup"),
            (
                "waiting_for_ownership",
                "Waiting for verified process ownership",
            ),
            ("checking_for_handoff", "Checking for process handoff"),
            ("old_process_exited", "Old Roblox process exited"),
            (
                "checking_for_surviving_client",
                "Checking for surviving client",
            ),
            ("preparing_replacement", "Preparing replacement launch"),
            (
                "waiting_for_network",
                "Waiting for internet / Roblox service",
            ),
            (
                "launch_failed_before_connection",
                "Launch failed before connection",
            ),
            ("retry_scheduled", "Recovery retry scheduled"),
        ] {
            let lc = AccountLifecycleView {
                phase: phase.into(),
                ..Default::default()
            };
            assert_eq!(
                format_waiting_detail_with_lifecycle(&account, Some(&lc), false, now),
                expected,
                "{phase}"
            );
        }
        account.next_retry = Some(now + ChronoDuration::seconds(15));
        let failed_lc = AccountLifecycleView {
            phase: "launch_failed_before_connection".into(),
            ..Default::default()
        };
        assert!(
            format_waiting_detail_with_lifecycle(&account, Some(&failed_lc), false, now)
                .starts_with("Launch failed before connection · Retry in ")
        );
        let retired_lc = AccountLifecycleView {
            phase: "preparing_replacement".into(),
            outstanding_retired_launches: 1,
            ..Default::default()
        };
        assert_eq!(
            format_waiting_detail_with_lifecycle(&account, Some(&retired_lc), false, now),
            "Restart pending · waiting for previous launch to clear"
        );
        let restart_acct = Account {
            status: "queued".into(),
            desired_running: true,
            last_error: Some("Restart pending; waiting for previous launch to clear".into()),
            ..Default::default()
        };
        assert_eq!(
            format_waiting_detail(&restart_acct, false, now),
            "Restart pending"
        );
    }
}
