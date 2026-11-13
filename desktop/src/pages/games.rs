//! Saved Roblox games library displaying place cards, thumbnails, and edit/delete actions.

use crate::app::{RoLauncherApp, on_action};
use crate::components::primitives::{ButtonVariant, button, card, section_title};
use crate::components::thumbnail::render_thumbnail_box;
use crate::state::{ActiveModal, ConfirmAction, ConfirmModal};
use gpui::{AnyElement, Context, MouseButton, Window, div, prelude::*, px};

pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;
    let snapshot = app.snapshot.as_ref();
    let games = snapshot
        .map(|s| s.game_profiles.clone())
        .unwrap_or_default();
    let accounts = snapshot.map(|s| s.accounts.clone()).unwrap_or_default();

    let header_bar = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(section_title(
            "Saved games",
            Some("Reuse destinations across accounts.".into()),
            &theme,
            &density,
        ))
        .child(button(
            "games-add-btn",
            "Add game",
            ButtonVariant::Primary,
            busy,
            false,
            &theme,
            &density,
            on_action(cx, |this, window, cx| {
                crate::dialogs::game_editor::open(this, None, window, cx);
            }),
        ));

    if games.is_empty() {
        return div()
            .flex()
            .flex_col()
            .gap(px(density.section_gap))
            .child(header_bar)
            .child(
                div()
                    .text_size(px(density.font_caption))
                    .text_color(theme.text_muted)
                    .child("No saved games yet."),
            )
            .into_any_element();
    }

    let mut pending_thumbnails = Vec::new();
    let mut grid = div().flex().flex_row().flex_wrap().gap(px(12.0));
    for (idx, game) in games.iter().enumerate() {
        let used_count = accounts
            .iter()
            .filter(|a| a.game_profile_id.as_deref() == Some(game.id.as_str()))
            .count();
        let g_edit = game.clone();
        let g_del = game.clone();

        let thumb_url = game.thumbnail_url.clone().or_else(|| {
            app.game_cache
                .resolved
                .get(&game.target.place_id)
                .and_then(|r| r.thumbnail_url.clone())
        });
        if let Some(url) = thumb_url.as_ref()
            && app.thumbnails.get(url).is_none()
        {
            pending_thumbnails.push(url.clone());
        }

        let gcard = card(&theme, &density)
            .id(("game-card", idx))
            .w(px(286.0))
            .cursor_pointer()
            .hover(|s| s.bg(theme.bg_surface_hover))
            .on_click(cx.listener(move |this, _ev, window, cx| {
                crate::dialogs::game_editor::open(this, Some(&g_edit), window, cx);
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.0))
                    .child(render_thumbnail_box(
                        &app.thumbnails,
                        thumb_url.as_deref(),
                        60.0,
                        &theme,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .child(
                                div()
                                    .text_size(px(density.font_section))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme.text_primary)
                                    .truncate()
                                    .child(game.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(density.font_caption))
                                    .text_color(theme.text_secondary)
                                    .child(format!(
                                        "Place {} · {}",
                                        game.target.place_id,
                                        game.target.kind_label()
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(density.font_caption))
                                    .text_color(theme.text_muted)
                                    .child(format!("Assigned to {used_count} account(s)")),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(8.0))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(button(
                        ("game-del", idx),
                        "Delete",
                        ButtonVariant::Danger,
                        busy,
                        false,
                        &theme,
                        &density,
                        on_action(cx, move |this, _w, cx| {
                            this.modal = ActiveModal::Confirm(ConfirmModal {
                                title: "Delete saved game".into(),
                                body: format!("Delete \"{}\" from saved games?", g_del.name),
                                confirm_label: "Delete".into(),
                                dangerous: true,
                                action: ConfirmAction::DeleteGameProfile(g_del.id.clone()),
                            });
                            cx.notify();
                        }),
                    )),
            );
        grid = grid.child(gcard);
    }
    for url in pending_thumbnails {
        app.ensure_thumbnail_loaded(url, cx);
    }

    div()
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(header_bar)
        .child(grid)
        .into_any_element()
}
