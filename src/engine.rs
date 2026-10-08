use crate::{
    logs::Signal,
    model::*,
    platform,
    roblox::{Failure, FailureKind, Roblox, normalize_cookie},
    store::Store,
};
use chrono::{Duration as ChronoDuration, Utc};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Notify, broadcast};
use uuid::Uuid;

mod management;
pub use management::{BulkPatch, ProfileSave};

#[derive(Clone)]
struct State {
    database: Database,
    operations: BTreeMap<Uuid, Operation>,
    network_suspended: bool,
    compatibility: String,
}
struct Inner {
    state: Mutex<State>,
    store: Store,
    roblox: Roblox,
    token: zeroize::Zeroizing<String>,
    wake: Notify,
    events: broadcast::Sender<Snapshot>,
    shutdown: AtomicBool,
    launch_allowed: Arc<AtomicBool>,
    bot_generation: AtomicU64,
    discord: crate::discord::Discord,
}
#[derive(Clone)]
pub struct Engine(Arc<Inner>);

const OWNERSHIP_TIMEOUT_ERROR: &str = "Launch tracker has no verified instance. Recovery paused; use Stop, then Start after checking the client";
const CLOSE_BLOCKED_PREFIX: &str = "Managed client close blocked: ";
const REJOIN_DISABLED: &str = "Automatic rejoin disabled; use Start to launch manually";
#[cfg(windows)]
const LAUNCH_COORDINATION_ERROR: &str = "Roblox launch coordination is not ready. Close existing Roblox clients yourself, wait a moment, then try Start again. If this persists, check Support and integrations for the coordination error";
#[cfg(not(windows))]
const LAUNCH_COORDINATION_ERROR: &str = "Multi-instance initialization failed. Check Support and integrations, resolve the reported issue, then restart RoLauncher";

fn recovery_due(account: &Account, now: chrono::DateTime<Utc>) -> bool {
    account.desired_running
        && account.auto_recovery
        && account.status == Status::Reconnecting
        && account
            .disconnected_since
            .is_some_and(|t| now - t >= ChronoDuration::seconds(30))
}

fn resume_after_confirmed_close(account: &mut Account) {
    let close_error = account.last_error.as_deref().is_some_and(|error| {
        error.starts_with(CLOSE_BLOCKED_PREFIX)
            || error == "Unable to close managed instance (Windows error 5)"
    });
    if account.status == Status::NeedsAttention
        && close_error
        && account.process.is_none()
        && account.tracker.is_none()
        && account.desired_running
        && account.auto_recovery
        && account.failures < 5
    {
        account.status = if account.failures == 0 {
            Status::Queued
        } else {
            Status::Backoff
        };
        if account.status == Status::Backoff && account.next_retry.is_none() {
            account.next_retry = Some(
                Utc::now()
                    + ChronoDuration::seconds(retry_seconds(account.failures).unwrap_or(160) as i64),
            );
        }
        account.last_error = Some("Managed client exit confirmed; recovery can continue".into());
    }
}

fn attach_verified_player(account: &mut Account, identity: ProcessIdentity) {
    account.process = Some(identity);
    // A late exact match resolves only the ownership-timeout pause, not auth or other failures.
    let ownership_resolved = account.last_error.as_deref() == Some(OWNERSHIP_TIMEOUT_ERROR);
    if account.desired_running && (account.status != Status::NeedsAttention || ownership_resolved) {
        account.status = Status::Unknown;
        account.last_error =
            Some("PID verified; waiting for a verified log and connection signal".into());
    }
}

#[derive(Clone, Default, serde::Deserialize)]
pub struct AccountPatch {
    pub alias: Option<String>,
    pub target: Option<Target>,
    pub auto_recovery: Option<bool>,
    pub fallback_policy: Option<FallbackPolicy>,
    pub group: Option<String>,
    #[serde(default)]
    pub clear_target: bool,
}

