//! Support tools: sanitized diagnostics export and controlled local API token modal.

use crate::app::{RoLauncherApp, on_action};
use crate::components::primitives::{ButtonVariant, button, card, section_title};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

const API_REFERENCE_URL: &str = "https://github.com/oangsa/RoLauncher/blob/main/docs/API.md";

/// Renders the Support page.
pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;

    let report_card = card(&theme, &density)
        .child(section_title(
            "Support report",
            Some("Includes recent launch and auto-rejoin decisions. Export soon after an unexpected launch or pause. No cookies or API tokens are included.".into()),
            &theme,
            &density,
        ))
        .child(
            div().flex().flex_row().child(button(
                "support-export-btn",
                "Export report",
                ButtonVariant::Primary,
                busy,
                false,
                &theme,
                &density,
                on_action(cx, |this, window, cx| {
                    this.export_diagnostics_report(window, cx);
                }),
            )),
        );

    let api_card = card(&theme, &density)
        .child(section_title(
            "Local API",
            Some(format!(
                "Connect local scripts and integrations to http://127.0.0.1:{}/v1 using your private bearer token.",
                app.bootstrap.port
            ).into()),
            &theme,
            &density,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(10.0))
                .child(button(
                    "support-token-btn",
                    "API token",
                    ButtonVariant::Secondary,
                    busy,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, window, cx| {
                        this.open_api_token_modal(window, cx);
                    }),
                ))
                .child(button(
                    "support-api-docs-btn",
                    "API reference",
                    ButtonVariant::Subtle,
                    false,
                    false,
                    &theme,
                    &density,
                    |_window, cx| {
                        cx.open_url(API_REFERENCE_URL);
                    },
                )),
        );

    div()
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(report_card)
        .child(api_card)
        .into_any_element()
}
