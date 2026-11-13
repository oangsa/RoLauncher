//! Account presets page: preset roster selection, Review & Launch, import/export, and creation builder.

use crate::app::{RoLauncherApp, on_action};
use crate::components::menu::{menu_item, popover, popover_box};
use crate::components::primitives::{ButtonVariant, button, card};
use crate::state::{ActiveModal, ConfirmAction, ConfirmModal, OpenMenu, PresetReviewModal};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;
    let open_menu = app.open_menu;
    let presets = app
        .snapshot
        .as_ref()
        .map(|s| s.profiles.clone())
        .unwrap_or_default();
    let sel_id = app
        .selected_preset_id
        .as_ref()
        .filter(|id| presets.iter().any(|p| &p.id == *id))
        .cloned()
        .or_else(|| presets.first().map(|p| p.id.clone()));

    let active_preset = sel_id
        .as_ref()
        .and_then(|id| presets.iter().find(|p| &p.id == id))
        .cloned();

    let mut picker_chips = div().flex().flex_row().flex_wrap().gap(px(6.0));
    for (idx, p) in presets.iter().enumerate() {
        let is_active = active_preset.as_ref().is_some_and(|a| a.id == p.id);
        let pid = p.id.clone();
        picker_chips = picker_chips.child(button(
            ("preset-pick", idx),
            format!("{} ({})", p.name, p.entries.len()),
            if is_active {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            },
            busy,
            true,
            &theme,
            &density,
            on_action(cx, move |this, _w, cx| {
                this.selected_preset_id = Some(pid.clone());
                cx.notify();
            }),
        ));
    }

    let chosen_preset_id = active_preset.as_ref().map(|p| p.id.clone());
    let chosen_preset_name = active_preset.as_ref().map(|p| p.name.clone());
    let has_selection = !app.selected_account_ids.is_empty();

    let saved_card = card(&theme, &density)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .child(
                    div()
                        .text_size(px(density.font_section))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .child("Use saved preset"),
                )
                .child(
                    div()
                        .id("preset-more-anchor")
                        .relative()
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(button(
                            "preset-more-btn",
                            "More ▾",
                            ButtonVariant::Secondary,
                            busy,
                            false,
                            &theme,
                            &density,
                            on_action(cx, |this, _w, cx| {
                                this.open_menu = if this.open_menu == Some(OpenMenu::PresetsMore) {
                                    None
                                } else {
                                    Some(OpenMenu::PresetsMore)
                                };
                                cx.notify();
                            }),
                        ))
                        .when(open_menu == Some(OpenMenu::PresetsMore), |wrap| {
                            let pid_rep = chosen_preset_id.clone();
                            let pname_rep = chosen_preset_name.clone();
                            let pid_del = chosen_preset_id.clone();
                            let pname_del = chosen_preset_name.clone();
                            let has_presets = !presets.is_empty();

                            wrap.child(popover(true, popover_box(&theme).w(px(264.0))
                                    .child(menu_item(
                                        "pm-replace",
                                        "Replace with Accounts selection".into(),
                                        pid_rep.is_none() || !has_selection,
                                        &theme,
                                        &density,
                                        cx.listener(move |this, _ev, _w, cx| {
                                            this.open_menu = None;
                                            if let (Some(pid), Some(pname)) = (&pid_rep, &pname_rep) {
                                                let ids: Vec<String> =
                                                    this.selected_account_ids.iter().cloned().collect();
                                                this.save_preset_by_id(Some(pid.clone()), pname.clone(), ids, cx);
                                            }
                                        }),
                                    ))
                                    .child(menu_item(
                                        "pm-export",
                                        "Export presets JSON…".into(),
                                        !has_presets,
                                        &theme,
                                        &density,
                                        cx.listener(|this, _ev, _w, cx| {
                                            this.open_menu = None;
                                            this.modal = ActiveModal::Confirm(ConfirmModal {
                                                title: "Export presets".into(),
                                                body: "Exported preset JSON contains account IDs, aliases, and saved destinations without passwords or session cookies. Continue?".into(),
                                                confirm_label: "Choose export file".into(),
                                                dangerous: false,
                                                action: ConfirmAction::ExportPresetsNotice,
                                            });
                                            cx.notify();
                                        }),
                                    ))
                                    .child(menu_item(
                                        "pm-import",
                                        "Import presets JSON…".into(),
                                        false,
                                        &theme,
                                        &density,
                                        cx.listener(|this, _ev, window, cx| {
                                            this.open_menu = None;
                                            this.import_presets_from_file(window, cx);
                                            cx.notify();
                                        }),
                                    ))
                                    .child(menu_item(
                                        "pm-delete",
                                        "Delete preset…".into(),
                                        pid_del.is_none(),
                                        &theme,
                                        &density,
                                        cx.listener(move |this, _ev, _w, cx| {
                                            this.open_menu = None;
                                            if let (Some(pid), Some(pname)) = (&pid_del, &pname_del) {
                                                this.modal = ActiveModal::Confirm(ConfirmModal {
                                                    title: "Delete preset".into(),
                                                    body: format!("Delete saved preset \"{pname}\"?"),
                                                    confirm_label: "Delete".into(),
                                                    dangerous: true,
                                                    action: ConfirmAction::DeletePreset(pid.clone()),
                                                });
                                                cx.notify();
                                            }
                                        }),
                                    )),
                            ))
                        }),
                ),
        )
        .when(presets.is_empty(), |c| {
            c.child(
                div()
                    .text_size(px(density.font_body))
                    .text_color(theme.text_muted)
                    .child("No saved presets yet. Create one below or import an exported JSON file via More ▾."),
            )
        })
        .when(!presets.is_empty(), |c| c.child(picker_chips))
        .when_some(active_preset.clone(), |c, preset| {
            let pid_apply = preset.id.clone();
            let pid_start = preset.id.clone();
            let preview_btn_preset = preset.clone();
            c.child(
                div()
                    .p(px(10.0))
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(theme.border_subtle)
                    .bg(theme.bg_surface_elevated)
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(10.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(density.font_body))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme.text_primary)
                                    .child(preset.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(density.font_caption))
                                    .text_color(theme.text_secondary)
                                    .child(format!(
                                        "{} account snapshot(s) ready to apply or launch.",
                                        preset.entries.len()
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap(px(8.0))
                            .child(button(
                                "preset-review-btn",
                                "Review & launch",
                                ButtonVariant::Secondary,
                                busy || preset.entries.is_empty(),
                                false,
                                &theme,
                                &density,
                                on_action(cx, move |this, _w, cx| {
                                    this.modal = ActiveModal::PresetReview(PresetReviewModal {
                                        preset: preview_btn_preset.clone(),
                                    });
                                    cx.notify();
                                }),
                            ))
                            .child(button(
                                "preset-apply-btn",
                                "Apply settings",
                                ButtonVariant::Secondary,
                                busy || preset.entries.is_empty(),
                                false,
                                &theme,
                                &density,
                                on_action(cx, move |this, _w, cx| {
                                    this.run_preset_action(&pid_apply, "apply", false, cx);
                                }),
                            ))
                            .child(button(
                                "preset-start-btn",
                                "Launch preset",
                                ButtonVariant::Primary,
                                busy || preset.entries.is_empty(),
                                false,
                                &theme,
                                &density,
                                on_action(cx, move |this, _w, cx| {
                                    this.run_preset_action(&pid_start, "start", false, cx);
                                }),
                            )),
                    ),
            )
        });

    let chosen_count = app.preset_draft_account_ids.len();
    let create_card = card(&theme, &density)
        .child(
            div()
                .text_size(px(density.font_section))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_primary)
                .child("Create preset"),
        )
        .child(
            div()
                .text_size(px(density.font_caption))
                .text_color(theme.text_secondary)
                .child("Captures each chosen account's current destination, automatic rejoin setting, and unavailable fallback rule."),
        )
        .child(app.preset_name_input.clone())
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(button(
                            "preset-choose-accts",
                            if chosen_count == 0 {
                                "Choose accounts…".into()
                            } else {
                                format!("Edit chosen accounts ({chosen_count})…")
                            },
                            ButtonVariant::Secondary,
                            busy,
                            false,
                            &theme,
                            &density,
                            on_action(cx, |this, window, cx| {
                                crate::dialogs::preset_lookup::open(this, window, cx);
                            }),
                        ))
                        .child(button(
                            "preset-use-selection",
                            "Use Accounts selection",
                            ButtonVariant::Subtle,
                            busy || !has_selection,
                            false,
                            &theme,
                            &density,
                            on_action(cx, |this, _w, cx| {
                                this.preset_draft_account_ids = this.selected_account_ids.clone();
                                cx.notify();
                            }),
                        )),
                )
                .child(button(
                    "preset-save-new",
                    if busy { "Saving…" } else { "Save preset" },
                    ButtonVariant::Primary,
                    busy || chosen_count == 0,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| {
                        let name = this.preset_name_input.read(cx).text().trim().to_string();
                        if name.is_empty() {
                            this.show_error("Enter a preset name before saving.", cx);
                            return;
                        }
                        let ids: Vec<String> = this.preset_draft_account_ids.iter().cloned().collect();
                        this.save_preset_by_id(None, name, ids, cx);
                    }),
                )),
        );

    div()
        .id("presets-page")
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .on_click(cx.listener(|this, _ev, _w, cx| {
            this.open_menu = None;
            cx.notify();
        }))
        .child(saved_card)
        .child(create_card)
        .into_any_element()
}
