//! Single-account editor modal with saved-game quick pick, destination validation, and fallback policy.

use crate::app::{RoLauncherApp, on_action, on_select};
use crate::components::modal::modal_frame;
use crate::components::primitives::{ButtonVariant, button, checkbox, segmented_control};
use crate::components::thumbnail::render_thumbnail_box;
use crate::domain::drafts::EditorDraft;
use crate::domain::models::Target;
use crate::state::ActiveModal;
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

/// Builds modal inputs from an existing account record cleanly without retaining secrets.
pub fn open_for_account(
    app: &mut RoLauncherApp,
    account_id: &str,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) {
    let Some(acct) = app
        .snapshot
        .as_ref()
        .and_then(|s| s.accounts.iter().find(|a| a.id == account_id))
        .cloned()
    else {
        return;
    };

    let alias_val = acct.alias.clone();
    let place_val = acct
        .target
        .as_ref()
        .map(|t| t.place_id.to_string())
        .unwrap_or_default();
    let job_val = acct
        .target
        .as_ref()
        .and_then(|t| t.job_id.clone())
        .unwrap_or_default();
    let priv_val = acct
        .target
        .as_ref()
        .and_then(|t| t.private_server_link.clone())
        .unwrap_or_default();
    let group_val = acct.group.clone();
    let game_profile_id = acct.game_profile_id.clone();
    let auto_recovery = acct.auto_recovery;
    let fallback_policy = acct.fallback_policy.clone();

    let alias_input = app.new_text_input("Display alias", false, false, false, None, cx);
    alias_input.update(cx, |ti, cx| {
        ti.set_text(alias_val, cx);
        ti.focus_and_select_all(window, cx);
    });

    let place_input = app.new_text_input("Place ID (e.g. 1818)", false, false, false, None, cx);
    place_input.update(cx, |ti, cx| ti.set_text(place_val.clone(), cx));

    let job_input = app.new_text_input("Optional Job UUID", false, false, false, Some(true), cx);
    job_input.update(cx, |ti, cx| ti.set_text(job_val, cx));

    let private_input = app.new_text_input(
        "Optional https://www.roblox.com/...",
        false,
        false,
        false,
        None,
        cx,
    );
    private_input.update(cx, |ti, cx| ti.set_text(priv_val, cx));

    let group_input = app.new_text_input("Optional group name", false, false, false, None, cx);
    group_input.update(cx, |ti, cx| ti.set_text(group_val, cx));

    let lookup_preview = place_val
        .trim()
        .parse::<u64>()
        .ok()
        .and_then(|pid| app.game_cache.resolved.get(&pid).cloned());

    app.modal = ActiveModal::AccountEditor(crate::state::AccountEditorModal {
        account_id: acct.id,
        username: acct.username,
        alias_input,
        place_input,
        job_input,
        private_input,
        group_input,
        game_profile_id,
        auto_recovery,
        fallback_policy,
        lookup_preview,
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
    let games = app
        .snapshot
        .as_ref()
        .map(|s| s.game_profiles.clone())
        .unwrap_or_default();
    let busy = app.busy;

    let ActiveModal::AccountEditor(ed) = &app.modal else {
        return div().into_any_element();
    };

    let username = ed.username.clone();
    let error = ed.error.clone();
    let game_profile_id = ed.game_profile_id.clone();
    let auto_recovery = ed.auto_recovery;
    let fallback_policy = ed.fallback_policy.clone();
    let preview = ed.lookup_preview.clone();
    let alias_el = ed.alias_input.clone();
    let place_el = ed.place_input.clone();
    let job_el = ed.job_input.clone();
    let priv_el = ed.private_input.clone();
    let group_el = ed.group_input.clone();

    let mut game_chips = div().flex().flex_row().flex_wrap().gap(px(6.0));
    game_chips = game_chips.child(button(
        "acct-ed-custom",
        "Custom destination",
        if game_profile_id.is_none() {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Secondary
        },
        busy,
        true,
        &theme,
        &density,
        on_action(cx, |this, _w, cx| {
            if let ActiveModal::AccountEditor(ed) = &mut this.modal {
                ed.game_profile_id = None;
                cx.notify();
            }
        }),
    ));
    for (idx, game) in games.iter().enumerate() {
        let selected = game_profile_id.as_deref() == Some(game.id.as_str());
        let g = game.clone();
        game_chips = game_chips.child(button(
            ("acct-ed-game", idx),
            g.name.clone(),
            if selected {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            },
            busy,
            true,
            &theme,
            &density,
            on_action(cx, move |this, _w, cx| {
                if let ActiveModal::AccountEditor(ed) = &mut this.modal {
                    ed.game_profile_id = Some(g.id.clone());
                    ed.place_input.update(cx, |ti, cx| {
                        ti.set_text(g.target.place_id.to_string(), cx);
                    });
                    ed.job_input.update(cx, |ti, cx| {
                        ti.set_text(g.target.job_id.clone().unwrap_or_default(), cx);
                    });
                    ed.private_input.update(cx, |ti, cx| {
                        ti.set_text(g.target.private_server_link.clone().unwrap_or_default(), cx);
                    });
                    let pid = g.target.place_id;
                    if let Some(cached) = this.game_cache.resolved.get(&pid).cloned() {
                        ed.lookup_preview = Some(cached);
                    } else {
                        this.fetch_game_preview_for_modal(pid, cx);
                    }
                    cx.notify();
                }
            }),
        ));
    }

    let preview_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .p(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_surface_elevated)
        .child(render_thumbnail_box(
            &app.thumbnails,
            preview.as_ref().and_then(|p| p.thumbnail_url.as_deref()),
            44.0,
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
                            preview.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| {
                                "Enter Place ID to preview game metadata".into()
                            }),
                        ),
                )
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_muted)
                        .child("Click Look up after changing Place ID to verify name and icon."),
                ),
        )
        .child(button(
            "acct-ed-lookup",
            "Look up",
            ButtonVariant::Secondary,
            busy,
            true,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                let place_txt = if let ActiveModal::AccountEditor(ed) = &this.modal {
                    ed.place_input.read(cx).text().trim().to_string()
                } else {
                    return;
                };
                if let Ok(pid) = place_txt.parse::<u64>()
                    && pid > 0
                {
                    this.fetch_game_preview_for_modal(pid, cx);
                } else if let ActiveModal::AccountEditor(ed) = &mut this.modal {
                    ed.error = Some("Enter a positive Place ID before looking up.".into());
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
        .child(field_block("Display alias", alias_el, &theme, &density))
        .child(field_block("Group", group_el, &theme, &density))
        .when(!games.is_empty(), |col| {
            col.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(field_label("Saved game shortcut", &theme, &density))
                    .child(game_chips),
            )
        })
        .child(preview_row)
        .child(field_block("Place ID", place_el, &theme, &density))
        .child(field_block("Server Job ID", job_el, &theme, &density))
        .child(field_block(
            "Private server link",
            priv_el,
            &theme,
            &density,
        ))
        .child(checkbox(
            "acct-ed-rejoin",
            auto_recovery,
            Some("Automatic rejoin if Roblox exits or disconnects".into()),
            busy,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                if let ActiveModal::AccountEditor(ed) = &mut this.modal {
                    ed.auto_recovery = !ed.auto_recovery;
                    cx.notify();
                }
            }),
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(field_label(
                    "If destination is unavailable",
                    &theme,
                    &density,
                ))
                .child(segmented_control(
                    "acct-ed-fallback",
                    &[
                        ("allow_public", "Join public server"),
                        ("stay", "Keep retrying"),
                        ("pause", "Stop and wait"),
                    ],
                    &fallback_policy,
                    busy,
                    &theme,
                    &density,
                    on_select(cx, |this, val, _w, cx| {
                        if let ActiveModal::AccountEditor(ed) = &mut this.modal {
                            ed.fallback_policy = val;
                            cx.notify();
                        }
                    }),
                )),
        )
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .w_full()
        .child(button(
            "acct-ed-clear",
            "Clear destination",
            ButtonVariant::Danger,
            busy,
            false,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.submit_account_editor(true, cx);
            }),
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(8.0))
                .child(button(
                    "acct-ed-cancel",
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
                    "acct-ed-save",
                    if busy { "Saving…" } else { "Save changes" },
                    ButtonVariant::Primary,
                    busy,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| {
                        this.submit_account_editor(false, cx);
                    }),
                )),
        )
        .into_any_element();

    modal_frame(
        "account-editor-modal",
        "Edit account",
        Some(format!("@{username}").into()),
        600.0,
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

fn field_label(
    label: &str,
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
) -> gpui::Div {
    div()
        .text_size(px(density.font_caption))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme.text_secondary)
        .child(label.to_string())
}

