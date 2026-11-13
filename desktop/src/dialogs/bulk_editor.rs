//! Bulk account editor modal with per-field Apply toggles and three-state preservation.

use crate::app::{RoLauncherApp, on_action, on_select};
use crate::components::modal::modal_frame;
use crate::components::primitives::{ButtonVariant, button, checkbox, segmented_control};
use crate::domain::drafts::{BulkDraft, EditorDraft, common_field};
use crate::state::{ActiveModal, BulkEditorModal};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

/// Opens the bulk editor frozen to the currently specified `account_ids`.
pub fn open_for_ids(
    app: &mut RoLauncherApp,
    ids: Vec<String>,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) {
    let Some(snapshot) = app.snapshot.as_ref() else {
        return;
    };
    let selected: Vec<_> = snapshot
        .accounts
        .iter()
        .filter(|a| ids.iter().any(|id| id == &a.id))
        .cloned()
        .collect();
    if selected.is_empty() {
        return;
    }

    let account_labels: Vec<String> = selected
        .iter()
        .map(|a| format!("{} (@{})", a.display_name(), a.username))
        .collect();
    let frozen_ids: Vec<String> = selected.iter().map(|a| a.id.clone()).collect();

    let common_group = common_field(&selected, |a| a.group.as_str());
    let first_place = selected
        .first()
        .and_then(|a| a.target.as_ref())
        .map(|t| t.place_id);
    let common_place = if selected
        .iter()
        .all(|a| a.target.as_ref().map(|t| t.place_id) == first_place)
    {
        first_place.map(|p| p.to_string()).unwrap_or_default()
    } else {
        String::new()
    };
    let common_rejoin = selected.first().map(|a| a.auto_recovery).unwrap_or(true);
    let common_fallback = {
        let fb = common_field(&selected, |a| a.fallback_policy.as_str());
        if fb.is_empty() {
            "allow_public".into()
        } else {
            fb
        }
    };

    let alias_input = app.new_text_input(
        "Pattern e.g. Farm {index} ({username})",
        false,
        false,
        false,
        None,
        cx,
    );
    let place_input = app.new_text_input("Place ID", false, false, false, None, cx);
    place_input.update(cx, |ti, cx| ti.set_text(common_place, cx));
    let job_input = app.new_text_input("Optional Job UUID", false, false, false, Some(true), cx);
    let private_input = app.new_text_input(
        "Optional private server link",
        false,
        false,
        false,
        None,
        cx,
    );
    let group_input = app.new_text_input(
        "Leave blank to remove from groups",
        false,
        false,
        false,
        None,
        cx,
    );
    group_input.update(cx, |ti, cx| ti.set_text(common_group, cx));
    let _ = window;

    app.modal = ActiveModal::BulkEditor(BulkEditorModal {
        account_ids: frozen_ids,
        account_labels,
        show_included: false,
        apply_alias: false,
        alias_input,
        apply_target: false,
        clear_target: false,
        game_profile_id: None,
        place_input,
        job_input,
        private_input,
        apply_rejoin: false,
        rejoin: common_rejoin,
        apply_fallback: false,
        fallback: common_fallback,
        apply_group: false,
        group_input,
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
    let games = app
        .snapshot
        .as_ref()
        .map(|s| s.game_profiles.clone())
        .unwrap_or_default();

    let ActiveModal::BulkEditor(bm) = &app.modal else {
        return div().into_any_element();
    };

    let count = bm.account_ids.len();
    let labels = bm.account_labels.clone();
    let show_included = bm.show_included;
    let apply_alias = bm.apply_alias;
    let apply_target = bm.apply_target;
    let clear_target = bm.clear_target;
    let game_profile_id = bm.game_profile_id.clone();
    let apply_rejoin = bm.apply_rejoin;
    let rejoin = bm.rejoin;
    let apply_fallback = bm.apply_fallback;
    let fallback = bm.fallback.clone();
    let apply_group = bm.apply_group;
    let error = bm.error.clone();

    let alias_el = bm.alias_input.clone();
    let place_el = bm.place_input.clone();
    let job_el = bm.job_input.clone();
    let priv_el = bm.private_input.clone();
    let group_el = bm.group_input.clone();

    let mut game_chips = div().flex().flex_row().flex_wrap().gap(px(6.0));
    for (idx, game) in games.iter().enumerate() {
        let selected = !clear_target && game_profile_id.as_deref() == Some(game.id.as_str());
        let g = game.clone();
        game_chips = game_chips.child(button(
            ("bulk-game", idx),
            g.name.clone(),
            if selected {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            },
            busy || !apply_target || clear_target,
            true,
            &theme,
            &density,
            on_action(cx, move |this, _w, cx| {
                if let ActiveModal::BulkEditor(bm) = &mut this.modal {
                    bm.clear_target = false;
                    bm.game_profile_id = Some(g.id.clone());
                    bm.place_input
                        .update(cx, |ti, cx| ti.set_text(g.target.place_id.to_string(), cx));
                    bm.job_input.update(cx, |ti, cx| {
                        ti.set_text(g.target.job_id.clone().unwrap_or_default(), cx)
                    });
                    bm.private_input.update(cx, |ti, cx| {
                        ti.set_text(g.target.private_server_link.clone().unwrap_or_default(), cx)
                    });
                    cx.notify();
                }
            }),
        ));
    }

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
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .p(px(8.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.border_subtle)
                .bg(theme.bg_surface_elevated)
                .child(button(
                    "bulk-toggle-included",
                    if show_included {
                        format!("Hide included accounts ({count})")
                    } else {
                        format!("Show included accounts ({count})")
                    },
                    ButtonVariant::Subtle,
                    false,
                    true,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| {
                        if let ActiveModal::BulkEditor(bm) = &mut this.modal {
                            bm.show_included = !bm.show_included;
                            cx.notify();
                        }
                    }),
                ))
                .when(show_included, |box_col| {
                    let mut list = div()
                        .id("bulk-included-list")
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .max_h(px(110.0))
                        .overflow_y_scroll();
                    for lbl in labels {
                        list = list.child(
                            div()
                                .text_size(px(density.font_caption))
                                .text_color(theme.text_secondary)
                                .child(lbl),
                        );
                    }
                    box_col.child(list)
                }),
        )
        // Alias
        .child(bulk_section(
            "bulk-apply-alias",
            "Apply display alias pattern",
            apply_alias,
            busy,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                if let ActiveModal::BulkEditor(bm) = &mut this.modal {
                    bm.apply_alias = !bm.apply_alias;
                    cx.notify();
                }
            }),
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(alias_el)
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_muted)
                        .child("Supports {username}, {id}, and 1-based {index}."),
                )
                .into_any_element(),
        ))
        // Destination
        .child(bulk_section(
            "bulk-apply-target",
            "Apply game destination",
            apply_target,
            busy,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                if let ActiveModal::BulkEditor(bm) = &mut this.modal {
                    bm.apply_target = !bm.apply_target;
                    cx.notify();
                }
            }),
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(checkbox(
                    "bulk-clear-target",
                    clear_target,
                    Some("Clear destination from selected accounts".into()),
                    busy,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| {
                        if let ActiveModal::BulkEditor(bm) = &mut this.modal {
                            bm.clear_target = !bm.clear_target;
                            if bm.clear_target {
                                bm.game_profile_id = None;
                            }
                            cx.notify();
                        }
                    }),
                ))
                .when(!clear_target && !games.is_empty(), |col| {
                    col.child(game_chips)
                })
                .when(!clear_target, |col| {
                    col.child(place_el).child(job_el).child(priv_el)
                })
                .into_any_element(),
        ))
        // Automatic rejoin
        .child(bulk_section(
            "bulk-apply-rejoin",
            "Apply automatic rejoin",
            apply_rejoin,
            busy,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                if let ActiveModal::BulkEditor(bm) = &mut this.modal {
                    bm.apply_rejoin = !bm.apply_rejoin;
                    cx.notify();
                }
            }),
            segmented_control(
                "bulk-rejoin-val",
                &[
                    ("on", "Automatic rejoin On"),
                    ("off", "Automatic rejoin Off"),
                ],
                if rejoin { "on" } else { "off" },
                busy,
                &theme,
                &density,
                on_select(cx, |this, val, _w, cx| {
                    if let ActiveModal::BulkEditor(bm) = &mut this.modal {
                        bm.rejoin = val == "on";
                        cx.notify();
                    }
                }),
            ),
        ))
        // Fallback
        .child(bulk_section(
            "bulk-apply-fallback",
            "Apply unavailable-server fallback",
            apply_fallback,
            busy,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                if let ActiveModal::BulkEditor(bm) = &mut this.modal {
                    bm.apply_fallback = !bm.apply_fallback;
                    cx.notify();
                }
            }),
            segmented_control(
                "bulk-fallback-val",
                &[
                    ("allow_public", "Join public"),
                    ("stay", "Keep retrying"),
                    ("pause", "Stop and wait"),
                ],
                &fallback,
                busy,
                &theme,
                &density,
                on_select(cx, |this, val, _w, cx| {
                    if let ActiveModal::BulkEditor(bm) = &mut this.modal {
                        bm.fallback = val;
                        cx.notify();
                    }
                }),
            ),
        ))
        // Group
        .child(bulk_section(
            "bulk-apply-group",
            "Apply account group",
            apply_group,
            busy,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                if let ActiveModal::BulkEditor(bm) = &mut this.modal {
                    bm.apply_group = !bm.apply_group;
                    cx.notify();
                }
            }),
            group_el.into_any_element(),
        ))
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap(px(8.0))
        .child(button(
            "bulk-cancel",
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
            "bulk-save",
            if busy { "Applying…" } else { "Apply changes" },
            ButtonVariant::Primary,
            busy,
            false,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.submit_bulk_editor(cx);
            }),
        ))
        .into_any_element();

    modal_frame(
        "bulk-editor-modal",
        format!("Edit {count} selected accounts"),
        Some("Only checked fields are updated across selected accounts.".into()),
        620.0,
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

#[allow(clippy::too_many_arguments)]
fn bulk_section(
    check_id: &'static str,
    label: &'static str,
    enabled: bool,
    busy: bool,
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
    on_toggle: impl Fn(&mut Window, &mut gpui::App) + 'static,
    content: AnyElement,
) -> gpui::Div {
    let mut wrap = div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(10.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_surface_elevated)
        .child(checkbox(
            check_id,
            enabled,
            Some(label.into()),
            busy,
            theme,
            density,
            on_toggle,
        ));
    if enabled {
        wrap = wrap.child(content);
    }
    wrap
}

/// Validates active bulk editor inputs using [`BulkDraft::try_patch`].
pub fn build_bulk_payload(
    bm: &BulkEditorModal,
    cx: &gpui::App,
) -> Result<(Vec<String>, serde_json::Value), String> {
    let draft = BulkDraft {
        apply_alias: bm.apply_alias,
        apply_target: bm.apply_target,
        clear_target: bm.clear_target,
        game_profile_id: bm.game_profile_id.clone(),
        apply_rejoin: bm.apply_rejoin,
        rejoin: Some(bm.rejoin),
        apply_fallback: bm.apply_fallback,
        fallback: Some(bm.fallback.clone()),
        apply_group: bm.apply_group,
        group: bm.group_input.read(cx).text().to_string(),
        fields: EditorDraft {
            alias: bm.alias_input.read(cx).text().to_string(),
            place: bm.place_input.read(cx).text().to_string(),
            job: bm.job_input.read(cx).text().to_string(),
            private: bm.private_input.read(cx).text().to_string(),
        },
    };
    let patch = draft.try_patch()?;
    Ok((bm.account_ids.clone(), serde_json::Value::Object(patch)))
}
