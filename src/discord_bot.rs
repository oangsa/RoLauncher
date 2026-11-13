//! Opt-in Discord commands. Serenity owns gateway reconnects and REST rate limits.
use crate::{
    engine::Engine,
    model::{Account, Status},
    resources::{
        AccountResourceResponse, ManagedInstanceResource, MetricAvailability, ProcessAvailability,
        ResourceSnapshot, format_bytes, format_uptime,
    },
};
use serde::{Deserialize, Serialize};
use serenity::{all::*, async_trait};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use zeroize::Zeroize;

const RESOURCES_PAGE_SIZE: usize = 8;
const RESOURCES_REFRESH_INTERVAL: Duration = Duration::from_secs(30);
const BOT_READY_NOTICE: &str =
    "Connected. /action, /status-register, /resources and /resources-register are ready.";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusRegistration {
    pub guild_id: String,
    pub channel_id: u64,
    pub message_id: u64,
    pub page: usize,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedBot {
    pub enabled: bool,
    pub encrypted_token: String,
    pub guild_id: String,
    pub allowed_users: Vec<String>,
    pub registration: Option<StatusRegistration>,
    pub resources_registration: Option<StatusRegistration>,
}
impl SavedBot {
    /// Strip transient message registrations so config equality checks stay aligned
    /// across `/status-register`, `/resources-register`, and `/action`.
    pub(crate) fn without_registrations(&self) -> Self {
        let mut stripped = self.clone();
        stripped.registration = None;
        stripped.resources_registration = None;
        stripped
    }
}
#[derive(Default, Deserialize)]
pub struct BotPatch {
    pub enabled: Option<bool>,
    pub token: Option<String>,
    pub guild_id: Option<String>,
    pub allowed_users: Option<Vec<String>>,
}
impl Drop for BotPatch {
    fn drop(&mut self) {
        if let Some(token) = &mut self.token {
            token.zeroize();
        }
    }
}
fn snowflake(value: &str) -> bool {
    value.parse::<u64>().is_ok_and(|n| n > 0) && value.bytes().all(|b| b.is_ascii_digit())
}
impl BotPatch {
    pub fn apply(&self, old: &SavedBot) -> Result<SavedBot, String> {
        let mut next = old.clone();
        if let Some(enabled) = self.enabled {
            next.enabled = enabled;
        }
        if let Some(token) = &self.token {
            let token = token.trim();
            if token.len() > 256 || token.chars().any(char::is_whitespace) {
                return Err("Invalid Discord bot token".into());
            }
            next.encrypted_token = if token.is_empty() {
                String::new()
            } else {
                crate::platform::protect(token)?
            };
        }
        if let Some(guild) = &self.guild_id {
            next.guild_id = guild.trim().into();
        }
        if let Some(users) = &self.allowed_users {
            next.allowed_users = users.iter().map(|u| u.trim().to_owned()).collect();
        }
        if next.allowed_users.len() > 100
            || next.allowed_users.iter().any(|id| !snowflake(id))
            || (!next.guild_id.is_empty() && !snowflake(&next.guild_id))
        {
            return Err("Use numeric Discord server and user IDs (up to 100 users)".into());
        }
        if next.enabled
            && (next.encrypted_token.is_empty()
                || !snowflake(&next.guild_id)
                || next.allowed_users.is_empty())
        {
            return Err("Save a bot token, server ID and at least one allowed user before enabling commands".into());
        }
        if next.guild_id != old.guild_id || next.encrypted_token != old.encrypted_token {
            next.registration = None;
            next.resources_registration = None;
        }
        Ok(next)
    }
}
#[derive(Clone, Serialize)]
pub struct BotView {
    enabled: bool,
    configured: bool,
    guild_id: String,
    allowed_users: Vec<String>,
    status: String,
    pub(crate) connection_state: String,
}
impl BotView {
    pub fn new(saved: &SavedBot, status: String, connection_state: String) -> Self {
        Self {
            enabled: saved.enabled,
            configured: !saved.encrypted_token.is_empty(),
            guild_id: saved.guild_id.clone(),
            allowed_users: saved.allowed_users.clone(),
            status,
            connection_state,
        }
    }
}

fn authorized(settings: &SavedBot, guild: Option<GuildId>, user: UserId) -> bool {
    settings.enabled
        && guild.is_some_and(|g| g.to_string() == settings.guild_id)
        && settings.allowed_users.contains(&user.to_string())
}
fn commands() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new("action")
            .description("Control a RoLauncher account")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "username", "Account username")
                    .required(true)
                    .set_autocomplete(true),
            )
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "action", "Action to perform")
                    .required(true)
                    .add_string_choice("Stop", "stop")
                    .add_string_choice("Restart", "restart"),
            ),
        CreateCommand::new("status-register")
            .description("Register a live account status message, ten per page")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::Channel,
                    "channel",
                    "Text channel for the account list",
                )
                .required(true)
                .channel_types(vec![ChannelType::Text]),
            ),
        CreateCommand::new("resources")
            .description("Show read-only host and managed Roblox resource usage")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "account",
                    "Optional account to inspect in detail",
                )
                .required(false)
                .set_autocomplete(true),
            ),
        CreateCommand::new("resources-register")
            .description("Register a live resource monitor message, updated automatically")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::Channel,
                    "channel",
                    "Text channel for the live resource monitor",
                )
                .required(true)
                .channel_types(vec![ChannelType::Text]),
            ),
    ]
}
fn status_label(status: &Status) -> &'static str {
    match status {
        Status::Running => "🟢 Online",
        Status::Stopped => "⚪ Stopped",
        Status::Stopping => "⚪ Stopping",
        Status::Queued => "🟡 Queued",
        Status::Launching => "🟡 Joining",
        Status::Reconnecting => "🟡 Reconnecting",
        Status::Backoff => "🟡 Rejoin queued",
        Status::NeedsAttention => "🔴 Error",
        Status::Unknown => "🔴 Status unknown",
    }
}
fn status_page(mut accounts: Vec<Account>, page: usize) -> (CreateEmbed, Vec<CreateActionRow>) {
    accounts.sort_by(|a, b| {
        a.username
            .to_lowercase()
            .cmp(&b.username.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    let pages = accounts.len().div_ceil(10).max(1);
    let page = page.min(pages - 1);
    let text = accounts
        .iter()
        .skip(page * 10)
        .take(10)
        .map(|a| {
            format!(
                "{} — {}",
                crate::discord::escape(&a.username, 40),
                status_label(&a.status)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let embed = CreateEmbed::new()
        .title("RoLauncher accounts")
        .description(if text.is_empty() {
            "No saved accounts".into()
        } else {
            text
        })
        .footer(CreateEmbedFooter::new(format!(
            "Page {} / {pages} · {} accounts · updates automatically",
            page + 1,
            accounts.len()
        )));
    let buttons = vec![CreateActionRow::Buttons(vec![
        CreateButton::new(format!("rlstatus:previous:{}", page.saturating_sub(1)))
            .label("Previous")
            .disabled(page == 0),
        CreateButton::new(format!("rlstatus:refresh:{page}")).label("Refresh"),
        CreateButton::new(format!("rlstatus:next:{}", page + 1))
            .label("Next")
            .disabled(page + 1 == pages),
    ])];
    (embed, buttons)
}
fn status_button_page(id: &str) -> Option<usize> {
    let value = id.strip_prefix("rlstatus:")?;
    let page = if let Some((action, page)) = value.split_once(':') {
        if !matches!(action, "previous" | "refresh" | "next") {
            return None;
        }
        page
    } else {
        // Accept buttons on messages posted by earlier development builds.
        value
    };
    page.parse().ok()
}
fn resources_button_page(id: &str) -> Option<usize> {
    let value = id.strip_prefix("rlres:")?;
    let (action, page) = value.split_once(':')?;
    if !matches!(action, "previous" | "refresh" | "next") {
        return None;
    }
    page.parse().ok()
}
fn resolve_account<'a>(accounts: &'a [Account], query: &str) -> Option<&'a Account> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_lowercase();
    accounts
        .iter()
        .find(|a| a.id == trimmed)
        .or_else(|| accounts.iter().find(|a| a.username.to_lowercase() == lower))
        .or_else(|| {
            accounts
                .iter()
                .find(|a| !a.alias.trim().is_empty() && a.alias.to_lowercase() == lower)
        })
}
fn account_autocomplete_label(account: &Account) -> String {
    let label = if account.alias.trim().is_empty() || account.alias.trim() == account.username {
        account.username.clone()
    } else {
        format!("{} ({})", account.alias.trim(), account.username)
    };
    label.chars().take(100).collect()
}
fn format_cpu_metric(value: Option<f64>, avail: MetricAvailability) -> String {
    match (avail, value) {
        (MetricAvailability::Available, Some(v)) => format!("{v:.1}%"),
        (MetricAvailability::Pending, _) => "Pending".into(),
        _ => "Unavailable".into(),
    }
}
fn format_system_ram(snap: &ResourceSnapshot) -> String {
    match (
        snap.system.ram_availability,
        snap.system.used_ram_bytes,
        snap.system.total_ram_bytes,
        snap.system.ram_utilization_percent,
    ) {
        (MetricAvailability::Available, Some(used), Some(total), Some(pct)) => {
            format!(
                "{} / {} ({pct:.1}%)",
                format_bytes(used),
                format_bytes(total)
            )
        }
        (MetricAvailability::Pending, _, _, _) => "Pending".into(),
        _ => "Unavailable".into(),
    }
}
fn process_availability_label(avail: ProcessAvailability) -> &'static str {
    match avail {
        ProcessAvailability::Available => "Live",
        ProcessAvailability::Pending => "Sampling",
        ProcessAvailability::Exited => "Exited",
        ProcessAvailability::Inaccessible => "Inaccessible",
        ProcessAvailability::OwnershipMismatch => "Identity mismatch",
        ProcessAvailability::Unavailable => "Unavailable",
    }
}
fn snapshot_availability_label(snap: &ResourceSnapshot) -> &'static str {
    if snap.stale {
        return "Stale";
    }
    match snap.availability {
        crate::resources::SnapshotAvailability::Available => "Live",
        crate::resources::SnapshotAvailability::Partial => "Partial",
        crate::resources::SnapshotAvailability::Pending => "Sampling",
        crate::resources::SnapshotAvailability::Unavailable => "Unavailable",
        crate::resources::SnapshotAvailability::Stale => "Stale",
    }
}
fn resources_visible_rows(snap: &ResourceSnapshot) -> Vec<&ManagedInstanceResource> {
    snap.accounts
        .iter()
        .filter(|row| {
            row.pid.is_some()
                || row.account_status != Status::Stopped
                || matches!(
                    row.availability,
                    ProcessAvailability::Available
                        | ProcessAvailability::Pending
                        | ProcessAvailability::Inaccessible
                        | ProcessAvailability::OwnershipMismatch
                )
        })
        .collect()
}
fn format_instance_line(row: &ManagedInstanceResource) -> String {
    let name = crate::discord::escape(&row.display_name, 28);
    let state = status_label(&row.account_status);
    let pid = row
        .pid
        .map(|p| format!("PID {p}"))
        .unwrap_or_else(|| "PID —".into());
    match row.availability {
        ProcessAvailability::Available => {
            let cpu = row
                .cpu_percent
                .map(|c| format!("{c:.1}%"))
                .unwrap_or_else(|| "—".into());
            let ram = row
                .working_set_bytes
                .map(format_bytes)
                .unwrap_or_else(|| "—".into());
            let gpu = match (row.gpu_availability, row.gpu_percent) {
                (MetricAvailability::Available, Some(g)) => format!(" · GPU {g:.1}%"),
                _ => String::new(),
            };
            let up = row
                .uptime_seconds
                .map(|u| format!(" · {}", format_uptime(u)))
                .unwrap_or_default();
            format!("• **{name}** ({state}, {pid}) — CPU {cpu} · RAM {ram}{gpu}{up}")
        }
        ProcessAvailability::Pending => {
            format!("• **{name}** ({state}, {pid}) — Sampling initial baseline…")
        }
        other => {
            format!(
                "• **{name}** ({state}, {pid}) — {}",
                process_availability_label(other)
            )
        }
    }
}
fn truncate_embed_description(mut combined: String) -> String {
    if combined.len() > 3800 {
        let mut end = 3797.min(combined.len());
        while end > 0 && !combined.is_char_boundary(end) {
            end -= 1;
        }
        combined.truncate(end);
        combined.push_str("...");
    }
    combined
}
fn resources_page(snap: &ResourceSnapshot, page: usize) -> (CreateEmbed, Vec<CreateActionRow>) {
    let visible = resources_visible_rows(snap);
    let pages = visible.len().div_ceil(RESOURCES_PAGE_SIZE).max(1);
    let page = page.min(pages - 1);

    let sys_cpu = format_cpu_metric(snap.system.cpu_percent, snap.system.cpu_availability);
    let sys_ram = format_system_ram(snap);
    let sys_gpu = match (snap.system.gpu_availability, snap.system.gpu_percent) {
        (MetricAvailability::Available, Some(v)) => format!(" · GPU {v:.1}%"),
        (MetricAvailability::Pending, _) => " · GPU Pending".into(),
        _ => String::new(),
    };
    let managed_cpu = format_cpu_metric(
        if snap.aggregates.cpu_availability == MetricAvailability::Available {
            Some(snap.aggregates.cpu_percent)
        } else {
            None
        },
        snap.aggregates.cpu_availability,
    );
    let managed_ram = format_bytes(snap.aggregates.working_set_bytes);
    let managed_gpu = match (
        snap.aggregates.gpu_availability,
        snap.aggregates.gpu_percent,
    ) {
        (MetricAvailability::Available, Some(v)) => format!(" · GPU {v:.1}%"),
        _ => String::new(),
    };

    let mut summary = vec![
        format!("**System:** CPU {sys_cpu} · RAM {sys_ram}{sys_gpu}"),
        format!(
            "**Managed Roblox ({}/{}):** CPU {managed_cpu} · RAM {managed_ram}{managed_gpu}",
            snap.aggregates.running_instances, snap.aggregates.total_accounts
        ),
    ];
    if let Some(unmanaged) = snap
        .aggregates
        .unmanaged_roblox_processes
        .filter(|&n| n > 0)
    {
        summary.push(format!("**Unmanaged Roblox:** {unmanaged} external"));
    }
    if snap.aggregates.exited_or_stale_instances > 0 {
        summary.push(format!(
            "**Attention:** {} exited, inaccessible, or mismatched",
            snap.aggregates.exited_or_stale_instances
        ));
    }

    let instance_lines = visible
        .iter()
        .skip(page * RESOURCES_PAGE_SIZE)
        .take(RESOURCES_PAGE_SIZE)
        .map(|row| format_instance_line(row))
        .collect::<Vec<_>>();
    let body = if instance_lines.is_empty() {
        format!(
            "{}\n\nNo managed Roblox instances are currently running.",
            summary.join("\n")
        )
    } else {
        truncate_embed_description(format!(
            "{}\n\n{}",
            summary.join("\n"),
            instance_lines.join("\n")
        ))
    };

    let ts = snap.timestamp.format("%Y-%m-%d %H:%M:%S UTC");
    let state_label = snapshot_availability_label(snap);
    let stale_note = if snap.stale {
        format!(" · {}s old", snap.sample_age_ms / 1000)
    } else {
        String::new()
    };
    let footer = format!(
        "Page {} / {pages} · {ts} · {state_label}{stale_note} · CPU machine %",
        page + 1
    );

    let embed = CreateEmbed::new()
        .title("RoLauncher Resources")
        .description(body)
        .footer(CreateEmbedFooter::new(footer));
    let buttons = vec![CreateActionRow::Buttons(vec![
        CreateButton::new(format!("rlres:previous:{}", page.saturating_sub(1)))
            .label("Previous")
            .disabled(page == 0),
        CreateButton::new(format!("rlres:refresh:{page}")).label("Refresh"),
        CreateButton::new(format!("rlres:next:{}", page + 1))
            .label("Next")
            .disabled(page + 1 == pages),
    ])];
    (embed, buttons)
}
fn account_resource_embed(resp: &AccountResourceResponse) -> CreateEmbed {
    let acc = &resp.account;
    let display = crate::discord::escape(&acc.display_name, 60);
    let user = crate::discord::escape(&acc.username, 40);
    let title = if acc.display_name == acc.username {
        format!("Resources — {display}")
    } else {
        format!("Resources — {display} ({user})")
    };
    let pid = acc.pid.map(|p| p.to_string()).unwrap_or_else(|| "—".into());
    let uptime = acc
        .uptime_seconds
        .map(format_uptime)
        .unwrap_or_else(|| "—".into());
    let cpu = match acc.availability {
        ProcessAvailability::Available => acc
            .cpu_percent
            .map(|c| format!("{c:.1}% (machine)"))
            .unwrap_or_else(|| "Pending".into()),
        ProcessAvailability::Pending => "Sampling".into(),
        _ => "Unavailable".into(),
    };
    let working_set = acc
        .working_set_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "Unavailable".into());
    let private_mem = acc
        .private_memory_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "Unavailable".into());
    let gpu = match (acc.gpu_availability, acc.gpu_percent) {
        (MetricAvailability::Available, Some(g)) => format!("{g:.1}%"),
        (MetricAvailability::Pending, _) => "Pending".into(),
        _ => "Unavailable".into(),
    };
    let ts = resp.timestamp.format("%Y-%m-%d %H:%M:%S UTC");
    let freshness = if resp.stale {
        format!("Stale ({}s old)", resp.sample_age_ms / 1000)
    } else {
        format!("Fresh ({} ms)", resp.sample_age_ms)
    };
    let lines = [
        format!("**Status:** {}", status_label(&acc.account_status)),
        format!(
            "**Sample:** {}",
            process_availability_label(acc.availability)
        ),
        format!("**PID:** `{pid}` · **Uptime:** {uptime}"),
        format!("**CPU:** {cpu}"),
        format!("**Physical RAM:** {working_set} · **Private:** {private_mem}"),
        format!("**GPU:** {gpu}"),
    ];
    CreateEmbed::new()
        .title(title)
        .description(lines.join("\n"))
        .footer(CreateEmbedFooter::new(format!("{ts} · {freshness}")))
}
fn resources_change_fingerprint(snap: &ResourceSnapshot, page: usize) -> String {
    let visible = resources_visible_rows(snap);
    let pages = visible.len().div_ceil(RESOURCES_PAGE_SIZE).max(1);
    let page = page.min(pages - 1);
    let bucket_pct = |v: Option<f64>| v.map(|n| (n / 2.0).round() as i64);
    let bucket_mib = |v: Option<u64>| v.map(|b| b / (4 * 1024 * 1024));
    let rows_key = visible
        .iter()
        .skip(page * RESOURCES_PAGE_SIZE)
        .take(RESOURCES_PAGE_SIZE)
        .map(|row| {
            format!(
                "{}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}:{:?}",
                row.account_id,
                row.availability,
                row.account_status,
                row.pid,
                bucket_pct(row.cpu_percent),
                bucket_mib(row.working_set_bytes),
                bucket_pct(row.gpu_percent),
                row.uptime_seconds.map(|s| s / 60),
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    format!(
        "p={page}/{pages};av={};rc={}/{};sc={:?}/{:?};sr={:?};sg={:?}/{:?};mc={}/{:?};mr={};mg={:?}/{:?};ex={};rows={rows_key}",
        snapshot_availability_label(snap),
        snap.aggregates.running_instances,
        snap.aggregates.total_accounts,
        snap.system.cpu_availability,
        bucket_pct(snap.system.cpu_percent),
        bucket_pct(snap.system.ram_utilization_percent),
        snap.system.gpu_availability,
        bucket_pct(snap.system.gpu_percent),
        (snap.aggregates.cpu_percent / 2.0).round() as i64,
        snap.aggregates.cpu_availability,
        snap.aggregates.working_set_bytes / (4 * 1024 * 1024),
        snap.aggregates.gpu_availability,
        bucket_pct(snap.aggregates.gpu_percent),
        snap.aggregates.exited_or_stale_instances,
    )
}
fn invalid_registration_error(error: &Error) -> bool {
    matches!(
        error,
        Error::Http(HttpError::UnsuccessfulRequest(r))
            if matches!(r.error.code, 10003 | 10008 | 50005)
    )
}
fn discord_failure(error: &Error) -> String {
    if let Error::Http(HttpError::UnsuccessfulRequest(response)) = error {
        discord_http_failure(response.status_code.as_u16(), response.error.code)
    } else {
        "Could not reach Discord or process its response. Check your connection and try again; saved settings are retained.".into()
    }
}
fn discord_http_failure(status: u16, code: isize) -> String {
    let advice = match code {
        50001 => {
            "The bot cannot access that channel. Check channel and category permission overrides."
        }
        50013 => {
            "The bot lacks channel permissions. Allow View Channel, Send Messages and Embed Links, including category overrides."
        }
        50035 => "Discord rejected the status message format. Update RoLauncher and try again.",
        10003 => "That channel no longer exists. Choose another text channel.",
        10008 => {
            "The registered message no longer exists. Run /status-register or /resources-register to replace it."
        }
        50005 => "The saved message belongs to another bot. Register a different channel.",
        _ if status == 401 => {
            "Discord rejected the bot credentials. Check the saved token, then Restart."
        }
        _ if status >= 500 => "Discord is temporarily unavailable. Try again shortly.",
        _ => {
            "Discord could not accept the status message. Try again or choose another text channel."
        }
    };
    // Do not include raw errors, request URLs, response contents, or credentials.
    format!("{advice} (HTTP {status}, Discord code {code})")
}
fn mentions() -> CreateAllowedMentions {
    CreateAllowedMentions::new()
        .all_users(false)
        .all_roles(false)
        .everyone(false)
}
fn message(text: &str) -> CreateInteractionResponse {
    CreateInteractionResponse::Message(
        CreateInteractionResponseMessage::new()
            .content(text)
            .ephemeral(true)
            .allowed_mentions(mentions()),
    )
}
struct Handler {
    engine: Engine,
    settings: SavedBot,
    generation: u64,
    message_lock: Arc<Mutex<()>>,
}
impl Handler {
    fn allowed(&self, guild: Option<GuildId>, user: UserId) -> bool {
        !self.engine.is_shutdown()
            && self.engine.bot_generation() == self.generation
            && self.engine.bot_settings() == self.settings
            && authorized(&self.settings, guild, user)
    }
    async fn command(&self, ctx: &Context, cmd: &CommandInteraction) {
        if !self.allowed(cmd.guild_id, cmd.user.id) {
            let _ = cmd
                .create_response(
                    &ctx.http,
                    message("You are not authorized to use this RoLauncher connection."),
                )
                .await;
            return;
        }
        // Acknowledge before filesystem work or channel requests.
        if cmd.defer_ephemeral(&ctx.http).await.is_err() {
            return;
        }
        let mut failure = None;
        let mut embed_response: Option<CreateEmbed> = None;
        let result = match cmd.data.name.as_str() {
            "action" => {
                let option = |name: &str| {
                    cmd.data
                        .options
                        .iter()
                        .find(|o| o.name == name)
                        .and_then(|o| o.value.as_str())
                };
                match (option("username"), option("action")) {
                    (Some(id), Some(action @ ("stop" | "restart"))) => {
                        let engine = self.engine.clone();
                        let settings = self.settings.clone();
                        let id = id.to_owned();
                        let action = action.to_owned();
                        tokio::task::spawn_blocking(move || {
                            engine.bot_command(&settings, &id, &action)
                        })
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .map(|_| "Action accepted. Check the registered status message for progress.")
                        .unwrap_or("Action could not be accepted. Check the account in RoLauncher.")
                    }
                    _ => "Choose an account from autocomplete and Stop or Restart.",
                }
            }
            "status-register" => {
                let channel = cmd
                    .data
                    .options
                    .iter()
                    .find(|o| o.name == "channel")
                    .and_then(|o| {
                        if let CommandDataOptionValue::Channel(id) = o.value {
                            Some(id)
                        } else {
                            None
                        }
                    });
                if let Some(channel) = channel {
                    // Verify channel membership/type; never post an account list outside the configured guild.
                    if let Ok(Channel::Guild(target)) = channel.to_channel(&ctx.http).await {
                        if Some(target.guild_id) == cmd.guild_id
                            && target.kind == ChannelType::Text
                            && self.allowed(cmd.guild_id, cmd.user.id)
                        {
                            let _guard = self.message_lock.lock().await;
                            let old = self.engine.bot_registration().filter(|r| {
                                r.guild_id == self.settings.guild_id
                                    && r.channel_id == channel.get()
                            });
                            let (embed, components) =
                                status_page(self.engine.snapshot().accounts, 0);
                            let posted = if let Some(old) = old {
                                channel
                                    .edit_message(
                                        &ctx.http,
                                        MessageId::new(old.message_id),
                                        EditMessage::new()
                                            .embed(embed)
                                            .components(components)
                                            .allowed_mentions(mentions()),
                                    )
                                    .await
                            } else {
                                channel
                                    .send_message(
                                        &ctx.http,
                                        CreateMessage::new()
                                            .embed(embed)
                                            .components(components)
                                            .allowed_mentions(mentions()),
                                    )
                                    .await
                            };
                            let posted = match posted {
                                Err(Error::Http(HttpError::UnsuccessfulRequest(ref response)))
                                    if response.error.code == 10008 =>
                                {
                                    let (embed, components) =
                                        status_page(self.engine.snapshot().accounts, 0);
                                    channel
                                        .send_message(
                                            &ctx.http,
                                            CreateMessage::new()
                                                .embed(embed)
                                                .components(components)
                                                .allowed_mentions(mentions()),
                                        )
                                        .await
                                }
                                other => other,
                            };
                            match posted {
                                Ok(posted) => {
                                    let engine = self.engine.clone();
                                    let settings = self.settings.clone();
                                    let registration = StatusRegistration {
                                        guild_id: settings.guild_id.clone(),
                                        channel_id: channel.get(),
                                        message_id: posted.id.get(),
                                        page: 0,
                                    };
                                    if tokio::task::spawn_blocking(move || {
                                        engine.set_bot_registration(&settings, registration)
                                    })
                                    .await
                                    .is_ok_and(|r| r.is_ok())
                                    {
                                        "Status message registered. It updates automatically while the bot is running."
                                    } else {
                                        "Message posted, but registration could not be saved. Try again."
                                    }
                                }
                                Err(error) => {
                                    failure = Some(discord_failure(&error));
                                    "Status registration failed."
                                }
                            }
                        } else {
                            "Choose a text channel in the configured server."
                        }
                    } else {
                        "The bot cannot access that channel."
                    }
                } else {
                    "Choose a text channel."
                }
            }
            "resources" => {
                let query = cmd
                    .data
                    .options
                    .iter()
                    .find(|o| o.name == "account")
                    .and_then(|o| o.value.as_str());
                if let Some(query) = query {
                    let accounts = self.engine.snapshot().accounts;
                    if let Some(account) = resolve_account(&accounts, query) {
                        if let Some(detail) = self
                            .engine
                            .account_resources(&account.id, false, 60, false, true, true)
                            .await
                        {
                            embed_response = Some(account_resource_embed(&detail));
                            ""
                        } else {
                            "Account not found."
                        }
                    } else {
                        "Account not found. Choose an account from autocomplete."
                    }
                } else {
                    let resp = self
                        .engine
                        .resources_snapshot(false, 60, false, true, true)
                        .await;
                    let (embed, _) = resources_page(&resp.snapshot, 0);
                    embed_response = Some(embed);
                    ""
                }
            }
            "resources-register" => {
                let channel = cmd
                    .data
                    .options
                    .iter()
                    .find(|o| o.name == "channel")
                    .and_then(|o| {
                        if let CommandDataOptionValue::Channel(id) = o.value {
                            Some(id)
                        } else {
                            None
                        }
                    });
                if let Some(channel) = channel {
                    if let Ok(Channel::Guild(target)) = channel.to_channel(&ctx.http).await {
                        if Some(target.guild_id) == cmd.guild_id
                            && target.kind == ChannelType::Text
                            && self.allowed(cmd.guild_id, cmd.user.id)
                        {
                            let _guard = self.message_lock.lock().await;
                            let old = self.engine.bot_resources_registration().filter(|r| {
                                r.guild_id == self.settings.guild_id
                                    && r.channel_id == channel.get()
                            });
                            let initial = self
                                .engine
                                .resources_snapshot(false, 60, false, true, true)
                                .await;
                            let (embed, components) = resources_page(&initial.snapshot, 0);
                            let posted = if let Some(old) = old {
                                channel
                                    .edit_message(
                                        &ctx.http,
                                        MessageId::new(old.message_id),
                                        EditMessage::new()
                                            .embed(embed.clone())
                                            .components(components.clone())
                                            .allowed_mentions(mentions()),
                                    )
                                    .await
                            } else {
                                channel
                                    .send_message(
                                        &ctx.http,
                                        CreateMessage::new()
                                            .embed(embed.clone())
                                            .components(components.clone())
                                            .allowed_mentions(mentions()),
                                    )
                                    .await
                            };
                            let posted = match posted {
                                Err(Error::Http(HttpError::UnsuccessfulRequest(ref response)))
                                    if response.error.code == 10008 =>
                                {
                                    channel
                                        .send_message(
                                            &ctx.http,
                                            CreateMessage::new()
                                                .embed(embed)
                                                .components(components)
                                                .allowed_mentions(mentions()),
                                        )
                                        .await
                                }
                                other => other,
                            };
                            match posted {
                                Ok(posted) => {
                                    let engine = self.engine.clone();
                                    let settings = self.settings.clone();
                                    let registration = StatusRegistration {
                                        guild_id: settings.guild_id.clone(),
                                        channel_id: channel.get(),
                                        message_id: posted.id.get(),
                                        page: 0,
                                    };
                                    if tokio::task::spawn_blocking(move || {
                                        engine.set_bot_resources_registration(
                                            &settings,
                                            Some(registration),
                                        )
                                    })
                                    .await
                                    .is_ok_and(|r| r.is_ok())
                                    {
                                        "Resource monitor registered. Channel viewers can see account names, statuses, process IDs, and resource statistics; it refreshes about every 30 seconds while running."
                                    } else {
                                        "Resource monitor posted, but registration could not be saved. Try again."
                                    }
                                }
                                Err(error) => {
                                    failure = Some(discord_failure(&error));
                                    "Resource monitor registration failed."
                                }
                            }
                        } else {
                            "Choose a text channel in the configured server."
                        }
                    } else {
                        "The bot cannot access that channel."
                    }
                } else {
                    "Choose a text channel."
                }
            }
            _ => "Unknown command.",
        };
        let mut edit = EditInteractionResponse::new().allowed_mentions(mentions());
        if let Some(err) = failure {
            edit = edit.content(err);
        } else if let Some(embed) = embed_response {
            edit = edit.embed(embed);
        } else {
            edit = edit.content(result);
        }
        let _ = cmd.edit_response(&ctx.http, edit).await;
    }
}
#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, _: Ready) {
        if self.engine.bot_settings() != self.settings
            || self.engine.bot_generation() != self.generation
        {
            return;
        }
        let guild = GuildId::new(self.settings.guild_id.parse().unwrap_or(1));
        // Remove the retired command while preserving unrelated application commands.
        let Ok(existing) = guild.get_commands(&ctx.http).await else {
            self.engine.bot_status(
                self.generation,
                "offline",
                "Cannot access server commands. Reinstall the bot, then Restart.",
            );
            return;
        };
        for command in existing.into_iter().filter(|c| c.name == "status") {
            if guild.delete_command(&ctx.http, command.id).await.is_err() {
                self.engine.bot_status(
                    self.generation,
                    "offline",
                    "Cannot remove the old status command. Check server installation.",
                );
                return;
            }
        }
        // Upsert only our commands; do not delete unrelated commands on the application.
        for command in commands() {
            if guild.create_command(&ctx.http, command).await.is_err() {
                self.engine.bot_status(
                    self.generation,
                    "offline",
                    "Command registration failed. Check the bot's server installation.",
                );
                return;
            }
        }
        self.engine
            .bot_status(self.generation, "online", BOT_READY_NOTICE);
    }
    async fn shard_stage_update(&self, _: Context, event: ShardStageUpdateEvent) {
        if event.new != ConnectionStage::Connected {
            self.engine.bot_status(
                self.generation,
                "starting",
                "Discord connection interrupted; reconnecting…",
            );
        }
    }
    async fn resume(&self, _: Context, _: ResumedEvent) {
        self.engine
            .bot_status(self.generation, "online", BOT_READY_NOTICE);
    }
    async fn guild_delete(&self, _: Context, guild: UnavailableGuild, _: Option<Guild>) {
        if guild.id.to_string() == self.settings.guild_id && !guild.unavailable {
            self.engine.bot_status(
                self.generation,
                "offline",
                "Bot removed from server. Reinstall it, then Restart; saved settings are retained.",
            );
        }
    }
    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(cmd) => self.command(&ctx, &cmd).await,
            Interaction::Autocomplete(cmd) => {
                let mut response = CreateAutocompleteResponse::new();
                if self.allowed(cmd.guild_id, cmd.user.id)
                    && matches!(cmd.data.name.as_str(), "action" | "resources")
                {
                    let prefix = cmd
                        .data
                        .autocomplete()
                        .map(|a| a.value.to_lowercase())
                        .unwrap_or_default();
                    let mut accounts = self.engine.snapshot().accounts;
                    accounts.sort_by(|a, b| a.username.cmp(&b.username));
                    let is_resources = cmd.data.name == "resources";
                    for account in accounts
                        .iter()
                        .filter(|a| {
                            a.username.to_lowercase().contains(&prefix)
                                || (is_resources
                                    && (a.alias.to_lowercase().contains(&prefix)
                                        || a.id.to_lowercase().contains(&prefix)))
                        })
                        .take(25)
                    {
                        let choice = if is_resources {
                            account_autocomplete_label(account)
                        } else {
                            account.username.chars().take(100).collect::<String>()
                        };
                        response = response.add_string_choice(choice, &account.id);
                    }
                }
                let _ = cmd
                    .create_response(&ctx.http, CreateInteractionResponse::Autocomplete(response))
                    .await;
            }
            Interaction::Component(cmd) => {
                if !self.allowed(cmd.guild_id, cmd.user.id) {
                    let _ = cmd
                        .create_response(
                            &ctx.http,
                            message("You are not authorized to interact with this message."),
                        )
                        .await;
                    return;
                }
                if let Some(page) = status_button_page(&cmd.data.custom_id) {
                    let _guard = self.message_lock.lock().await;
                    let Some(mut registration) = self.engine.bot_registration().filter(|r| {
                        r.channel_id == cmd.channel_id.get() && r.message_id == cmd.message.id.get()
                    }) else {
                        return;
                    };
                    let count = self.engine.snapshot().accounts.len();
                    registration.page = page.min(count.div_ceil(10).max(1) - 1);
                    let engine = self.engine.clone();
                    let settings = self.settings.clone();
                    let page = registration.page;
                    if !tokio::task::spawn_blocking(move || {
                        engine.set_bot_registration(&settings, registration)
                    })
                    .await
                    .is_ok_and(|r| r.is_ok())
                    {
                        return;
                    }
                    let (embed, components) = status_page(self.engine.snapshot().accounts, page);
                    let _ = cmd
                        .create_response(
                            &ctx.http,
                            CreateInteractionResponse::UpdateMessage(
                                CreateInteractionResponseMessage::new()
                                    .embed(embed)
                                    .components(components)
                                    .allowed_mentions(mentions()),
                            ),
                        )
                        .await;
                } else if let Some(page) = resources_button_page(&cmd.data.custom_id) {
                    // Acknowledge within Discord's 3s window before sampling metrics.
                    if cmd.defer(&ctx.http).await.is_err() {
                        return;
                    }
                    let _guard = self.message_lock.lock().await;
                    let Some(mut registration) =
                        self.engine.bot_resources_registration().filter(|r| {
                            r.channel_id == cmd.channel_id.get()
                                && r.message_id == cmd.message.id.get()
                        })
                    else {
                        return;
                    };
                    let snap = self
                        .engine
                        .resources_snapshot(false, 60, false, true, true)
                        .await;
                    let visible = resources_visible_rows(&snap.snapshot);
                    let pages = visible.len().div_ceil(RESOURCES_PAGE_SIZE).max(1);
                    registration.page = page.min(pages - 1);
                    let page = registration.page;
                    let engine = self.engine.clone();
                    let settings = self.settings.clone();
                    if !tokio::task::spawn_blocking(move || {
                        engine.set_bot_resources_registration(&settings, Some(registration))
                    })
                    .await
                    .is_ok_and(|r| r.is_ok())
                    {
                        return;
                    }
                    let (embed, components) = resources_page(&snap.snapshot, page);
                    let _ = cmd
                        .edit_response(
                            &ctx.http,
                            EditInteractionResponse::new()
                                .embed(embed)
                                .components(components)
                                .allowed_mentions(mentions()),
                        )
                        .await;
                }
            }
            _ => {}
        }
    }
}
async fn refresh_status(
    engine: Engine,
    settings: SavedBot,
    generation: u64,
    http: Arc<Http>,
    message_lock: Arc<Mutex<()>>,
) {
    let mut previous = String::new();
    loop {
        tokio::time::sleep(Duration::from_secs(15)).await;
        if engine.is_shutdown()
            || engine.bot_generation() != generation
            || engine.bot_settings() != settings
        {
            return;
        }
        if engine.discord_settings().bot.connection_state != "online" {
            continue;
        }
        let _guard = message_lock.lock().await;
        let Some(registration) = engine
            .bot_registration()
            .filter(|r| r.guild_id == settings.guild_id)
        else {
            continue;
        };
        let (embed, components) = status_page(engine.snapshot().accounts, registration.page);
        let payload =
            serde_json::to_string(&(registration.clone(), embed.clone(), components.clone()))
                .unwrap_or_default();
        if payload == previous {
            continue;
        }
        let edited = ChannelId::new(registration.channel_id)
            .edit_message(
                &http,
                MessageId::new(registration.message_id),
                EditMessage::new()
                    .embed(embed)
                    .components(components)
                    .allowed_mentions(mentions()),
            )
            .await;
        if let Err(error) = edited {
            // Retry the same message; never create duplicates on permission/network failures.
            engine.bot_online_notice(generation, &discord_failure(&error));
        } else {
            previous = payload;
            engine.bot_online_notice(generation, BOT_READY_NOTICE);
        }
    }
}
async fn refresh_resources(
    engine: Engine,
    settings: SavedBot,
    generation: u64,
    http: Arc<Http>,
    message_lock: Arc<Mutex<()>>,
) {
    let mut previous = String::new();
    loop {
        tokio::time::sleep(RESOURCES_REFRESH_INTERVAL).await;
        if engine.is_shutdown()
            || engine.bot_generation() != generation
            || engine.bot_settings() != settings
        {
            return;
        }
        if engine.discord_settings().bot.connection_state != "online" {
            continue;
        }
        let _guard = message_lock.lock().await;
        let Some(registration) = engine
            .bot_resources_registration()
            .filter(|r| r.guild_id == settings.guild_id)
        else {
            previous.clear();
            continue;
        };
        let resp = engine
            .resources_snapshot(false, 60, false, true, true)
            .await;
        let fingerprint = format!(
            "{}:{}:{}",
            registration.channel_id,
            registration.message_id,
            resources_change_fingerprint(&resp.snapshot, registration.page)
        );
        if fingerprint == previous {
            continue;
        }
        let (embed, components) = resources_page(&resp.snapshot, registration.page);
        let edited = ChannelId::new(registration.channel_id)
            .edit_message(
                &http,
                MessageId::new(registration.message_id),
                EditMessage::new()
                    .embed(embed)
                    .components(components)
                    .allowed_mentions(mentions()),
            )
            .await;
        if let Err(error) = edited {
            if invalid_registration_error(&error) {
                let clear_engine = engine.clone();
                let clear_settings = settings.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    clear_engine.set_bot_resources_registration(&clear_settings, None)
                })
                .await;
                previous.clear();
            }
            engine.bot_online_notice(generation, &discord_failure(&error));
        } else {
            previous = fingerprint;
            engine.bot_online_notice(generation, BOT_READY_NOTICE);
        }
    }
}
pub async fn run(engine: Engine) {
    while !engine.is_shutdown() {
        let settings = engine.bot_settings();
        let generation = engine.bot_generation();
        if !settings.enabled || generation.is_multiple_of(2) {
            engine.bot_status(
                generation,
                "offline",
                "Bot stopped. Saved settings are retained. Use Start to connect.",
            );
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }
        let Ok(token) = crate::platform::unprotect(&settings.encrypted_token) else {
            engine.bot_status(
                generation,
                "offline",
                "Cannot unlock bot token. Save it again.",
            );
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        };
        engine.bot_status(generation, "starting", "Connecting to Discord…");
        let message_lock = Arc::new(Mutex::new(()));
        let client = Client::builder(token.as_str(), GatewayIntents::GUILDS)
            .event_handler(Handler {
                engine: engine.clone(),
                settings: settings.clone(),
                generation,
                message_lock: message_lock.clone(),
            })
            .await;
        let Ok(mut client) = client else {
            engine.bot_status(
                generation,
                "offline",
                "Unable to prepare bot connection. Saved settings are retained.",
            );
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        };
        drop(token);
        let manager = client.shard_manager.clone();
        let updater = tokio::spawn(refresh_status(
            engine.clone(),
            settings.clone(),
            generation,
            client.http.clone(),
            message_lock.clone(),
        ));
        let resources_updater = tokio::spawn(refresh_resources(
            engine.clone(),
            settings.clone(),
            generation,
            client.http.clone(),
            message_lock,
        ));
        let mut worker = tokio::spawn(async move { client.start().await });
        loop {
            tokio::select! {
                _ = &mut worker => { engine.bot_status(generation, "offline", "Bot disconnected. Saved settings are retained; retrying shortly."); break; },
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if engine.is_shutdown() || engine.bot_generation() != generation || engine.bot_settings() != settings {
                        manager.shutdown_all().await; worker.abort(); break;
                    }
                }
            }
        }
        updater.abort();
        resources_updater.abort();
        manager.shutdown_all().await;
        if !engine.is_shutdown()
            && engine.bot_generation() == generation
            && engine.bot_settings() == settings
        {
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_buttons_have_unique_ids_on_every_page() {
        for count in [0usize, 1, 10, 11, 20, 21] {
            let accounts = (0..count)
                .map(|i| Account::new(i.to_string(), format!("user{i}")))
                .collect::<Vec<_>>();
            for page in [0, 1, 2, usize::MAX] {
                let (_, buttons) = status_page(accounts.clone(), page);
                let json = serde_json::to_value(buttons).unwrap();
                let ids = json[0]["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|button| button["custom_id"].as_str().unwrap())
                    .collect::<std::collections::HashSet<_>>();
                assert_eq!(
                    ids.len(),
                    3,
                    "Discord rejects duplicate button IDs (count={count}, page={page})"
                );
                let current_page = page.min(count.div_ceil(10).max(1) - 1);
                for button in json[0]["components"].as_array().unwrap() {
                    let expected = match button["label"].as_str().unwrap() {
                        "Previous" => current_page.saturating_sub(1),
                        "Refresh" => current_page,
                        _ => current_page + 1,
                    };
                    assert_eq!(
                        status_button_page(button["custom_id"].as_str().unwrap()),
                        Some(expected)
                    );
                }
            }
        }
    }
    #[test]
    fn status_button_parser_retains_old_messages_and_rejects_invalid_actions() {
        assert_eq!(status_button_page("rlstatus:2"), Some(2));
        for invalid in [
            "rlstatus:delete:1",
            "rlstatus:refresh:bad",
            "other:refresh:1",
            "rlstatus:",
        ] {
            assert_eq!(status_button_page(invalid), None);
        }
    }
    #[test]
    fn discord_errors_distinguish_payloads_permissions_and_network_without_raw_data() {
        for (code, expected) in [
            (50035, "format"),
            (50013, "permissions"),
            (50001, "access"),
            (10008, "no longer exists"),
        ] {
            let advice = discord_http_failure(400, code);
            assert!(advice.contains(expected), "{advice}");
            assert!(advice.contains(&format!("Discord code {code}")));
        }
        assert!(discord_http_failure(401, 0).contains("credentials"));
        assert!(discord_http_failure(503, 0).contains("temporarily unavailable"));
        assert!(!discord_failure(&Error::Other("SECRET_RESPONSE")).contains("SECRET_RESPONSE"));
    }
    #[test]
    fn commands_require_explicit_server_and_user_access() {
        let mut saved = SavedBot {
            enabled: true,
            guild_id: "42".into(),
            allowed_users: vec!["123".into()],
            ..Default::default()
        };
        assert!(authorized(&saved, Some(GuildId::new(42)), UserId::new(123)));
        assert!(!authorized(&saved, None, UserId::new(123)));
        assert!(!authorized(
            &saved,
            Some(GuildId::new(43)),
            UserId::new(123)
        ));
        assert!(!authorized(
            &saved,
            Some(GuildId::new(42)),
            UserId::new(124)
        ));
        saved.enabled = false;
        assert!(!authorized(
            &saved,
            Some(GuildId::new(42)),
            UserId::new(123)
        ));
        let mut patch = BotPatch::default();
        patch.enabled = Some(true);
        assert!(patch.apply(&SavedBot::default()).is_err());
    }
    #[test]
    fn pagination_handles_empty_exact_boundary_and_shrinking_lists() {
        for count in [0, 10, 11, 20, 21] {
            let accounts = (0..count)
                .map(|i| Account::new(i.to_string(), format!("user{i:02}")))
                .collect::<Vec<_>>();
            let (embed, buttons) = status_page(accounts.clone(), 0);
            let json = serde_json::to_value(embed).unwrap();
            assert_eq!(
                json["description"].as_str().unwrap().lines().count(),
                count.clamp(1, 10)
            );
            let json = serde_json::to_value(buttons).unwrap();
            assert_eq!(json[0]["components"][2]["disabled"], count <= 10);
            let (embed, _) = status_page(accounts, usize::MAX);
            assert!(serde_json::to_string(&embed).unwrap().contains("Page"));
        }
    }

    fn fixture_resource_snapshot(count: usize, sys_cpu: f64, row_cpu: f64) -> ResourceSnapshot {
        use crate::resources::{
            ManagedAggregates, ResourceMetadata, SnapshotAvailability, SystemResources,
        };
        let now = chrono::Utc::now();
        let accounts = (0..count)
            .map(|i| ManagedInstanceResource {
                account_id: format!("id-{i}"),
                display_name: if i == 0 {
                    "@everyone <@123> *Farm*_0".into()
                } else {
                    format!("Account-{i:02}")
                },
                username: format!("user{i:02}"),
                account_status: Status::Running,
                pid: Some(4000 + i as u32),
                creation_time: Some("1000".into()),
                cpu_percent: Some(row_cpu),
                working_set_bytes: Some(256 * 1024 * 1024),
                private_memory_bytes: Some(200 * 1024 * 1024),
                gpu_percent: None,
                gpu_availability: MetricAvailability::Unavailable,
                uptime_seconds: Some(125),
                sampled_at: now,
                availability: ProcessAvailability::Available,
            })
            .collect::<Vec<_>>();
        ResourceSnapshot {
            timestamp: now,
            sample_age_ms: 120,
            stale: false,
            availability: SnapshotAvailability::Available,
            system: SystemResources {
                logical_processors: 8,
                cpu_percent: Some(sys_cpu),
                cpu_availability: MetricAvailability::Available,
                total_ram_bytes: Some(16 * 1024 * 1024 * 1024),
                available_ram_bytes: Some(8 * 1024 * 1024 * 1024),
                used_ram_bytes: Some(8 * 1024 * 1024 * 1024),
                ram_utilization_percent: Some(50.0),
                ram_availability: MetricAvailability::Available,
                gpu_percent: None,
                gpu_availability: MetricAvailability::Unavailable,
            },
            aggregates: ManagedAggregates {
                running_instances: count,
                total_accounts: count,
                cpu_percent: (row_cpu * count as f64).min(100.0),
                cpu_availability: MetricAvailability::Available,
                working_set_bytes: (count as u64) * 256 * 1024 * 1024,
                private_memory_bytes: Some((count as u64) * 200 * 1024 * 1024),
                gpu_percent: None,
                gpu_availability: MetricAvailability::Unavailable,
                unmanaged_roblox_processes: Some(0),
                inaccessible_roblox_processes: Some(0),
                exited_or_stale_instances: 0,
            },
            accounts,
            metadata: ResourceMetadata::default(),
        }
    }

    #[test]
    fn commands_include_resources_and_resources_register_without_replacing_existing() {
        let cmds = serde_json::to_value(commands()).unwrap();
        let names = cmds
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "action",
                "status-register",
                "resources",
                "resources-register"
            ]
        );
        let resources_cmd = &cmds[2];
        assert_eq!(resources_cmd["options"][0]["name"], "account");
        assert_eq!(resources_cmd["options"][0]["required"], false);
        assert_eq!(resources_cmd["options"][0]["autocomplete"], true);
    }

    #[test]
    fn resources_buttons_pagination_and_escaping_handle_boundaries() {
        assert_eq!(resources_button_page("rlres:previous:0"), Some(0));
        assert_eq!(resources_button_page("rlres:refresh:2"), Some(2));
        assert_eq!(resources_button_page("rlres:next:3"), Some(3));
        assert_eq!(resources_button_page("rlstatus:next:1"), None);
        assert_eq!(resources_button_page("rlres:delete:0"), None);
        assert_eq!(resources_button_page("rlres:refresh:abc"), None);

        let empty = fixture_resource_snapshot(0, 12.4, 0.0);
        let (empty_embed, empty_btns) = resources_page(&empty, 0);
        let empty_json = serde_json::to_value(empty_embed).unwrap();
        assert!(
            empty_json["description"]
                .as_str()
                .unwrap()
                .contains("No managed Roblox instances are currently running")
        );
        let btn_json = serde_json::to_value(empty_btns).unwrap();
        assert_eq!(btn_json[0]["components"][0]["disabled"], true);
        assert_eq!(btn_json[0]["components"][2]["disabled"], true);

        for count in [8usize, 9, 17] {
            let snap = fixture_resource_snapshot(count, 35.0, 5.5);
            for page in [0, 1, usize::MAX] {
                let (embed, buttons) = resources_page(&snap, page);
                let emb_json = serde_json::to_value(embed).unwrap();
                let desc = emb_json["description"].as_str().unwrap();
                assert!(!desc.contains("@everyone"), "mentions must be escaped");
                assert!(!desc.contains("<@123>"), "user mentions must be escaped");
                let row_json = serde_json::to_value(buttons).unwrap();
                let ids = row_json[0]["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|b| b["custom_id"].as_str().unwrap())
                    .collect::<std::collections::HashSet<_>>();
                assert_eq!(ids.len(), 3);
            }
        }
    }

    #[test]
    fn account_resource_embed_formats_fields_and_marks_unavailable_gpu() {
        let snap = fixture_resource_snapshot(1, 20.0, 7.5);
        let resp = AccountResourceResponse {
            timestamp: snap.timestamp,
            sample_age_ms: 500,
            stale: false,
            metadata: snap.metadata.clone(),
            account: snap.accounts[0].clone(),
            history: None,
        };
        let embed_json = serde_json::to_value(account_resource_embed(&resp)).unwrap();
        let title = embed_json["title"].as_str().unwrap();
        let desc = embed_json["description"].as_str().unwrap();
        assert!(!title.contains("@everyone"));
        assert!(desc.contains("7.5% (machine)"));
        assert!(desc.contains("**GPU:** Unavailable"));
        assert!(!desc.contains(".ROBLOSECURITY"));
    }

    #[test]
    fn resources_fingerprint_suppresses_jitter_and_detects_real_changes() {
        let a = fixture_resource_snapshot(2, 20.1, 4.1);
        let mut b = fixture_resource_snapshot(2, 20.4, 4.3);
        b.timestamp = a.timestamp + chrono::Duration::seconds(30);
        b.sample_age_ms = 400;
        assert_eq!(
            resources_change_fingerprint(&a, 0),
            resources_change_fingerprint(&b, 0)
        );

        let c = fixture_resource_snapshot(3, 20.4, 4.3);
        assert_ne!(
            resources_change_fingerprint(&a, 0),
            resources_change_fingerprint(&c, 0)
        );
    }

    #[test]
    fn resources_registration_stays_independent_of_status_registration() {
        let base = SavedBot {
            enabled: true,
            encrypted_token: "enc".into(),
            guild_id: "42".into(),
            allowed_users: vec!["123".into()],
            registration: Some(StatusRegistration {
                guild_id: "42".into(),
                channel_id: 100,
                message_id: 200,
                page: 0,
            }),
            resources_registration: Some(StatusRegistration {
                guild_id: "42".into(),
                channel_id: 101,
                message_id: 201,
                page: 0,
            }),
        };
        let mut patch = BotPatch::default();
        patch.allowed_users = Some(vec!["123".into(), "456".into()]);
        let same_guild = patch.apply(&base).unwrap();
        assert!(same_guild.registration.is_some());
        assert!(same_guild.resources_registration.is_some());

        let mut new_guild = BotPatch::default();
        new_guild.guild_id = Some("99".into());
        let changed = new_guild.apply(&base).unwrap();
        assert!(changed.registration.is_none());
        assert!(changed.resources_registration.is_none());
    }

    #[test]
    fn resources_page_truncates_safely_on_multibyte_utf8_boundaries() {
        for pad in 0..8 {
            let input = format!("{}{}", "a".repeat(pad), "界".repeat(1400));
            assert!(input.len() > 3800);
            let truncated = truncate_embed_description(input);
            assert!(truncated.len() <= 3800);
            assert!(truncated.ends_with("..."));
        }
    }
}