impl Engine {
    pub fn open(store: Store, compatibility: String) -> Result<Self, String> {
        Self::open_with_launch_gate(store, compatibility, Arc::new(AtomicBool::new(true)))
    }
    pub fn open_with_launch_gate(
        store: Store,
        compatibility: String,
        launch_allowed: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let mut database = store.load()?;
        let token = if database.encrypted_api_token.is_empty() {
            let token = zeroize::Zeroizing::new(format!(
                "{}{}",
                Uuid::new_v4().simple(),
                Uuid::new_v4().simple()
            ));
            database.encrypted_api_token = platform::protect(&token)?;
            store.save(&database)?;
            token
        } else {
            platform::unprotect(&database.encrypted_api_token)?
        };
        for saved in &mut database.accounts {
            let a = &mut saved.account;
            a.track_run(Utc::now());
            resume_after_confirmed_close(a);
            if a.desired_running && a.status != Status::NeedsAttention {
                a.status = if a.process.is_some() || a.tracker.is_some() {
                    Status::Unknown
                } else if a.status == Status::Backoff {
                    Status::Backoff
                } else {
                    Status::Queued
                };
            } else if !a.desired_running {
                a.status = Status::Stopped;
            }
        }
        database.version = 2;
        store.save(&database)?;
        let (events, _) = broadcast::channel(128);
        let discord = crate::discord::Discord::new(database.discord.clone())?;
        let bot_running = database.discord_bot_running && database.discord.bot.enabled;
        if bot_running {
            discord.bot_status("starting", "Connecting to Discord…");
        }
        Ok(Self(Arc::new(Inner {
            state: Mutex::new(State {
                database,
                operations: BTreeMap::new(),
                network_suspended: false,
                compatibility,
            }),
            store,
            roblox: Roblox::new()?,
            token,
            wake: Notify::new(),
            events,
            shutdown: AtomicBool::new(false),
            bot_generation: AtomicU64::new(u64::from(bot_running)),
            launch_allowed,
            discord,
        })))
    }
    pub fn token(&self) -> &str {
        &self.0.token
    }
    pub fn data_directory(&self) -> &std::path::Path {
        &self.0.store.directory
    }
    pub fn snapshot(&self) -> Snapshot {
        let s = self.0.state.lock().unwrap();
        snapshot(&s)
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Snapshot> {
        self.0.events.subscribe()
    }
    pub fn operation(&self, id: Uuid) -> Option<Operation> {
        self.0.state.lock().unwrap().operations.get(&id).cloned()
    }
    pub fn shutdown(&self) {
        self.0.shutdown.store(true, Ordering::Release);
        self.0.discord.stop();
        self.0.wake.notify_waiters();
    }
    pub fn is_shutdown(&self) -> bool {
        self.0.shutdown.load(Ordering::Acquire)
    }
    pub fn set_launch_allowed(&self, allowed: bool) {
        self.0.launch_allowed.store(allowed, Ordering::Release);
    }
    fn transaction<T>(
        &self,
        edit: impl FnOnce(&mut State) -> Result<T, String>,
    ) -> Result<T, String> {
        self.transaction_inner(edit, false)
    }
    fn transaction_inner<T>(
        &self,
        edit: impl FnOnce(&mut State) -> Result<T, String>,
        reset_discord: bool,
    ) -> Result<T, String> {
        let mut current = self.0.state.lock().unwrap();
        let mut next = current.clone();
        let result = edit(&mut next)?;
        let now = Utc::now();
        for saved in &mut next.database.accounts {
            let a = &mut saved.account;
            a.track_run(now);
            if a.status == Status::Reconnecting && a.desired_running {
                let known_error = a.last_error.as_deref().is_some_and(|e| {
                    e.starts_with("Session lost (code") || e.starts_with("Connection failed:")
                });
                if known_error
                    || a.disconnected_since
                        .is_some_and(|t| Utc::now() - t >= ChronoDuration::seconds(30))
                {
                    a.disconnect_notified = true;
                }
            } else {
                a.disconnect_notified = false;
            }
        }
        management::record_activity(&current, &mut next);
        self.0.store.save(&next.database)?;
        while next.operations.len() > 1000 {
            if let Some(key) = next
                .operations
                .iter()
                .find(|(_, v)| {
                    v.state == "completed" || v.state == "failed" || v.state == "cancelled"
                })
                .map(|(k, _)| *k)
            {
                next.operations.remove(&key);
            } else {
                break;
            }
        }
        if reset_discord || current.database.discord != next.database.discord {
            self.0.discord.configure(next.database.discord.clone());
        }
        if current.network_suspended != next.network_suspended {
            let _ = self.0.discord.record(
                crate::discord::Notice::network(next.network_suspended),
                false,
            );
        }
        for saved in &next.database.accounts {
            if let Some(old) = current
                .database
                .accounts
                .iter()
                .find(|a| a.account.id == saved.account.id)
                && let Some(mut notice) =
                    crate::discord::account_notice(&old.account, &saved.account)
            {
                notice.game_name = next
                    .database
                    .game_profiles
                    .iter()
                    .find(|p| Some(p.target.place_id) == notice.place_id)
                    .map(|p| p.game_name.clone());
                let _ = self.0.discord.record(notice, false);
            }
        }
        *current = next;
        let event = snapshot(&current);
        drop(current);
        let _ = self.0.events.send(event);
        self.0.wake.notify_one();
        Ok(result)
    }
    pub fn discord_settings(&self) -> crate::discord::DiscordView {
        self.0.discord.view()
    }
    pub(crate) fn bot_settings(&self) -> crate::discord_bot::SavedBot {
        let mut saved = self.0.state.lock().unwrap().database.discord.bot.clone();
        saved.registration = None;
        saved
    }
    pub(crate) fn bot_generation(&self) -> u64 {
        self.0.bot_generation.load(Ordering::Acquire)
    }
    pub(crate) fn bot_status(&self, generation: u64, state: &str, value: &str) {
        if self.bot_generation() == generation {
            self.0.discord.bot_status(state, value);
        }
    }
    pub(crate) fn bot_online_notice(&self, generation: u64, value: &str) {
        if self.bot_generation() == generation {
            self.0.discord.bot_online_notice(value);
        }
    }
    pub fn bot_control(&self, action: &str) -> Result<crate::discord::DiscordView, String> {
        if !matches!(action, "start" | "stop" | "restart") {
            return Err("Unsupported bot control".into());
        }
        let settings = self.bot_settings();
        if action != "stop"
            && (!settings.enabled
                || settings.encrypted_token.is_empty()
                || settings.guild_id.is_empty()
                || settings.allowed_users.is_empty())
        {
            return Err("Save and enable bot commands before starting the bot".into());
        }
        self.transaction(|s| {
            s.database.discord_bot_running = action != "stop";
            Ok(())
        })?;
        let generation = self
            .0
            .bot_generation
            .try_update(Ordering::AcqRel, Ordering::Acquire, |old| match action {
                "start" if old % 2 == 1 => Some(old),
                "start" | "restart" => Some((old / 2 + 1) * 2 + 1),
                "stop" => Some((old / 2 + 1) * 2),
                _ => None,
            })
            .map_err(|_| "Unsupported bot control")?;
        if action != "start" || generation.is_multiple_of(2) {
            self.0.discord.bot_status(
                if action == "stop" {
                    "offline"
                } else {
                    "starting"
                },
                if action == "stop" {
                    "Bot stopped. Saved settings are retained."
                } else {
                    "Connecting to Discord…"
                },
            );
        }
        Ok(self.discord_settings())
    }
    pub(crate) fn bot_registration(&self) -> Option<crate::discord_bot::StatusRegistration> {
        self.0
            .state
            .lock()
            .unwrap()
            .database
            .discord
            .bot
            .registration
            .clone()
    }
    pub(crate) fn set_bot_registration(
        &self,
        settings: &crate::discord_bot::SavedBot,
        registration: crate::discord_bot::StatusRegistration,
    ) -> Result<(), String> {
        self.transaction(|s| {
            let mut current = s.database.discord.bot.clone();
            current.registration = None;
            if &current != settings {
                return Err("Bot settings changed".into());
            }
            s.database.discord.bot.registration = Some(registration);
            Ok(())
        })
    }
    pub fn set_discord(
        &self,
        patch: crate::discord::DiscordPatch,
    ) -> Result<crate::discord::DiscordView, String> {
        let encrypted = patch
            .webhook_url
            .as_ref()
            .map(|value| {
                if value.trim().is_empty() {
                    Ok(String::new())
                } else {
                    crate::discord::validate_webhook(value).and_then(|url| platform::protect(&url))
                }
            })
            .transpose()?;
        self.transaction_inner(
            |s| {
                let discord = &mut s.database.discord;
                if let Some(bot) = &patch.bot {
                    discord.bot = bot.apply(&discord.bot)?;
                }
                if let Some(value) = encrypted {
                    discord.encrypted_webhook = value;
                }
                if let Some(value) = patch.enabled {
                    discord.enabled = value;
                }
                if let Some(value) = patch.notify_recovery {
                    discord.notify_recovery = value;
                }
                if discord.enabled && discord.encrypted_webhook.is_empty() {
                    return Err("Save a Discord webhook before enabling notifications".into());
                }
                Ok(())
            },
            true,
        )?;
        if patch.bot.is_some() && !self.bot_settings().enabled {
            self.bot_control("stop")?;
        }
        Ok(self.discord_settings())
    }
    pub fn test_discord(&self) -> Result<(), String> {
        self.0.discord.record(crate::discord::Notice::test(), true)
    }
    pub async fn import(&self, value: &str) -> Result<Account, String> {
        self.import_for(value, None).await
    }
    pub async fn import_for(&self, value: &str, expected: Option<&str>) -> Result<Account, String> {
        let cookie = normalize_cookie(value)?;
        let user = self.0.roblox.user(&cookie).await.map_err(|e| e.message)?;
        self.save_authenticated_session(user, &cookie, expected)
    }
    fn save_authenticated_session(
        &self,
        user: crate::roblox::User,
        cookie: &str,
        expected: Option<&str>,
    ) -> Result<Account, String> {
        if expected.is_some_and(|id| id != user.id.to_string()) {
            return Err("Sign-in account does not match the account being repaired".into());
        }
        let encrypted_session = platform::protect(cookie)?;
        self.transaction(|s| {
            let id = user.id.to_string();
            if expected.is_some() && !s.database.accounts.iter().any(|a| a.account.id == id) {
                return Err("Account was removed while sign-in was open".into());
            }
            if let Some(existing) = s.database.accounts.iter_mut().find(|a| a.account.id == id) {
                existing.encrypted_session = encrypted_session;
                existing.account.username = user.name;
                existing.account.last_error = None;
                existing.account.recovery_reason.clear();
                if existing.account.status == Status::NeedsAttention {
                    existing.account.desired_running = false;
                    existing.account.status = Status::Stopped;
                    existing.account.failures = 0;
                }
                return Ok(existing.account.clone());
            }
            if s.database.accounts.len() >= 50 {
                return Err("This version supports up to 50 saved accounts".into());
            }
            let account = Account::new(id, user.name);
            s.database.accounts.push(SavedAccount {
                account: account.clone(),
                encrypted_session,
            });
            Ok(account)
        })
    }
    pub fn patch(&self, id: &str, patch: AccountPatch) -> Result<Account, String> {
        management::validate_patch(&patch)?;
        self.transaction(|s| {
            let a = account_mut(s, id)?;
            management::apply_patch(a, patch);
            Ok(a.clone())
        })
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        self.transaction(|s| {
            let a = account_mut(s, id)?;
            if a.desired_running || a.process.is_some() || a.tracker.is_some() {
                return Err(
                    "Stop the account and wait for its managed instance to exit before removing it"
                        .into(),
                );
            }
            s.database.accounts.retain(|a| a.account.id != id);
            for profile in &mut s.database.profiles {
                profile.entries.retain(|e| e.account_id != id);
            }
            s.database.profiles.retain(|p| !p.entries.is_empty());
            Ok(())
        })
    }
    pub fn command(&self, id: &str, action: &str) -> Result<Operation, String> {
        self.command_inner(id, action, None)
    }
    pub(crate) fn bot_command(
        &self,
        settings: &crate::discord_bot::SavedBot,
        id: &str,
        action: &str,
    ) -> Result<Operation, String> {
        if !matches!(action, "stop" | "restart") {
            return Err("Unsupported bot action".into());
        }
        self.command_inner(id, action, Some(settings))
    }
    fn command_inner(
        &self,
        id: &str,
        action: &str,
        bot: Option<&crate::discord_bot::SavedBot>,
    ) -> Result<Operation, String> {
        if action == "retry" {
            return self.retry_now(id);
        }
        if !matches!(action, "start" | "stop" | "restart") {
            return Err("Unknown command".into());
        }
        self.transaction(|s| {
            if bot.is_some_and(|b| { let mut saved = s.database.discord.bot.clone(); saved.registration = None; !b.enabled || *b != saved || self.bot_generation().is_multiple_of(2) }) || self.is_shutdown() { return Err("Command connection changed".into()); }
            let existing = account_mut(s,id)?.clone();
            if (action == "start" && existing.desired_running && existing.status != Status::NeedsAttention) || (action == "stop" && !existing.desired_running) {
                if let Some(op) = existing.operation_id.and_then(|id|s.operations.get(&id)).cloned() { return Ok(op); }
                let op = Operation { id:Uuid::new_v4(), account_id:id.into(), state:"completed".into(), error:None }; s.operations.insert(op.id,op.clone()); return Ok(op);
            }
            if action != "stop" { existing.target.as_ref().ok_or("Set a launch target first")?.validate()?; }
            if action!="stop" && !self.0.launch_allowed.load(Ordering::Acquire){return Err(LAUNCH_COORDINATION_ERROR.into());}
            if action != "stop" && s.database.retired_launches.iter().any(|r|r.account_id==id && r.deadline>Utc::now()) { return Err("A cancelled launch is still being observed; wait up to 90 seconds before starting again".into()); }
            if action == "restart" && matches!(existing.status,Status::Launching|Status::Queued) { return Err("A launch is already pending; Stop it before restarting".into()); }
            if let Some(previous) = existing.operation_id.and_then(|id|s.operations.get_mut(&id)) && previous.state == "queued" { previous.state = "cancelled".into(); }
            let op = Operation { id:Uuid::new_v4(),account_id:id.into(),state:if action=="stop" && existing.process.is_none(){"completed"}else{"queued"}.into(),error:None };
            if existing.process.is_none() && let Some(tracker)=existing.tracker {
                s.database.retired_launches.push(RetiredLaunch {tracker,generation:existing.generation,account_id:id.into(),deadline:existing.ownership_deadline.unwrap_or_else(||Utc::now()+ChronoDuration::seconds(90))});
            }
            let a = account_mut(s,id)?; a.finish_run(Utc::now()); a.desired_running = action != "stop";
            if a.process.is_none() { a.tracker=None;a.ownership_deadline=None; }
            a.generation = Uuid::new_v4(); a.failures = 0; a.next_retry = None; a.connected_since = None; a.disconnected_since = None; a.last_error = None;
            a.status = if action == "stop" { Status::Stopped } else { Status::Queued };
            a.public_fallback_active = false; a.recovery_reason.clear(); a.operation_id = Some(op.id);
            s.operations.insert(op.id,op.clone()); Ok(op)
        })
    }
    fn update(
        &self,
        id: &str,
        generation: Uuid,
        edit: impl FnOnce(&mut Account),
    ) -> Result<(), String> {
        self.transaction(|s| {
            let a = account_mut(s, id)?;
            if a.generation == generation {
                edit(a);
            }
            Ok(())
        })
    }
    fn finish_operation(&self, id: &str, generation: Uuid, error: Option<String>) {
        let _ = self.transaction(|s| {
            let a = account_mut(s, id)?;
            if a.generation != generation {
                return Ok(());
            }
            if let Some(opid) = a.operation_id
                && let Some(op) = s.operations.get_mut(&opid)
            {
                op.state = if error.is_some() {
                    "failed"
                } else {
                    "completed"
                }
                .into();
                op.error = error;
            }
            Ok(())
        });
    }
    fn failure(&self, id: &str, generation: Uuid, error: Failure) {
        let _ = self.transaction(|s| {
            if account_mut(s, id)?.generation != generation {
                return Ok(());
            }
            if error.kind == FailureKind::Network {
                s.network_suspended = true;
            }
            let a = account_mut(s, id)?;
            apply_failure(a, &error);
            let paused = a.status == Status::NeedsAttention;
            let opid = a.operation_id;
            if paused && let Some(op) = opid.and_then(|id| s.operations.get_mut(&id)) {
                op.state = "failed".into();
                op.error = Some(error.message);
            }
            Ok(())
        });
    }
    pub fn spawn(&self) {
        let engine = self.clone();
        tokio::spawn(async move {
            crate::discord_bot::run(engine).await;
        });
        let engine = self.clone();
        tokio::spawn(async move {
            engine.0.discord.run(Some(engine.clone())).await;
        });
        let engine = self.clone();
        tokio::spawn(async move {
            engine.launch_loop().await;
        });
        let engine = self.clone();
        tokio::spawn(async move {
            engine.monitor_loop().await;
        });
    }
    async fn launch_loop(&self) {
        let mut next_launch = Instant::now();
        while !self.is_shutdown() {
            let snapshot = self.snapshot();
            if snapshot.network_suspended {
                if self.0.roblox.reachable().await {
                    let _ = self.transaction(|s| {
                        s.network_suspended = false;
                        Ok(())
                    });
                } else {
                    tokio::select! { _=tokio::time::sleep(Duration::from_secs(30))=>{}, _=self.0.wake.notified()=>{} }
                    continue;
                }
            }
            let next = self.snapshot().accounts.into_iter().find(|a| {
                a.desired_running
                    && a.process.is_none()
                    && a.tracker.is_none()
                    && matches!(a.status, Status::Queued | Status::Backoff)
                    && a.next_retry.is_none_or(|t| t <= Utc::now())
            });
            if let Some(account) = next {
                tokio::time::sleep_until(next_launch.into()).await;
                if self.is_shutdown() {
                    break;
                }
                let current = self
                    .snapshot()
                    .accounts
                    .into_iter()
                    .find(|a| a.id == account.id);
                let current = current.filter(|a| {
                    a.generation == account.generation
                        && a.desired_running
                        && matches!(a.status, Status::Queued | Status::Backoff)
                        && a.next_retry.is_none_or(|t| t <= Utc::now())
                });
                let Some(current) = current else {
                    continue;
                };
                self.launch_one(current).await;
                next_launch = Instant::now() + Duration::from_secs(5);
            } else {
                tokio::select! { _=tokio::time::sleep(Duration::from_secs(1))=>{}, _=self.0.wake.notified()=>{} }
            }
        }
    }
    async fn launch_one(&self, account: Account) {
        if !self.0.launch_allowed.load(Ordering::Acquire) {
            self.failure(
                &account.id,
                account.generation,
                Failure::new(FailureKind::Unsupported, LAUNCH_COORDINATION_ERROR),
            );
            return;
        }
        let target = match account.effective_target() {
            Some(t) => t,
            None => {
                self.failure(
                    &account.id,
                    account.generation,
                    Failure::new(FailureKind::Other, "Set a launch target"),
                );
                return;
            }
        };
        let saved = self
            .0
            .state
            .lock()
            .unwrap()
            .database
            .accounts
            .iter()
            .find(|a| a.account.id == account.id)
            .cloned();
        let Some(saved) = saved else { return };
        let cookie = match platform::unprotect(&saved.encrypted_session) {
            Ok(c) => c,
            Err(_) => {
                self.failure(
                    &account.id,
                    account.generation,
                    Failure::new(
                        FailureKind::Auth,
                        "Session cannot be decrypted; sign in again",
                    ),
                );
                return;
            }
        };
        let tracker = loop {
            let candidate = format!(
                "{}",
                rand::random_range(100_000_000_000u64..999_999_999_999)
            );
            let state = self.0.state.lock().unwrap();
            if !state
                .database
                .accounts
                .iter()
                .any(|a| a.account.tracker.as_deref() == Some(&candidate))
                && !state
                    .database
                    .retired_launches
                    .iter()
                    .any(|r| r.tracker == candidate)
            {
                break candidate;
            }
        };
        if self
            .update(&account.id, account.generation, |a| {
                a.status = Status::Launching;
            })
            .is_err()
        {
            return;
        }
        let uri = match self.0.roblox.launch_uri(&cookie, &target, &tracker).await {
            Ok(uri) => uri,
            Err(e) => {
                self.failure(&account.id, account.generation, e);
                return;
            }
        };
        // Persist the tracker BEFORE launching. Holding the command mutex through ShellExecute
        // makes Stop linearize either before launch or after durable ownership was recorded.
        let launched = {
            let mut state = self.0.state.lock().unwrap();
            let mut next = state.clone();
            let Ok(a) = account_mut(&mut next, &account.id) else {
                return;
            };
            if a.generation != account.generation || !a.desired_running || self.is_shutdown() {
                return;
            }
            if account.status == Status::Backoff && !a.auto_recovery {
                a.status = Status::NeedsAttention;
                a.next_retry = None;
                a.last_error = Some(REJOIN_DISABLED.into());
                if self.0.store.save(&next.database).is_ok() {
                    *state = next;
                    let _ = self.0.events.send(snapshot(&state));
                }
                return;
            }
            a.tracker = Some(tracker.clone());
            a.ownership_deadline = Some(Utc::now() + ChronoDuration::seconds(90));
            if let Err(e) = self.0.store.save(&next.database) {
                Err(e)
            } else {
                *state = next;
                platform::launch_for(&uri, &account.id)
            }
        };
        if let Err(message) = launched {
            let _ = self.update(&account.id, account.generation, |a| a.tracker = None);
            self.failure(
                &account.id,
                account.generation,
                Failure::new(FailureKind::Unsupported, &message),
            );
        }
        // Return immediately: shared discovery binds the PID or expires the 90s deadline.
    }
    async fn monitor_loop(&self) {
        let mut tails: HashMap<(u32, String), crate::logs::Tail> = HashMap::new();
        let mut last_discovery = Instant::now() - Duration::from_secs(30);
        let mut last_log_discovery = Instant::now() - Duration::from_secs(30);
        let mut close_attempts: HashMap<(u32, String), Instant> = HashMap::new();
        let mut exit_watches: HashMap<(u32, String), platform::ExitWatch> = HashMap::new();
        while !self.is_shutdown() {
            let retired = self
                .0
                .state
                .lock()
                .unwrap()
                .database
                .retired_launches
                .clone();
            if last_discovery.elapsed() >= Duration::from_secs(5)
                && (self.snapshot().accounts.iter().any(|a| a.tracker.is_some())
                    || !retired.is_empty())
            {
                let result = tokio::task::spawn_blocking(platform::players).await;
                if let Ok(Ok(players)) = result {
                    for old in &retired {
                        for player in players.iter().filter(|p| p.tracker == old.tracker) {
                            let identity = player.identity(old.generation);
                            let key = (identity.pid, identity.creation_time.clone());
                            let started = *close_attempts.entry(key).or_insert_with(Instant::now);
                            let _ = platform::close(
                                &identity,
                                started.elapsed() >= Duration::from_secs(5),
                            );
                        }
                    }
                    for account in self.snapshot().accounts {
                        if account.process.is_some() || account.tracker.is_none() {
                            continue;
                        }
                        let matches: Vec<_> = players
                            .iter()
                            .filter(|p| Some(&p.tracker) == account.tracker.as_ref())
                            .collect();
                        if matches.len() == 1 {
                            let identity = matches[0].identity(account.generation);
                            let _ = self.update(&account.id, account.generation, |a| {
                                attach_verified_player(a, identity)
                            });
                            // Discover its log now; an empty scan during startup
                            // must not leave a newly verified player waiting 30s.
                            last_log_discovery = Instant::now() - Duration::from_secs(30);
                        } else if matches.len() > 1 {
                            let _=self.update(&account.id,account.generation,|a| { a.status=Status::NeedsAttention;a.last_error=Some("Multiple primary instances share this launch tracker; ownership is ambiguous".into()); });
                        } else if account.desired_running
                            && account.process.is_none()
                            && account.status != Status::NeedsAttention
                            && account.ownership_deadline.is_none_or(|t| t <= Utc::now())
                        {
                            let _ = self.update(&account.id, account.generation, |a| {
                                a.status = Status::NeedsAttention;
                                a.last_error = Some(OWNERSHIP_TIMEOUT_ERROR.into());
                            });
                            self.finish_operation(
                                &account.id,
                                account.generation,
                                Some("Launch ownership could not be verified".into()),
                            );
                        } else if !account.desired_running && account.process.is_none() {
                            // Stopped processes are handled below; late launches use durable tombstones.
                        }
                    }
                }
                last_discovery = Instant::now();
            }
            if last_log_discovery.elapsed() >= Duration::from_secs(30) {
                let identities: Vec<_> = self
                    .snapshot()
                    .accounts
                    .iter()
                    .filter_map(|a| a.process.clone())
                    .filter(|p| !tails.contains_key(&(p.pid, p.creation_time.clone())))
                    .collect();
                if !identities.is_empty()
                    && let Ok(Ok(logs)) =
                        tokio::task::spawn_blocking(move || platform::owned_logs(&identities)).await
                {
                    for (key, path) in logs {
                        if let Ok(tail) = crate::logs::Tail::new(path, false) {
                            tails.insert(key, tail);
                        }
                    }
                }
                last_log_discovery = Instant::now();
            }
            for account in self.snapshot().accounts {
                let Some(identity) = account.process.clone() else {
                    continue;
                };
                let key = (identity.pid, identity.creation_time.clone());
                if !exit_watches.contains_key(&key)
                    && let Some(watch) = platform::ExitWatch::new(&identity)
                {
                    exit_watches.insert(key.clone(), watch);
                }
                if !account.auto_recovery && account.last_error.as_deref() == Some(REJOIN_DISABLED)
                {
                    close_attempts.remove(&key);
                }
                match platform::alive(&identity) {
                    Ok(false) => {
                        let clean =
                            exit_watches.remove(&key).and_then(|w| w.clean_exit()) == Some(true);
                        tails.remove(&key);
                        close_attempts.remove(&key);
                        let _ = self.update(&account.id, account.generation, |a| {
                            a.process = None;
                            a.tracker = None;
                            a.ownership_deadline = None;
                            a.connected_since = None;
                            a.disconnected_since = None;
                            resume_after_confirmed_close(a);
                            if a.desired_running
                                && a.auto_recovery
                                && !matches!(
                                    a.status,
                                    Status::NeedsAttention | Status::Queued | Status::Backoff
                                )
                            {
                                apply_failure(
                                    a,
                                    &Failure::new(
                                        FailureKind::Other,
                                        "Managed client exited; scheduling recovery",
                                    ),
                                );
                            } else if !a.desired_running || !a.auto_recovery {
                                a.desired_running = false;
                                a.status = Status::Stopped;
                            }
                            if clean && !account.disconnect_notified {
                                a.last_error = Some("Client closed normally".into());
                                a.recovery_reason = "NormalExit".into();
                            }
                        });
                        if !account.desired_running {
                            self.finish_operation(&account.id, account.generation, None);
                        }
                        continue;
                    }
                    Err(_) => {
                        if account.desired_running && account.status != Status::Unknown {
                            let _=self.update(&account.id,account.generation,|a| { a.status=Status::Unknown;a.last_error=Some("Process metadata inaccessible; automatic replacement is disabled".into()); });
                        }
                        continue;
                    }
                    Ok(true) => {}
                }
                if account.desired_running
                    && account.status != Status::Queued
                    && !close_attempts.contains_key(&key)
                    && !(account.recovery_reason == "TargetUnavailable"
                        && account.status == Status::Backoff)
                    && let Some(tail) = tails.get_mut(&key)
                {
                    match tail.read() {
                        Ok(signals) => {
                            for signal in signals {
                                self.signal(&account.id, account.generation, signal);
                            }
                        }
                        Err(_) => {
                            tails.remove(&key);
                            let _ = self.update(&account.id, account.generation, |a| {
                                a.status = Status::Unknown;
                                a.connected_since = None;
                                a.disconnected_since = None;
                                a.last_error =
                                    Some("Log unavailable; disconnect monitoring disabled".into());
                            });
                        }
                    }
                }
                // Read fresh reconnect/disconnect events before deciding to replace a client.
                if !account.disconnect_notified
                    && account.status == Status::Reconnecting
                    && account
                        .disconnected_since
                        .is_some_and(|t| Utc::now() - t >= ChronoDuration::seconds(30))
                {
                    self.0.discord.prepare_capture(&identity).await;
                    let _ = self.update(&account.id, account.generation, |_| {});
                }
                let Some(account) = self.snapshot().accounts.into_iter().find(|a| {
                    a.id == account.id
                        && a.generation == account.generation
                        && a.process.as_ref() == Some(&identity)
                }) else {
                    continue;
                };
                let recovering = recovery_due(&account, Utc::now());
                let should_close = !account.desired_running
                    || account.status == Status::Queued
                    || recovering
                    || close_attempts.contains_key(&key)
                    || account.recovery_reason == "TargetUnavailable"
                        && account.status == Status::Backoff;
                if should_close {
                    if recovering && !close_attempts.contains_key(&key) {
                        self.failure(
                            &account.id,
                            account.generation,
                            Failure::new(
                                FailureKind::Other,
                                "Disconnected for more than 30 seconds; scheduling recovery",
                            ),
                        );
                    }
                    let started = *close_attempts
                        .entry(key.clone())
                        .or_insert_with(Instant::now);
                    let force = started.elapsed() >= Duration::from_secs(5);
                    if let Err(e) = platform::close(&identity, force) {
                        // Recheck the exact identity: it may have exited while close was in flight.
                        if matches!(platform::alive(&identity), Ok(false)) {
                            continue;
                        }
                        let _ = self.update(&account.id, account.generation, |a| {
                            a.status = Status::NeedsAttention;
                            a.last_error = Some(format!("{CLOSE_BLOCKED_PREFIX}{e}"));
                        });
                    }
                    continue;
                }
                if account
                    .connected_since
                    .is_some_and(|t| Utc::now() - t >= ChronoDuration::seconds(120))
                    && account.failures > 0
                {
                    let _ = self.update(&account.id, account.generation, |a| a.failures = 0);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    fn signal(&self, id: &str, generation: Uuid, signal: Signal) {
        match signal {
            Signal::Connected => {
                let _ = self.update(id, generation, |a| {
                    if a.desired_running && a.status != Status::NeedsAttention {
                        a.status = Status::Running;
                        a.connected_since = Some(Utc::now());
                        a.disconnected_since = None;
                        a.last_error = None;
                        a.recovery_reason.clear();
                    }
                });
                self.finish_operation(id, generation, None);
            }
            Signal::Disconnected | Signal::ConnectionFailed | Signal::SessionLost(_) => {
                let _ = self.update(id, generation, |a| {
                    if a.desired_running && a.status != Status::NeedsAttention {
                        if a.disconnected_since.is_none() {
                            a.last_error = Some("Game connection lost; allowing 30 seconds to reconnect before recovery".into());
                        }
                        a.status = Status::Reconnecting;
                        a.disconnected_since.get_or_insert_with(Utc::now);
                        a.connected_since = None;
                        let kick_known = a.last_error.as_deref().is_some_and(|error| error.starts_with("Session lost (code 267)"));
                        if signal == Signal::ConnectionFailed && !kick_known {
                            a.recovery_reason = "ConnectionFailed".into();
                            a.last_error = Some("Connection failed: server did not respond (279); allowing 30 seconds to reconnect before recovery".into());
                        } else if let Signal::SessionLost(code) = signal && !kick_known {
                            a.recovery_reason = if code == Some(267) { "Kick267" } else { "SessionLost" }.into();
                            a.last_error = Some(match code {
                                Some(code) => format!("Session lost (code {code}); allowing 30 seconds to reconnect before recovery"),
                                None => "Session lost; allowing 30 seconds to reconnect before recovery".into(),
                            });
                        }
                    }
                });
            }
            Signal::TargetUnavailable => self.failure(
                id,
                generation,
                Failure::new(
                    FailureKind::TargetUnavailable,
                    "Configured destination unavailable; applying the account fallback policy",
                ),
            ),
            Signal::PermissionDenied => self.failure(
                id,
                generation,
                Failure::new(
                    FailureKind::Permission,
                    "Game denied access; check permissions before restarting",
                ),
            ),
        }
    }
}
fn account_mut<'a>(s: &'a mut State, id: &str) -> Result<&'a mut Account, String> {
    s.database
        .accounts
        .iter_mut()
        .find(|a| a.account.id == id)
        .map(|a| &mut a.account)
        .ok_or_else(|| "Account not found".into())
}
fn snapshot(s: &State) -> Snapshot {
    Snapshot {
        game_profiles: s.database.game_profiles.clone(),
        close_to_tray: s.database.close_to_tray,
        accounts: s
            .database
            .accounts
            .iter()
            .map(|a| a.account.clone())
            .collect(),
        network_suspended: s.network_suspended,
        compatibility: s.compatibility.clone(),
        profiles: s.database.profiles.clone(),
        update_repository: s.database.update_repository.clone(),
        include_beta_updates: s.database.include_beta_updates,
    }
}
fn apply_failure(a: &mut Account, error: &Failure) {
    a.recovery_reason = format!("{:?}", error.kind);
    a.last_error = Some(error.message.clone());
    a.connected_since = None;
    a.disconnected_since = None;
    if error.kind == FailureKind::Network {
        a.status = Status::Backoff;
        a.next_retry = None;
        return;
    }
    if matches!(
        error.kind,
        FailureKind::Auth | FailureKind::Permission | FailureKind::Unsupported
    ) {
        a.status = Status::NeedsAttention;
        a.next_retry = None;
        return;
    }
    if error.kind == FailureKind::TargetUnavailable {
        match a.fallback_policy {
            FallbackPolicy::AllowPublic => a.public_fallback_active = true,
            FallbackPolicy::Stay => a.public_fallback_active = false,
            FallbackPolicy::Pause => {
                a.status = Status::NeedsAttention;
                a.next_retry = None;
                a.last_error =
                    Some("Destination unavailable; fallback policy requires your attention".into());
                return;
            }
        }
    }
    a.failures += 1;
    if a.failures >= 5 || !a.auto_recovery {
        a.status = Status::NeedsAttention;
        a.next_retry = None;
    } else {
        let delay =
            retry_seconds(a.failures)
                .unwrap_or(160)
                .max(if error.kind == FailureKind::RateLimit {
                    60
                } else {
                    0
                })
                + rand::random_range(0..=3);
        a.status = Status::Backoff;
        a.next_retry = Some(Utc::now() + ChronoDuration::seconds(delay as i64));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bot_controls_reuse_saved_settings_without_starting_on_save() {
        let path = std::env::temp_dir().join(format!("rbx-bot-control-test-{}", Uuid::new_v4()));
        let engine = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
        let mut patch = crate::discord::DiscordPatch::default();
        patch.bot = Some(crate::discord_bot::BotPatch {
            enabled: Some(true),
            token: Some("TEST_BOT_TOKEN".into()),
            guild_id: Some("42".into()),
            allowed_users: Some(vec!["123".into()]),
        });
        engine.set_discord(patch).unwrap();
        let saved = engine.bot_settings();
        assert_eq!(engine.bot_generation(), 0);
        assert_eq!(engine.discord_settings().bot.connection_state, "offline");
        assert!(engine.bot_control("invalid").is_err());
        engine.bot_control("start").unwrap();
        let first = engine.bot_generation();
        assert_eq!(first % 2, 1);
        engine.bot_control("start").unwrap();
        assert_eq!(engine.bot_generation(), first);
        engine.bot_control("restart").unwrap();
        assert!(engine.bot_generation() > first);
        engine.bot_status(first, "online", "stale connection");
        assert_eq!(engine.discord_settings().bot.connection_state, "starting");
        let registration = crate::discord_bot::StatusRegistration {
            guild_id: "42".into(),
            channel_id: 43,
            message_id: 44,
            page: 1,
        };
        engine
            .set_bot_registration(&saved, registration.clone())
            .unwrap();
        assert!(engine.bot_settings() == saved);
        assert!(engine.bot_registration() == Some(registration.clone()));
        engine.bot_control("stop").unwrap();
        assert_eq!(engine.bot_generation() % 2, 0);
        assert!(engine.bot_settings() == saved);
        engine.bot_control("start").unwrap();
        assert_eq!(engine.bot_generation() % 2, 1);
        let reopened = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
        assert!(reopened.bot_settings() == saved);
        assert!(reopened.bot_registration() == Some(registration));
        assert_eq!(reopened.bot_generation(), 1);
        reopened.bot_control("stop").unwrap();
        let stopped = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
        assert_eq!(stopped.bot_generation(), 0);
        stopped.shutdown();
        engine.shutdown();
        reopened.shutdown();
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn start_recovers_when_launch_coordination_becomes_ready() {
        let (engine, path) = fixture(1);
        let gate = engine.0.launch_allowed.clone();
        gate.store(false, Ordering::Release);
        assert_eq!(
            engine.command("1", "start").unwrap_err(),
            LAUNCH_COORDINATION_ERROR
        );
        assert!(!engine.snapshot().accounts[0].desired_running);
        assert!(engine.command("1", "stop").is_ok());
        gate.store(true, Ordering::Release);
        assert!(engine.command("1", "start").is_ok());
        assert!(engine.snapshot().accounts[0].desired_running);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn discord_persists_encrypted_settings_and_records_only_committed_incidents() {
        let (engine, path) = fixture(1);
        let hook = "https://discord.com/api/webhooks/123456/SECRET_WEBHOOK";
        engine
            .set_discord(crate::discord::DiscordPatch {
                enabled: Some(true),
                webhook_url: Some(hook.into()),
                notify_recovery: Some(true),
                bot: None,
            })
            .unwrap();
        let saved = std::fs::read_to_string(path.join("accounts.json")).unwrap();
        assert!(!saved.contains(hook));
        assert!(!saved.contains("SECRET_WEBHOOK"));
        let reopened = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
        assert!(reopened.discord_settings().configured);
        assert!(reopened.discord_settings().enabled);
        assert!(
            !serde_json::to_string(&reopened.discord_settings())
                .unwrap()
                .contains("SECRET")
        );
        drop(reopened);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.signal("1", generation, Signal::Connected);
        engine.signal("1", generation, Signal::Disconnected);
        engine.signal("1", generation, Signal::SessionLost(Some(267)));
        let count = engine.discord_settings().recent.len();
        assert_eq!(count, 1); // Transient disconnect is silent; the confirmed kick is reported.
        engine.signal("1", generation, Signal::ConnectionFailed);
        engine.signal("1", generation, Signal::SessionLost(None));
        engine.signal("1", generation, Signal::SessionLost(Some(267)));
        engine.signal("1", Uuid::new_v4(), Signal::SessionLost(Some(267)));
        assert_eq!(engine.discord_settings().recent.len(), count);
        engine.signal("1", generation, Signal::Connected);
        assert_eq!(
            engine.discord_settings().recent[0].title,
            "Account is back online"
        );
        engine
            .set_discord(crate::discord::DiscordPatch {
                enabled: None,
                webhook_url: None,
                notify_recovery: None,
                bot: None,
            })
            .unwrap();
        assert!(
            engine
                .discord_settings()
                .delivery_status
                .starts_with("Settings saved")
        );
        assert_eq!(engine.discord_settings().recent.len(), count + 1);
        std::fs::create_dir(path.join("accounts.json.tmp")).unwrap();
        engine.signal("1", generation, Signal::Disconnected);
        assert_eq!(engine.snapshot().accounts[0].status, Status::Running);
        assert_eq!(engine.discord_settings().recent.len(), count + 1);
        assert!(
            engine
                .set_discord(crate::discord::DiscordPatch {
                    enabled: Some(false),
                    webhook_url: None,
                    notify_recovery: None,
                    bot: None,
                })
                .is_err()
        );
        assert!(engine.discord_settings().enabled);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn discord_redirect_grace_only_reports_persistent_disconnects_and_recovers_once() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.signal("1", generation, Signal::Connected);
        engine.signal("1", generation, Signal::Disconnected);
        engine.signal("1", generation, Signal::Connected);
        assert!(engine.discord_settings().recent.is_empty());
        engine.signal("1", generation, Signal::Disconnected);
        engine
            .update("1", generation, |a| {
                a.disconnected_since = Some(Utc::now() - ChronoDuration::seconds(31))
            })
            .unwrap();
        assert_eq!(engine.discord_settings().recent.len(), 1);
        engine.update("1", generation, |_| {}).unwrap();
        assert_eq!(engine.discord_settings().recent.len(), 1);
        engine.signal("1", generation, Signal::Connected);
        assert_eq!(engine.discord_settings().recent.len(), 2);
        assert_eq!(
            engine.discord_settings().recent[0].title,
            "Account is back online"
        );
        engine.command("1", "stop").unwrap();
        assert_eq!(engine.discord_settings().recent.len(), 2);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn switching_rejoin_off_cancels_backoff_and_on_resumes_only_that_pause() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.failure(
            "1",
            generation,
            Failure::new(FailureKind::Other, "test disconnect"),
        );
        assert_eq!(engine.snapshot().accounts[0].status, Status::Backoff);
        engine
            .patch(
                "1",
                AccountPatch {
                    alias: None,
                    target: None,
                    auto_recovery: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
        let paused = engine.snapshot().accounts.remove(0);
        assert_eq!(paused.status, Status::NeedsAttention);
        assert!(paused.next_retry.is_none());
        engine
            .patch(
                "1",
                AccountPatch {
                    alias: None,
                    target: None,
                    auto_recovery: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(engine.snapshot().accounts[0].status, Status::Backoff);
        engine.failure(
            "1",
            generation,
            Failure::new(FailureKind::Auth, "Expired session"),
        );
        engine
            .patch(
                "1",
                AccountPatch {
                    alias: None,
                    target: None,
                    auto_recovery: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn generic_kicks_wait_for_grace_and_reconnection_stop_or_disable_prevent_replacement() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.signal("1", generation, Signal::SessionLost(Some(267)));
        let mut account = engine.snapshot().accounts.remove(0);
        let start = account.disconnected_since.unwrap();
        assert!(!recovery_due(&account, start + ChronoDuration::seconds(29)));
        assert!(recovery_due(&account, start + ChronoDuration::seconds(30)));
        assert!(!account.public_fallback_active);
        assert_eq!(account.failures, 0);
        engine.signal("1", generation, Signal::Connected);
        assert!(!recovery_due(
            &engine.snapshot().accounts[0],
            start + ChronoDuration::seconds(40)
        ));
        account.auto_recovery = false;
        assert!(!recovery_due(&account, start + ChronoDuration::seconds(40)));
        account.auto_recovery = true;
        account.desired_running = false;
        assert!(!recovery_due(&account, start + ChronoDuration::seconds(40)));
        engine.command("1", "stop").unwrap();
        engine.signal("1", generation, Signal::SessionLost(Some(267)));
        assert_eq!(engine.snapshot().accounts[0].status, Status::Stopped);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn connection_failure_preserves_target_grace_and_stop() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let before = engine.snapshot().accounts.remove(0);
        engine.signal("1", before.generation, Signal::ConnectionFailed);
        let failed = engine.snapshot().accounts.remove(0);
        assert_eq!(failed.status, Status::Reconnecting);
        assert!(failed.disconnected_since.is_some());
        assert_eq!(failed.failures, 0);
        assert!(!failed.public_fallback_active);
        engine.signal("1", before.generation, Signal::ConnectionFailed);
        assert_eq!(
            engine.snapshot().accounts[0].disconnected_since,
            failed.disconnected_since
        );
        engine.signal("1", before.generation, Signal::Connected);
        assert_eq!(engine.snapshot().accounts[0].status, Status::Running);
        engine.command("1", "stop").unwrap();
        engine.signal("1", before.generation, Signal::ConnectionFailed);
        assert_eq!(engine.snapshot().accounts[0].status, Status::Stopped);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn confirmed_close_resumes_retry_without_counting_another_failure() {
        let mut account = Account::new("1".into(), "test".into());
        account.desired_running = true;
        account.status = Status::NeedsAttention;
        account.failures = 1;
        account.last_error = Some("Unable to close managed instance (Windows error 5)".into());
        let retry = Utc::now() + ChronoDuration::seconds(20);
        account.next_retry = Some(retry);
        resume_after_confirmed_close(&mut account);
        assert_eq!(account.status, Status::Backoff);
        assert_eq!(account.failures, 1);
        assert_eq!(account.next_retry, Some(retry));
        account.status = Status::NeedsAttention;
        account.last_error = Some("Session expired; sign in again".into());
        resume_after_confirmed_close(&mut account);
        assert_eq!(account.status, Status::NeedsAttention);
        account.last_error = Some(format!("{CLOSE_BLOCKED_PREFIX}access denied"));
        account.process = Some(ProcessIdentity {
            pid: 123,
            creation_time: "456".into(),
            tracker: "123456789".into(),
            generation: account.generation,
        });
        resume_after_confirmed_close(&mut account);
        assert_eq!(account.status, Status::NeedsAttention);
        account.process = None;
        account.failures = 5;
        resume_after_confirmed_close(&mut account);
        assert_eq!(account.status, Status::NeedsAttention);
        account.failures = 1;
        account.desired_running = false;
        resume_after_confirmed_close(&mut account);
        assert_eq!(account.status, Status::NeedsAttention);
    }
    #[test]
    fn exact_late_player_match_resolves_only_ownership_timeout() {
        let mut account = Account::new("1".into(), "test".into());
        account.desired_running = true;
        account.status = Status::NeedsAttention;
        account.last_error = Some(OWNERSHIP_TIMEOUT_ERROR.into());
        let identity = ProcessIdentity {
            pid: 123,
            creation_time: "456".into(),
            tracker: "123456789".into(),
            generation: account.generation,
        };
        attach_verified_player(&mut account, identity.clone());
        assert_eq!(account.status, Status::Unknown);
        assert_eq!(account.process.as_ref().unwrap().pid, 123);
        account.status = Status::NeedsAttention;
        account.last_error = Some("Session expired; sign in again".into());
        attach_verified_player(&mut account, identity.clone());
        assert_eq!(account.status, Status::NeedsAttention);
        account.desired_running = false;
        account.status = Status::Stopped;
        attach_verified_player(&mut account, identity);
        assert_eq!(account.status, Status::Stopped);
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn bulk_settings_are_atomic_preserve_omissions_and_expand_aliases() {
        let (engine, path) = fixture(3);
        engine
            .patch(
                "2",
                AccountPatch {
                    target: Some(Target {
                        place_id: 222,
                        job_id: None,
                        private_server_link: None,
                    }),
                    auto_recovery: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
        engine
            .bulk_patch(BulkPatch {
                account_ids: vec!["1".into(), "2".into()],
                patch: AccountPatch {
                    group: Some("Team".into()),
                    ..Default::default()
                },
            })
            .unwrap();
        let accounts = engine.snapshot().accounts;
        assert_eq!(accounts[0].group, "Team");
        assert_eq!(accounts[1].target.as_ref().unwrap().place_id, 222);
        assert!(!accounts[1].auto_recovery);
        assert!(accounts[2].group.is_empty());
        let before = serde_json::to_string(&accounts).unwrap();
        for ids in [
            vec!["1".into(), "missing".into()],
            vec!["1".into(), "1".into()],
        ] {
            assert!(
                engine
                    .bulk_patch(BulkPatch {
                        account_ids: ids,
                        patch: AccountPatch {
                            alias: Some("changed".into()),
                            ..Default::default()
                        }
                    })
                    .is_err()
            );
        }
        assert_eq!(
            serde_json::to_string(&engine.snapshot().accounts).unwrap(),
            before
        );
        engine
            .bulk_patch(BulkPatch {
                account_ids: vec!["2".into(), "1".into()],
                patch: AccountPatch {
                    alias: Some("{index}-{username}-{id}".into()),
                    fallback_policy: Some(FallbackPolicy::Pause),
                    ..Default::default()
                },
            })
            .unwrap();
        assert_eq!(engine.snapshot().accounts[0].alias, "2-account0-1");
        assert_eq!(engine.snapshot().accounts[1].alias, "1-account1-2");
        std::fs::create_dir(path.join("accounts.json.tmp")).unwrap();
        assert!(
            engine
                .bulk_patch(BulkPatch {
                    account_ids: vec!["1".into(), "2".into()],
                    patch: AccountPatch {
                        group: Some("Rollback".into()),
                        ..Default::default()
                    }
                })
                .is_err()
        );
        assert_eq!(engine.snapshot().accounts[0].group, "Team");
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn profiles_capture_each_account_and_import_validates_membership_atomically() {
        let (engine, path) = fixture(2);
        engine
            .patch(
                "2",
                AccountPatch {
                    target: Some(Target {
                        place_id: 222,
                        job_id: None,
                        private_server_link: None,
                    }),
                    group: Some("Second".into()),
                    auto_recovery: Some(false),
                    fallback_policy: Some(FallbackPolicy::Stay),
                    ..Default::default()
                },
            )
            .unwrap();
        let p = engine
            .save_profile(ProfileSave {
                name: "Team".into(),
                account_ids: vec!["1".into(), "2".into()],
                id: None,
            })
            .unwrap();
        assert_eq!(p.entries[1].target.as_ref().unwrap().place_id, 222);
        engine
            .bulk_patch(BulkPatch {
                account_ids: vec!["1".into(), "2".into()],
                patch: AccountPatch {
                    group: Some("Changed".into()),
                    auto_recovery: Some(true),
                    ..Default::default()
                },
            })
            .unwrap();
        engine.apply_profile(p.id).unwrap();
        let restored = engine.snapshot().accounts;
        assert_eq!(restored[1].group, "Second");
        assert!(!restored[1].auto_recovery);
        assert_eq!(restored[1].fallback_policy, FallbackPolicy::Stay);
        let mut bad = p.clone();
        bad.name = "Bad".into();
        bad.entries[1].account_id = "unknown".into();
        let mut valid = p.clone();
        valid.name = "Imported".into();
        assert!(engine.import_profiles(vec![valid.clone(), bad]).is_err());
        assert_eq!(engine.snapshot().profiles.len(), 1);
        assert_eq!(engine.import_profiles(vec![valid]).unwrap(), 1);
        let reopened = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
        assert_eq!(reopened.snapshot().profiles.len(), 2);
        let queued = reopened.launch_profile(p.id).unwrap();
        assert_eq!(queued.len(), 2);
        assert!(queued.iter().all(|r| r.operation.is_some()));
        reopened.command("2", "stop").unwrap();
        reopened.remove("2").unwrap();
        assert!(
            reopened
                .snapshot()
                .profiles
                .iter()
                .all(|p| p.entries.len() == 1)
        );
        drop(reopened);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn fallback_policies_only_respond_to_confirmed_unavailability() {
        for policy in [
            FallbackPolicy::Stay,
            FallbackPolicy::Pause,
            FallbackPolicy::AllowPublic,
        ] {
            let mut a = Account::new("1".into(), "test".into());
            a.fallback_policy = policy;
            apply_failure(
                &mut a,
                &Failure::new(FailureKind::TargetUnavailable, "Unavailable"),
            );
            assert_eq!(
                a.public_fallback_active,
                policy == FallbackPolicy::AllowPublic
            );
            assert_eq!(
                a.status,
                if policy == FallbackPolicy::Pause {
                    Status::NeedsAttention
                } else {
                    Status::Backoff
                }
            );
            for kind in [
                FailureKind::Auth,
                FailureKind::Permission,
                FailureKind::Unsupported,
                FailureKind::Network,
                FailureKind::RateLimit,
            ] {
                let mut other = Account::new("1".into(), "test".into());
                other.fallback_policy = policy;
                apply_failure(&mut other, &Failure::new(kind, "error"));
                assert!(!other.public_fallback_active);
            }
        }
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn retry_now_retains_failures_and_respects_rate_limits_and_attention() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.failure("1", generation, Failure::new(FailureKind::Other, "failure"));
        let before = engine.snapshot().accounts[0].next_retry.unwrap();
        let retry = engine.retry_now("1").unwrap();
        assert_eq!(engine.retry_now("1").unwrap().id, retry.id);
        assert_eq!(engine.snapshot().accounts[0].failures, 1);
        assert!(engine.snapshot().accounts[0].next_retry.unwrap() < before);
        engine.failure(
            "1",
            generation,
            Failure::new(FailureKind::RateLimit, "rate limited"),
        );
        assert!(engine.retry_now("1").is_err());
        engine.failure("1", generation, Failure::new(FailureKind::Auth, "expired"));
        assert!(engine.retry_now("1").is_err());
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn history_persists_deduplicates_and_support_reports_allowlist_fields() {
        let (engine, path) = fixture(1);
        engine
            .patch(
                "1",
                AccountPatch {
                    alias: Some("SECRET_ALIAS".into()),
                    target: Some(Target {
                        place_id: 1,
                        job_id: None,
                        private_server_link: Some(
                            "https://www.roblox.com/games/1?privateServerLinkCode=PRIVATE-SECRET"
                                .into(),
                        ),
                    }),
                    ..Default::default()
                },
            )
            .unwrap();
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.signal("1", generation, Signal::SessionLost(Some(267)));
        let count = engine.activity(None).len();
        engine.signal("1", generation, Signal::SessionLost(Some(267)));
        assert_eq!(engine.activity(None).len(), count);
        assert!(engine.activity(Some("1"))[0].message.contains("267"));
        engine.failure(
            "1",
            generation,
            Failure::new(FailureKind::Auth, "RAW_SECRET_ERROR"),
        );
        let report = engine.diagnostics().to_string();
        for secret in [
            "SECRET_ALIAS",
            "PRIVATE-SECRET",
            "RAW_SECRET_ERROR",
            "encrypted_session",
            engine.token(),
        ] {
            assert!(!report.contains(secret));
        }
        engine.command("1", "stop").unwrap();
        let history = engine.activity(None);
        let reopened = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
        assert_eq!(reopened.activity(None).len(), history.len());
        engine
            .transaction(|s| {
                s.database.activity = vec![history[0].clone(); 1100];
                Ok(())
            })
            .unwrap();
        assert_eq!(engine.activity(None).len(), 1000);
        drop(reopened);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn repair_rejects_wrong_or_removed_account_and_retains_settings() {
        let (engine, path) = fixture(2);
        engine
            .patch(
                "1",
                AccountPatch {
                    alias: Some("Keep alias".into()),
                    group: Some("Keep group".into()),
                    fallback_policy: Some(FallbackPolicy::Pause),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(
            engine
                .save_authenticated_session(
                    crate::roblox::User {
                        id: 2,
                        name: "wrong".into()
                    },
                    "NEW_SESSION",
                    Some("1")
                )
                .is_err()
        );
        let account = engine
            .save_authenticated_session(
                crate::roblox::User {
                    id: 1,
                    name: "account0".into(),
                },
                "NEW_SESSION",
                Some("1"),
            )
            .unwrap();
        assert_eq!(account.alias, "Keep alias");
        assert_eq!(account.group, "Keep group");
        assert_eq!(account.target.unwrap().place_id, 1);
        assert_eq!(account.fallback_policy, FallbackPolicy::Pause);
        let persisted = std::fs::read_to_string(path.join("accounts.json")).unwrap();
        assert!(!persisted.contains("NEW_SESSION"));
        engine.remove("1").unwrap();
        assert!(
            engine
                .save_authenticated_session(
                    crate::roblox::User {
                        id: 1,
                        name: "removed".into()
                    },
                    "NEW_SESSION",
                    Some("1")
                )
                .is_err()
        );
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn encrypted_backups_rotate_restore_stopped_and_recover_corrupt_database() {
        let (engine, path) = fixture(1);
        engine
            .transaction(|s| {
                s.database.accounts[0].encrypted_session =
                    platform::protect("TEST_SESSION_SECRET")?;
                Ok(())
            })
            .unwrap();
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.signal("1", generation, Signal::Connected);
        engine
            .update("1", generation, |a| {
                a.running_since = Some(Utc::now() - ChronoDuration::hours(2))
            })
            .unwrap();
        let backup = engine.backup().unwrap();
        let blob = std::fs::read_to_string(path.join("backups").join(&backup)).unwrap();
        assert!(!blob.contains("TEST_SESSION_SECRET"));
        assert!(!blob.contains("account0"));
        assert!(engine.restore(&backup).is_err());
        engine.command("1", "stop").unwrap();
        let token = engine.token().to_string();
        engine.restore(&backup).unwrap();
        let a = engine.snapshot().accounts.remove(0);
        assert_eq!(a.status, Status::Stopped);
        assert!(!a.desired_running);
        assert!(a.running_since.is_none());
        assert!(a.longest_streak_seconds >= 7200);
        assert!(a.process.is_none() && a.tracker.is_none());
        assert_eq!(engine.token(), token);
        assert!(engine.restore("../accounts.json").is_err());
        for _ in 0..12 {
            engine.backup().unwrap();
        }
        assert_eq!(engine.backups().unwrap().len(), 10);
        let latest = engine.backups().unwrap()[0].clone();
        drop(engine);
        std::fs::write(path.join("accounts.json"), "invalid").unwrap();
        let store = Store::new(path.clone()).unwrap();
        assert!(store.load().is_err());
        store.recover(&latest).unwrap();
        assert!(store.load().unwrap().encrypted_api_token.is_empty());
        assert!(
            std::fs::read_dir(&path)
                .unwrap()
                .filter_map(Result::ok)
                .any(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with("accounts-before-restore-"))
        );
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn account_run_persists_through_reopen_and_ends_on_manual_restart_or_stop() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.signal("1", generation, Signal::Connected);
        let start = Utc::now() - ChronoDuration::hours(2);
        engine
            .update("1", generation, |a| a.running_since = Some(start))
            .unwrap();
        engine.signal("1", generation, Signal::Disconnected);
        let reopened = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
        assert_eq!(reopened.snapshot().accounts[0].running_since, Some(start));
        drop(reopened);
        engine.command("1", "restart").unwrap();
        let account = engine.snapshot().accounts[0].clone();
        assert!(account.running_since.is_none());
        assert!(account.longest_streak_seconds >= 7200);
        engine.signal("1", account.generation, Signal::Connected);
        engine.command("1", "stop").unwrap();
        let reopened = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
        assert!(reopened.snapshot().accounts[0].running_since.is_none());
        assert!(reopened.snapshot().accounts[0].longest_streak_seconds >= 7200);
        engine.shutdown();
        reopened.shutdown();
        drop(engine);
        drop(reopened);
        std::fs::remove_dir_all(path).unwrap();
    }
    fn fixture(count: usize) -> (Engine, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("rbx-engine-test-{}", Uuid::new_v4()));
        let store = Store::new(path.clone()).unwrap();
        let engine = Engine::open(store, "simulated".into()).unwrap();
        engine
            .transaction(|s| {
                for i in 0..count {
                    let mut account = Account::new((i + 1).to_string(), format!("account{i}"));
                    account.target = Some(Target {
                        place_id: 1,
                        job_id: None,
                        private_server_link: None,
                    });
                    s.database.accounts.push(SavedAccount {
                        account,
                        encrypted_session: "encrypted-test-marker".into(),
                    });
                }
                Ok(())
            })
            .unwrap();
        (engine, path)
    }
    #[test]
    fn game_profiles_survive_restarts_validate_and_leave_account_targets_independent() {
        let (engine, path) = fixture(1);
        engine
            .transaction(|s| {
                s.database.accounts[0].encrypted_session = platform::protect("TEST_ONLY_SESSION")?;
                Ok(())
            })
            .unwrap();
        let mut profile = GameProfile {
            id: Uuid::new_v4(),
            name: "Private game".into(),
            game_name: "Game name".into(),
            thumbnail_url: Some("https://tr.rbxcdn.com/game.png".into()),
            target: Target {
                place_id: 1818,
                job_id: None,
                private_server_link: Some(
                    "https://www.roblox.com/games/1818?privateServerLinkCode=abc123".into(),
                ),
            },
        };
        engine.save_game_profile(profile.clone()).unwrap();
        let mut invalid = profile.clone();
        invalid.id = Uuid::new_v4();
        assert!(engine.save_game_profile(invalid.clone()).is_err());
        invalid.name = "Other".into();
        invalid.thumbnail_url = Some("https://example.com/game.png".into());
        assert!(engine.save_game_profile(invalid).is_err());
        profile.name = "Updated private game".into();
        engine.save_game_profile(profile.clone()).unwrap();
        engine
            .patch(
                "1",
                AccountPatch {
                    target: Some(profile.target.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        engine.set_close_to_tray(false).unwrap();
        drop(engine);
        let reopened = Engine::open(Store::new(path.clone()).unwrap(), "simulated".into()).unwrap();
        assert!(!reopened.snapshot().close_to_tray);
        assert_eq!(reopened.snapshot().game_profiles[0].name, profile.name);
        let backup = reopened.backup().unwrap();
        reopened.delete_game_profile(profile.id).unwrap();
        assert_eq!(reopened.snapshot().accounts[0].target, Some(profile.target));
        reopened.restore(&backup).unwrap();
        assert_eq!(reopened.snapshot().game_profiles.len(), 1);
        assert!(!reopened.snapshot().close_to_tray);
        drop(reopened);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn fifty_accounts_stop_start_idempotence_and_stale_signals() {
        let (engine, path) = fixture(50);
        for id in 1..=50 {
            let id = id.to_string();
            let first = engine.command(&id, "start").unwrap();
            let duplicate = engine.command(&id, "start").unwrap();
            assert_eq!(first.id, duplicate.id);
            let old = engine
                .snapshot()
                .accounts
                .into_iter()
                .find(|a| a.id == id)
                .unwrap()
                .generation;
            let stop = engine.command(&id, "stop").unwrap();
            assert_eq!(stop.id, engine.command(&id, "stop").unwrap().id);
            engine.signal(&id, old, Signal::Connected);
            engine.failure(&id, old, Failure::network());
            let current = engine
                .snapshot()
                .accounts
                .into_iter()
                .find(|a| a.id == id)
                .unwrap();
            assert_eq!(current.status, Status::Stopped);
            assert!(!current.desired_running);
            assert!(!engine.snapshot().network_suspended);
        }
        assert_eq!(engine.snapshot().accounts.len(), 50);
        assert!(
            !serde_json::to_string(&engine.snapshot())
                .unwrap()
                .contains("encrypted-test-marker")
        );
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn stop_cancels_backoff_and_retains_late_launch_ownership() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let a = engine.snapshot().accounts.remove(0);
        engine
            .update("1", a.generation, |a| {
                a.tracker = Some("123456789012".into());
                a.ownership_deadline = Some(Utc::now() + ChronoDuration::seconds(90));
            })
            .unwrap();
        engine.command("1", "stop").unwrap();
        assert!(engine.command("1", "start").is_err());
        let state = engine.0.state.lock().unwrap();
        assert_eq!(state.database.retired_launches.len(), 1);
        assert!(!state.database.accounts[0].account.desired_running);
        drop(state);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn save_failure_rolls_back_state() {
        let (engine, path) = fixture(1);
        std::fs::create_dir(path.join("accounts.json.tmp")).unwrap();
        assert!(engine.command("1", "start").is_err());
        assert_eq!(engine.snapshot().accounts[0].status, Status::Stopped);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn reconnect_clears_disconnect_without_relaunch() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.signal("1", generation, Signal::Disconnected);
        assert_eq!(engine.snapshot().accounts[0].status, Status::Reconnecting);
        engine.signal("1", generation, Signal::Connected);
        let a = engine.snapshot().accounts.remove(0);
        assert_eq!(a.status, Status::Running);
        assert!(a.disconnected_since.is_none());
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn five_failures_pause_and_network_does_not_count() {
        let mut a = Account::new("1".into(), "test".into());
        a.desired_running = true;
        apply_failure(&mut a, &Failure::network());
        assert_eq!(a.failures, 0);
        for _ in 0..4 {
            apply_failure(&mut a, &Failure::new(FailureKind::Other, "Failure"));
            assert_eq!(a.status, Status::Backoff);
        }
        apply_failure(&mut a, &Failure::new(FailureKind::Other, "Failure"));
        assert_eq!(a.status, Status::NeedsAttention);
    }
    #[test]
    fn only_explicit_unavailability_enables_public_fallback() {
        for kind in [
            FailureKind::Network,
            FailureKind::Auth,
            FailureKind::Permission,
            FailureKind::RateLimit,
        ] {
            let mut a = Account::new("1".into(), "test".into());
            a.auto_recovery = true;
            apply_failure(&mut a, &Failure::new(kind, "error"));
            assert!(!a.public_fallback_active);
        }
        let mut a = Account::new("1".into(), "test".into());
        apply_failure(
            &mut a,
            &Failure::new(FailureKind::TargetUnavailable, "unavailable"),
        );
        assert!(a.public_fallback_active);
    }
}
