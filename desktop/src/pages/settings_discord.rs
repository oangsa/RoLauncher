//! Discord notifications and Discord bot slash-command configuration.

use crate::app::{RoLauncherApp, on_action};
use crate::components::primitives::{
    ButtonVariant, badge, button, card, checkbox, section_title, status_badge,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

/// Renders the Discord notifications and bot configuration page.
pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;
    let discord = app.discord_view.as_ref();
    let ready = discord.is_some();

    let webhook_enabled = discord.is_some_and(|d| d.enabled);
    let webhook_recovery = discord.is_some_and(|d| d.notify_recovery);
    let webhook_configured = discord.is_some_and(|d| d.configured);
    let webhook_status = discord
        .map(|d| {
            format!(
                "{} · {}",
                if d.configured {
                    "Webhook saved"
                } else {
                    "No webhook saved"
                },
                d.delivery_status
            )
        })
        .unwrap_or_else(|| "Loading Discord settings…".to_string());

    let webhook_placeholder = if webhook_configured {
        "●●●●●●●●●●●●●●●●"
    } else {
        "Discord webhook URL"
    };
    app.discord_webhook_input.update(cx, |input, _cx| {
        input.placeholder = webhook_placeholder.into()
    });

    let bot_configured = discord
        .and_then(|d| d.bot.as_ref())
        .is_some_and(|b| b.configured);
    let bot_conn = discord
        .and_then(|d| d.bot.as_ref())
        .map(|b| b.connection_state.as_str())
        .unwrap_or("offline");
    let bot_conn_label = match bot_conn {
        "online" => "Online",
        "starting" => "Starting",
        _ => "Offline",
    };
    let bot_status_text = discord
        .and_then(|d| d.bot.as_ref())
        .map(|b| b.status.clone())
        .unwrap_or_else(|| "Commands disabled.".to_string());

    let token_placeholder = if bot_configured {
        "Token saved"
    } else {
        "Bot token"
    };
    app.discord_bot_token_input.update(cx, |input, _cx| {
        input.placeholder = token_placeholder.into()
    });

    let webhook_card = card(&theme, &density)
        .child(section_title(
            "Webhook notifications",
            Some("Create a webhook in Discord / Integrations. Applies to all accounts.".into()),
            &theme,
            &density,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(18.0))
                .child(checkbox(
                    "discord-enabled-check",
                    webhook_enabled,
                    Some("Enable notifications".into()),
                    busy || !ready,
                    &theme,
                    &density,
                    on_action(cx, move |this, _w, cx| {
                        this.set_discord_preferences(!webhook_enabled, webhook_recovery, cx);
                    }),
                ))
                .child(checkbox(
                    "discord-recovery-check",
                    webhook_recovery,
                    Some("Include rejoin and back online".into()),
                    busy || !ready,
                    &theme,
                    &density,
                    on_action(cx, move |this, _w, cx| {
                        this.set_discord_preferences(webhook_enabled, !webhook_recovery, cx);
                    }),
                )),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme.text_secondary)
                        .child("Webhook URL · masked, never revealed from saved state"),
                )
                .child(app.discord_webhook_input.clone())
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_muted)
                        .child("Leave unchanged to keep the saved webhook."),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(8.0))
                .child(button(
                    "discord-webhook-save",
                    "Save webhook",
                    ButtonVariant::Primary,
                    busy || !ready,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.save_discord_webhook(cx)),
                ))
                .child(button(
                    "discord-webhook-test",
                    "Send test",
                    ButtonVariant::Secondary,
                    busy || !ready,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.test_discord_webhook(cx)),
                ))
                .child(button(
                    "discord-webhook-remove",
                    "Remove webhook",
                    ButtonVariant::Danger,
                    busy || !ready,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.remove_discord_webhook(cx)),
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .child(badge(
                    webhook_status,
                    if webhook_configured {
                        theme.info
                    } else {
                        theme.text_secondary
                    },
                    if webhook_configured {
                        theme.info_bg
                    } else {
                        theme.bg_badge
                    },
                    if webhook_configured {
                        theme.info_border
                    } else {
                        theme.border_subtle
                    },
                    &density,
                ))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_muted)
                        .child("Error alerts include a game screenshot when available."),
                ),
        );

    let bot_enabled_draft = app.bot_enabled_draft;
    let bot_card = card(&theme, &density)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(section_title(
                    "Bot commands",
                    Some("Install with bot and applications.commands scopes (View Channel, Send Messages, Embed Links).".into()),
                    &theme,
                    &density,
                ))
                .child(status_badge(
                    match bot_conn {
                        "online" => "running",
                        "starting" => "launching",
                        _ => "needs_attention",
                    },
                    bot_conn_label,
                    &theme,
                    &density,
                )),
        )
        .child(
            div()
                .text_size(px(density.font_caption))
                .text_color(theme.text_muted)
                .child("Only listed users can run /action, /status-register, /resources, and /resources-register."),
        )
        .child(checkbox(
            "discord-bot-enabled-check",
            bot_enabled_draft,
            Some("Enable /action, /status-register, /resources, and /resources-register".into()),
            busy || !ready,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.bot_enabled_draft = !this.bot_enabled_draft;
                cx.notify();
            }),
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme.text_secondary)
                        .child("Bot token · blank keeps saved token"),
                )
                .child(app.discord_bot_token_input.clone()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(12.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(200.0))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(density.font_caption))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.text_secondary)
                                .child("Server ID (Guild ID)"),
                        )
                        .child(app.discord_guild_input.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(260.0))
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(density.font_caption))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.text_secondary)
                                .child("Allowed user IDs · comma separated"),
                        )
                        .child(app.discord_users_input.clone()),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(8.0))
                .child(button(
                    "discord-bot-save",
                    "Save bot",
                    ButtonVariant::Primary,
                    busy || !ready,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.save_discord_bot(cx)),
                ))
                .child(button(
                    "discord-bot-start",
                    "Start",
                    ButtonVariant::Secondary,
                    busy || !ready,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.control_discord_bot("start", cx)),
                ))
                .child(button(
                    "discord-bot-stop",
                    "Stop",
                    ButtonVariant::Danger,
                    busy || !ready,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.control_discord_bot("stop", cx)),
                ))
                .child(button(
                    "discord-bot-restart",
                    "Restart",
                    ButtonVariant::Secondary,
                    busy || !ready,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.control_discord_bot("restart", cx)),
                ))
                .child(button(
                    "discord-bot-remove",
                    "Remove bot",
                    ButtonVariant::Secondary,
                    busy || !ready,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.remove_discord_bot(cx)),
                )),
        )
        .child(
            div()
                .text_size(px(density.font_caption))
                .text_color(theme.text_muted)
                .child(format!("Save, then Start or Restart to connect. {bot_status_text}")),
        );

    div()
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(webhook_card)
        .child(bot_card)
        .into_any_element()
}
