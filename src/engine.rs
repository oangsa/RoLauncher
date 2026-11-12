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

mod close_safety;
#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod lifecycle_tests;
mod management;
use close_safety::{CloseAttempts, resume_verified_handoff, stop_ended_session};
mod server_selection;
pub use management::{BulkPatch, ProfileSave};

#[derive(Clone)]
struct State {
    active_multi_instance: bool,
    launch_allowed: bool,
    database: Database,
    operations: BTreeMap<Uuid, Operation>,
    network_suspended: bool,
    compatibility: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LaunchReadinessStage {
    ProtocolAccepted,
    BootstrapperStarted,
    ProcessObserved,
    OwnershipVerified,
    Connected,
    StartupUnresolved,
    StartupFailed,
}
#[derive(Clone)]
struct PendingLaunchReadiness {
    account_id: String,
    operation_id: Option<Uuid>,
    generation: Uuid,
    tracker: String,
    dispatched_at: Instant,
    stage: LaunchReadinessStage,
    stage_updated_at: Instant,
    observed_pid: Option<u32>,
    deferred_logged: bool,
}
struct LaunchCoordinator {
    next_allowed_at: Instant,
    last_dispatched: Option<PendingLaunchReadiness>,
    unverified_processes: usize,
    inaccessible_processes: usize,
    bootstrapper_active: bool,
    ambiguous_logged_for: Option<String>,
    ambiguous_first_seen: Option<Instant>,
    ambiguous_timeout_logged_for: Option<String>,
}
impl Default for LaunchCoordinator {
    fn default() -> Self {
        Self {
            next_allowed_at: Instant::now(),
            last_dispatched: None,
            unverified_processes: 0,
            inaccessible_processes: 0,
            bootstrapper_active: false,
            ambiguous_logged_for: None,
            ambiguous_first_seen: None,
            ambiguous_timeout_logged_for: None,
        }
    }
}
#[cfg(test)]
type LaunchHook = Arc<dyn Fn(&str, &str) -> Result<(), String> + Send + Sync>;
#[cfg(test)]
type PreDispatchHook = Arc<dyn Fn(&str, Uuid, &str) + Send + Sync>;
#[cfg(test)]
type ClockHook = Arc<dyn Fn() -> Instant + Send + Sync>;

/// Strict Partial Lock Hierarchy (Level 1 -> Level 2 -> Level 3 -> Level 4)
/// Locks must ALWAYS be acquired in top-to-bottom order to guarantee provable ordering and zero AB-BA deadlocks:
///
/// 1. `launch_slot`: Tokio Mutex (async, held only by `launch_one` to serialize launch attempts across accounts)
/// 2. `dispatch_lock`: std Mutex (synchronous, serializes dispatch authorization, OS `effect()` submission, gate disable, shutdown completion boundary, and `execute_command`)
/// 3. `state`: std Mutex (synchronous, held during state transactions, snapshot reads, and account updates)
/// 4. `launch_coordinator`: std Mutex (synchronous leaf lock, held only for brief coordinator updates; NEVER acquires `state` or `dispatch_lock`)
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
    journal: crate::diagnostics::Journal,
    launch_slot: tokio::sync::Mutex<()>,
    dispatch_lock: Mutex<()>,
    launch_coordinator: Mutex<LaunchCoordinator>,
    launch_spacing: Duration,
    launch_readiness_timeout: Duration,
    startup_verification_timeout: Duration,
    #[cfg(test)]
    launch_result: Option<Result<(), String>>,
    #[cfg(test)]
    launch_hook: Option<LaunchHook>,
    #[cfg(test)]
    pre_dispatch_hook: Option<PreDispatchHook>,
    #[cfg(test)]
    post_auth_hook: Option<PreDispatchHook>,
    #[cfg(test)]
    discovery_hook: Option<Arc<dyn Fn() -> Result<platform::ProcessScan, String> + Send + Sync>>,
    #[cfg(test)]
    clock: Option<ClockHook>,
}
#[derive(Clone)]
pub struct Engine(Arc<Inner>);
#[cfg(windows)]
type MemoryProbe = (
    ProcessIdentity,
    Option<platform::MemoryLogReader>,
    Option<(chrono::DateTime<Utc>, Signal)>,
);

struct PendingExit {
    observed: Instant,
    clean: bool,
}
impl PendingExit {
    fn settled(&self, account: &Account, now: Instant, utc: chrono::DateTime<Utc>) -> bool {
        let grace = if account.ownership_deadline.is_none() && account.close_intent.is_some() {
            1
        } else {
            5
        };
        now.saturating_duration_since(self.observed) >= Duration::from_secs(grace)
            && account
                .ownership_deadline
                .is_none_or(|deadline| utc >= deadline)
    }
}

const OWNERSHIP_TIMEOUT_ERROR: &str = "Launch tracker has no verified instance. Recovery paused; use Stop, then Start after checking the client";
const STARTUP_UNRESOLVED_ERROR: &str = "Roblox did not confirm client startup within the readiness window. Automatic launch is paused while watching for a late client; inspect Roblox or use Stop before retrying";
const STARTUP_IN_PROGRESS_WAIT_ERROR: &str = "Roblox startup is still in progress; waiting for verified client ownership before starting the next account";
const STARTUP_AMBIGUOUS_PROCESS_WAIT_ERROR: &str = "Waiting for an unverified Roblox process to finish startup inspection before launching the next account";
const STARTUP_AMBIGUOUS_TIMEOUT_ERROR: &str = "Roblox startup verification timed out while an unverified process or bootstrapper remained present. Automatic launch is paused; inspect Roblox or use Stop before retrying";
const CLOSE_BLOCKED_PREFIX: &str = "Managed client close blocked: ";
const HANDOFF_UNRESOLVED: &str = "Unable to verify the surviving Roblox client. Waiting for process ownership before closing or rejoining.";
const CLOSE_FAILED_ERROR: &str = "Managed client close blocked: verified process close failed; inspect the client and use Stop to retry";
const REJOIN_DISABLED: &str = "Automatic rejoin disabled; use Start to launch manually";
const SINGLE_INSTANCE_WAIT_ERROR: &str =
    "Multiple instances disabled; waiting for the other managed client to close";
const SINGLE_INSTANCE_RESTART_REQUIRED_ERROR: &str = "Multiple instances disabled until RoLauncher restarts; waiting for the other managed client to close";
#[cfg(windows)]
const LAUNCH_COORDINATION_ERROR: &str = "Roblox launch coordination is not ready. Enable background cleanup in Game joining and retry Start, or use Accounts → More actions → Stop all Roblox";
#[cfg(not(windows))]
const LAUNCH_COORDINATION_ERROR: &str = "Multi-instance initialization failed. Check Support and integrations, resolve the reported issue, then restart RoLauncher";

fn single_instance_wait_error(multi_instance_enabled: bool) -> &'static str {
    if multi_instance_enabled {
        SINGLE_INSTANCE_RESTART_REQUIRED_ERROR
    } else {
        SINGLE_INSTANCE_WAIT_ERROR
    }
}

fn single_instance_reason(multi_instance_enabled: bool) -> &'static str {
    if multi_instance_enabled {
        "multi_instance_restart_required"
    } else {
        "single_instance_wait"
    }
}

fn recovery_due(account: &Account, now: chrono::DateTime<Utc>) -> bool {
    account.desired_running
        && account.auto_recovery
        && account.status == Status::Reconnecting
        && account.recovery_evidence == RecoveryEvidence::ConfirmedTerminalDisconnect
        && account
            .disconnected_since
            .is_some_and(|t| now - t >= ChronoDuration::seconds(30))
}

#[derive(Clone, Copy)]
enum Observation {
    DiskLog(crate::logs::LogEvent),
    #[cfg(any(windows, test))]
    UnverifiedMemory(Signal),
}

fn apply_observation(account: &mut Account, observation: Observation) {
    if !account.desired_running || account.status == Status::NeedsAttention {
        return;
    }
    let (signal, confirmed) = match observation {
        #[cfg(any(windows, test))]
        Observation::UnverifiedMemory(signal) => {
            if signal != Signal::Connected
                && !matches!(
                    account.recovery_evidence,
                    RecoveryEvidence::ConfirmedTerminalDisconnect
                        | RecoveryEvidence::RecoveryStarted
                )
            {
                account.recovery_evidence = RecoveryEvidence::SuspectedDisconnect;
                account.last_error = Some("Unverified RAM disconnect candidate; waiting for owned disk-log or process-exit evidence".into());
            }
            return;
        }
        Observation::DiskLog(event) => (event.signal, event.confirmed_terminal),
    };
    let blocked = match signal {
        Signal::PermissionDenied | Signal::SessionLost(Some(524 | 533 | 600 | 773)) => {
            Some(Failure::new(
                FailureKind::Permission,
                "Game denied access; check permissions or bans before restarting",
            ))
        }
        Signal::ClientUpdateRequired => Some(Failure::new(
            FailureKind::Unsupported,
            "Roblox is out of date (280); update Roblox, then use Start",
        )),
        Signal::SessionLost(Some(268)) => Some(Failure::new(
            FailureKind::Unsupported,
            "Roblox rejected client behavior (268); check the client before restarting",
        )),
        _ => None,
    };
    if let Some(error) = blocked {
        account.recovery_evidence = RecoveryEvidence::Unavailable;
        apply_failure(account, &error);
        return;
    }
    if signal == Signal::Connected {
        let rate_limited = account.recovery_reason == "RateLimit"
            && account.next_retry.is_some_and(|time| time > Utc::now());
        if account
            .close_intent
            .as_ref()
            .is_some_and(|i| i.reason.automatic())
        {
            account.close_intent = None;
        }
        account.status = Status::Running;
        // An owned connection completes bootstrap. Its deadline must not delay a later close.
        account.ownership_deadline = None;
        account.connected_since.get_or_insert_with(Utc::now);
        account.disconnected_since = None;
        account.last_error = None;
        if !rate_limited {
            account.next_retry = None;
            account.recovery_reason.clear();
        }
        account.recovery_evidence = RecoveryEvidence::Unavailable;
        return;
    }
    if !confirmed {
        if !matches!(
            account.recovery_evidence,
            RecoveryEvidence::ConfirmedTerminalDisconnect | RecoveryEvidence::RecoveryStarted
        ) {
            account.recovery_evidence = RecoveryEvidence::SuspectedDisconnect;
            account.last_error = Some("Unconfirmed connection event; automatic replacement waits for independent evidence".into());
        }
        return;
    }
    if account.recovery_evidence == RecoveryEvidence::RecoveryStarted {
        return;
    }
    if signal == Signal::TargetUnavailable {
        apply_failure(
            account,
            &Failure::new(
                FailureKind::TargetUnavailable,
                "Configured destination unavailable; applying the account fallback policy",
            ),
        );
        account.recovery_evidence = if account.status == Status::Backoff {
            RecoveryEvidence::RecoveryStarted
        } else {
            RecoveryEvidence::Unavailable
        };
        return;
    }
    account.recovery_evidence = RecoveryEvidence::ConfirmedTerminalDisconnect;
    account.status = if account.auto_recovery {
        Status::Reconnecting
    } else {
        Status::Stopping
    };
    account.disconnected_since.get_or_insert_with(Utc::now);
    account.connected_since = None;
    if !(account.recovery_reason == "RateLimit"
        && account.next_retry.is_some_and(|time| time > Utc::now()))
    {
        account.recovery_reason = match signal {
            Signal::ConnectionFailed => "ConnectionFailed",
            Signal::SessionLost(_) => "SessionLost",
            _ => "ConfirmedTerminalDisconnect",
        }
        .into();
    }
    account.last_error = Some(match signal {
        _ if !account.auto_recovery => "Game disconnected; waiting for the session to settle before closing Roblox".into(),
        Signal::ConnectionFailed => {
            "Connection failed: server did not respond (279); allowing 30 seconds to reconnect before recovery".into()
        }
        Signal::SessionLost(Some(code)) => {
            format!("Session lost (code {code}); allowing 30 seconds to reconnect before recovery")
        }
        Signal::SessionLost(None) => "Session lost; allowing 30 seconds to reconnect before recovery".into(),
        _ => "Game connection lost; allowing 30 seconds to reconnect before recovery".into(),
    });
}

fn launch_ready(account: &Account, now: chrono::DateTime<Utc>) -> bool {
    account.desired_running
        && account.process.is_none()
        && account.tracker.is_none()
        && matches!(account.status, Status::Queued | Status::Backoff)
        && account.next_retry.is_none_or(|time| time <= now)
}

#[cfg(test)]
fn close_authorized(account: &Account) -> bool {
    !account.desired_running
        || account.status == Status::Queued
        || account.auto_recovery
            && account.recovery_process.is_some()
            && account.recovery_process == account.process
            && account.recovery_evidence == RecoveryEvidence::RecoveryStarted
}
fn log_unavailable(account: &mut Account) {
    // A transient read failure must not erase an already confirmed disconnect.
    if matches!(account.status, Status::Reconnecting | Status::Stopping)
        || account.status == Status::NeedsAttention
        || account.recovery_evidence == RecoveryEvidence::RecoveryStarted
    {
        return;
    }
    account.status = Status::Unknown;
    account.connected_since = None;
    account.last_error =
        Some("Log unavailable; retrying verified log discovery and read-only monitoring".into());
}
fn note_process_exit(a: &mut Account, identity: &ProcessIdentity, clean: bool) {
    if a.process.as_ref() != Some(identity) {
        return;
    }
    if !a.auto_recovery
        && a.desired_running
        && a.close_intent.is_none()
        && matches!(
            a.status,
            Status::Stopping | Status::Running | Status::Reconnecting
        )
        && (a.recovery_evidence == RecoveryEvidence::ConfirmedTerminalDisconnect
            || clean && a.connected_since.is_some())
    {
        stop_ended_session(a);
    } else if matches!(
        a.status,
        Status::Running | Status::Launching | Status::Unknown
    ) {
        a.status = Status::Unknown;
        // Reconciliation needs the previous connection evidence to
        // distinguish a clean game exit from a bootstrap handoff.
        a.last_error = Some(
            "Managed process exited; checking for a surviving Roblox client before rejoin".into(),
        );
    }
}

fn process_unavailable(account: &mut Account) {
    // Inaccessibility supplies no new evidence and cannot reset a terminal/attention decision.
    if account.status == Status::NeedsAttention
        || matches!(
            account.recovery_evidence,
            RecoveryEvidence::ConfirmedTerminalDisconnect | RecoveryEvidence::RecoveryStarted
        )
    {
        return;
    }
    account.status = Status::Unknown;
    account.last_error =
        Some("Process metadata inaccessible; automatic replacement is disabled".into());
}
async fn completed_task<T>(
    task: &mut Option<tokio::task::JoinHandle<T>>,
) -> Option<Result<T, tokio::task::JoinError>> {
    if task.as_ref().is_some_and(|task| task.is_finished()) {
        Some(task.take().unwrap().await)
    } else {
        None
    }
}

fn reserve_recovery(account: &mut Account) {
    let retry = account.next_retry;
    let rate_limited =
        account.recovery_reason == "RateLimit" && retry.is_some_and(|time| time > Utc::now());
    let message = account
        .last_error
        .as_deref()
        .unwrap_or("Confirmed disconnect; controlled recovery started")
        .to_owned();
    apply_failure(
        account,
        &Failure::new(
            if rate_limited {
                FailureKind::RateLimit
            } else {
                FailureKind::Other
            },
            &message,
        ),
    );
    if let (Some(previous), Some(next)) = (retry, account.next_retry) {
        account.next_retry = Some(previous.max(next));
    }
    account.recovery_evidence = RecoveryEvidence::RecoveryStarted;
}

fn normalize_accounts(next: &mut State) {
    let now = Utc::now();
    for saved in &mut next.database.accounts {
        let a = &mut saved.account;
        if a.status == Status::NeedsAttention
            && (a.close_intent.as_ref().is_some_and(|i| i.denied)
                || next.database.retired_launches.iter().any(|r| {
                    r.account_id == a.id && r.close_intent.as_ref().is_some_and(|i| i.denied)
                }))
            && let Some(op) = a.operation_id.and_then(|id| next.operations.get_mut(&id))
        {
            op.state = "failed".into();
            op.error = a.last_error.clone();
        }
        if !a.desired_running && matches!(a.status, Status::Stopped | Status::Stopping) {
            let pending = a.process.is_some()
                || next
                    .database
                    .retired_launches
                    .iter()
                    .any(|r| r.account_id == a.id);
            a.status = if pending {
                Status::Stopping
            } else {
                Status::Stopped
            };
            if let Some(op) = a.operation_id.and_then(|id| next.operations.get_mut(&id)) {
                op.state = if pending { "queued" } else { "completed" }.into();
            }
        }
        a.track_run(now);
        if a.status == Status::Reconnecting && a.desired_running {
            let known_error = a.last_error.as_deref().is_some_and(|error| {
                error.starts_with("Session lost (code") || error.starts_with("Connection failed:")
            });
            a.disconnect_notified = a.recovery_evidence
                == RecoveryEvidence::ConfirmedTerminalDisconnect
                && (a.disconnect_notified
                    || known_error
                    || a.disconnected_since
                        .is_some_and(|t| now - t >= ChronoDuration::seconds(30)));
        } else {
            a.disconnect_notified = false;
        }
    }
}

fn retired_blocks_launch(database: &Database, id: &str) -> bool {
    database.retired_launches.iter().any(|r| r.account_id == id)
}