fn field_block(
    label: &str,
    el: impl IntoElement,
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(field_label(label, theme, density))
        .child(el)
}

/// Validates and builds the `PATCH /v1/accounts/{id}` JSON payload from editor inputs.
pub fn validate_account_editor_patch(
    draft: &EditorDraft,
    group: &str,
    game_profile_id: Option<&str>,
    auto_recovery: bool,
    fallback_policy: &str,
    clear_target: bool,
) -> Result<(serde_json::Value, Option<Target>), String> {
    if draft.alias.chars().count() > 100 || draft.alias.chars().any(char::is_control) {
        return Err("Display alias: max 100 characters; no control characters.".into());
    }
    if group.chars().count() > 100 || group.chars().any(char::is_control) {
        return Err("Group: max 100 characters; no control characters.".into());
    }

    if clear_target {
        let mut map = serde_json::Map::new();
        map.insert("alias".into(), serde_json::json!(draft.alias));
        map.insert("group".into(), serde_json::json!(group));
        map.insert("auto_recovery".into(), serde_json::json!(auto_recovery));
        map.insert("fallback_policy".into(), serde_json::json!(fallback_policy));
        map.insert("clear_target".into(), serde_json::json!(true));
        return Ok((serde_json::Value::Object(map), None));
    }

    let empty_target = draft.place.trim().is_empty()
        && draft.job.trim().is_empty()
        && draft.private.trim().is_empty();
    let target = if empty_target {
        None
    } else {
        Some(draft.try_target().map_err(str::to_string)?)
    };
    let mut map = serde_json::Map::new();
    map.insert("alias".into(), serde_json::json!(draft.alias));
    map.insert("group".into(), serde_json::json!(group));
    map.insert("auto_recovery".into(), serde_json::json!(auto_recovery));
    map.insert("fallback_policy".into(), serde_json::json!(fallback_policy));
    match target.as_ref() {
        Some(t) => {
            map.insert("target".into(), serde_json::to_value(t).unwrap());
            if let Some(gp) = game_profile_id {
                map.insert("game_profile_id".into(), serde_json::json!(gp));
            }
        }
        None => {
            map.insert("clear_target".into(), serde_json::json!(true));
        }
    }
    Ok((serde_json::Value::Object(map), target))
}
