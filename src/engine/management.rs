use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Deserialize)]
pub struct BulkPatch {
    pub account_ids: Vec<String>,
    pub patch: AccountPatch,
}
#[derive(Deserialize)]
pub struct ProfileSave {
    pub name: String,
    pub account_ids: Vec<String>,
    pub id: Option<Uuid>,
}
#[derive(Serialize)]
pub struct ProfileResult {
    pub account_id: String,
    pub operation: Option<Operation>,
    pub error: Option<String>,
}

fn text_valid(value: &str, empty: bool) -> bool {
    (empty || !value.trim().is_empty())
        && value.len() <= 100
        && !value.chars().any(char::is_control)
}
pub(super) fn validate_patch(p: &AccountPatch) -> Result<(), String> {
    if let Some(t) = &p.target {
        t.validate()?;
    }
    if p.clear_target && p.target.is_some() {
        return Err("Choose a destination or clear it, not both".into());
    }
    if p.alias.as_ref().is_some_and(|v| !text_valid(v, false)) {
        return Err("Alias must be 1–100 characters without control characters".into());
    }
    if p.group.as_ref().is_some_and(|v| !text_valid(v, true)) {
        return Err("Group must be at most 100 characters without control characters".into());
    }
    Ok(())
}
pub(super) fn apply_patch(a: &mut Account, p: AccountPatch) {
    if let Some(alias) = p.alias {
        a.alias = alias;
    }
    if p.clear_target {
        a.target = None;
        a.public_fallback_active = false;
    }
    if let Some(target) = p.target {
        a.target = Some(target);
        a.public_fallback_active = false;
    }
    if let Some(group) = p.group {
        a.group = group.trim().to_string();
    }
    if let Some(policy) = p.fallback_policy {
        a.fallback_policy = policy;
        if policy != FallbackPolicy::AllowPublic {
            a.public_fallback_active = false;
        }
    }
    if let Some(enabled) = p.auto_recovery {
        a.auto_recovery = enabled;
        if !enabled && a.status == Status::Backoff {
            a.status = Status::NeedsAttention;
            a.next_retry = None;
            a.last_error = Some(REJOIN_DISABLED.into());
        } else if enabled
            && a.desired_running
            && a.last_error.as_deref() == Some(REJOIN_DISABLED)
            && a.failures < 5
        {
            a.status = Status::Backoff;
            a.next_retry = Some(
                Utc::now()
                    + ChronoDuration::seconds(retry_seconds(a.failures).unwrap_or(10) as i64),
            );
            a.last_error = None;
        }
    }
}
fn ids_valid(ids: &[String]) -> Result<(), String> {
    if ids.is_empty() || ids.len() > 50 || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err("Select 1–50 distinct accounts".into());
    }
    Ok(())
}
fn expand_alias(value: &str, a: &Account, index: usize) -> String {
    value
        .replace("{username}", &a.username)
        .replace("{id}", &a.id)
        .replace("{index}", &(index + 1).to_string())
}