fn resume_after_confirmed_close(account: &mut Account) {
    let failed_automatic_close = account.last_error.as_deref() == Some(CLOSE_FAILED_ERROR)
        && account
            .close_intent
            .as_ref()
            .is_some_and(|i| i.denied && i.reason == CloseReason::AutomaticRecovery);
    let close_error = account.last_error.as_deref().is_some_and(|error| {
        error.starts_with(CLOSE_BLOCKED_PREFIX)
            || error == "Unable to close managed instance (Windows error 5)"
    });
    if account.status == Status::NeedsAttention
        && account
            .close_intent
            .as_ref()
            .is_none_or(|i| !i.denied || failed_automatic_close)
        && close_error
        && account.process.is_none()
        && account.tracker.is_none()
        && account.desired_running
        && account.auto_recovery
        && account.failures < 5
    {
        // Exit retires this failed close; it never renews authority over a live client.
        if failed_automatic_close {
            account.close_intent = None;
        }
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
    account.recovery_evidence = RecoveryEvidence::Unavailable;
    account.recovery_process = None;
    // A late exact match resolves only the ownership-timeout or startup-unresolved pause, not auth or other failures.
    let ownership_resolved = matches!(
        account.last_error.as_deref(),
        Some(OWNERSHIP_TIMEOUT_ERROR | STARTUP_UNRESOLVED_ERROR)
    );
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
    pub game_profile_id: Option<Uuid>,
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
        let loaded_accounts: Vec<_> = database
            .accounts
            .iter()
            .map(|s| s.account.clone())
            .collect();
        let active_multi_instance = database.multi_instance_enabled;
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
            } else if !a.desired_running && a.status != Status::NeedsAttention {
                a.status = if a.process.is_some()
                    || database
                        .retired_launches
                        .iter()
                        .any(|r| r.account_id == a.id)
                {
                    Status::Stopping
                } else {
                    Status::Stopped
                };
            }
        }
        database.version = 2;
        store.save(&database)?;
        let journal = crate::diagnostics::Journal::new(&store.directory);
        journal.record(
            "engine_opened",
            serde_json::json!({
                "multi_instance": active_multi_instance,
                "launch_allowed": launch_allowed.load(Ordering::Acquire),
                "external_background_cleanup": database.external_background_cleanup_enabled,
                "retired_launches": database.retired_launches.len(),
            }),
        );
        for (before, saved) in loaded_accounts.iter().zip(&database.accounts) {
            journal.record(
                "account_restored",
                serde_json::json!({
                    "before": crate::diagnostics::account(before),
                    "after": crate::diagnostics::account(&saved.account),
                }),
            );
        }
        let (events, _) = broadcast::channel(128);
        let discord = crate::discord::Discord::new(database.discord.clone())?;
        let bot_running = database.discord_bot_running && database.discord.bot.enabled;
        if bot_running {
            discord.bot_status("starting", "Connecting to Discord…");
        }
        let initial_launch_allowed = launch_allowed.load(Ordering::Acquire);
        Ok(Self(Arc::new(Inner {
            launch_slot: tokio::sync::Mutex::new(()),
            dispatch_lock: Mutex::new(()),
            launch_coordinator: Mutex::new(LaunchCoordinator::default()),
            launch_spacing: Duration::from_secs(5),
            launch_readiness_timeout: Duration::from_secs(8),
            startup_verification_timeout: Duration::from_secs(35),
            #[cfg(test)]
            launch_result: None,
            #[cfg(test)]
            launch_hook: None,
            #[cfg(test)]
            pre_dispatch_hook: None,
            #[cfg(test)]
            post_auth_hook: None,
            #[cfg(test)]
            discovery_hook: None,
            #[cfg(test)]
            clock: None,
            state: Mutex::new(State {
                active_multi_instance,
                launch_allowed: initial_launch_allowed,
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
            journal,
        })))
    }
    pub fn token(&self) -> &str {
        &self.0.token
    }
    pub fn data_directory(&self) -> &std::path::Path {
        &self.0.store.directory
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut s = self.0.state.lock().unwrap();
        s.launch_allowed = self.0.launch_allowed.load(Ordering::Acquire);
        snapshot(&s)
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Snapshot> {
        self.0.events.subscribe()
    }
    pub fn operation(&self, id: Uuid) -> Option<Operation> {
        self.0.state.lock().unwrap().operations.get(&id).cloned()
    }
    pub fn shutdown(&self) {
        self.0
            .journal
            .record("shutdown_requested", serde_json::json!({}));
        self.0.shutdown.store(true, Ordering::Release);
        self.0.discord.stop();
        self.0.wake.notify_waiters();
        let _dispatch = self.0.dispatch_lock.lock().unwrap();
        self.0
            .journal
            .record("shutdown_completed", serde_json::json!({}));
    }
    pub fn is_shutdown(&self) -> bool {
        self.0.shutdown.load(Ordering::Acquire)
    }
    pub fn set_launch_allowed(&self, allowed: bool) {
        let _dispatch = self.0.dispatch_lock.lock().unwrap();
        let mut state = self.0.state.lock().unwrap();
        state.launch_allowed = allowed;
        if self.0.launch_allowed.swap(allowed, Ordering::AcqRel) != allowed {
            self.0.journal.record(
                "launch_coordination_changed",
                serde_json::json!({"allowed": allowed}),
            );
            self.0.wake.notify_waiters();
        }
    }
    pub fn set_multi_instance(&self, enabled: bool) -> Result<(), String> {
        self.transaction(|s| {
            s.database.multi_instance_enabled = enabled;
            Ok(())
        })
    }
    pub fn set_external_background_cleanup(&self, enabled: bool) -> Result<(), String> {
        self.transaction(|s| {
            s.database.external_background_cleanup_enabled = enabled;
            Ok(())
        })
    }
    fn transaction<T>(
        &self,
        edit: impl FnOnce(&mut State) -> Result<T, String>,
    ) -> Result<T, String> {
        self.transaction_inner(edit, false, |_| true)
    }
    fn transaction_inner<T>(
        &self,
        edit: impl FnOnce(&mut State) -> Result<T, String>,
        reset_discord: bool,
        should_commit: impl FnOnce(&T) -> bool,
    ) -> Result<T, String> {
        let mut current = self.0.state.lock().unwrap();
        let mut next = current.clone();
        next.launch_allowed = self.0.launch_allowed.load(Ordering::Acquire);
        let result = edit(&mut next)?;
        if !should_commit(&result) {
            return Ok(result);
        }
        next.launch_allowed = self.0.launch_allowed.load(Ordering::Acquire);
        normalize_accounts(&mut next);
        management::record_activity(&current, &mut next);
        if let Err(error) = self.0.store.save(&next.database) {
            self.0.journal.record(
                "state_save_failed",
                serde_json::json!({"effect": "transaction_rolled_back"}),
            );
            return Err(error);
        }
        self.journal_changes(&current, &next);
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
    fn journal_changes(&self, old: &State, next: &State) {
        if old.network_suspended != next.network_suspended {
            self.0.journal.record(
                "network_suspension_changed",
                serde_json::json!({"suspended": next.network_suspended}),
            );
        }
        for saved in &next.database.accounts {
            let before = old
                .database
                .accounts
                .iter()
                .find(|s| s.account.id == saved.account.id);
            let after = crate::diagnostics::account(&saved.account);
            let before_value = before.map(|s| crate::diagnostics::account(&s.account));
            if before_value.as_ref() != Some(&after)
                || before.is_some_and(|s| s.account.last_error != saved.account.last_error)
            {
                self.0.journal.record(
                    "account_state_changed",
                    serde_json::json!({"before": before_value, "after": after}),
                );
            }
        }
        // Retired trackers block replacement even when the current account has no PID.
        let retired = |state: &State| {
            state.database.retired_launches.iter().map(|r| serde_json::json!({
            "account_id": r.account_id.parse::<u64>().ok(), "generation": r.generation,
            "tracker": r.tracker.parse::<u64>().ok(), "deadline": r.deadline,
            "process": r.process.as_ref().map(crate::diagnostics::process), "close_intent": r.close_intent,
        })).collect::<Vec<_>>()
        };
        let before = retired(old);
        let after = retired(next);
        if before != after {
            self.0.journal.record(
                "retired_launches_changed",
                serde_json::json!({"before": before, "after": after}),
            );
        }
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
            |_| true,
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
    pub async fn import_batch(&self, values: &mut [String]) -> Vec<Result<Account, String>> {
        use futures_util::StreamExt;
        use zeroize::Zeroize;
        let cookies: Vec<_> = values
            .iter_mut()
            .map(|value| {
                let cookie = normalize_cookie(value);
                value.zeroize();
                cookie
            })
            .collect();
        let mut sessions = futures_util::stream::iter(cookies)
            .map(|cookie| async move {
                let cookie = cookie?;
                let user = self.0.roblox.user(&cookie).await.map_err(|e| e.message)?;
                Ok((user, cookie))
            })
            .buffered(4);
        let mut results = Vec::new();
        // Validate up to four sessions concurrently, but save in submitted order.
        while let Some(session) = sessions.next().await {
            results.push(
                session.and_then(|(user, cookie)| {
                    self.save_authenticated_session(user, &cookie, None)
                }),
            );
        }
        results
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
            management::validate_game_assignment(&s.database, &patch)?;
            let a = account_mut(s, id)?;
            management::apply_patch(a, patch);
            Ok(a.clone())
        })
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        self.transaction(|s| {
            let a = account_mut(s, id)?;
            if a.desired_running
                || a.process.is_some()
                || a.tracker.is_some()
                || s.database
                    .retired_launches
                    .iter()
                    .any(|r| r.account_id == id)
            {
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
        let result = self.execute_command(id, action, bot);
        self.journal_command(
            id,
            action,
            if bot.is_some() {
                "discord"
            } else {
                "api_or_desktop"
            },
            &result,
        );
        result
    }
    fn journal_command(
        &self,
        id: &str,
        action: &str,
        source: &'static str,
        result: &Result<Operation, String>,
    ) {
        let operation_id = result.as_ref().ok().map(|op| op.id);
        let reason = result.as_ref().err().map(|e| crate::diagnostics::reason(e));
        self.0.journal.record("command_result", serde_json::json!({
            "account_id": id.parse::<u64>().ok(), "action": match action { "start" | "stop" | "restart" | "retry" => action, _ => "unknown" },
            "source": source, "accepted": result.is_ok(), "operation_id": operation_id,
            "reason": reason,
        }));
        if matches!(action, "start" | "restart" | "retry") {
            self.0.journal.record(
                "launch_requested",
                serde_json::json!({
                    "account_id": id.parse::<u64>().ok(),
                    "action": action,
                    "source": source,
                    "accepted": result.is_ok(),
                    "operation_id": operation_id,
                    "stage": "requested",
                    "reason": reason,
                }),
            );
            if result.is_ok()
                && let Some(account) = self.snapshot().accounts.into_iter().find(|a| a.id == id)
            {
                self.0.journal.record(
                    "launch_queued",
                    serde_json::json!({
                        "account": crate::diagnostics::account(&account),
                        "operation_id": account.operation_id,
                        "generation": account.generation,
                        "stage": "queued",
                    }),
                );
            }
        }
    }
    fn execute_command(
        &self,
        id: &str,
        action: &str,
        bot: Option<&crate::discord_bot::SavedBot>,
    ) -> Result<Operation, String> {
        if !matches!(action, "start" | "stop" | "restart") {
            return Err("Unknown command".into());
        }
        let _dispatch = self.0.dispatch_lock.lock().unwrap();
        self.transaction(|s| {
            if bot.is_some_and(|b| { let mut saved = s.database.discord.bot.clone(); saved.registration = None; !b.enabled || *b != saved || self.bot_generation().is_multiple_of(2) }) || self.is_shutdown() { return Err("Command connection changed".into()); }
            let existing = account_mut(s,id)?.clone();
            if (action == "start" && existing.desired_running && existing.status != Status::NeedsAttention) || (action == "stop" && !existing.desired_running && existing.close_intent.as_ref().is_some_and(|i| i.reason == CloseReason::UserStop && !i.denied && !i.id.is_nil()) && !s.database.retired_launches.iter().any(|r| r.account_id == id && r.process.is_some() && r.close_intent != existing.close_intent)) {
                if let Some(op) = existing.operation_id.and_then(|id|s.operations.get(&id)).cloned() { return Ok(op); }
                let pending = if action == "stop" { existing.process.is_some() || existing.tracker.is_some() || retired_blocks_launch(&s.database, id) } else { existing.status != Status::Running };
                let op = Operation { id:Uuid::new_v4(), account_id:id.into(), state:if pending { "queued" } else { "completed" }.into(), error:None };
                let a = account_mut(s, id)?;
                a.operation_id = Some(op.id);
                if let Some(intent) = &mut a.close_intent { intent.operation_id = Some(op.id); }
                let intent = a.close_intent.clone();
                for old in s.database.retired_launches.iter_mut().filter(|r| r.account_id == id && r.close_intent == existing.close_intent) { old.close_intent = intent.clone(); }
                s.operations.insert(op.id,op.clone()); return Ok(op);
            }
            if action != "stop" { existing.target.as_ref().ok_or("Set a launch target first")?.validate()?; }
            if action != "stop" && existing.process.is_none() && existing.tracker.is_some() { return Err("Previous launch is unresolved; use Stop and wait for process discovery before starting again".into()); }
            if action != "stop" && retired_blocks_launch(&s.database, id) { return Err("A cancelled launch is still being observed; inspect late clients and use Stop, or wait for discovery to confirm exit".into()); }
            if action == "restart" && matches!(existing.status,Status::Launching|Status::Queued) { return Err("A launch is already pending; Stop it before restarting".into()); }
            #[cfg(windows)]
            if action != "stop" {
                self.cleanup_launch_conflict(s, |protected| {
                    platform::close_startup_background_players(protected, true, true)?;
                    let deadline = Instant::now() + Duration::from_secs(1);
                    while !self.0.launch_allowed.load(Ordering::Acquire) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Ok(())
                })?;
            }
            if action != "stop" && !self.0.launch_allowed.load(Ordering::Acquire) {
                self.0.journal.record(
                    "launch_coordination_unavailable",
                    serde_json::json!({
                        "account": crate::diagnostics::account(&existing),
                        "operation_id": existing.operation_id,
                        "generation": existing.generation,
                        "stage": "requested",
                        "reason": "launch_coordination_unavailable",
                    }),
                );
                return Err(LAUNCH_COORDINATION_ERROR.into());
            }
            if existing.desired_running
                && existing.process.is_none()
                && matches!(existing.status, Status::Queued | Status::Launching | Status::Backoff)
            {
                self.0.journal.record(
                    "launch_cancelled",
                    serde_json::json!({
                        "account": crate::diagnostics::account(&existing),
                        "operation_id": existing.operation_id,
                        "generation": existing.generation,
                        "stage": "cancelled",
                        "reason": if action == "stop" { "user_stop" } else { "manual_restart" },
                    }),
                );
            }
            if let Some(previous) = existing.operation_id.and_then(|id|s.operations.get_mut(&id)) && previous.state == "queued" { previous.state = "cancelled".into(); }
            let op = Operation { id:Uuid::new_v4(),account_id:id.into(),state:if action=="stop" && existing.process.is_none() && existing.tracker.is_none() && !retired_blocks_launch(&s.database, id){"completed"}else{"queued"}.into(),error:None };
            let generation = Uuid::new_v4();
            let close_intent = CloseIntent { id: Uuid::new_v4(), reason: match action {
                "stop" => CloseReason::UserStop,
                "restart" => CloseReason::ManualRestart,
                _ => CloseReason::ManualStartReplacement,
            }, generation, operation_id: Some(op.id), denied: false };
            if existing.process.is_none() && let Some(tracker)=existing.tracker {
                s.database.retired_launches.push(RetiredLaunch {tracker,generation:existing.generation,account_id:id.into(),deadline:existing.ownership_deadline.unwrap_or_else(||Utc::now()+ChronoDuration::seconds(90)), close_intent:Some(CloseIntent {reason: if action == "stop" {CloseReason::UserStop} else {CloseReason::ObsoleteLaunch}, ..close_intent.clone()}), process:None});
            }
            if action == "stop" {
                for old in s.database.retired_launches.iter_mut().filter(|r| r.account_id == id) {
                    if old.process.is_some() || old.close_intent.as_ref().is_some_and(|i| i.reason == CloseReason::UserStop && !i.denied) {
                        old.close_intent = Some(close_intent.clone());
                    }
                }
            }
            let single_instance_deferred = action != "stop"
                && !s.active_multi_instance
                && s.database.accounts.iter().any(|other| {
                    other.account.id != id
                        && (other.account.process.is_some()
                            || other.account.tracker.is_some()
                            || (other.account.desired_running
                                && matches!(
                                    other.account.status,
                                    Status::Queued
                                        | Status::Launching
                                        | Status::Running
                                        | Status::Unknown
                                        | Status::Backoff
                                )))
                });
            let multi_instance_enabled = s.database.multi_instance_enabled;
            let a = account_mut(s,id)?; a.finish_run(Utc::now()); a.desired_running = action != "stop";
            if a.process.is_none() { a.tracker=None;a.ownership_deadline=None; }
            a.generation = generation; a.close_intent = if action == "stop" || a.process.is_some() { Some(close_intent) } else { None }; a.failures = 0;
            a.next_retry = if single_instance_deferred {
                Some(Utc::now() + ChronoDuration::seconds(5))
            } else {
                None
            };
            a.connected_since = None; a.disconnected_since = None;
            a.last_error = if single_instance_deferred {
                Some(single_instance_wait_error(multi_instance_enabled).into())
            } else {
                None
            };
            a.recovery_evidence = RecoveryEvidence::Unavailable; a.recovery_process = None;
            a.status = if action == "stop" {
                if op.state == "completed" {
                    Status::Stopped
                } else {
                    Status::Stopping
                }
            } else {
                Status::Queued
            };
            a.public_fallback_active = false; a.recovery_reason.clear(); a.operation_id = Some(op.id);
            if single_instance_deferred {
                self.0.journal.record(
                    "launch_coordination_unavailable",
                    serde_json::json!({
                        "account": crate::diagnostics::account(a),
                        "operation_id": a.operation_id,
                        "generation": a.generation,
                        "stage": "queued",
                        "reason": single_instance_reason(multi_instance_enabled),
                    }),
                );
            }
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
            if account_mut(s, id)?
                .close_intent
                .as_ref()
                .is_some_and(|i| i.denied)
            {
                return Ok(());
            }
            self.0.journal.record("failure_observed", serde_json::json!({
                "account_id": id.parse::<u64>().ok(), "generation": generation,
                "kind": format!("{:?}", error.kind), "reason": crate::diagnostics::reason(&error.message),
                "error_code": crate::diagnostics::error_code(&error.message),
            }));
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
        let mut pending: HashMap<String, (Uuid, tokio::task::JoinHandle<()>)> = HashMap::new();
        let mut dispatch_order: HashMap<String, u64> = HashMap::new();
        let mut next_dispatch_seq = 0u64;
        let mut network_probe_at = Instant::now();
        let mut previous_gate = None;
        while !self.is_shutdown() {
            let allowed = self.0.launch_allowed.load(Ordering::Acquire);
            if previous_gate != Some(allowed) {
                self.0.journal.record(
                    "launch_gate_observed",
                    serde_json::json!({"allowed": allowed}),
                );
                previous_gate = Some(allowed);
            }
            let snapshot = self.snapshot();
            let finished: Vec<_> = pending
                .iter()
                .filter(|(_, (_, task))| task.is_finished())
                .map(|(id, _)| id.clone())
                .collect();
            for id in finished {
                let (generation, task) = pending.remove(&id).unwrap();
                if let Err(error) = task.await
                    && !error.is_cancelled()
                {
                    self.failure(
                        &id,
                        generation,
                        Failure::new(FailureKind::Other, "Launch preparation worker failed"),
                    );
                }
            }
            for (id, (generation, task)) in &pending {
                if !snapshot
                    .accounts
                    .iter()
                    .any(|a| a.id == *id && a.generation == *generation && a.desired_running)
                {
                    task.abort();
                }
            }
            if snapshot.network_suspended {
                if pending.is_empty() && Instant::now() >= network_probe_at {
                    network_probe_at = Instant::now() + Duration::from_secs(30);
                    let reachable = tokio::select! { ready = self.0.roblox.reachable() => ready, _=self.0.wake.notified() => false };
                    if reachable {
                        let _ = self.transaction(|s| {
                            s.network_suspended = false;
                            Ok(())
                        });
                    }
                }
            } else if allowed {
                let max_concurrent = if snapshot.active_multi_instance { 4 } else { 1 };
                let mut indices: Vec<usize> = (0..snapshot.accounts.len()).collect();
                indices.sort_by_key(|&idx| {
                    let a = &snapshot.accounts[idx];
                    (
                        a.failures > 0,
                        dispatch_order.get(&a.id).copied().unwrap_or(0),
                        idx,
                    )
                });
                for idx in indices {
                    if pending.len() >= max_concurrent {
                        break;
                    }
                    let account = &snapshot.accounts[idx];
                    if pending.contains_key(&account.id) || !launch_ready(account, Utc::now()) {
                        continue;
                    }
                    if !snapshot.active_multi_instance
                        && snapshot.accounts.iter().any(|other| {
                            other.id != account.id
                                && (other.process.is_some()
                                    || other.tracker.is_some()
                                    || other.status == Status::Launching
                                    || pending.contains_key(&other.id))
                        })
                    {
                        let wait_msg = single_instance_wait_error(snapshot.multi_instance_enabled);
                        let _ = self.update(&account.id, account.generation, |a| {
                            a.status = Status::Queued;
                            a.next_retry = Some(Utc::now() + ChronoDuration::seconds(5));
                            a.last_error = Some(wait_msg.into());
                        });
                        continue;
                    }
                    let blocked = {
                        let state = self.0.state.lock().unwrap();
                        retired_blocks_launch(&state.database, &account.id)
                            || state.database.join_type == JoinType::Friend
                                && snapshot.accounts.iter().any(|peer| {
                                    pending.contains_key(&peer.id)
                                        && server_selection::same_preset(account, peer)
                                })
                    };
                    if blocked {
                        continue;
                    }
                    next_dispatch_seq += 1;
                    dispatch_order.insert(account.id.clone(), next_dispatch_seq);
                    let engine = self.clone();
                    let account = account.clone();
                    let id = account.id.clone();
                    let generation = account.generation;
                    pending.insert(
                        id,
                        (
                            generation,
                            tokio::spawn(async move {
                                engine.launch_one(account).await;
                            }),
                        ),
                    );
                }
            }
            tokio::select! { _=tokio::time::sleep(Duration::from_millis(100))=>{}, _=self.0.wake.notified()=>{} }
        }
        for (_, (_, task)) in pending {
            task.abort();
            let _ = task.await;
        }
    }
    fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(clock) = &self.0.clock {
            return clock();
        }
        Instant::now()
    }
    pub(super) fn note_launch_stage(
        &self,
        account_id: &str,
        generation: Uuid,
        stage: LaunchReadinessStage,
        observed_pid: Option<u32>,
    ) {
        let now = self.now();
        let mut coord = self.0.launch_coordinator.lock().unwrap();
        if let Some(prev) = &mut coord.last_dispatched
            && prev.account_id == account_id
            && prev.generation == generation
        {
            prev.stage = stage;
            prev.stage_updated_at = now;
            if observed_pid.is_some() {
                prev.observed_pid = observed_pid;
            }
        }
    }
    fn mark_startup_unresolved(
        &self,
        prev: &PendingLaunchReadiness,
        reason: &str,
        wait_ms: u128,
        next_account_id: Option<&str>,
    ) {
        let _ = self.transaction(|state| {
            let Ok(current) = account_mut(state, &prev.account_id) else {
                return Ok(());
            };
            if current.generation != prev.generation
                || !current.desired_running
                || current.process.is_some()
                || current.tracker.as_deref() != Some(prev.tracker.as_str())
                || current.status == Status::NeedsAttention
            {
                return Ok(());
            }
            current.status = Status::NeedsAttention;
            current.last_error = Some(STARTUP_UNRESOLVED_ERROR.into());
            let operation = current.operation_id;
            if let Some(op) = operation.and_then(|id| state.operations.get_mut(&id))
                && op.state == "queued"
            {
                op.state = "failed".into();
                op.error = Some(STARTUP_UNRESOLVED_ERROR.into());
            }
            self.0.journal.record(
                "launch_readiness_timeout",
                serde_json::json!({
                    "account_id": prev.account_id.parse::<u64>().ok(),
                    "operation_id": prev.operation_id,
                    "generation": prev.generation,
                    "tracker": prev.tracker.parse::<u64>().ok(),
                    "stage": LaunchReadinessStage::StartupUnresolved,
                    "wait_ms": wait_ms,
                    "next_account_id": next_account_id.and_then(|id| id.parse::<u64>().ok()),
                    "reason": reason,
                }),
            );
            Ok(())
        });
        self.0.wake.notify_waiters();
    }
    fn previous_launch_resolved(&self, prev: &PendingLaunchReadiness) -> bool {
        let state = self.0.state.lock().unwrap();
        let Some(saved) = state
            .database
            .accounts
            .iter()
            .find(|s| s.account.id == prev.account_id)
        else {
            return true;
        };
        let a = &saved.account;
        a.generation != prev.generation
            || !a.desired_running
            || a.tracker.as_deref() != Some(prev.tracker.as_str())
            || a.process.is_some()
            || matches!(
                a.status,
                Status::Running
                    | Status::Stopped
                    | Status::Stopping
                    | Status::NeedsAttention
                    | Status::Backoff
            )
    }
    pub(super) fn poll_launch_readiness(&self, waiting_account: &Account, now: Instant) -> bool {
        let mut mark_unresolved = None;
        let mut notify_in_progress = None;
        let prev_candidate = {
            self.0
                .launch_coordinator
                .lock()
                .unwrap()
                .last_dispatched
                .clone()
        };
        let is_resolved = if let Some(prev) = &prev_candidate {
            if prev.account_id == waiting_account.id {
                true
            } else {
                self.previous_launch_resolved(prev)
            }
        } else {
            false
        };
        {
            let mut coord = self.0.launch_coordinator.lock().unwrap();
            if let Some(prev) = coord.last_dispatched.clone() {
                if (is_resolved
                    && prev_candidate.as_ref().is_some_and(|c| {
                        c.account_id == prev.account_id && c.generation == prev.generation
                    }))
                    || prev.account_id == waiting_account.id
                {
                    coord.last_dispatched = None;
                } else {
                    let elapsed = now.saturating_duration_since(prev.dispatched_at);
                    let startup_active = matches!(
                        prev.stage,
                        LaunchReadinessStage::BootstrapperStarted
                            | LaunchReadinessStage::ProcessObserved
                    ) || coord.bootstrapper_active
                        || coord.unverified_processes > 0
                        || coord.inaccessible_processes > 0;
                    if startup_active {
                        if elapsed >= self.0.launch_readiness_timeout && !prev.deferred_logged {
                            if let Some(active) = &mut coord.last_dispatched {
                                active.deferred_logged = true;
                            }
                            self.0.journal.record(
                                "launch_readiness_deferred",
                                serde_json::json!({
                                    "account_id": prev.account_id.parse::<u64>().ok(),
                                    "operation_id": prev.operation_id,
                                    "generation": prev.generation,
                                    "tracker": prev.tracker.parse::<u64>().ok(),
                                    "stage": prev.stage,
                                    "wait_ms": elapsed.as_millis(),
                                    "next_account_id": waiting_account.id.parse::<u64>().ok(),
                                    "reason": "startup_process_active_waiting_for_ownership",
                                }),
                            );
                            notify_in_progress = Some((prev.account_id.clone(), prev.generation));
                        }
                        if elapsed >= self.0.startup_verification_timeout {
                            coord.last_dispatched = None;
                            mark_unresolved = Some((
                                prev,
                                "startup_verification_timeout_exceeded",
                                elapsed.as_millis(),
                            ));
                        }
                    } else if elapsed >= self.0.launch_readiness_timeout {
                        coord.last_dispatched = None;
                        mark_unresolved = Some((
                            prev,
                            "previous_launch_process_unverified_within_readiness_window",
                            elapsed.as_millis(),
                        ));
                    }
                }
            }
        }
        if let Some((prev_id, prev_gen)) = notify_in_progress {
            let _ = self.update(&prev_id, prev_gen, |a| {
                if a.process.is_none() && a.status == Status::Launching {
                    a.last_error = Some(STARTUP_IN_PROGRESS_WAIT_ERROR.into());
                }
            });
            let _ = self.update(&waiting_account.id, waiting_account.generation, |a| {
                if a.process.is_none() && a.tracker.is_none() {
                    a.last_error = Some(STARTUP_IN_PROGRESS_WAIT_ERROR.into());
                }
            });
        }
        if let Some((prev, reason, wait_ms)) = mark_unresolved {
            self.mark_startup_unresolved(&prev, reason, wait_ms, Some(&waiting_account.id));
        }
        let mut notify_ambiguous = false;
        let mut notify_ambiguous_timeout = false;
        let ready = {
            let mut coord = self.0.launch_coordinator.lock().unwrap();
            let has_unverified = coord.unverified_processes > 0
                || coord.inaccessible_processes > 0
                || coord.bootstrapper_active;
            if coord.last_dispatched.is_some() {
                coord.ambiguous_first_seen = None;
                coord.ambiguous_timeout_logged_for = None;
                false
            } else if has_unverified {
                let first_seen = *coord.ambiguous_first_seen.get_or_insert(now);
                let ambiguous_elapsed = now.saturating_duration_since(first_seen);
                if ambiguous_elapsed >= self.0.startup_verification_timeout {
                    let reason = if coord.bootstrapper_active {
                        "bootstrapper_active_timeout_exceeded"
                    } else if coord.inaccessible_processes > 0 {
                        "inaccessible_process_timeout_exceeded"
                    } else {
                        "unverified_roblox_process_timeout_exceeded"
                    };
                    if coord.ambiguous_timeout_logged_for.as_deref()
                        != Some(waiting_account.id.as_str())
                    {
                        coord.ambiguous_timeout_logged_for = Some(waiting_account.id.clone());
                        self.0.journal.record(
                            "launch_ambiguous_process_timeout",
                            serde_json::json!({
                                "account_id": waiting_account.id.parse::<u64>().ok(),
                                "operation_id": waiting_account.operation_id,
                                "generation": waiting_account.generation,
                                "unverified_processes": coord.unverified_processes,
                                "inaccessible_processes": coord.inaccessible_processes,
                                "bootstrapper_active": coord.bootstrapper_active,
                                "wait_ms": ambiguous_elapsed.as_millis(),
                                "reason": reason,
                            }),
                        );
                        notify_ambiguous_timeout = true;
                    }
                    false
                } else {
                    if coord.ambiguous_logged_for.as_deref() != Some(waiting_account.id.as_str()) {
                        coord.ambiguous_logged_for = Some(waiting_account.id.clone());
                        self.0.journal.record(
                            "launch_blocked_ambiguous_handoff",
                            serde_json::json!({
                                "account_id": waiting_account.id.parse::<u64>().ok(),
                                "operation_id": waiting_account.operation_id,
                                "generation": waiting_account.generation,
                                "stage": "blocked_unverified_process",
                                "unverified_processes": coord.unverified_processes,
                                "inaccessible_processes": coord.inaccessible_processes,
                                "bootstrapper_active": coord.bootstrapper_active,
                                "reason": "unverified_roblox_process_present",
                            }),
                        );
                        notify_ambiguous = true;
                    }
                    false
                }
            } else {
                coord.ambiguous_logged_for = None;
                coord.ambiguous_first_seen = None;
                coord.ambiguous_timeout_logged_for = None;
                now >= coord.next_allowed_at
            }
        };
        if notify_ambiguous_timeout {
            let _ = self.transaction(|state| {
                let Ok(a) = account_mut(state, &waiting_account.id) else {
                    return Ok(());
                };
                if a.generation != waiting_account.generation
                    || !a.desired_running
                    || a.process.is_some()
                    || a.tracker.is_some()
                    || a.status == Status::NeedsAttention
                {
                    return Ok(());
                }
                a.status = Status::NeedsAttention;
                a.last_error = Some(STARTUP_AMBIGUOUS_TIMEOUT_ERROR.into());
                if let Some(op) = a.operation_id.and_then(|id| state.operations.get_mut(&id))
                    && op.state == "queued"
                {
                    op.state = "failed".into();
                    op.error = Some(STARTUP_AMBIGUOUS_TIMEOUT_ERROR.into());
                }
                Ok(())
            });
        }
        if notify_ambiguous {
            let _ = self.update(&waiting_account.id, waiting_account.generation, |a| {
                if a.process.is_none() && a.tracker.is_none() {
                    a.last_error = Some(STARTUP_AMBIGUOUS_PROCESS_WAIT_ERROR.into());
                }
            });
        }
        ready
    }
    fn launch_cancelled_or_superseded(
        &self,
        expected: &Account,
        stage: &'static str,
        wait_ms: u128,
    ) -> bool {
        let state = self.0.state.lock().unwrap();
        if retired_blocks_launch(&state.database, &expected.id) {
            self.0.journal.record(
                "launch_cancelled",
                serde_json::json!({
                    "account": crate::diagnostics::account(expected),
                    "operation_id": expected.operation_id,
                    "generation": expected.generation,
                    "stage": stage,
                    "wait_ms": wait_ms,
                    "reason": "retired_launch_pending",
                }),
            );
            return true;
        }
        let Some(saved) = state
            .database
            .accounts
            .iter()
            .find(|s| s.account.id == expected.id)
        else {
            return true;
        };
        let a = &saved.account;
        if a.generation != expected.generation
            || !a.desired_running
            || a.process.is_some()
            || a.tracker.is_some()
            || a.status == Status::NeedsAttention
            || self.is_shutdown()
        {
            self.0.journal.record(
                "launch_cancelled",
                serde_json::json!({
                    "account": crate::diagnostics::account(a),
                    "operation_id": expected.operation_id,
                    "generation": expected.generation,
                    "stage": stage,
                    "wait_ms": wait_ms,
                    "reason": if a.status == Status::NeedsAttention {
                        "needs_attention"
                    } else {
                        "superseded_stopped_owned_or_shutdown"
                    },
                    "shutdown": self.is_shutdown(),
                }),
            );
            return true;
        }
        false
    }
    async fn launch_one(&self, account: Account) {
        if !launch_ready(&account, Utc::now()) {
            return;
        }
        let snap = self.snapshot();
        if !snap.active_multi_instance
            && snap.accounts.iter().any(|a| {
                a.id != account.id
                    && (a.process.is_some() || a.tracker.is_some() || a.status == Status::Launching)
            })
        {
            let wait_msg = single_instance_wait_error(snap.multi_instance_enabled);
            let _ = self.update(&account.id, account.generation, |a| {
                a.status = Status::Queued;
                a.next_retry = Some(Utc::now() + ChronoDuration::seconds(5));
                a.last_error = Some(wait_msg.into());
            });
            self.0.journal.record(
                "launch_coordination_unavailable",
                serde_json::json!({
                    "account": crate::diagnostics::account(&account),
                    "operation_id": account.operation_id,
                    "generation": account.generation,
                    "stage": "queued",
                    "reason": single_instance_reason(snap.multi_instance_enabled),
                }),
            );
            return;
        }
        if retired_blocks_launch(&self.0.state.lock().unwrap().database, &account.id) {
            return;
        }
        if !self.0.launch_allowed.load(Ordering::Acquire) {
            self.0.journal.record(
                "launch_coordination_unavailable",
                serde_json::json!({
                    "account": crate::diagnostics::account(&account),
                    "operation_id": account.operation_id,
                    "generation": account.generation,
                    "stage": "preparing",
                    "reason": "launch_coordination_unavailable",
                }),
            );
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
        self.0.journal.record(
            "launch_preparing",
            serde_json::json!({
                "account": crate::diagnostics::account(&account),
                "operation_id": account.operation_id,
                "generation": account.generation,
                "tracker": tracker.parse::<u64>().ok(),
                "stage": "preparing",
            }),
        );
        let target = match tokio::time::timeout(
            Duration::from_secs(120),
            self.select_launch_target(&account, target, &cookie),
        )
        .await
        {
            Ok(Ok(Some(target))) => target,
            Ok(Ok(None)) => {
                let _ = self.update(&account.id, account.generation, |a| {
                    if a.desired_running {
                        if account.status == Status::Backoff && !a.auto_recovery {
                            a.status = Status::NeedsAttention;
                            a.next_retry = None;
                            a.last_error = Some(REJOIN_DISABLED.into());
                            return;
                        }
                        a.status = account.status.clone();
                        a.next_retry = Some(Utc::now() + ChronoDuration::seconds(5));
                        a.last_error =
                            Some("Waiting for a same-preset account to finish joining".into());
                    }
                });
                return;
            }
            result => {
                let error = match result {
                    Ok(Err(error)) => error,
                    _ => Failure::new(
                        FailureKind::Other,
                        "Server search exceeded two minutes; scheduling another attempt",
                    ),
                };
                self.failure(&account.id, account.generation, error);
                return;
            }
        };
        let uri = match self.0.roblox.launch_uri(&cookie, &target, &tracker).await {
            Ok(uri) => uri,
            Err(e) => {
                self.failure(&account.id, account.generation, e);
                return;
            }
        };
        if self.launch_cancelled_or_superseded(&account, "ready_for_dispatch", 0) {
            return;
        }
        self.0.journal.record(
            "launch_ready_for_dispatch",
            serde_json::json!({
                "account": crate::diagnostics::account(&account),
                "operation_id": account.operation_id,
                "generation": account.generation,
                "tracker": tracker.parse::<u64>().ok(),
                "stage": "ready_for_dispatch",
            }),
        );
        let wait_started = Instant::now();
        self.0.journal.record(
            "launch_waiting_for_slot",
            serde_json::json!({
                "account": crate::diagnostics::account(&account),
                "operation_id": account.operation_id,
                "generation": account.generation,
                "tracker": tracker.parse::<u64>().ok(),
                "stage": "waiting_for_slot",
            }),
        );
        // Preparation overlaps; handoffs are serialized with minimum spacing and bounded
        // readiness observation of the previous launch.
        let _launch_slot = loop {
            if self.launch_cancelled_or_superseded(
                &account,
                "waiting_for_slot",
                wait_started.elapsed().as_millis(),
            ) {
                return;
            }
            match tokio::time::timeout(Duration::from_millis(25), self.0.launch_slot.lock()).await {
                Ok(guard) => break guard,
                Err(_) => continue,
            }
        };
        loop {
            if self.launch_cancelled_or_superseded(
                &account,
                "waiting_for_slot",
                wait_started.elapsed().as_millis(),
            ) {
                return;
            }
            if self.poll_launch_readiness(&account, self.now()) {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(25)) => {}
                _ = self.0.wake.notified() => {}
            }
        }
        let slot_wait_ms = wait_started.elapsed().as_millis();
        // Persist the tracker BEFORE launching. Holding the command mutex through ShellExecute
        // makes Stop linearize either before launch or after durable ownership was recorded.
        let prepared: Result<Account, String> = {
            let mut state = self.0.state.lock().unwrap();
            if retired_blocks_launch(&state.database, &account.id) {
                self.0.journal.record(
                    "launch_cancelled",
                    serde_json::json!({
                        "account": crate::diagnostics::account(&account),
                        "operation_id": account.operation_id,
                        "generation": account.generation,
                        "stage": "waiting_for_slot",
                        "wait_ms": slot_wait_ms,
                        "reason": "retired_launch_pending",
                    }),
                );
                return;
            }
            let mut next = state.clone();
            let Ok(a) = account_mut(&mut next, &account.id) else {
                return;
            };
            if a.generation != account.generation
                || !a.desired_running
                || a.process.is_some()
                || a.tracker.is_some()
                || self.is_shutdown()
            {
                self.0.journal.record(
                    "launch_cancelled",
                    serde_json::json!({
                        "account": crate::diagnostics::account(a),
                        "operation_id": account.operation_id,
                        "generation": account.generation,
                        "stage": "waiting_for_slot",
                        "wait_ms": slot_wait_ms,
                        "reason": "superseded_stopped_owned_or_shutdown",
                        "shutdown": self.is_shutdown(),
                    }),
                );
                return;
            }
            if !self.0.launch_allowed.load(Ordering::Acquire) || state.network_suspended {
                let allowed = self.0.launch_allowed.load(Ordering::Acquire);
                self.0.journal.record(
                    "launch_coordination_unavailable",
                    serde_json::json!({
                        "account": crate::diagnostics::account(a),
                        "operation_id": account.operation_id,
                        "generation": account.generation,
                        "stage": "waiting_for_slot",
                        "wait_ms": slot_wait_ms,
                        "reason": if !allowed { "launch_coordination_unavailable" } else { "network_suspended" },
                    }),
                );
                a.status = account.status.clone();
                a.next_retry = Some(Utc::now() + ChronoDuration::seconds(5));
                let _ = self.commit_close_state(&mut state, next);
                return;
            }
            if account.status == Status::Backoff && !a.auto_recovery {
                a.status = Status::NeedsAttention;
                a.next_retry = None;
                a.last_error = Some(REJOIN_DISABLED.into());
                if self.0.store.save(&next.database).is_ok() {
                    self.journal_changes(&state, &next);
                    *state = next;
                    let _ = self.0.events.send(snapshot(&state));
                }
                return;
            }
            if !state.active_multi_instance
                && state.database.accounts.iter().any(|s| {
                    s.account.id != account.id
                        && (s.account.process.is_some() || s.account.tracker.is_some())
                })
            {
                let multi_enabled = state.database.multi_instance_enabled;
                a.status = account.status.clone();
                a.next_retry = Some(Utc::now() + ChronoDuration::seconds(5));
                a.last_error = Some(single_instance_wait_error(multi_enabled).into());
                self.0.journal.record(
                    "launch_coordination_unavailable",
                    serde_json::json!({
                        "account": crate::diagnostics::account(a),
                        "operation_id": account.operation_id,
                        "generation": account.generation,
                        "stage": "waiting_for_slot",
                        "wait_ms": slot_wait_ms,
                        "reason": single_instance_reason(multi_enabled),
                    }),
                );
                let _ = self.commit_close_state(&mut state, next);
                return;
            }
            if matches!(
                a.last_error.as_deref(),
                Some(STARTUP_IN_PROGRESS_WAIT_ERROR) | Some(STARTUP_AMBIGUOUS_PROCESS_WAIT_ERROR)
            ) {
                a.last_error = None;
            }
            a.tracker = Some(tracker.clone());
            a.ownership_deadline = Some(Utc::now() + ChronoDuration::seconds(90));
            if let Err(e) = self.0.store.save(&next.database) {
                self.0.journal.record(
                    "launch_cancelled",
                    serde_json::json!({
                        "account": crate::diagnostics::account(&account),
                        "operation_id": account.operation_id,
                        "generation": account.generation,
                        "stage": "submitted",
                        "wait_ms": slot_wait_ms,
                        "reason": "tracker_save_failed",
                    }),
                );
                Err(e)
            } else {
                self.journal_changes(&state, &next);
                *state = next;
                let current = state
                    .database
                    .accounts
                    .iter()
                    .find(|s| s.account.id == account.id)
                    .unwrap()
                    .account
                    .clone();
                Ok(current)
            }
        };
        let current_account = match prepared {
            Ok(acc) => acc,
            Err(_) => return,
        };
        #[cfg(test)]
        if let Some(hook) = &self.0.pre_dispatch_hook {
            hook(&account.id, current_account.generation, &tracker);
        }
        let launched =
            self.submit_launch_with_wait(&current_account, &target, slot_wait_ms, || {
                #[cfg(test)]
                if let Some(hook) = &self.0.launch_hook {
                    return hook(&uri, &account.id);
                }
                #[cfg(test)]
                if let Some(result) = &self.0.launch_result {
                    return result.clone();
                }
                platform::launch_for(&uri, &account.id)
            });
        if launched.is_ok() {
            let dispatched_at = self.now();
            {
                let mut coord = self.0.launch_coordinator.lock().unwrap();
                coord.next_allowed_at = dispatched_at + self.0.launch_spacing;
                coord.last_dispatched = Some(PendingLaunchReadiness {
                    account_id: account.id.clone(),
                    operation_id: account.operation_id,
                    generation: account.generation,
                    tracker: tracker.clone(),
                    dispatched_at,
                    stage: LaunchReadinessStage::ProtocolAccepted,
                    stage_updated_at: dispatched_at,
                    observed_pid: None,
                    deferred_logged: false,
                });
            }
            self.0.journal.record(
                "launch_stage_transition",
                serde_json::json!({
                    "account_id": account.id.parse::<u64>().ok(),
                    "operation_id": account.operation_id,
                    "generation": account.generation,
                    "tracker": tracker.parse::<u64>().ok(),
                    "stage": LaunchReadinessStage::ProtocolAccepted,
                }),
            );
            self.0.wake.notify_waiters();
        }
        if let Err(message) = launched
            && !message.starts_with("Launch cancelled before dispatch:")
        {
            self.note_launch_stage(
                &account.id,
                account.generation,
                LaunchReadinessStage::StartupFailed,
                None,
            );
            let _ = self.update(&account.id, account.generation, |a| {
                a.tracker = None;
                a.ownership_deadline = None;
            });
            self.failure(
                &account.id,
                account.generation,
                Failure::new(FailureKind::Unsupported, &message),
            );
        }
        // Return immediately: shared discovery binds the PID or expires the 90s deadline.
    }
    #[cfg(test)]
    fn submit_launch(
        &self,
        account: &Account,
        target: &Target,
        effect: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        self.submit_launch_with_wait(account, target, 0, effect)
    }
    fn check_dispatch_authorization(
        state: &State,
        account: &Account,
        is_shutdown: bool,
        launch_allowed: bool,
    ) -> Result<(), (&'static str, bool)> {
        if is_shutdown {
            Err(("shutdown", true))
        } else if !launch_allowed {
            Err(("launch_coordination_unavailable", false))
        } else if state.network_suspended {
            Err(("network_suspended", false))
        } else if retired_blocks_launch(&state.database, &account.id) {
            Err(("retired_launch_pending", false))
        } else if let Ok(current) = account_mut(&mut state.clone(), &account.id) {
            if current.generation != account.generation {
                Err(("generation_changed", false))
            } else if !current.desired_running {
                Err(("superseded_or_stopped", false))
            } else if current.tracker.is_some()
                && current.tracker.as_deref() != account.tracker.as_deref()
            {
                Err(("tracker_mismatch", false))
            } else if current.status == Status::Stopping
                || current.status == Status::Stopped
                || current.close_intent.as_ref().is_some_and(|i| {
                    matches!(i.reason, CloseReason::UserStop | CloseReason::ManualRestart)
                        && !i.denied
                })
            {
                Err(("superseded_or_stopped", false))
            } else {
                Ok(())
            }
        } else {
            Err(("account_not_found", false))
        }
    }

    // Only call after dispatch authorization rejected this reservation before the OS effect.
    fn cleanup_unsubmitted_launch(
        &self,
        account_id: &str,
        generation: Uuid,
        tracker: &str,
        operation_id: Option<Uuid>,
    ) -> Result<(), String> {
        let context = serde_json::json!({
            "account_id": account_id.parse::<u64>().ok(),
            "generation": generation,
            "tracker": tracker.parse::<u64>().ok(),
            "operation_id": operation_id,
            "reason": "unsubmitted_launch_cancelled",
        });
        let result = self.transaction_inner(
            |state| {
                let mut removed = false;
                let mut stop_intent = None;
                state.database.retired_launches.retain(|r| {
                    if r.account_id == account_id
                        && r.generation == generation
                        && r.tracker == tracker
                        && r.process.is_none()
                    {
                        removed = true;
                        stop_intent = r.close_intent.clone();
                        false
                    } else {
                        true
                    }
                });
                let has_other_retired = state
                    .database
                    .retired_launches
                    .iter()
                    .any(|r| r.account_id == account_id);
                let Some(slot) = state
                    .database
                    .accounts
                    .iter_mut()
                    .find(|s| s.account.id == account_id)
                else {
                    return Ok(removed);
                };
                let account = &mut slot.account;
                let clear_tracker = account.generation == generation
                    && account.tracker.as_deref() == Some(tracker)
                    && account.operation_id == operation_id
                    && account.process.is_none()
                    && !has_other_retired;
                if clear_tracker {
                    account.tracker = None;
                    account.ownership_deadline = None;
                }
                if removed
                    && !has_other_retired
                    && account.process.is_none()
                    && account.tracker.is_none()
                    && !account.desired_running
                    && account.status == Status::Stopping
                    && let Some(intent) = stop_intent
                    && intent.reason == CloseReason::UserStop
                    && account
                        .close_intent
                        .as_ref()
                        .is_some_and(|i| i.id == intent.id)
                    && account.generation == intent.generation
                    && account.operation_id == intent.operation_id
                {
                    // The transaction normalizes Stopping and completes the current Stop.
                    account.close_intent = None;
                }
                Ok(removed || clear_tracker)
            },
            false,
            |changed| *changed,
        );
        match result {
            Ok(changed) => {
                if changed {
                    self.0.journal.record("launch_reservation_cleaned", context);
                    self.0.wake.notify_waiters();
                }
                Ok(())
            }
            Err(error) => {
                self.0
                    .journal
                    .record("launch_reservation_cleanup_failed", context);
                Err(error)
            }
        }
    }

    fn handle_launch_cancellation(
        &self,
        account: &Account,
        wait_ms: u128,
        reason: &str,
        is_shutdown: bool,
    ) -> Result<(), String> {
        if let Some(tracker) = account.tracker.as_deref() {
            self.cleanup_unsubmitted_launch(
                &account.id,
                account.generation,
                tracker,
                account.operation_id,
            )
            .map_err(|error| {
                format!("Launch cancelled before dispatch: reservation cleanup failed: {error}")
            })?;
        }

        if reason == "launch_coordination_unavailable" {
            self.0.journal.record(
                "launch_coordination_unavailable",
                serde_json::json!({
                    "account": crate::diagnostics::account(account),
                    "operation_id": account.operation_id,
                    "generation": account.generation,
                    "stage": "submitted",
                    "wait_ms": wait_ms,
                    "reason": reason,
                }),
            );
            return Err(LAUNCH_COORDINATION_ERROR.into());
        }

        self.0.journal.record(
            "launch_cancelled",
            serde_json::json!({
                "account": crate::diagnostics::account(account),
                "operation_id": account.operation_id,
                "generation": account.generation,
                "stage": "submitted",
                "wait_ms": wait_ms,
                "reason": reason,
                "shutdown": is_shutdown,
            }),
        );

        Err(format!("Launch cancelled before dispatch: {reason}"))
    }

    fn submit_launch_with_wait(
        &self,
        account: &Account,
        target: &Target,
        wait_ms: u128,
        effect: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let auth = {
            let state = self.0.state.lock().unwrap();
            Self::check_dispatch_authorization(
                &state,
                account,
                self.is_shutdown(),
                self.0.launch_allowed.load(Ordering::Acquire),
            )
        };

        if let Err((reason, is_shutdown)) = auth {
            return self.handle_launch_cancellation(account, wait_ms, reason, is_shutdown);
        }

        let context = crate::diagnostics::account(account);
        self.0.journal.record(
            "launch_dispatch_authorized",
            serde_json::json!({
                "account": context,
                "operation_id": account.operation_id,
                "generation": account.generation,
                "stage": "submitted",
                "wait_ms": wait_ms,
            }),
        );

        #[cfg(test)]
        if let Some(hook) = &self.0.post_auth_hook {
            hook(
                &account.id,
                account.generation,
                account.tracker.as_deref().unwrap_or(""),
            );
        }

        let _dispatch = self.0.dispatch_lock.lock().unwrap();

        let final_check = {
            let state = self.0.state.lock().unwrap();
            Self::check_dispatch_authorization(
                &state,
                account,
                self.is_shutdown(),
                self.0.launch_allowed.load(Ordering::Acquire),
            )
        };

        if let Err((reason, is_shutdown)) = final_check {
            return self.handle_launch_cancellation(account, wait_ms, reason, is_shutdown);
        }

        // No URI, cookie, ticket, private link or command line reaches the journal.
        self.0.journal.record(
            "launch_submitted",
            serde_json::json!({
                "account": context,
                "operation_id": account.operation_id,
                "generation": account.generation,
                "stage": "submitted",
                "wait_ms": wait_ms,
                "selected_place_id": target.place_id,
                "specific_server": target.job_id.is_some(),
                "private_destination": target.private_server_link.is_some(),
            }),
        );
        let result = effect();
        self.0.journal.record(
            "launch_result",
            serde_json::json!({
                "account": context,
                "operation_id": account.operation_id,
                "generation": account.generation,
                "stage": "result",
                "wait_ms": wait_ms,
                "accepted_by_os": result.is_ok(),
                "reason": result.as_ref().err().map(|e| crate::diagnostics::reason(e)),
                "error_code": result.as_ref().err().and_then(|e| crate::diagnostics::error_code(e)),
            }),
        );
        result
    }
    fn expire_unowned_launches(&self, now: chrono::DateTime<Utc>) {
        for a in self.snapshot().accounts.into_iter().filter(|a| {
            a.desired_running
                && a.process.is_none()
                && a.tracker.is_some()
                && a.status != Status::NeedsAttention
                && a.ownership_deadline.is_some_and(|deadline| deadline <= now)
        }) {
            let _ = self.transaction(|state| {
                let current = account_mut(state, &a.id)?;
                if current.generation != a.generation || current.process.is_some() || current.tracker != a.tracker || current.status == Status::NeedsAttention { return Ok(()); }
                current.status = Status::NeedsAttention;
                current.last_error = Some(OWNERSHIP_TIMEOUT_ERROR.into());
                let operation = current.operation_id;
                if let Some(op) = operation.and_then(|id| state.operations.get_mut(&id)) {
                    op.state = "failed".into();
                    op.error = Some("Launch ownership could not be verified".into());
                }
                self.0.journal.record("launch_timeout", serde_json::json!({"account_id": a.id.parse::<u64>().ok(), "generation": a.generation, "operation_id": operation, "reason": "no_verified_process_before_deadline"}));
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    fn observe_players_logged(&self) -> Result<platform::ProcessScan, String> {
        #[cfg(test)]
        if let Some(hook) = &self.0.discovery_hook {
            let started = Instant::now();
            let scan = hook()?;
            self.0.journal.record(
                "process_observation",
                serde_json::json!({
                    "observed": scan.players.len(),
                    "inaccessible": scan.inaccessible,
                    "rejected_image": scan.rejected,
                    "unowned": scan.unowned,
                    "uninspected": scan.uninspected,
                    "metadata_pending": scan.metadata_pending,
                    "bootstrapper_active": scan.bootstrapper_active,
                    "duration_ms": started.elapsed().as_millis(),
                    "processes": scan
                        .players
                        .iter()
                        .take(50)
                        .map(|p| crate::diagnostics::process(&p.identity(Uuid::nil())))
                        .collect::<Vec<_>>(),
                }),
            );
            return Ok(scan);
        }
        let started = Instant::now();
        let scan = platform::observe_players()?;
        self.0.journal.record(
            "process_observation",
            serde_json::json!({
                "observed": scan.players.len(),
                "inaccessible": scan.inaccessible,
                "rejected_image": scan.rejected,
                "unowned": scan.unowned,
                "uninspected": scan.uninspected,
                "metadata_pending": scan.metadata_pending,
                "bootstrapper_active": scan.bootstrapper_active,
                "duration_ms": started.elapsed().as_millis(),
                "processes": scan
                    .players
                    .iter()
                    .take(50)
                    .map(|p| crate::diagnostics::process(&p.identity(Uuid::nil())))
                    .collect::<Vec<_>>(),
            }),
        );
        Ok(scan)
    }
    fn discover_for_target(
        &self,
        target: &ProcessIdentity,
    ) -> Result<Vec<platform::Player>, String> {
        #[cfg(windows)]
        {
            self.observe_players_logged()?.verified_for_target(target)
        }
        #[cfg(not(windows))]
        {
            let _ = target;
            platform::players()
        }
    }
    fn discover_for_retired(
        &self,
        retired: &RetiredLaunch,
    ) -> Result<Vec<platform::Player>, String> {
        #[cfg(windows)]
        {
            self.observe_players_logged()?
                .verified_for_tracker(&retired.tracker, retired.process.as_ref())
        }
        #[cfg(not(windows))]
        {
            let _ = retired;
            platform::players()
        }
    }
    #[cfg(any(not(windows), test))]
    pub(super) fn reconcile_launch_discovery(
        &self,
        players: &[platform::Player],
        complete: bool,
    ) -> bool {
        let uninspected = if complete {
            0
        } else {
            players.iter().filter(|p| p.tracker.is_empty()).count()
        };
        self.observe_startup_scan(players, uninspected, 0, !complete, false)
    }
    pub(super) fn observe_startup_scan(
        &self,
        players: &[platform::Player],
        uninspected: usize,
        inaccessible: usize,
        metadata_pending: bool,
        bootstrapper_active: bool,
    ) -> bool {
        let complete = !metadata_pending && uninspected == 0 && inaccessible == 0;
        let now = self.now();
        let mut exited_before_verification = None;
        let mut stage_to_record = None;
        {
            let mut coord = self.0.launch_coordinator.lock().unwrap();
            coord.unverified_processes = uninspected;
            coord.inaccessible_processes = inaccessible;
            coord.bootstrapper_active = bootstrapper_active;
            if uninspected == 0 && inaccessible == 0 && !bootstrapper_active {
                coord.ambiguous_logged_for = None;
                coord.ambiguous_first_seen = None;
                coord.ambiguous_timeout_logged_for = None;
            } else {
                coord.ambiguous_first_seen.get_or_insert(now);
            }
            if let Some(prev) = coord.last_dispatched.as_mut() {
                let matching_tracker_pid = players
                    .iter()
                    .find(|p| !p.tracker.is_empty() && p.tracker == prev.tracker)
                    .map(|p| p.pid);
                let unowned_candidate_pid =
                    players.iter().find(|p| p.tracker.is_empty()).map(|p| p.pid);
                if let Some(pid) = matching_tracker_pid {
                    stage_to_record = Some((
                        prev.account_id.clone(),
                        prev.generation,
                        LaunchReadinessStage::ProcessObserved,
                        Some(pid),
                    ));
                } else if uninspected > 0 || inaccessible > 0 {
                    stage_to_record = Some((
                        prev.account_id.clone(),
                        prev.generation,
                        LaunchReadinessStage::ProcessObserved,
                        unowned_candidate_pid,
                    ));
                } else if bootstrapper_active
                    && prev.stage == LaunchReadinessStage::ProtocolAccepted
                {
                    stage_to_record = Some((
                        prev.account_id.clone(),
                        prev.generation,
                        LaunchReadinessStage::BootstrapperStarted,
                        None,
                    ));
                } else if prev.stage == LaunchReadinessStage::ProcessObserved
                    && let Some(observed_pid) = prev.observed_pid
                {
                    if !players.iter().any(|p| p.pid == observed_pid) && inaccessible == 0 {
                        let elapsed_ms = now
                            .saturating_duration_since(prev.dispatched_at)
                            .as_millis();
                        let removed = coord.last_dispatched.take().unwrap();
                        exited_before_verification = Some((removed, elapsed_ms));
                    } else if !metadata_pending && uninspected == 0 && inaccessible == 0 {
                        prev.stage = LaunchReadinessStage::ProtocolAccepted;
                        prev.observed_pid = None;
                        prev.stage_updated_at = now;
                    }
                }
            }
        }
        if let Some((account_id, generation, stage, pid)) = stage_to_record {
            self.note_launch_stage(&account_id, generation, stage, pid);
        }
        if let Some((prev, wait_ms)) = exited_before_verification {
            self.mark_startup_unresolved(
                &prev,
                "observed_process_exited_before_verification",
                wait_ms,
                None,
            );
        }
        let mut attached_any = false;
        for account in self.snapshot().accounts {
            if account.process.is_some() || account.tracker.is_none() {
                continue;
            }
            let matches: Vec<_> = players
                .iter()
                .filter(|p| !p.tracker.is_empty() && Some(&p.tracker) == account.tracker.as_ref())
                .collect();
            if !matches.is_empty() {
                self.note_launch_stage(
                    &account.id,
                    account.generation,
                    LaunchReadinessStage::ProcessObserved,
                    Some(matches[0].pid),
                );
                self.0.journal.record(
                    "launch_process_observed",
                    serde_json::json!({
                        "account": crate::diagnostics::account(&account),
                        "operation_id": account.operation_id,
                        "generation": account.generation,
                        "tracker": account.tracker.as_ref().and_then(|t| t.parse::<u64>().ok()),
                        "stage": "process_observed",
                        "candidates": matches
                            .iter()
                            .map(|p| crate::diagnostics::process(&p.identity(account.generation)))
                            .collect::<Vec<_>>(),
                    }),
                );
            }
            if !complete {
                continue;
            }
            self.0.journal.record(
                "launch_discovery",
                serde_json::json!({
                    "account": crate::diagnostics::account(&account),
                    "candidates": matches
                        .iter()
                        .map(|p| crate::diagnostics::process(&p.identity(account.generation)))
                        .collect::<Vec<_>>(),
                }),
            );
            if matches.len() == 1 {
                let identity = matches[0].identity(account.generation);
                if self.attach_player(&account, identity, players).is_ok() {
                    attached_any = true;
                }
            } else if matches.len() > 1 {
                let message =
                    "Multiple primary instances share this launch tracker; ownership is ambiguous";
                let _ = self.update(&account.id, account.generation, |a| {
                    a.status = Status::NeedsAttention;
                    a.last_error = Some(message.into());
                });
                self.note_launch_stage(
                    &account.id,
                    account.generation,
                    LaunchReadinessStage::StartupFailed,
                    Some(matches[0].pid),
                );
                self.finish_operation(&account.id, account.generation, Some(message.into()));
            } else if account.desired_running
                && account.process.is_none()
                && account.status != Status::NeedsAttention
                && account.ownership_deadline.is_none_or(|t| t <= Utc::now())
            {
                let _ = self.update(&account.id, account.generation, |a| {
                    a.status = Status::NeedsAttention;
                    a.last_error = Some(OWNERSHIP_TIMEOUT_ERROR.into());
                });
                self.note_launch_stage(
                    &account.id,
                    account.generation,
                    LaunchReadinessStage::StartupUnresolved,
                    None,
                );
                self.finish_operation(
                    &account.id,
                    account.generation,
                    Some("Launch ownership could not be verified".into()),
                );
            } else if !account.desired_running && account.process.is_none() {
                // Stopped processes are handled below; late launches use durable tombstones.
            }
        }
        if attached_any
            || (!metadata_pending && uninspected == 0 && inaccessible == 0 && !bootstrapper_active)
        {
            self.0.wake.notify_waiters();
        }
        attached_any
    }
    async fn monitor_loop(&self) {
        let mut tails: HashMap<(u32, String), crate::logs::Tail> = HashMap::new();
        let mut last_discovery = Instant::now() - Duration::from_secs(30);
        let mut last_log_discovery = Instant::now() - Duration::from_secs(30);
        let mut close_attempts = CloseAttempts::default();
        let mut exit_watches: HashMap<(u32, String), platform::ExitWatch> = HashMap::new();
        let mut pending_exits: HashMap<(u32, String), PendingExit> = HashMap::new();
        let mut discovery = None;
        let mut metadata_pending = false;
        let mut log_discovery = None;
        #[cfg(windows)]
        let mut memory_task: Option<tokio::task::JoinHandle<MemoryProbe>> = None;
        #[cfg(windows)]
        let mut memory_readers = HashMap::new();
        #[cfg(windows)]
        let mut memory_attempts = HashMap::<(u32, String), Instant>::new();
        #[cfg(windows)]
        let mut log_progress = HashMap::<(u32, String), (u64, Instant)>::new();
        #[cfg(windows)]
        let memory_epoch = Instant::now();
        while !self.is_shutdown() {
            let retired = self
                .0
                .state
                .lock()
                .unwrap()
                .database
                .retired_launches
                .clone();
            let has_unresolved_launch = self
                .snapshot()
                .accounts
                .iter()
                .any(|a| a.process.is_none() && a.tracker.is_some());
            let has_waiting_launch = self.snapshot().accounts.iter().any(|a| {
                a.desired_running
                    && a.process.is_none()
                    && a.status != Status::Stopped
                    && a.status != Status::Stopping
            });
            let coord_needs_discovery = {
                let coord = self.0.launch_coordinator.lock().unwrap();
                (coord.unverified_processes > 0
                    || coord.inaccessible_processes > 0
                    || coord.bootstrapper_active
                    || coord.last_dispatched.is_some())
                    && (has_waiting_launch || has_unresolved_launch)
            };
            let should_discover = has_unresolved_launch
                || !retired.is_empty()
                || !pending_exits.is_empty()
                || coord_needs_discovery
                || metadata_pending;
            let discovery_interval = if metadata_pending {
                Duration::from_millis(250)
            } else if !pending_exits.is_empty() || has_unresolved_launch || coord_needs_discovery {
                Duration::from_secs(1)
            } else {
                Duration::from_secs(5)
            };
            if discovery.is_none()
                && last_discovery.elapsed() >= discovery_interval
                && should_discover
            {
                #[cfg(windows)]
                let engine = self.clone();
                discovery = Some(tokio::task::spawn_blocking(move || {
                    #[cfg(windows)]
                    {
                        engine.observe_players_logged()
                    }
                    #[cfg(not(windows))]
                    {
                        platform::players()
                    }
                }));
                last_discovery = Instant::now();
            }
            let discovered = completed_task(&mut discovery).await;
            match discovered {
                Some(Err(_)) | Some(Ok(Err(_))) => {
                    metadata_pending = false;
                    self.observe_startup_scan(&[], 0, 1, false, false);
                    self.0
                        .journal
                        .record("process_discovery_failed", serde_json::json!({}));
                    for a in self.snapshot().accounts {
                        if let Some(p) = &a.process
                            && let Some(exit) = pending_exits.get(&(p.pid, p.creation_time.clone()))
                            && last_discovery >= exit.observed
                            && exit.settled(&a, Instant::now(), Utc::now())
                        {
                            self.pause_unresolved_exit(&a, p);
                        }
                    }
                }
                #[cfg(windows)]
                Some(Ok(Ok(scan))) => {
                    metadata_pending = scan.metadata_pending;
                    let startup_verified = self.observe_startup_scan(
                        &scan.players,
                        scan.uninspected,
                        scan.inaccessible,
                        scan.metadata_pending,
                        scan.bootstrapper_active,
                    );
                    if scan.inaccessible == 0 && startup_verified {
                        // Discover its log now; an empty scan during startup
                        // must not leave a newly verified player waiting 30s.
                        last_log_discovery = Instant::now() - Duration::from_secs(30);
                    }
                    if scan.inaccessible > 0 || (scan.uninspected > 0 && !scan.metadata_pending) {
                        self.0
                            .journal
                            .record("process_discovery_failed", serde_json::json!({}));
                    }
                    for account in self.snapshot().accounts {
                        let Some(identity) = account.process.as_ref() else {
                            continue;
                        };
                        let key = (identity.pid, identity.creation_time.clone());
                        let Some(pending) = pending_exits.get(&key) else {
                            continue;
                        };
                        // A scan started before the exit cannot establish successor absence.
                        if last_discovery < pending.observed {
                            continue;
                        }
                        let settled = pending.settled(&account, Instant::now(), Utc::now());
                        match scan.verified_for_target(identity) {
                            Ok(players) => {
                                if self.reconcile_exit(
                                    &account,
                                    identity,
                                    pending.clean,
                                    &players,
                                    settled,
                                ) {
                                    pending_exits.remove(&key);
                                    last_log_discovery = Instant::now() - Duration::from_secs(30);
                                }
                            }
                            Err(_) if settled => {
                                self.pause_unresolved_exit(&account, identity);
                            }
                            Err(_) => {}
                        }
                    }
                    for old in &retired {
                        if let Some(identity) = &old.process
                            && matches!(platform::alive(identity), Ok(false))
                        {
                            let _ = self.transaction(|s| {
                                s.database.retired_launches.retain(|r| {
                                    !(r.account_id == old.account_id
                                        && r.generation == old.generation
                                        && r.tracker == old.tracker
                                        && r.process.as_ref() == Some(identity))
                                });
                                Ok(())
                            });
                            close_attempts.remove(identity);
                            continue;
                        }
                        let Ok(players) =
                            scan.verified_for_tracker(&old.tracker, old.process.as_ref())
                        else {
                            continue;
                        };
                        if old.process.is_none()
                            && old.deadline <= Utc::now()
                            && !players.iter().any(|p| p.tracker == old.tracker)
                        {
                            let _ = self.transaction(|s| {
                                s.database.retired_launches.retain(|r| {
                                    !(r.tracker == old.tracker
                                        && r.generation == old.generation
                                        && r.process.is_none())
                                });
                                Ok(())
                            });
                            continue;
                        }
                        let _ = self.close_retired(
                            old,
                            &players,
                            Instant::now(),
                            &mut close_attempts,
                            || self.discover_for_retired(old),
                            platform::close,
                        );
                    }
                    last_discovery = Instant::now();
                }
                #[cfg(not(windows))]
                Some(Ok(Ok(players))) => {
                    metadata_pending = false;
                    if self.reconcile_launch_discovery(&players, true) {
                        last_log_discovery = Instant::now() - Duration::from_secs(30);
                    }
                    for account in self.snapshot().accounts {
                        let Some(identity) = account.process.as_ref() else {
                            continue;
                        };
                        let key = (identity.pid, identity.creation_time.clone());
                        let Some(pending) = pending_exits.get(&key) else {
                            continue;
                        };
                        if last_discovery < pending.observed {
                            continue;
                        }
                        if self.reconcile_exit(
                            &account,
                            identity,
                            pending.clean,
                            &players,
                            pending.settled(&account, Instant::now(), Utc::now()),
                        ) {
                            pending_exits.remove(&key);
                            last_log_discovery = Instant::now() - Duration::from_secs(30);
                        }
                    }
                    for old in &retired {
                        if old.process.is_none()
                            && old.deadline <= Utc::now()
                            && !players.iter().any(|p| p.tracker == old.tracker)
                        {
                            let _ = self.transaction(|s| {
                                s.database.retired_launches.retain(|r| {
                                    !(r.tracker == old.tracker
                                        && r.generation == old.generation
                                        && r.process.is_none())
                                });
                                Ok(())
                            });
                            continue;
                        }
                        if let Some(identity) = &old.process
                            && matches!(platform::alive(identity), Ok(false))
                        {
                            let _ = self.transaction(|s| {
                                s.database.retired_launches.retain(|r| {
                                    !(r.account_id == old.account_id
                                        && r.generation == old.generation
                                        && r.tracker == old.tracker
                                        && r.process.as_ref() == Some(identity))
                                });
                                Ok(())
                            });
                            close_attempts.remove(identity);
                            continue;
                        }
                        let _ = self.close_retired(
                            old,
                            &players,
                            Instant::now(),
                            &mut close_attempts,
                            || self.discover_for_retired(old),
                            platform::close,
                        );
                    }
                    last_discovery = Instant::now();
                }
                None => {}
            }
            // Even an unavailable metadata provider must finish the operation's
            // timeout. Retain its tracker so timeout never authorizes a duplicate.
            self.expire_unowned_launches(Utc::now());
            if log_discovery.is_none() && last_log_discovery.elapsed() >= Duration::from_secs(30) {
                let identities: Vec<_> = self
                    .snapshot()
                    .accounts
                    .iter()
                    .filter_map(|a| a.process.clone())
                    .filter(|p| !tails.contains_key(&(p.pid, p.creation_time.clone())))
                    .collect();
                if !identities.is_empty() {
                    log_discovery = Some(tokio::task::spawn_blocking(move || {
                        #[cfg(windows)]
                        {
                            platform::owned_log_tails(&identities)
                        }
                        #[cfg(not(windows))]
                        {
                            platform::owned_logs(&identities).map(|logs| {
                                logs.into_iter()
                                    .filter_map(|(key, path)| {
                                        crate::logs::Tail::new(path, false)
                                            .ok()
                                            .map(|tail| (key, tail))
                                    })
                                    .collect::<HashMap<_, _>>()
                            })
                        }
                    }));
                }
                last_log_discovery = Instant::now();
            }
            let discovered_logs = completed_task(&mut log_discovery).await;
            if matches!(discovered_logs, Some(Err(_)) | Some(Ok(Err(_)))) {
                self.0
                    .journal
                    .record("log_discovery_failed", serde_json::json!({}));
            }
            if let Some(Ok(Ok(logs))) = discovered_logs {
                for (key, tail) in logs {
                    self.0
                        .journal
                        .record("owned_log_attached", serde_json::json!({"pid": key.0}));
                    tails.insert(key, tail);
                }
            }
            let accounts = self.snapshot().accounts;
            let active: std::collections::HashSet<_> = accounts
                .iter()
                .filter_map(|a| a.process.as_ref())
                .map(|p| (p.pid, p.creation_time.clone()))
                .collect();
            tails.retain(|key, _| active.contains(key));
            exit_watches.retain(|key, _| active.contains(key));
            pending_exits.retain(|key, _| active.contains(key));
            #[cfg(windows)]
            {
                memory_readers.retain(|key, _| active.contains(key));
                memory_attempts.retain(|key, _| active.contains(key));
                log_progress.retain(|key, _| active.contains(key));
            }
            for account in accounts {
                let Some(identity) = account.process.clone() else {
                    continue;
                };
                let key = (identity.pid, identity.creation_time.clone());
                if pending_exits.contains_key(&key) {
                    continue;
                }
                if !exit_watches.contains_key(&key)
                    && let Some(watch) = platform::ExitWatch::new(&identity)
                {
                    exit_watches.insert(key.clone(), watch);
                }
                let alive = exit_watches.get(&key).map_or_else(
                    || platform::alive(&identity),
                    |watch| watch.alive(&identity),
                );
                match alive {
                    Ok(false) => {
                        let clean =
                            exit_watches.remove(&key).and_then(|w| w.clean_exit()) == Some(true);
                        tails.remove(&key);
                        close_attempts.remove(&identity);
                        pending_exits.insert(
                            key,
                            PendingExit {
                                observed: Instant::now(),
                                clean,
                            },
                        );
                        self.0.journal.record("process_exit_pending", serde_json::json!({
                            "account": crate::diagnostics::account(&account), "clean_exit": clean,
                            "reason": "checking_for_surviving_client_before_replacement",
                        }));
                        let _ = self.update(&account.id, account.generation, |a| {
                            note_process_exit(a, &identity, clean);
                        });
                        continue;
                    }
                    Err(_) => {
                        if account.desired_running && account.status != Status::Unknown {
                            let _ =
                                self.update(&account.id, account.generation, process_unavailable);
                        }
                        continue;
                    }
                    Ok(true) => {}
                }
                if account.desired_running
                    && account.status != Status::Queued
                    && let Some(tail) = tails.get_mut(&key)
                {
                    match tail.read_events() {
                        Ok(signals) => {
                            #[cfg(windows)]
                            {
                                let progress = log_progress
                                    .entry(key.clone())
                                    .or_insert((tail.offset, Instant::now()));
                                if progress.0 != tail.offset {
                                    *progress = (tail.offset, Instant::now());
                                }
                            }
                            for event in signals {
                                self.observe(
                                    &account.id,
                                    account.generation,
                                    &identity,
                                    Observation::DiskLog(event),
                                );
                            }
                        }
                        Err(error) => {
                            self.0.journal.record("owned_log_read_failed", serde_json::json!({"account": crate::diagnostics::account(&account), "io_error": format!("{:?}", error.kind())}));
                            tails.remove(&key);
                            let _ = self.update(&account.id, account.generation, log_unavailable);
                        }
                    }
                }
                let Some(account) = self
                    .0
                    .state
                    .lock()
                    .unwrap()
                    .database
                    .accounts
                    .iter()
                    .map(|s| &s.account)
                    .find(|a| {
                        a.id == account.id
                            && a.generation == account.generation
                            && a.process.as_ref() == Some(&identity)
                    })
                    .cloned()
                else {
                    continue;
                };
                // As in 1.7.0, report a generic loss only after fresh logs and grace.
                if !account.disconnect_notified && recovery_due(&account, Utc::now()) {
                    let _ = self.update(&account.id, account.generation, |_| {});
                }
                // This boundary reads current intent under the command mutex and verifies
                // the process again, including on every force escalation.
                if self
                    .close_owned(
                        &account,
                        Instant::now(),
                        &mut close_attempts,
                        || self.discover_for_target(&identity),
                        platform::close,
                    )
                    .unwrap_or(false)
                {
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
            #[cfg(windows)]
            {
                if let Some(Ok((identity, reader, event))) = completed_task(&mut memory_task).await
                {
                    let key = (identity.pid, identity.creation_time.clone());
                    let current = self.snapshot().accounts.into_iter().find(|a| {
                        a.generation == identity.generation
                            && a.process.as_ref() == Some(&identity)
                            && a.desired_running
                            && matches!(
                                a.status,
                                Status::Unknown | Status::Running | Status::Reconnecting
                            )
                    });
                    if let Some(account) = current {
                        if let Some(reader) = reader {
                            memory_readers.insert(key.clone(), reader);
                        }
                        if let Some((time, signal)) = event
                            && time >= Utc::now() - ChronoDuration::seconds(15)
                            && time <= Utc::now()
                            && tails
                                .get(&key)
                                .and_then(|tail| tail.last_signal_time)
                                .is_none_or(|disk| time > disk)
                        {
                            self.observe(
                                &account.id,
                                account.generation,
                                &identity,
                                Observation::UnverifiedMemory(signal),
                            );
                        }
                    }
                }
                if memory_task.is_none() {
                    let candidate = self
                        .snapshot()
                        .accounts
                        .into_iter()
                        .filter(|a| {
                            a.desired_running
                                && matches!(
                                    a.status,
                                    Status::Unknown | Status::Running | Status::Reconnecting
                                )
                        })
                        .filter_map(|a| a.process)
                        .filter(|id| {
                            let key = (id.pid, id.creation_time.clone());
                            let quiet = log_progress
                                .get(&key)
                                .map_or(memory_epoch, |(_, time)| *time)
                                .elapsed()
                                >= Duration::from_secs(15);
                            quiet
                                && memory_attempts
                                    .get(&key)
                                    .map_or(memory_epoch, |time| *time)
                                    .elapsed()
                                    >= Duration::from_secs(30)
                        })
                        .min_by_key(|id| {
                            memory_attempts
                                .get(&(id.pid, id.creation_time.clone()))
                                .copied()
                                .unwrap_or(memory_epoch)
                        });
                    if let Some(identity) = candidate {
                        let key = (identity.pid, identity.creation_time.clone());
                        memory_attempts.insert(key.clone(), Instant::now());
                        let reader = memory_readers.remove(&key);
                        let disk_time = tails.get(&key).and_then(|tail| tail.last_signal_time);
                        memory_task = Some(tokio::task::spawn_blocking(move || {
                            let mut reader =
                                reader.or_else(|| platform::MemoryLogReader::new(&identity));
                            let event = reader.as_mut().and_then(|reader| reader.read(disk_time));
                            (identity, reader, event)
                        }));
                    }
                }
            }
            let sleep_duration = if metadata_pending {
                Duration::from_millis(250)
            } else {
                Duration::from_secs(1)
            };
            tokio::select! { _ = tokio::time::sleep(sleep_duration) => {}, _ = self.0.wake.notified() => {} }
        }
    }
    fn observe(
        &self,
        id: &str,
        generation: Uuid,
        identity: &ProcessIdentity,
        observation: Observation,
    ) {
        let accepted = self
            .transaction(|state| {
                let account = account_mut(state, id)?;
                let (source, signal, confirmed, code, timestamp) = match observation {
                    Observation::DiskLog(event) => ("owned_disk_log", event.signal, event.confirmed_terminal, event.code, event.timestamp),
                    #[cfg(any(windows, test))]
                    Observation::UnverifiedMemory(signal) => ("unverified_memory", signal, false, match signal { Signal::SessionLost(code) => code, Signal::ConnectionFailed => Some(279), _ => None }, None),
                };
                self.0.journal.record("connection_signal", serde_json::json!({
                    "account_id": id.parse::<u64>().ok(), "generation": generation,
                    "process": crate::diagnostics::process(identity), "source": source,
                    "signal": format!("{signal:?}"), "confirmed_terminal": confirmed,
                    "error_code": code, "source_timestamp": timestamp,
                    "ownership_matches": account.generation == generation && identity.generation == generation && account.process.as_ref() == Some(identity),
                    "desired_running": account.desired_running, "status_before": account.status,
                }));
                if account.generation != generation
                    || identity.generation != generation
                    || account.process.as_ref() != Some(identity)
                    || !account.desired_running
                {
                    return Ok(false);
                }
                if account.recovery_process.as_ref() != Some(identity) {
                    account.recovery_evidence = RecoveryEvidence::Unavailable;
                    account.recovery_process = None;
                }
                apply_observation(account, observation);
                account.recovery_process = (account.recovery_evidence
                    != RecoveryEvidence::Unavailable)
                    .then(|| identity.clone());
                let connected = matches!(
                    observation,
                    Observation::DiskLog(crate::logs::LogEvent {
                        signal: Signal::Connected,
                        ..
                    })
                ) && account.status == Status::Running;
                if connected {
                    self.0.journal.record(
                        "launch_connected",
                        serde_json::json!({
                            "account": crate::diagnostics::account(account),
                            "operation_id": account.operation_id,
                            "generation": account.generation,
                            "tracker": identity.tracker.parse::<u64>().ok(),
                            "process": crate::diagnostics::process(identity),
                            "stage": "connected",
                            "source": source,
                        }),
                    );
                }
                let paused = account.status == Status::NeedsAttention;
                let operation = account.operation_id;
                let error = account.last_error.clone();
                if paused && let Some(op) = operation.and_then(|id| state.operations.get_mut(&id)) {
                    op.state = "failed".into();
                    op.error = error;
                }
                Ok(connected)
            })
            .unwrap_or(false);
        if accepted {
            self.note_launch_stage(
                id,
                generation,
                LaunchReadinessStage::Connected,
                Some(identity.pid),
            );
            self.finish_operation(id, generation, None);
        }
    }

    fn confirmed_exit(&self, id: &str, generation: Uuid, identity: &ProcessIdentity, clean: bool) {
        let _ = self.update(id, generation, |account| {
            if account.process.as_ref() != Some(identity) {
                return;
            }
            self.0.journal.record("process_exit_confirmed", serde_json::json!({"account": crate::diagnostics::account(account), "clean_exit": clean}));
            resume_verified_handoff(account);
            let ordinary_exit = account.recovery_evidence != RecoveryEvidence::RecoveryStarted
                && account.status != Status::NeedsAttention
                && !account.disconnect_notified;
            account.process = None;
            account.tracker = None;
            account.ownership_deadline = None;
            account.connected_since = None;
            account.disconnected_since = None;
            account.recovery_evidence = RecoveryEvidence::Unavailable;
            account.recovery_process = None;
            if account
                .close_intent
                .as_ref()
                .is_none_or(|i| !i.denied && i.reason != CloseReason::UserStop)
            {
                account.close_intent = None;
            }
            resume_after_confirmed_close(account);
            if account.desired_running
                && account.auto_recovery
                && !matches!(
                    account.status,
                    Status::NeedsAttention | Status::Queued | Status::Backoff
                )
            {
                apply_failure(
                    account,
                    &Failure::new(
                        FailureKind::Other,
                        "Managed client exited; scheduling recovery",
                    ),
                );
            } else if !account.desired_running || !account.auto_recovery {
                account.desired_running = false;
                account.status = Status::Stopped;
            }
            if clean && ordinary_exit {
                account.last_error = Some("Client closed normally".into());
                account.recovery_reason = "NormalExit".into();
            }
        });
    }

    #[cfg(test)]
    fn close_managed_with(
        &self,
        id: &str,
        generation: Uuid,
        identity: &ProcessIdentity,
        force: bool,
        close: impl FnOnce(&ProcessIdentity, bool) -> Result<(), String>,
    ) -> Result<bool, String> {
        // Commit the retry exactly once, after reconnect grace and before shutdown.
        let pending = self
            .0
            .state
            .lock()
            .unwrap()
            .database
            .accounts
            .iter()
            .any(|saved| saved.account.id == id && recovery_due(&saved.account, Utc::now()));
        if pending {
            self.transaction(|state| {
                let account = account_mut(state, id)?;
                if account.generation == generation
                    && identity.generation == generation
                    && account.process.as_ref() == Some(identity)
                    && account.recovery_process.as_ref() == Some(identity)
                    && recovery_due(account, Utc::now())
                {
                    reserve_recovery(account);
                    let paused = account.status == Status::NeedsAttention;
                    let operation = account.operation_id;
                    let error = account.last_error.clone();
                    if paused
                        && let Some(op) = operation.and_then(|id| state.operations.get_mut(&id))
                    {
                        op.state = "failed".into();
                        op.error = error;
                    }
                }
                Ok(())
            })?;
        }
        // Same command lock as Stop/settings/launch. Hold through the OS identity check
        // and close, so cancellation cannot race between authorization and termination.
        let state = self.0.state.lock().unwrap();
        let Some(account) = state
            .database
            .accounts
            .iter()
            .map(|saved| &saved.account)
            .find(|a| a.id == id)
        else {
            return Ok(false);
        };
        if self.is_shutdown()
            || account.generation != generation
            || account.process.as_ref() != Some(identity)
            || !close_authorized(account)
        {
            return Ok(false);
        }
        close(identity, force)?;
        Ok(true)
    }

    #[cfg(test)]
    fn elapse_reconnect_grace(&self, id: &str, generation: Uuid) {
        self.update(id, generation, |a| {
            assert!(a.disconnected_since.is_some());
            a.disconnected_since = Some(Utc::now() - ChronoDuration::seconds(31));
        })
        .unwrap();
    }

    #[cfg(test)]
    fn signal(&self, id: &str, generation: Uuid, signal: Signal) {
        if let Some(identity) = self
            .snapshot()
            .accounts
            .iter()
            .find(|a| a.id == id && a.generation == generation)
            .and_then(|a| a.process.clone())
        {
            self.observe(
                id,
                generation,
                &identity,
                Observation::DiskLog(crate::logs::LogEvent {
                    signal,
                    confirmed_terminal: signal != Signal::JoinRequestFailed,
                    code: None,
                    timestamp: None,
                }),
            );
            return;
        }
        // Synthetic state-machine tests only; production observations require an exact process.
        let _ = self.update(id, generation, |a| {
            apply_observation(
                a,
                Observation::DiskLog(crate::logs::LogEvent {
                    signal,
                    confirmed_terminal: signal != Signal::JoinRequestFailed,
                    code: None,
                    timestamp: None,
                }),
            )
        });
        if signal == Signal::Connected {
            self.finish_operation(id, generation, None);
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
fn effective_compatibility(s: &State) -> String {
    let mut text = s.compatibility.clone();
    if s.database.multi_instance_enabled != s.active_multi_instance {
        let suffix = if s.database.multi_instance_enabled {
            "restart RoLauncher to enable multiple instances"
        } else {
            "restart RoLauncher to disable multiple instances"
        };
        if !text.to_lowercase().contains("restart") {
            text = format!("{text}; {suffix}");
        }
    }
    if !s.launch_allowed
        && !text.to_lowercase().contains("blocked")
        && !text.to_lowercase().contains("failed")
        && !text.to_lowercase().contains("conflict")
    {
        text = format!("{text}; launch coordination blocked by an existing Roblox client");
    }
    text
}
fn snapshot(s: &State) -> Snapshot {
    Snapshot {
        external_background_cleanup_enabled: s.database.external_background_cleanup_enabled,
        multi_instance_enabled: s.database.multi_instance_enabled,
        active_multi_instance: s.active_multi_instance,
        server_selection: s.database.server_selection,
        join_type: s.database.join_type,
        game_profiles: s.database.game_profiles.clone(),
        close_to_tray: s.database.close_to_tray,
        accounts: s
            .database
            .accounts
            .iter()
            .map(|a| a.account.clone())
            .collect(),
        network_suspended: s.network_suspended,
        compatibility: effective_compatibility(s),
        profiles: s.database.profiles.clone(),
        update_repository: s.database.update_repository.clone(),
        include_beta_updates: s.database.include_beta_updates,
    }
}
fn apply_failure(a: &mut Account, error: &Failure) {
    a.close_intent = None;
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
        if error.kind == FailureKind::TargetUnavailable && a.process.is_some() {
            a.close_intent = Some(CloseIntent {
                id: Uuid::new_v4(),
                reason: CloseReason::TargetReplacement,
                generation: a.generation,
                operation_id: a.operation_id,
                denied: false,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_journal_correlates_commands_launches_signals_and_pause_without_secrets() {
        let (engine, path) = fixture(1);
        let op = engine.command("1", "start").unwrap();
        assert_eq!(engine.command("1", "start").unwrap().id, op.id);
        let mut account = engine.snapshot().accounts.remove(0);
        account.tracker = Some("123456789012".into());
        let target = Target {
            place_id: 1,
            job_id: None,
            private_server_link: Some("https://roblox.com/?code=SECRET_DESTINATION".into()),
        };
        let result = engine.submit_launch(&account, &target, || {
            let rows = engine.0.journal.recent();
            assert_eq!(rows.last().unwrap()["event"], "launch_submitted");
            Err("Unable to launch Roblox (Windows error 5) SECRET_TICKET".into())
        });
        assert!(result.is_err());
        engine.failure(
            "1",
            account.generation,
            Failure::new(FailureKind::Unsupported, "SECRET_FAILURE"),
        );
        let report = engine.diagnostics();
        let rows = report["recent_recovery_events"].as_array().unwrap();
        let commands: Vec<_> = rows
            .iter()
            .filter(|r| r["event"] == "command_result")
            .collect();
        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[0]["details"]["operation_id"],
            commands[1]["details"]["operation_id"]
        );
        let launch = rows.iter().find(|r| r["event"] == "launch_result").unwrap();
        assert_eq!(launch["details"]["account"]["tracker"], 123456789012u64);
        assert_eq!(
            launch["details"]["account"]["generation"],
            account.generation.to_string()
        );
        assert_eq!(launch["details"]["accepted_by_os"], false);
        assert_eq!(launch["details"]["error_code"], 5);
        assert!(rows.iter().any(|r| r["event"] == "account_state_changed"
            && r["details"]["after"]["status"] == "needs_attention"));
        assert!(!report.to_string().contains("SECRET"));
        assert!(!report.to_string().contains(engine.token()));
        assert!(engine.command("1", "retry").is_err());
        assert_eq!(
            engine
                .0
                .journal
                .recent()
                .iter()
                .filter(|r| r["event"] == "command_result" && r["details"]["action"] == "retry")
                .count(),
            1
        );
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn recovery_journal_keeps_disk_memory_and_stale_process_evidence_distinct() {
        let (engine, path, identity) = owned_fixture();
        engine.observe(
            "1",
            identity.generation,
            &identity,
            Observation::UnverifiedMemory(Signal::SessionLost(Some(273))),
        );
        engine.observe(
            "1",
            identity.generation,
            &identity,
            disk("[FLog::Network] Sending disconnect with reason: 273"),
        );
        let mut stale = identity.clone();
        stale.pid += 1;
        engine.observe(
            "1",
            identity.generation,
            &stale,
            disk("[FLog::Network] Connection accepted"),
        );
        engine.observe(
            "1",
            identity.generation,
            &identity,
            disk("[FLog::Network] Sending disconnect with reason: 524"),
        );
        let rows = engine.0.journal.recent();
        let signals: Vec<_> = rows
            .iter()
            .filter(|r| r["event"] == "connection_signal")
            .collect();
        assert_eq!(signals.len(), 4);
        assert_eq!(signals[0]["details"]["source"], "unverified_memory");
        assert_eq!(signals[0]["details"]["confirmed_terminal"], false);
        assert_eq!(signals[1]["details"]["source"], "owned_disk_log");
        assert_eq!(signals[1]["details"]["confirmed_terminal"], true);
        assert_eq!(signals[1]["details"]["signal"], "SessionLost(Some(273))");
        assert_eq!(signals[2]["details"]["ownership_matches"], false);
        assert_eq!(signals[3]["details"]["error_code"], 524);
        assert!(rows.iter().any(|r| r["event"] == "account_state_changed"
            && r["details"]["after"]["reason"] == "game_access_denied"));
        let sequences: Vec<_> = rows
            .iter()
            .map(|r| r["sequence"].as_u64().unwrap())
            .collect();
        assert!(sequences.windows(2).all(|w| w[0] < w[1]));
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    fn owned_fixture() -> (Engine, std::path::PathBuf, ProcessIdentity) {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        let identity = ProcessIdentity {
            pid: 4242,
            creation_time: "verified-fixture-start".into(),
            tracker: "fixture-tracker".into(),
            generation,
        };
        engine
            .update("1", generation, |a| {
                a.process = Some(identity.clone());
                a.status = Status::Running;
                a.connected_since = Some(Utc::now());
            })
            .unwrap();
        (engine, path, identity)
    }
    fn disk(line: &str) -> Observation {
        Observation::DiskLog(crate::logs::parse_event(line).unwrap())
    }
    #[test]
    #[ignore = "explicit recovery latency fixture; no live Roblox processes"]
    fn recovery_policy_latency_fixture() {
        use std::io::Write;
        fn summary(mut samples: Vec<f64>) -> serde_json::Value {
            samples.sort_by(f64::total_cmp);
            serde_json::json!({
                "samples": samples.len(), "p50_ms": samples[samples.len()/2],
                "p95_ms": samples[samples.len()*95/100], "max_ms": samples[samples.len()-1]
            })
        }
        let mut reports = Vec::new();
        for backlog in [false, true] {
            let (engine, path, identity) = owned_fixture();
            let log = path.join("latency.log");
            let mut detection = Vec::new();
            let mut scheduling = Vec::new();
            let mut controlled_close = Vec::new();
            for _ in 0..100 {
                engine
                    .update("1", identity.generation, |a| {
                        a.status = Status::Running;
                        a.disconnected_since = None;
                        a.failures = 0;
                        a.next_retry = None;
                        a.recovery_reason.clear();
                        a.recovery_evidence = RecoveryEvidence::Unavailable;
                        a.recovery_process = None;
                    })
                    .unwrap();
                std::fs::write(
                    &log,
                    if backlog {
                        "[FLog::Output] unrelated game output\n".repeat(100_000)
                    } else {
                        String::new()
                    },
                )
                .unwrap();
                let mut tail = crate::logs::Tail::new(log.clone(), false).unwrap();
                let mut writer = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
                writeln!(
                    writer,
                    "[FLog::Network] Sending disconnect with reason: 273"
                )
                .unwrap();
                writer.flush().unwrap();
                let start = Instant::now();
                let events = tail.read_events().unwrap();
                detection.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(events.len(), 1);
                let accepted = Instant::now();
                engine.observe(
                    "1",
                    identity.generation,
                    &identity,
                    Observation::DiskLog(events[0]),
                );
                engine.elapse_reconnect_grace("1", identity.generation);
                assert!(recovery_due(&engine.snapshot().accounts[0], Utc::now()));
                scheduling.push(accepted.elapsed().as_secs_f64() * 1000.0);
                assert!(
                    engine
                        .close_managed_with(
                            "1",
                            identity.generation,
                            &identity,
                            false,
                            |_, force| {
                                assert!(!force);
                                // Cancellation shares this lock through identity verification and close.
                                assert!(engine.0.state.try_lock().is_err());
                                controlled_close.push(accepted.elapsed().as_secs_f64() * 1000.0);
                                Ok(())
                            }
                        )
                        .unwrap()
                );
            }
            reports.push(serde_json::json!({
                "backlog_bytes": if backlog { 3_500_000 } else { 0 },
                "detection": summary(detection), "recovery_scheduling": summary(scheduling),
                "accepted_event_to_controlled_close": summary(controlled_close),
                "reconnect_grace_ms": 30000, "grace_elapsed_by_fixture": true
            }));
            drop(engine);
            std::fs::remove_dir_all(path).unwrap();
        }
        println!(
            "RECOVERY_LATENCY {}",
            serde_json::to_string(&reports).unwrap()
        );
    }
    #[test]
    fn policy_confirmed_events_wait_for_grace_then_verified_exit() {
        for line in [
            "[FLog::Network] Connection lost",
            "[DFLog::NetworkClient] ID_CONNECTION_LOST",
            "[FLog::GameJoinUtil] GameJoinFailed errorCode: 279",
            "[FLog::Network] Sending disconnect with reason: 273",
            "[FLog::Network] Sending disconnect with reason: 264",
            "[FLog::Network] Sending disconnect with reason: 267",
            "[FLog::GameJoinUtil] GameJoinFailed errorCode: 529",
        ] {
            let (engine, path, identity) = owned_fixture();
            engine.observe("1", identity.generation, &identity, disk(line));
            assert!(!recovery_due(&engine.snapshot().accounts[0], Utc::now()));
            engine.elapse_reconnect_grace("1", identity.generation);
            let accepted = engine.snapshot().accounts.remove(0);
            assert_eq!(
                accepted.recovery_evidence,
                RecoveryEvidence::ConfirmedTerminalDisconnect
            );
            assert!(recovery_due(&accepted, Utc::now()));
            let mut calls = 0;
            assert!(
                engine
                    .close_managed_with("1", identity.generation, &identity, false, |id, force| {
                        assert_eq!(id, &identity);
                        assert!(!force);
                        calls += 1;
                        Ok(())
                    })
                    .unwrap()
            );
            assert_eq!(calls, 1);
            let closing = engine.snapshot().accounts.remove(0);
            assert_eq!(closing.status, Status::Backoff);
            assert_eq!(closing.failures, 1);
            assert_eq!(closing.recovery_evidence, RecoveryEvidence::RecoveryStarted);
            assert!(!launch_ready(
                &closing,
                Utc::now() + ChronoDuration::days(1)
            ));
            let retry = closing.next_retry.unwrap();
            engine.confirmed_exit("1", identity.generation, &identity, false);
            let exited = engine.snapshot().accounts.remove(0);
            assert!(exited.process.is_none() && exited.tracker.is_none());
            assert_eq!(exited.failures, 1); // Exit must not count the same disconnect twice.
            assert_eq!(exited.next_retry, Some(retry));
            assert!(!launch_ready(&exited, retry - ChronoDuration::seconds(1)));
            assert!(launch_ready(&exited, retry));
            drop(engine);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
    #[test]
    fn policy_verified_exit_schedules_without_grace_and_preserves_pause_or_stop() {
        for enabled in [true, false] {
            let (engine, path, identity) = owned_fixture();
            engine
                .update("1", identity.generation, |a| a.auto_recovery = enabled)
                .unwrap();
            let before = Utc::now();
            engine.confirmed_exit("1", identity.generation, &identity, false);
            let account = engine.snapshot().accounts.remove(0);
            assert!(account.process.is_none());
            if enabled {
                assert_eq!(account.status, Status::Backoff);
                assert_eq!(account.failures, 1);
                let retry = account.next_retry.unwrap();
                assert!(retry >= before + ChronoDuration::seconds(10));
                assert!(retry <= Utc::now() + ChronoDuration::seconds(13));
            } else {
                assert_eq!(account.status, Status::Stopped);
                assert!(!account.desired_running);
            }
            drop(engine);
            std::fs::remove_dir_all(path).unwrap();
        }
        let (engine, path, identity) = owned_fixture();
        engine.observe(
            "1",
            identity.generation,
            &identity,
            disk("[FLog::GameJoinUtil] GameJoinFailed errorCode: 600"),
        );
        engine.confirmed_exit("1", identity.generation, &identity, true);
        assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);
        assert_eq!(engine.snapshot().accounts[0].recovery_reason, "Permission");
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn policy_unverified_ram_copies_never_authorize_close_or_cancel_disk_evidence() {
        let (engine, path, identity) = owned_fixture();
        for _ in 0..10 {
            for signal in [
                Signal::Disconnected,
                Signal::SessionLost(Some(273)),
                Signal::PermissionDenied,
                Signal::ClientUpdateRequired,
                Signal::Connected,
            ] {
                engine.observe(
                    "1",
                    identity.generation,
                    &identity,
                    Observation::UnverifiedMemory(signal),
                );
            }
        }
        let account = engine.snapshot().accounts.remove(0);
        assert_eq!(account.status, Status::Running);
        assert_eq!(
            account.recovery_evidence,
            RecoveryEvidence::SuspectedDisconnect
        );
        assert!(!recovery_due(
            &account,
            Utc::now() + ChronoDuration::days(1)
        ));
        assert!(
            !engine
                .close_managed_with("1", identity.generation, &identity, true, |_, _| panic!(
                    "RAM-only close"
                ))
                .unwrap()
        );
        assert_eq!(account.failures, 0);
        assert!(account.next_retry.is_none());
        engine.observe(
            "1",
            identity.generation,
            &identity,
            disk("[FLog::Network] Connection lost"),
        );
        engine.observe(
            "1",
            identity.generation,
            &identity,
            Observation::UnverifiedMemory(Signal::Connected),
        );
        engine.elapse_reconnect_grace("1", identity.generation);
        assert!(recovery_due(&engine.snapshot().accounts[0], Utc::now()));
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn policy_ambiguous_transport_http_and_missing_logs_never_become_terminal_on_timeout() {
        let (engine, path, identity) = owned_fixture();
        for line in [
            "[FLog::Network] Sending disconnect with reason: 0",
            "[FLog::Network] Sending disconnect with reason: 282",
            "[FLog::Network] Sending disconnect with reason: 20",
            "[FLog::Network] Sending disconnect with reason: 285",
            "[FLog::Network] Client:Disconnect",
            "[FLog::Network] ID_DISCONNECTION_NOTIFICATION",
            "[DFLog::RbxTransportDummyClient] Connection closed",
            "[FLog::HttpTraceError] HttpResponse(#1) status:503 Error url:{ \"https://gamejoin.roblox.com/v1/join-game\" }",
        ] {
            engine.observe("1", identity.generation, &identity, disk(line));
            assert!(
                !engine
                    .close_managed_with("1", identity.generation, &identity, true, |_, _| panic!(
                        "Unconfirmed close"
                    ))
                    .unwrap()
            );
        }
        engine
            .update("1", identity.generation, log_unavailable)
            .unwrap();
        let account = engine.snapshot().accounts.remove(0);
        assert_eq!(
            account.recovery_evidence,
            RecoveryEvidence::SuspectedDisconnect
        );
        assert!(!recovery_due(
            &account,
            Utc::now() + ChronoDuration::days(1)
        ));
        assert_eq!(account.failures, 0);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn policy_fresh_connected_disk_event_cancels_pending_and_force_close() {
        let (engine, path, identity) = owned_fixture();
        for already_started in [false, true] {
            engine.observe(
                "1",
                identity.generation,
                &identity,
                disk("[FLog::Network] Connection lost"),
            );
            if already_started {
                engine.elapse_reconnect_grace("1", identity.generation);
                assert!(
                    engine
                        .close_managed_with("1", identity.generation, &identity, false, |_, _| Ok(
                            ()
                        ))
                        .unwrap()
                );
            }
            engine.observe(
                "1",
                identity.generation,
                &identity,
                disk("[FLog::Network] Connection accepted"),
            );
            assert!(
                !engine
                    .close_managed_with("1", identity.generation, &identity, true, |_, _| panic!(
                        "Connected client closed"
                    ))
                    .unwrap()
            );
            assert_eq!(engine.snapshot().accounts[0].status, Status::Running);
            assert!(engine.snapshot().accounts[0].next_retry.is_none());
        }
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn policy_duplicates_schedule_one_retry_and_do_not_launch_a_second_client() {
        let (engine, path, identity) = owned_fixture();
        let event = disk("[FLog::Network] Sending disconnect with reason: 273");
        engine.observe("1", identity.generation, &identity, event);
        engine.elapse_reconnect_grace("1", identity.generation);
        engine
            .close_managed_with("1", identity.generation, &identity, false, |_, _| Ok(()))
            .unwrap();
        let initial = engine.snapshot().accounts.remove(0);
        for _ in 0..10 {
            engine.observe("1", identity.generation, &identity, event);
            engine.observe(
                "1",
                identity.generation,
                &identity,
                Observation::UnverifiedMemory(Signal::Disconnected),
            );
        }
        let duplicate = engine.snapshot().accounts.remove(0);
        assert_eq!(duplicate.failures, 1);
        assert_eq!(duplicate.next_retry, initial.next_retry);
        assert_eq!(duplicate.operation_id, initial.operation_id);
        assert!(!launch_ready(
            &duplicate,
            Utc::now() + ChronoDuration::days(1)
        ));
        engine.confirmed_exit("1", identity.generation, &identity, false);
        engine.observe("1", identity.generation, &identity, event); // Delayed old disk/RAM results are stale now.
        assert_eq!(engine.snapshot().accounts[0].failures, 1);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn policy_destination_fallback_closes_only_bound_client_and_counts_once() {
        for policy in [
            FallbackPolicy::AllowPublic,
            FallbackPolicy::Stay,
            FallbackPolicy::Pause,
        ] {
            let (engine, path, identity) = owned_fixture();
            engine
                .update("1", identity.generation, |a| a.fallback_policy = policy)
                .unwrap();
            let event = disk("[FLog::GameJoinUtil] GameJoinFailed errorCode: 528");
            engine.observe("1", identity.generation, &identity, event);
            let first = engine.snapshot().accounts.remove(0);
            engine.observe("1", identity.generation, &identity, event);
            assert_eq!(engine.snapshot().accounts[0].failures, first.failures);
            assert_eq!(engine.snapshot().accounts[0].next_retry, first.next_retry);
            let mut calls = 0;
            let closed = engine
                .close_managed_with("1", identity.generation, &identity, false, |_, _| {
                    calls += 1;
                    Ok(())
                })
                .unwrap();
            assert_eq!(closed, policy != FallbackPolicy::Pause);
            assert_eq!(calls, usize::from(closed));
            assert_eq!(
                first.public_fallback_active,
                policy == FallbackPolicy::AllowPublic
            );
            assert!(!launch_ready(&first, Utc::now() + ChronoDuration::days(1)));
            engine.observe(
                "1",
                identity.generation,
                &identity,
                disk("[FLog::Network] Connection accepted"),
            );
            assert!(
                !engine
                    .close_managed_with("1", identity.generation, &identity, true, |_, _| panic!(
                        "Cancelled fallback close"
                    ))
                    .unwrap()
            );
            drop(engine);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
    #[test]
    fn policy_inaccessible_logs_or_metadata_preserve_retry_and_attention() {
        let (engine, path, identity) = owned_fixture();
        engine.observe(
            "1",
            identity.generation,
            &identity,
            disk("[FLog::Network] Connection lost"),
        );
        engine
            .update("1", identity.generation, process_unavailable)
            .unwrap();
        engine.elapse_reconnect_grace("1", identity.generation);
        assert!(recovery_due(&engine.snapshot().accounts[0], Utc::now()));
        engine
            .close_managed_with("1", identity.generation, &identity, false, |_, _| Ok(()))
            .unwrap();
        let closing = engine.snapshot().accounts.remove(0);
        engine
            .update("1", identity.generation, log_unavailable)
            .unwrap();
        engine
            .update("1", identity.generation, process_unavailable)
            .unwrap();
        engine.confirmed_exit("1", identity.generation, &identity, false);
        let exited = engine.snapshot().accounts.remove(0);
        assert_eq!(exited.failures, 1);
        assert_eq!(exited.next_retry, closing.next_retry);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
        let (engine, path, identity) = owned_fixture();
        engine.observe(
            "1",
            identity.generation,
            &identity,
            disk("[FLog::GameJoinUtil] GameJoinFailed errorCode: 600"),
        );
        engine
            .update("1", identity.generation, log_unavailable)
            .unwrap();
        engine
            .update("1", identity.generation, process_unavailable)
            .unwrap();
        engine.observe(
            "1",
            identity.generation,
            &identity,
            disk("[FLog::Network] Connection accepted"),
        );
        assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);
        assert_eq!(engine.snapshot().accounts[0].recovery_reason, "Permission");
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn policy_saved_fallback_reason_plus_ram_hint_cannot_authorize_close() {
        let (engine, path, identity) = owned_fixture();
        engine
            .update("1", identity.generation, |a| {
                a.status = Status::Backoff;
                a.recovery_reason = "TargetUnavailable".into();
            })
            .unwrap();
        engine.observe(
            "1",
            identity.generation,
            &identity,
            Observation::UnverifiedMemory(Signal::Disconnected),
        );
        assert!(
            !engine
                .close_managed_with("1", identity.generation, &identity, true, |_, _| panic!(
                    "Persisted reason plus RAM close"
                ))
                .unwrap()
        );
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn policy_nonrecoverable_codes_stay_in_attention_and_cannot_be_bypassed_by_connected() {
        for code in [268, 280, 524, 533, 600, 773] {
            let (engine, path, identity) = owned_fixture();
            engine.observe(
                "1",
                identity.generation,
                &identity,
                disk(&format!(
                    "[FLog::GameJoinUtil] GameJoinFailed errorCode: {code}"
                )),
            );
            engine.observe(
                "1",
                identity.generation,
                &identity,
                disk("[FLog::Network] Connection accepted"),
            );
            let account = engine.snapshot().accounts.remove(0);
            assert_eq!(account.status, Status::NeedsAttention, "{code}");
            assert!(account.next_retry.is_none());
            assert!(!launch_ready(
                &account,
                Utc::now() + ChronoDuration::days(1)
            ));
            assert!(
                !engine
                    .close_managed_with("1", identity.generation, &identity, true, |_, _| panic!(
                        "Attention bypass"
                    ))
                    .unwrap()
            );
            assert_eq!(
                engine
                    .operation(account.operation_id.unwrap())
                    .unwrap()
                    .state,
                "failed"
            );
            drop(engine);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
    #[test]
    fn policy_stop_disable_shutdown_and_identity_changes_cancel_destructive_recovery() {
        for action in ["stop", "disable", "identity", "shutdown"] {
            let (engine, path, identity) = owned_fixture();
            let event = disk("[FLog::Network] Connection lost");
            let mut wrong = identity.clone();
            wrong.pid += 1;
            engine.observe("1", identity.generation, &wrong, event);
            let mut wrong = identity.clone();
            wrong.generation = Uuid::new_v4();
            engine.observe("1", identity.generation, &wrong, event);
            let mut wrong = identity.clone();
            wrong.creation_time.push('x');
            engine.observe("1", identity.generation, &wrong, event);
            let mut wrong = identity.clone();
            wrong.tracker.push('x');
            engine.observe("1", identity.generation, &wrong, event);
            engine.observe("1", Uuid::new_v4(), &identity, event);
            assert_eq!(engine.snapshot().accounts[0].status, Status::Running);
            engine.observe("1", identity.generation, &identity, event);
            engine.elapse_reconnect_grace("1", identity.generation);
            match action {
                "stop" => {
                    engine.command("1", "stop").unwrap();
                }
                "disable" => {
                    engine
                        .patch(
                            "1",
                            AccountPatch {
                                auto_recovery: Some(false),
                                ..Default::default()
                            },
                        )
                        .unwrap();
                }
                "identity" => {
                    engine
                        .update("1", identity.generation, |a| {
                            a.process.as_mut().unwrap().creation_time.push('x');
                        })
                        .unwrap();
                }
                _ => engine.shutdown(),
            }
            assert!(
                !engine
                    .close_managed_with("1", identity.generation, &identity, true, |_, _| panic!(
                        "Cancelled or mismatched close"
                    ))
                    .unwrap()
            );
            assert!(!launch_ready(
                &engine.snapshot().accounts[0],
                Utc::now() + ChronoDuration::days(1)
            ));
            drop(engine);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
    #[test]
    fn policy_rate_limit_deadline_and_failure_limit_survive_reconnect_grace() {
        let (engine, path, identity) = owned_fixture();
        let cooldown = Utc::now() + ChronoDuration::seconds(120);
        engine
            .update("1", identity.generation, |a| {
                a.recovery_reason = "RateLimit".into();
                a.next_retry = Some(cooldown);
            })
            .unwrap();
        engine.observe(
            "1",
            identity.generation,
            &identity,
            disk("[FLog::Network] Connection lost"),
        );
        engine.elapse_reconnect_grace("1", identity.generation);
        assert!(
            engine
                .close_managed_with("1", identity.generation, &identity, false, |_, _| Ok(()))
                .unwrap()
        );
        assert_eq!(engine.snapshot().accounts[0].next_retry, Some(cooldown));
        assert_eq!(engine.snapshot().accounts[0].recovery_reason, "RateLimit");
        engine
            .update("1", identity.generation, |a| {
                a.failures = 4;
                a.recovery_evidence = RecoveryEvidence::ConfirmedTerminalDisconnect;
                a.status = Status::Reconnecting;
                a.disconnected_since = Some(Utc::now() - ChronoDuration::seconds(31));
            })
            .unwrap();
        engine
            .close_managed_with("1", identity.generation, &identity, false, |_, _| Ok(()))
            .unwrap();
        let paused = engine.snapshot().accounts.remove(0);
        assert_eq!(paused.failures, 5);
        assert_eq!(paused.status, Status::NeedsAttention);
        assert!(paused.next_retry.is_none());
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn policy_runtime_evidence_is_not_trusted_after_reopen_or_exported() {
        let (engine, path, identity) = owned_fixture();
        engine.observe(
            "1",
            identity.generation,
            &identity,
            disk("[FLog::Network] Connection lost"),
        );
        let reopened = Engine::open(Store::new(path.clone()).unwrap(), "fixture".into()).unwrap();
        let account = reopened.snapshot().accounts.remove(0);
        assert_eq!(account.recovery_evidence, RecoveryEvidence::Unavailable);
        assert!(account.recovery_process.is_none());
        assert!(!recovery_due(
            &account,
            Utc::now() + ChronoDuration::days(1)
        ));
        let json = serde_json::to_string(&engine.snapshot()).unwrap();
        assert!(!json.contains("recovery_evidence") && !json.contains("recovery_process"));
        drop(reopened);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn confirmed_disconnect_waits_30_seconds_without_resetting_on_duplicates() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.signal("1", generation, Signal::Disconnected);
        let account = engine.snapshot().accounts.remove(0);
        let start = account.disconnected_since.unwrap();
        assert!(!recovery_due(&account, start));
        assert!(!recovery_due(&account, start + ChronoDuration::seconds(29)));
        assert!(recovery_due(&account, start + ChronoDuration::seconds(30)));
        engine.signal("1", generation, Signal::Disconnected);
        assert_eq!(
            engine.snapshot().accounts[0].disconnected_since,
            Some(start)
        );
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[tokio::test]
    async fn slow_discovery_never_blocks_ready_monitor_work() {
        let release = Arc::new(std::sync::Barrier::new(2));
        let worker_release = release.clone();
        let mut task = Some(tokio::task::spawn_blocking(move || {
            worker_release.wait();
            50
        }));
        let result =
            tokio::time::timeout(Duration::from_millis(100), completed_task(&mut task)).await;
        release.wait();
        assert!(matches!(result, Ok(None)));
        assert_eq!(task.take().unwrap().await.unwrap(), 50);
    }
    #[test]
    fn fifty_accounts_keep_confirmed_273_during_log_failure_and_cancel_on_reconnect() {
        let (engine, path) = fixture(50);
        for account in engine.snapshot().accounts {
            engine.command(&account.id, "start").unwrap();
            let generation = engine
                .snapshot()
                .accounts
                .into_iter()
                .find(|a| a.id == account.id)
                .unwrap()
                .generation;
            engine.signal(&account.id, generation, Signal::SessionLost(Some(273)));
            engine
                .update(&account.id, generation, log_unavailable)
                .unwrap();
            let a = engine
                .snapshot()
                .accounts
                .into_iter()
                .find(|a| a.id == account.id)
                .unwrap();
            assert_eq!(a.status, Status::Reconnecting);
            assert!(a.last_error.as_ref().unwrap().contains("273"));
            assert!(recovery_due(&a, Utc::now() + ChronoDuration::seconds(31)));
            engine.signal(&account.id, generation, Signal::Connected);
        }
        assert!(
            engine
                .snapshot()
                .accounts
                .iter()
                .all(|a| a.status == Status::Running && a.disconnected_since.is_none())
        );
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn instance_preference_defaults_on_and_changes_only_after_reopen() {
        let (engine, path) = fixture(1);
        assert!(
            engine.snapshot().multi_instance_enabled && engine.snapshot().active_multi_instance
        );
        engine.set_multi_instance(false).unwrap();
        assert!(
            !engine.snapshot().multi_instance_enabled && engine.snapshot().active_multi_instance
        );
        drop(engine);
        let reopened = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
        assert!(
            !reopened.snapshot().multi_instance_enabled
                && !reopened.snapshot().active_multi_instance
        );
        drop(reopened);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn explicit_stop_persists_close_intent_instead_of_inferring_it_from_state() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let a = engine.snapshot().accounts.remove(0);
        engine
            .update("1", a.generation, |a| {
                a.tracker = Some("123456789012".into());
            })
            .unwrap();
        engine.command("1", "stop").unwrap();
        let db = Store::new(path.clone()).unwrap().load().unwrap();
        let json = serde_json::to_value(&db).unwrap();
        assert_eq!(
            json["accounts"][0]["account"]["close_intent"]["reason"],
            "UserStop"
        );
        assert_eq!(
            json["retired_launches"][0]["close_intent"]["reason"],
            "UserStop"
        );
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
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
        engine.signal("1", generation, Signal::SessionLost(Some(273)));
        let count = engine.discord_settings().recent.len();
        assert_eq!(count, 1); // Confirmed recoverable disconnect is reported immediately.
        engine.signal("1", generation, Signal::ConnectionFailed);
        engine.signal("1", generation, Signal::SessionLost(None));
        engine.signal("1", generation, Signal::SessionLost(Some(273)));
        engine.signal("1", Uuid::new_v4(), Signal::SessionLost(Some(273)));
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
    fn discord_redirect_grace_reports_only_persistent_disconnects_once() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.signal("1", generation, Signal::Connected);
        engine.signal("1", generation, Signal::Disconnected);
        assert!(engine.discord_settings().recent.is_empty());
        engine.signal("1", generation, Signal::Connected);
        assert!(engine.discord_settings().recent.is_empty());
        engine.signal("1", generation, Signal::Disconnected);
        assert!(engine.discord_settings().recent.is_empty());
        engine.elapse_reconnect_grace("1", generation);
        assert_eq!(engine.discord_settings().recent.len(), 1);
        engine.signal("1", generation, Signal::Disconnected);
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
    fn recoverable_disconnect_waits_for_grace_and_connected_stop_or_disable_cancel() {
        let (engine, path) = fixture(1);
        engine.command("1", "start").unwrap();
        let generation = engine.snapshot().accounts[0].generation;
        engine.signal("1", generation, Signal::SessionLost(Some(273)));
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
        engine.signal("1", generation, Signal::SessionLost(Some(273)));
        assert_eq!(engine.snapshot().accounts[0].status, Status::Stopped);
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn connection_failure_preserves_target_and_stop() {
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
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn unknown_client_distinguishes_join_failure_from_http_request_error() {
        for (line, terminal) in [
            ("[FLog::GameJoinUtil] GameJoinFailed errorCode: 529", true),
            (
                "[DFLog::HttpTraceError] HttpResponse(#19) error:5 message:HttpError: Timedout url:{ \"https://gamejoin.roblox.com/v1/join-game-instance\" }",
                false,
            ),
            (
                "[FLog::HttpTraceError] HttpResponse(#1) status:503 Service Unavailable url:{ \"https://gamejoin.roblox.com/v1/join-private-game\" }",
                false,
            ),
        ] {
            let (engine, path, identity) = owned_fixture();
            let generation = identity.generation;
            engine
                .update("1", generation, |a| a.status = Status::Unknown)
                .unwrap();
            let before = engine.snapshot().accounts.remove(0);
            let log = path.join("Player.log");
            std::fs::write(&log, format!("{line}\n")).unwrap();
            let mut tail = crate::logs::Tail::new(log, false).unwrap();
            let events = tail.read_events().unwrap();
            assert_eq!(events.len(), 1);
            engine.observe("1", generation, &identity, Observation::DiskLog(events[0]));
            let failed = engine.snapshot().accounts.remove(0);
            assert_eq!(
                failed.status,
                if terminal {
                    Status::Reconnecting
                } else {
                    Status::Unknown
                }
            );
            assert!(!failed.last_error.as_deref().unwrap().contains("roblox.com"));
            assert_eq!(failed.target, before.target);
            assert!(!failed.public_fallback_active);
            assert_eq!(failed.failures, 0);
            assert!(!recovery_due(&failed, Utc::now()));
            assert_eq!(
                recovery_due(&failed, Utc::now() + ChronoDuration::seconds(30)),
                terminal
            );
            engine.observe("1", generation, &identity, Observation::DiskLog(events[0]));
            assert_eq!(
                engine.snapshot().accounts[0].disconnected_since,
                failed.disconnected_since
            );
            engine
                .update("1", generation, |a| a.auto_recovery = false)
                .unwrap();
            assert!(!recovery_due(
                &engine.snapshot().accounts[0],
                Utc::now() + ChronoDuration::seconds(31)
            ));
            engine
                .update("1", generation, |a| a.auto_recovery = true)
                .unwrap();
            engine.observe(
                "1",
                generation,
                &identity,
                disk("[FLog::Network] Connection accepted"),
            );
            assert_eq!(engine.snapshot().accounts[0].status, Status::Running);
            assert!(!recovery_due(
                &engine.snapshot().accounts[0],
                Utc::now() + ChronoDuration::seconds(31)
            ));
            engine.command("1", "stop").unwrap();
            engine.observe("1", generation, &identity, Observation::DiskLog(events[0]));
            assert_eq!(engine.snapshot().accounts[0].status, Status::Stopping);
            drop(engine);
            std::fs::remove_dir_all(path).unwrap();
        }
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
        engine.signal("1", generation, Signal::SessionLost(Some(524)));
        let count = engine.activity(None).len();
        engine.signal("1", generation, Signal::SessionLost(Some(524)));
        assert_eq!(engine.activity(None).len(), count);
        assert!(
            engine.activity(Some("1"))[0]
                .message
                .contains("Access denied")
        );
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
    pub(super) fn fixture(count: usize) -> (Engine, std::path::PathBuf) {
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
    #[tokio::test]
    async fn friend_join_uses_managed_peer_session_when_viewer_presence_is_hidden() {
        use axum::{
            Json, Router,
            http::{HeaderMap, StatusCode},
            routing::post,
        };
        let place = Arc::new(AtomicU64::new(1));
        let requests = Arc::new(AtomicU64::new(0));
        let app = Router::new().route(
            "/presence",
            post({
                let place = place.clone();
                let requests = requests.clone();
                move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                    let place = place.clone();
                    let requests = requests.clone();
                    async move {
                        requests.fetch_add(1, Ordering::Relaxed);
                        assert_eq!(body["userIds"], serde_json::json!([2]));
                        let own = headers["cookie"] == ".ROBLOSECURITY=PEER_TEST_SESSION";
                        if own && place.load(Ordering::Relaxed) == 0 {
                            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({})));
                        }
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({"userPresences": [{
                                "userId": 2, "userPresenceType": if own { 2 } else { 0 },
                                "placeId": own.then(|| place.load(Ordering::Relaxed)),
                                "gameId": own.then_some("00000000-0000-0000-0000-000000000002")
                            }]})),
                        )
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/presence", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (mut engine, path) = fixture(2);
        Arc::get_mut(&mut engine.0).unwrap().roblox.presence_url = Some(url);
        engine
            .transaction(|s| {
                s.database.join_type = JoinType::Friend;
                let profile = Uuid::new_v4();
                for saved in &mut s.database.accounts {
                    saved.account.game_profile_id = Some(profile);
                    saved.account.desired_running = true;
                }
                let peer = &mut s.database.accounts[1];
                peer.encrypted_session = platform::protect("PEER_TEST_SESSION")?;
                peer.account.status = Status::Running;
                peer.account.tracker = Some("peer-tracker".into());
                Ok(())
            })
            .unwrap();
        let account = engine.snapshot().accounts[0].clone();
        let target = account.effective_target().unwrap();
        let selected = engine
            .select_launch_target(&account, target.clone(), "JOINING_TEST_SESSION")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(selected.job_id.unwrap().as_u128(), 2);
        assert_eq!(requests.load(Ordering::Relaxed), 2);

        // A peer in another place must not pull the follower out of its configured game.
        place.store(99, Ordering::Relaxed);
        let error = engine
            .select_launch_target(&account, target.clone(), "JOINING_TEST_SESSION")
            .await
            .unwrap_err();
        assert!(error.message.contains("Same-preset server"));
        engine
            .transaction(|s| {
                s.database.accounts[1].account.ownership_deadline =
                    Some(Utc::now() + ChronoDuration::seconds(90));
                Ok(())
            })
            .unwrap();
        assert!(
            engine
                .select_launch_target(&account, target.clone(), "JOINING_TEST_SESSION")
                .await
                .unwrap()
                .is_none()
        );
        // An expired peer session must not request reauthentication of the follower.
        engine
            .transaction(|s| {
                s.database.accounts[1].account.ownership_deadline = None;
                Ok(())
            })
            .unwrap();
        place.store(0, Ordering::Relaxed);
        let error = engine
            .select_launch_target(&account, target.clone(), "JOINING_TEST_SESSION")
            .await
            .unwrap_err();
        assert_eq!(error.kind, FailureKind::Other);
        // Unreadable peer credentials must not blame the joining account's valid session.
        engine
            .transaction(|s| {
                s.database.accounts[1].account.ownership_deadline = None;
                s.database.accounts[1].encrypted_session = "unreadable".into();
                Ok(())
            })
            .unwrap();
        let error = engine
            .select_launch_target(&account, target, "JOINING_TEST_SESSION")
            .await
            .unwrap_err();
        assert_eq!(error.kind, FailureKind::Other);
        server.abort();
        engine.shutdown();
        drop(engine);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[tokio::test]
    async fn saved_game_identity_survives_presets_and_explicit_destinations_bypass_selection() {
        let (engine, path) = fixture(2);
        engine
            .transaction(|s| {
                for saved in &mut s.database.accounts {
                    saved.encrypted_session = platform::protect("TEST_ONLY_SESSION")?;
                }
                Ok(())
            })
            .unwrap();
        let target = Target {
            place_id: 1818,
            job_id: None,
            private_server_link: None,
        };
        let game = GameProfile {
            id: Uuid::new_v4(),
            name: "Team game".into(),
            game_name: "Game".into(),
            thumbnail_url: None,
            target: target.clone(),
        };
        engine.save_game_profile(game.clone()).unwrap();
        let assign = AccountPatch {
            target: Some(target.clone()),
            game_profile_id: Some(game.id),
            ..Default::default()
        };
        engine
            .bulk_patch(BulkPatch {
                account_ids: vec!["1".into(), "2".into()],
                patch: assign.clone(),
            })
            .unwrap();
        let preset = engine
            .save_profile(ProfileSave {
                name: "Team".into(),
                account_ids: vec!["1".into(), "2".into()],
                id: None,
            })
            .unwrap();
        assert_eq!(preset.entries[0].game_profile_id, Some(game.id));
        engine
            .patch(
                "1",
                AccountPatch {
                    target: Some(target.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(engine.snapshot().accounts[0].game_profile_id.is_none());
        assert_eq!(
            engine.apply_profile(preset.id).unwrap()[0].game_profile_id,
            Some(game.id)
        );
        let mut invalid = assign.clone();
        invalid.game_profile_id = Some(Uuid::new_v4());
        assert!(
            engine
                .bulk_patch(BulkPatch {
                    account_ids: vec!["1".into(), "2".into()],
                    patch: invalid
                })
                .is_err()
        );
        let mut invalid = assign;
        invalid.target.as_mut().unwrap().place_id = 99;
        assert!(engine.patch("1", invalid).is_err());
        engine
            .set_game_settings(ServerSelection::Highest, JoinType::Friend)
            .unwrap();
        let account = engine.snapshot().accounts[0].clone();
        for explicit in [
            Target {
                job_id: Some(Uuid::new_v4()),
                ..target.clone()
            },
            Target {
                private_server_link: Some(
                    "https://www.roblox.com/games/1818?privateServerLinkCode=abc".into(),
                ),
                ..target.clone()
            },
        ] {
            assert_eq!(
                engine
                    .select_launch_target(&account, explicit.clone(), "invalid-test-cookie")
                    .await
                    .unwrap(),
                Some(explicit)
            );
        }
        let backup = engine.backup().unwrap();
        engine.delete_game_profile(game.id).unwrap();
        assert!(
            engine
                .snapshot()
                .accounts
                .iter()
                .all(|a| a.game_profile_id.is_none())
        );
        engine.restore(&backup).unwrap();
        assert_eq!(engine.snapshot().accounts[0].game_profile_id, Some(game.id));
        drop(engine);
        let reopened = Engine::open(Store::new(path.clone()).unwrap(), "simulated".into()).unwrap();
        assert_eq!(
            reopened.snapshot().accounts[1].game_profile_id,
            Some(game.id)
        );
        assert_eq!(reopened.snapshot().join_type, JoinType::Friend);
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
