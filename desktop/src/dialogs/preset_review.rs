//! Preset review dialog inspecting saved destinations before applying or launching.

use crate::app::{RoLauncherApp, on_action};
use crate::components::modal::modal_frame;
use crate::components::primitives::{ButtonVariant, button};
use crate::domain::models::format_fallback_policy;
use crate::state::ActiveModal;
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;
    let existing_ids: std::collections::HashSet<String> = app
        .snapshot
        .as_ref()
        .map(|s| s.accounts.iter().map(|a| a.id.clone()).collect())
        .unwrap_or_default();

    let ActiveModal::PresetReview(pr) = &app.modal else {
        return div().into_any_element();
    };
    let preset = pr.preset.clone();
    let pid = preset.id.clone();
    let pid_apply = pid.clone();
    let pid_start = pid.clone();

    let mut entries_list = div()
        .id("pr-entries-list")
        .flex()
        .flex_col()
        .gap(px(6.0))
        .max_h(px(300.0))
        .overflow_y_scroll();

    for entry in &preset.entries {
        let missing = !existing_ids.contains(&entry.account_id);
        let dest = entry
            .target
            .as_ref()
            .map(|t| format!("{} · Place {}", t.kind_label(), t.place_id))
            .unwrap_or_else(|| "No destination".into());
        let name = if entry.alias.trim().is_empty() {
            entry.account_id.clone()
        } else {
            entry.alias.clone()
        };
        entries_list = entries_list.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .p(px(8.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(if missing {
                    theme.warning_border
                } else {
                    theme.border_subtle
                })
                .bg(theme.bg_surface_elevated)
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
                                .child(if missing {
                                    format!("{name} (missing from current accounts)")
                                } else {
                                    name
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(density.font_caption))
                                .text_color(theme.text_secondary)
                                .child(format!(
                                    "{dest} · Rejoin: {} · Fallback: {}",
                                    if entry.auto_recovery { "On" } else { "Off" },
                                    format_fallback_policy(&entry.fallback_policy)
                                )),
                        ),
                ),
        );
    }

    let body = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .text_size(px(density.font_body))
                .text_color(theme.text_secondary)
                .child(format!(
                    "{} saved account snapshot(s) in \"{}\".",
                    preset.entries.len(),
                    preset.name
                )),
        )
        .child(entries_list)
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap(px(8.0))
        .child(button(
            "pr-close",
            "Close",
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
            "pr-apply",
            "Apply settings",
            ButtonVariant::Secondary,
            busy || preset.entries.is_empty(),
            false,
            &theme,
            &density,
            on_action(cx, move |this, _w, cx| {
                this.run_preset_action(&pid_apply, "apply", true, cx);
            }),
        ))
        .child(button(
            "pr-launch",
            if busy { "Starting…" } else { "Launch preset" },
            ButtonVariant::Primary,
            busy || preset.entries.is_empty(),
            false,
            &theme,
            &density,
            on_action(cx, move |this, _w, cx| {
                this.run_preset_action(&pid_start, "start", true, cx);
            }),
        ))
        .into_any_element();

    modal_frame(
        "preset-review-modal",
        format!("Review preset: {}", preset.name),
        None,
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
