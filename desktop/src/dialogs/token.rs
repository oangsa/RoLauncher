//! Controlled read-only Local API bearer token reveal dialog.

use crate::app::{RoLauncherApp, on_action};
use crate::components::modal::modal_frame;
use crate::components::primitives::{ButtonVariant, button};
use crate::state::{ActiveModal, TokenModal};
use gpui::{AnyElement, ClipboardItem, Context, Window, div, prelude::*, px};

/// Reveals the loopback API bearer token inside an isolated selectable dialog.
pub fn open(app: &mut RoLauncherApp, window: &mut Window, cx: &mut Context<RoLauncherApp>) {
    let raw_token = app.api.reveal_token_for_support();
    let token_input = app.new_text_input("Token", false, true, false, Some(true), cx);
    token_input.update(cx, |ti, cx| {
        ti.set_text(raw_token.as_str(), cx);
        ti.focus_and_select_all(window, cx);
    });
    app.modal = ActiveModal::Token(TokenModal {
        token_input,
        raw_token,
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

    let ActiveModal::Token(tm) = &app.modal else {
        return div().into_any_element();
    };
    let input_el = tm.token_input.clone();

    let body = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .child(
            div()
                .p(px(10.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(theme.warning_border)
                .bg(theme.warning_bg)
                .text_size(px(density.font_caption))
                .text_color(theme.warning)
                .child("Keep this token private. Local software that possesses it can start, stop, and inspect your managed accounts."),
        )
        .child(input_el)
        .into_any_element();

    let footer = div()
        .flex()
        .flex_row()
        .gap(px(8.0))
        .child(button(
            "token-copy",
            "Copy token",
            ButtonVariant::Secondary,
            false,
            false,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                if let ActiveModal::Token(tm) = &this.modal {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        tm.raw_token.as_str().to_string(),
                    ));
                    this.show_info("Local API token copied to clipboard.", Some(4), cx);
                }
            }),
        ))
        .child(button(
            "token-close",
            "Close",
            ButtonVariant::Primary,
            false,
            false,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.close_modal_clearing_secrets(cx);
            }),
        ))
        .into_any_element();

    modal_frame(
        "token-modal",
        "Local API token",
        Some(format!("http://127.0.0.1:{}/v1/", app.bootstrap.port).into()),
        540.0,
        true,
        &theme,
        &density,
        on_action(cx, |this, _w, cx| {
            this.close_modal_clearing_secrets(cx);
        }),
        body,
        footer,
    )
}
