//! Accessible modal dialog shell with scrim backdrop, Escape dismissal, and scrollable body.

use crate::components::primitives::{ButtonVariant, button};
use crate::theme::{AppTheme, DensityTokens};
use gpui::{
    AnyElement, App, ElementId, InteractiveElement, KeyDownEvent, MouseButton, SharedString,
    StatefulInteractiveElement, Window, div, prelude::*, px,
};

/// Wraps modal body and footer elements inside a centered overlay with scrim and `Esc` dismissal.
#[allow(clippy::too_many_arguments)]
pub fn modal_frame(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    subtitle: Option<SharedString>,
    max_width_px: f32,
    dismiss_enabled: bool,
    theme: &AppTheme,
    density: &DensityTokens,
    on_close: impl Fn(&mut Window, &mut App) + 'static,
    body: AnyElement,
    footer: AnyElement,
) -> AnyElement {
    let on_close = std::sync::Arc::new(on_close);
    let close_scrim = on_close.clone();
    let close_key = on_close.clone();
    let close_btn = on_close;

    let header = div()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap(px(12.0))
        .pb(px(10.0))
        .border_b_1()
        .border_color(theme.border_subtle)
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(3.0))
                .child(
                    div()
                        .text_size(px(density.font_section))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .child(title.into()),
                )
                .when_some(subtitle, |col, sub| {
                    col.child(
                        div()
                            .text_size(px(density.font_caption))
                            .text_color(theme.text_secondary)
                            .child(sub),
                    )
                }),
        )
        .child(button(
            "modal-close-x",
            "Close",
            ButtonVariant::Subtle,
            !dismiss_enabled,
            true,
            theme,
            density,
            move |w, cx| close_btn(w, cx),
        ));

    let dialog = div()
        .id("modal-card")
        .flex()
        .flex_col()
        .w_full()
        .max_w(px(max_width_px))
        .max_h(gpui::relative(0.88))
        .p(px(density.card_pad + 4.0))
        .gap(px(12.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_dialog)
        .shadow_lg()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(header)
        .child(
            div()
                .id("modal-scroll-body")
                .flex_1()
                .min_h(px(40.0))
                .overflow_y_scroll()
                .pr(px(4.0))
                .child(body),
        )
        .child(
            div()
                .pt(px(10.0))
                .border_t_1()
                .border_color(theme.border_subtle)
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(8.0))
                .child(footer),
        );

    div()
        .id(id)
        .occlude()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .p(px(24.0))
        .bg(theme.scrim)
        .on_key_down(move |ev: &KeyDownEvent, window, cx| {
            if dismiss_enabled && ev.keystroke.key == "escape" {
                cx.stop_propagation();
                close_key(window, cx);
            }
        })
        .on_click(move |_, window, cx| {
            if dismiss_enabled {
                close_scrim(window, cx);
            }
        })
        .child(dialog)
        .into_any_element()
}