impl Engine {
    pub fn set_close_to_tray(&self, enabled: bool) -> Result<(), String> {
        self.transaction(|s| {
            s.database.close_to_tray = enabled;
            Ok(())
        })
    }
    pub fn save_game_profile(&self, mut profile: GameProfile) -> Result<GameProfile, String> {
        profile.validate()?;
        profile.name = profile.name.trim().into();
        self.transaction(|s| {
            if s.database
                .game_profiles
                .iter()
                .any(|p| p.id != profile.id && p.name.eq_ignore_ascii_case(&profile.name))
            {
                return Err("A game profile with that name already exists".into());
            }
            if s.database.game_profiles.len() >= 100
                && !s.database.game_profiles.iter().any(|p| p.id == profile.id)
            {
                return Err("Up to 100 game profiles are supported".into());
            }
            s.database.game_profiles.retain(|p| p.id != profile.id);
            s.database.game_profiles.push(profile.clone());
            Ok(profile)
        })
    }
    pub fn delete_game_profile(&self, id: Uuid) -> Result<(), String> {
        self.transaction(|s| {
            if !s.database.game_profiles.iter().any(|p| p.id == id) {
                return Err("Game profile not found".into());
            }
            s.database.game_profiles.retain(|p| p.id != id);
            Ok(())
        })
    }
    pub async fn game_details(&self, place_id: u64) -> Result<(String, Option<String>), String> {
        let encrypted = self
            .0
            .state
            .lock()
            .unwrap()
            .database
            .accounts
            .first()
            .map(|a| a.encrypted_session.clone())
            .ok_or("Add an account to look up games")?;
        let cookie = platform::unprotect(&encrypted)?;
        self.0
            .roblox
            .game_details(&cookie, place_id)
            .await
            .map_err(|e| e.message)
    }
    pub fn set_beta_updates(&self, include_beta: bool) -> Result<(), String> {
        self.transaction(|s| {
            s.database.include_beta_updates = include_beta;
            Ok(())
        })
    }
    pub fn set_update_repository(&self, repository: String) -> Result<(), String> {
        let repository = repository.trim().to_string();
        if !repository.is_empty() {
            crate::updates::validate_repository(&repository)?;
        }
        self.transaction(|s| {
            s.database.update_repository = repository;
            Ok(())
        })
    }
    pub fn retry_now(&self, id: &str) -> Result<Operation, String> {
        self.transaction(|s| {
            if s.network_suspended { return Err("Connectivity is suspended; wait for the network to return".into()); }
            let a = account_mut(s, id)?;
            if a.status != Status::Backoff || a.next_retry.is_none() || !a.desired_running || !a.auto_recovery {
                return Err("Retry now is available only for a scheduled automatic retry; use Start after resolving attention states".into());
            }
            if a.recovery_reason == "RateLimit" { return Err("The rate-limit cooldown must finish before retrying".into()); }
            a.next_retry = Some(Utc::now());
            let pending = a.operation_id;
            if let Some(op) = pending.and_then(|id|s.operations.get(&id)).filter(|op|op.state == "queued").cloned() { return Ok(op); }
            let op = Operation { id: Uuid::new_v4(), account_id: id.into(), state: "queued".into(), error: None };
            account_mut(s, id)?.operation_id = Some(op.id); s.operations.insert(op.id, op.clone()); Ok(op)
        })
    }
    /// Validate every expanded patch before committing anything, including missing IDs.
    pub fn bulk_patch(&self, input: BulkPatch) -> Result<Vec<Account>, String> {
        ids_valid(&input.account_ids)?;
        self.transaction(|s| {
            let patches = input
                .account_ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    let a = account_mut(s, id)?;
                    let mut p = input.patch.clone();
                    if let Some(alias) = &p.alias {
                        p.alias = Some(expand_alias(alias, a, i));
                    }
                    validate_patch(&p)?;
                    Ok((id.clone(), p))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let mut result = Vec::new();
            for (id, patch) in patches {
                let a = account_mut(s, &id)?;
                apply_patch(a, patch);
                result.push(a.clone());
            }
            Ok(result)
        })
    }
    pub fn save_profile(&self, input: ProfileSave) -> Result<LaunchProfile, String> {
        ids_valid(&input.account_ids)?;
        if !text_valid(&input.name, false) {
            return Err("Profile name must be 1–100 characters".into());
        }
        self.transaction(|s| {
            if input
                .id
                .is_some_and(|id| !s.database.profiles.iter().any(|p| p.id == id))
            {
                return Err("Profile not found".into());
            }
            if s.database.profiles.len() >= 50 && input.id.is_none() {
                return Err("Up to 50 profiles are supported".into());
            }
            if s.database
                .profiles
                .iter()
                .any(|p| p.name.eq_ignore_ascii_case(input.name.trim()) && Some(p.id) != input.id)
            {
                return Err("A profile with that name already exists".into());
            }
            let entries = input
                .account_ids
                .iter()
                .map(|id| {
                    let a = account_mut(s, id)?;
                    let target = a
                        .target
                        .clone()
                        .ok_or("Every profile account needs a destination")?;
                    target.validate()?;
                    Ok(ProfileEntry {
                        account_id: id.clone(),
                        alias: a.alias.clone(),
                        target: Some(target),
                        auto_recovery: a.auto_recovery,
                        fallback_policy: a.fallback_policy,
                        group: a.group.clone(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let profile = LaunchProfile {
                id: input.id.unwrap_or_else(Uuid::new_v4),
                name: input.name.trim().into(),
                entries,
            };
            s.database.profiles.retain(|p| p.id != profile.id);
            s.database.profiles.push(profile.clone());
            Ok(profile)
        })
    }
    pub fn delete_profile(&self, id: Uuid) -> Result<(), String> {
        self.transaction(|s| {
            if !s.database.profiles.iter().any(|p| p.id == id) {
                return Err("Profile not found".into());
            }
            s.database.profiles.retain(|p| p.id != id);
            Ok(())
        })
    }
    pub fn apply_profile(&self, id: Uuid) -> Result<Vec<Account>, String> {
        self.transaction(|s| {
            let profile = s
                .database
                .profiles
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .ok_or("Profile not found")?;
            validate_profile(&profile)?;
            for entry in &profile.entries {
                account_mut(s, &entry.account_id)?;
            }
            let mut result = Vec::new();
            for entry in profile.entries {
                let a = account_mut(s, &entry.account_id)?;
                apply_patch(
                    a,
                    AccountPatch {
                        alias: Some(entry.alias),
                        target: entry.target,
                        auto_recovery: Some(entry.auto_recovery),
                        fallback_policy: Some(entry.fallback_policy),
                        group: Some(entry.group),
                        clear_target: false,
                    },
                );
                result.push(a.clone());
            }
            Ok(result)
        })
    }
    pub fn launch_profile(&self, id: Uuid) -> Result<Vec<ProfileResult>, String> {
        let accounts = self.apply_profile(id)?;
        Ok(accounts
            .iter()
            .map(|a| match self.command(&a.id, "start") {
                Ok(op) => ProfileResult {
                    account_id: a.id.clone(),
                    operation: Some(op),
                    error: None,
                },
                Err(error) => ProfileResult {
                    account_id: a.id.clone(),
                    operation: None,
                    error: Some(error),
                },
            })
            .collect())
    }
    pub fn import_profiles(&self, mut profiles: Vec<LaunchProfile>) -> Result<usize, String> {
        if profiles.is_empty() || profiles.len() > 50 {
            return Err("Import 1–50 profiles".into());
        }
        for p in &profiles {
            validate_profile(p)?;
        }
        self.transaction(|s| {
            if s.database.profiles.len() + profiles.len() > 50 {
                return Err("Up to 50 profiles are supported".into());
            }
            let mut names: HashSet<String> = s
                .database
                .profiles
                .iter()
                .map(|p| p.name.to_lowercase())
                .collect();
            for p in &mut profiles {
                if !names.insert(p.name.to_lowercase()) {
                    return Err("Profile names must be unique; rename before importing".into());
                }
                for e in &p.entries {
                    account_mut(s, &e.account_id)?;
                }
                p.id = Uuid::new_v4();
            }
            let count = profiles.len();
            s.database.profiles.extend(profiles);
            Ok(count)
        })
    }
    pub fn activity(&self, account_id: Option<&str>) -> Vec<Activity> {
        self.0
            .state
            .lock()
            .unwrap()
            .database
            .activity
            .iter()
            .rev()
            .filter(|a| account_id.is_none_or(|id| a.account_id == id))
            .cloned()
            .collect()
    }
    pub fn diagnostics(&self) -> serde_json::Value {
        let s = self.0.state.lock().unwrap();
        // Allowlist only: no aliases, private destinations, trackers, errors, tokens or credentials.
        serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "generated_at": Utc::now(), "network_suspended": s.network_suspended,
            "live_validation": "See docs/VALIDATION.md; unperformed checks remain unverified",
            "accounts": s.database.accounts.iter().map(|a| serde_json::json!({"id": a.account.id,"status": a.account.status,"failures": a.account.failures,"next_retry": a.account.next_retry,"verified_pid": a.account.process.as_ref().map(|p|p.pid),"fallback_policy": a.account.fallback_policy,"public_fallback_active": a.account.public_fallback_active})).collect::<Vec<_>>(),
            "activity": s.database.activity })
    }
    pub fn backup(&self) -> Result<String, String> {
        let s = self.0.state.lock().unwrap();
        self.0.store.backup(&s.database)
    }
    pub fn backups(&self) -> Result<Vec<String>, String> {
        self.0.store.backups()
    }
    pub fn restore(&self, name: &str) -> Result<(), String> {
        let restored = self.0.store.read_backup(name)?;
        self.transaction_inner(
            |s| {
                if s.database.accounts.iter().any(|a| {
                    a.account.desired_running
                        || a.account.process.is_some()
                        || a.account.tracker.is_some()
                }) || s
                    .database
                    .retired_launches
                    .iter()
                    .any(|r| r.deadline > Utc::now())
                {
                    return Err(
                        "Stop all accounts and wait for pending launches before restoring".into(),
                    );
                }
                self.0.store.backup(&s.database)?;
                let token = s.database.encrypted_api_token.clone();
                s.database = restored;
                s.database.encrypted_api_token = token;
                Ok(())
            },
            true,
        )
    }
}
pub(super) fn validate_profile(p: &LaunchProfile) -> Result<(), String> {
    if !text_valid(&p.name, false) {
        return Err("Invalid profile name".into());
    }
    ids_valid(
        &p.entries
            .iter()
            .map(|e| e.account_id.clone())
            .collect::<Vec<_>>(),
    )?;
    for e in &p.entries {
        let target = e.target.clone().ok_or("Profile destination missing")?;
        validate_patch(&AccountPatch {
            alias: Some(e.alias.clone()),
            target: Some(target),
            group: Some(e.group.clone()),
            ..Default::default()
        })?;
    }
    Ok(())
}
pub(super) fn record_activity(old: &State, next: &mut State) {
    for saved in &next.database.accounts {
        let a = &saved.account;
        let before = old
            .database
            .accounts
            .iter()
            .find(|b| b.account.id == a.id)
            .map(|b| &b.account);
        if before.is_some_and(|b| {
            b.status == a.status
                && b.failures == a.failures
                && b.recovery_reason == a.recovery_reason
                && b.public_fallback_active == a.public_fallback_active
        }) {
            continue;
        }
        let summary = match a.status {
            Status::Stopped => "Stopped; use Start when ready",
            Status::Queued => "Launch queued",
            Status::Launching => "Launch issued; waiting for verified ownership",
            Status::Running => "Connection confirmed in a verified client log",
            Status::Reconnecting => match a.recovery_reason.as_str() {
                "Kick267" => "Kick detected (267); waiting through the reconnect grace period",
                "ConnectionFailed" => {
                    "Connection failed (279); waiting through the reconnect grace period"
                }
                "SessionLost" => "Session lost; waiting through the reconnect grace period",
                _ => "Connection lost; waiting through the reconnect grace period",
            },
            Status::Backoff => "Retry scheduled; inspect the next retry time",
            Status::Unknown => "Connection or process ownership is not yet verified",
            Status::NeedsAttention => match a.recovery_reason.as_str() {
                "Auth" => "Session needs repair; sign in to this account again",
                "Permission" => "Access denied; check experience permissions",
                "TargetUnavailable" => "Destination unavailable; check the fallback policy",
                "Unsupported" => {
                    "Compatibility changed; inspect supported integration requirements"
                }
                _ => "Recovery paused; inspect account status before retrying",
            },
        };
        next.database.activity.push(Activity {
            timestamp: Utc::now(),
            account_id: a.id.clone(),
            place_id: a.target.as_ref().map(|t| t.place_id),
            status: a.status.clone(),
            failures: a.failures,
            message: if a.public_fallback_active {
                format!("{summary}. Public fallback is active")
            } else {
                summary.into()
            },
        });
    }
    if old.network_suspended != next.network_suspended {
        next.database.activity.push(Activity {
            timestamp: Utc::now(),
            account_id: String::new(),
            place_id: None,
            status: if next.network_suspended {
                Status::Backoff
            } else {
                Status::Queued
            },
            failures: 0,
            message: if next.network_suspended {
                "Internet connectivity unavailable; launches suspended"
            } else {
                "Internet connectivity restored"
            }
            .into(),
        });
    }
    if next.database.activity.len() > 1000 {
        next.database
            .activity
            .drain(..next.database.activity.len() - 1000);
    }
}
