//! Create / edit saved Roblox game destination dialog with metadata lookup.

use crate::app::{RoLauncherApp, on_action};
use crate::components::modal::modal_frame;
use crate::components::primitives::{ButtonVariant, button};
use crate::components::thumbnail::render_thumbnail_box;
use crate::domain::drafts::EditorDraft;
use crate::domain::models::GameProfile;
use crate::state::{ActiveModal, GameEditorModal};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

/// Opens the Saved Game dialog for creating a new game (`existing = None`) or editing one.
pub fn open(
    app: &mut RoLauncherApp,
    existing: Option<&GameProfile>,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) {
    let id = existing
        .map(|g| g.id.clone())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let name = existing.map(|g| g.name.clone()).unwrap_or_default();
    let place = existing
        .map(|g| g.target.place_id.to_string())
        .unwrap_or_default();
    let job = existing
        .and_then(|g| g.target.job_id.clone())
        .unwrap_or_default();
    let priv_link = existing
        .and_then(|g| g.target.private_server_link.clone())
        .unwrap_or_default();

    let name_input = app.new_text_input("Game profile name", false, false, false, None, cx);
    name_input.update(cx, |ti, cx| {
        ti.set_text(name, cx);
        ti.focus_and_select_all(window, cx);
    });
    let place_input = app.new_text_input("Place ID", false, false, false, None, cx);
    place_input.update(cx, |ti, cx| ti.set_text(place.clone(), cx));
    let job_input = app.new_text_input("Optional Job UUID", false, false, false, Some(true), cx);
    job_input.update(cx, |ti, cx| ti.set_text(job, cx));
    let private_input = app.new_text_input(
        "Optional private server link",
        false,
        false,
        false,
        None,
        cx,
    );
    private_input.update(cx, |ti, cx| ti.set_text(priv_link, cx));

    let lookup = place
        .trim()
        .parse::<u64>()
        .ok()
        .and_then(|pid| app.game_cache.resolved.get(&pid).cloned());

    app.modal = ActiveModal::GameEditor(GameEditorModal {
        id,
        is_new: existing.is_none(),
        name_input,
        place_input,
        job_input,
        private_input,
        lookup,
        error: None,
    });
    cx.notify();
}

pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;

    let ActiveModal::GameEditor(ge) = &app.modal else {
        return div().into_any_element();
    };

    let is_new = ge.is_new;
    let error = ge.error.clone();
    let lookup = ge.lookup.clone();
    let name_el = ge.name_input.clone();
    let place_el = ge.place_input.clone();
    let job_el = ge.job_input.clone();
    let priv_el = ge.private_input.clone();

    let preview = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .p(px(10.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_surface_elevated)
        .child(render_thumbnail_box(
            &app.thumbnails,
            lookup.as_ref().and_then(|l| l.thumbnail_url.as_deref()),
            48.0,
            &theme,
        ))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(density.font_body))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme.text_primary)
                        .child(
                            lookup
                                .as_ref()
                                .map(|l| l.name.clone())
                                .unwrap_or_else(|| "Enter Place ID and look up metadata".into()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_muted)
                        .child("Name autofills if empty when Place ID is resolved."),
                ),
        )
        .child(button(
            "game-ed-lookup",
            "Look up",
            ButtonVariant::Secondary,
            busy,
            true,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                let place_txt = if let ActiveModal::GameEditor(ge) = &this.modal {
                    ge.place_input.read(cx).text().trim().to_string()
                } else {
                    return;
                };
                if let Ok(pid) = place_txt.parse::<u64>()
                    && pid > 0
                {
                    this.fetch_game_preview_for_modal(pid, cx);
                } else if let ActiveModal::GameEditor(ge) = &mut this.modal {
                    ge.error = Some("Enter a positive Place ID first.".into());
                    cx.notify();
                }
            }),
        ));

    let body = div()
        .flex()
        .flex_col()
        .gap(px(12.0))
        .when_some(error, |col, err| {
            col.child(
                div()
                    .p(px(10.0))
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(theme.danger_border)
                    .bg(theme.danger_bg)
                    .text_size(px(density.font_caption))
                    .text_color(theme.danger)
                    .child(err),
            )
        })
        .child(preview)
        .child(labeled_box("Saved game name", name_el, &theme, &density))
        .child(labeled_box("Place ID", place_el, &theme, &density))
        .child(labeled_box("Optional Job UUID", job_el, &theme, &density))
        .child(labeled_box(
            "Optional private server link",
            priv_el,
            &theme,
            &density,
        ))
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap(px(8.0))
        .child(button(
            "game-ed-cancel",
            "Cancel",
            ButtonVariant::Secondary,
            busy,
            false,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.modal = ActiveModal::None;
                cx.notify();
            }),
        ))
        .child(button(
            "game-ed-save",
            if busy { "Saving…" } else { "Save game" },
            ButtonVariant::Primary,
            busy,
            false,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.submit_game_editor(cx);
            }),
        ))
        .into_any_element();

    modal_frame(
        "game-editor-modal",
        if is_new {
            "Add saved game"
        } else {
            "Edit saved game"
        },
        None,
        560.0,
        !busy,
        &theme,
        &density,
        on_action(cx, |this, _w, cx| {
            this.modal = ActiveModal::None;
            cx.notify();
        }),
        body,
        footer,
    )
}

fn labeled_box(
    label: &str,
    el: impl IntoElement,
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(density.font_caption))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text_secondary)
                .child(label.to_string()),
        )
        .child(el)
}

/// Validates and converts modal inputs into a backend-ready [`GameProfile`].
pub fn build_game_profile(ge: &GameEditorModal, cx: &gpui::App) -> Result<GameProfile, String> {
    let name = ge.name_input.read(cx).text().trim().to_string();
    if name.is_empty() || name.len() > 100 || name.chars().any(char::is_control) {
        return Err("Enter a saved game name (1–100 characters, no control chars).".into());
    }
    let draft = EditorDraft {
        alias: String::new(),
        place: ge.place_input.read(cx).text().to_string(),
        job: ge.job_input.read(cx).text().to_string(),
        private: ge.private_input.read(cx).text().to_string(),
    };
    let target = draft.try_target().map_err(str::to_string)?;
    let game_name = ge
        .lookup
        .as_ref()
        .map(|l| l.name.clone())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| format!("Place {}", target.place_id));
    Ok(GameProfile {
        id: ge.id.clone(),
        name,
        game_name,
        target,
        thumbnail_url: ge.lookup.as_ref().and_then(|l| l.thumbnail_url.clone()),
    })
}
