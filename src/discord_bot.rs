//! Opt-in Discord commands. Serenity owns gateway reconnects and REST rate limits.
use crate::{
    engine::Engine,
    model::{Account, Status},
};
use serde::{Deserialize, Serialize};
use serenity::{all::*, async_trait};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use zeroize::Zeroize;

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
    ]
}
fn status_label(status: &Status) -> &'static str {
    match status {
        Status::Running => "🟢 Online",
        Status::Stopped => "⚪ Stopped",
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
        10008 => "The status message no longer exists. Run /status-register to replace it.",
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
            _ => "Unknown command.",
        };
        let _ = cmd
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new()
                    .content(failure.unwrap_or_else(|| result.to_owned()))
                    .allowed_mentions(mentions()),
            )
            .await;
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
        self.engine.bot_status(
            self.generation,
            "online",
            "Connected. /action and /status-register are ready.",
        );
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
        self.engine.bot_status(
            self.generation,
            "online",
            "Connected. /action and /status-register are ready.",
        );
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
                if self.allowed(cmd.guild_id, cmd.user.id) && cmd.data.name == "action" {
                    let prefix = cmd
                        .data
                        .autocomplete()
                        .map(|a| a.value.to_lowercase())
                        .unwrap_or_default();
                    let mut accounts = self.engine.snapshot().accounts;
                    accounts.sort_by(|a, b| a.username.cmp(&b.username));
                    for account in accounts
                        .iter()
                        .filter(|a| a.username.to_lowercase().contains(&prefix))
                        .take(25)
                    {
                        response = response.add_string_choice(
                            account.username.chars().take(100).collect::<String>(),
                            &account.id,
                        );
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
                            message("You are not authorized to view this account list."),
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
            engine.bot_online_notice(
                generation,
                "Connected. /action and /status-register are ready.",
            );
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
}
