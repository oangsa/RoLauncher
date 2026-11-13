//! Game joining preferences and Roblox multi-instance / background cleanup configuration.

use crate::app::{RoLauncherApp, on_action};
use crate::components::menu::settings_dropdown;
use crate::components::primitives::{badge, card, checkbox, section_title, segmented_control};
use crate::state::OpenMenu;
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

/// Renders the Game joining settings tab.
pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;
    let snap = app.snapshot.as_ref();

    let server_selection = snap
        .map(|s| s.server_selection.clone())
        .unwrap_or_else(|| "smart".to_string());
    let join_type = snap
        .map(|s| s.join_type.clone())
        .unwrap_or_else(|| "random".to_string());
    let multi_enabled = snap.is_none_or(|s| s.multi_instance_enabled);
    let active_multi = snap.is_none_or(|s| s.active_multi_instance);
    let startup_cleanup = snap.is_some_and(|s| s.external_background_cleanup_enabled);
    let ready = snap.is_some();

    let entity_multi = cx.entity();
    let server_for_join = server_selection.clone();
    let join_for_server = join_type.clone();

    let joining_card = card(&theme, &density)
        .child(section_title(
            "Joining preferences",
            Some("Saved for future launches and automatic rejoins.".into()),
            &theme,
            &density,
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
                        .child("Join type"),
                )
                .child(settings_dropdown(
                    OpenMenu::SettingsJoinType,
                    app.open_menu,
                    &[("friend", "Friend"), ("random", "Random")],
                    &join_type,
                    busy || !ready,
                    &theme,
                    &density,
                    cx,
                    move |this, value, _window, cx| {
                        let srv = server_for_join.clone();
                        this.set_game_settings(srv, value, cx);
                    },
                ))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_muted)
                        .child("Friend joins managed accounts on the same saved game (requires game access and free slots). Random joins independently."),
                ),
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
                        .child("Server selector"),
                )
                .child(settings_dropdown(
                    OpenMenu::SettingsServer,
                    app.open_menu,
                    &[
                        ("smart", "Smart"),
                        ("lowest", "Lowest"),
                        ("highest", "Highest"),
                    ],
                    &server_selection,
                    busy || !ready,
                    &theme,
                    &density,
                    cx,
                    move |this, value, _window, cx| {
                        let jt = join_for_server.clone();
                        this.set_game_settings(value, jt, cx);
                    },
                ))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_muted)
                        .child("Smart searches available pages for low reported ping and free slots. Lowest / Highest use player count. Searches time out after 2 minutes; Job IDs and private links override."),
                ),
        );

    let restart_pending = ready && multi_enabled != active_multi;
    let instances_card = card(&theme, &density)
        .child(section_title(
            "Roblox instances",
            Some(if restart_pending {
                "Restart RoLauncher to apply multi-instance change. Running clients stay open.".into()
            } else {
                "Changes apply after restarting RoLauncher. Off launches one managed client at a time.".into()
            }),
            &theme,
            &density,
        ))
        .when(restart_pending, |c| {
            c.child(badge(
                "Restart required to apply change",
                theme.warning,
                theme.warning_bg,
                theme.warning_border,
                &density,
            ))
        })
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
                        .child("Multiple Roblox instances"),
                )
                .child(segmented_control(
                    "multi-instance",
                    &[("on", "On (default)"), ("off", "Off")],
                    if multi_enabled { "on" } else { "off" },
                    busy || !ready,
                    &theme,
                    &density,
                    move |value, _window, cx| {
                        let on = value == "on";
                        entity_multi.update(cx, |this, cx| {
                            this.set_multi_instance(on, cx);
                        });
                    },
                )),
        )
        .child(checkbox(
            "startup-cleanup-check",
            startup_cleanup,
            Some("Automatically close conflicting background Roblox instances".into()),
            busy || !ready,
            &theme,
            &density,
            on_action(cx, move |this, _window, cx| {
                this.set_startup_cleanup(!startup_cleanup, cx);
            }),
        ))
        .child(
            div()
                .pl(px(24.0))
                .text_size(px(density.font_caption))
                .text_color(theme.text_muted)
                .child("Windows only. When enabled, RoLauncher may close hidden Roblox processes outside its management that block startup. Visible game windows stay protected. Runs at startup and when Start is blocked; off by default."),
        );

    div()
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(joining_card)
        .child(instances_card)
        .into_any_element()
}
