//! Generic confirmation modal for destructive, privacy-sensitive, and restart actions.

use crate::app::{RoLauncherApp, on_action};
use crate::components::modal::modal_frame;
use crate::components::primitives::{ButtonVariant, button};
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

    let ActiveModal::Confirm(cm) = &app.modal else {
        return div().into_any_element();
    };
    let cm = cm.clone();

    let body = div()
        .text_size(px(density.font_body))
        .text_color(theme.text_secondary)
        .child(cm.body)
        .into_any_element();

    let action = cm.action.clone();
    let footer = div()
        .flex()
        .flex_row()
        .gap(px(8.0))
        .child(button(
            "confirm-cancel",
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
            "confirm-ok",
            if busy {
                "Working…".to_string()
            } else {
                cm.confirm_label
            },
            if cm.dangerous {
                ButtonVariant::Danger
            } else {
                ButtonVariant::Primary
            },
            busy,
            false,
            &theme,
            &density,
            on_action(cx, move |this, window, cx| {
                this.execute_confirmed_action(action.clone(), window, cx);
            }),
        ))
        .into_any_element();

    modal_frame(
        "confirm-modal",
        cm.title,
        None,
        480.0,
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
