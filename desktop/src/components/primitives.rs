//! Shared styled buttons, cards, status pills, banners, toggles, checkboxes, and segmented pickers.

use crate::theme::{AppTheme, DensityTokens};
use gpui::{
    AnyElement, App, Div, ElementId, Hsla, InteractiveElement, MouseButton, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::*, px, rems,
};

/// Visual prominence variant for clickable action buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Danger,
    Subtle,
}

/// Semantic severity tone for inline alerts and feedback banners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerTone {
    Info,
    Success,
    Warning,
    Danger,
}

/// Renders an accessible button with hover, active, disabled, and keyboard Enter/Space handling.
#[allow(clippy::too_many_arguments)]
pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    disabled: bool,
    compact_override: bool,
    theme: &AppTheme,
    density: &DensityTokens,
    on_press: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    button_surface(
        id,
        variant,
        disabled,
        compact_override,
        theme,
        density,
        on_press,
    )
    .child(label.into())
    .into_any_element()
}

/// Glyph from the same Windows font used by the WinUI navigation.
pub fn icon(glyph: &'static str, size: f32) -> Div {
    div()
        .font_family("Segoe Fluent Icons")
        .font_weight(gpui::FontWeight::NORMAL)
        .text_size(px(size))
        .child(glyph)
}

#[allow(clippy::too_many_arguments)]
pub fn icon_button(
    id: impl Into<ElementId>,
    glyph: &'static str,
    label: &'static str,
    variant: ButtonVariant,
    disabled: bool,
    compact: bool,
    theme: &AppTheme,
    density: &DensityTokens,
    on_press: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let theme = *theme;
    button_surface(id, variant, disabled, compact, &theme, density, on_press)
        .w(px(if compact {
            30.0
        } else {
            button_height(density, false)
        }))
        .px(px(0.0))
        .flex_shrink_0()
        .tooltip(move |_, cx| cx.new(|_| IconTooltip { label, theme }).into())
        .child(icon(glyph, 16.0))
        .into_any_element()
}

struct IconTooltip {
    label: &'static str,
    theme: AppTheme,
}

impl gpui::Render for IconTooltip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .px(px(8.0))
            .py(px(4.0))
            .rounded(px(4.0))
            .border_1()
            .border_color(self.theme.border_strong)
            .bg(self.theme.bg_dialog)
            .text_size(px(12.0))
            .text_color(self.theme.text_primary)
            .child(self.label)
    }
}

fn button_height(density: &DensityTokens, compact: bool) -> f32 {
    let height = density.control_h - if compact { 6.0 } else { 0.0 };
    (height * 0.8).round().max(24.0)
}

fn button_surface(
    id: impl Into<ElementId>,
    variant: ButtonVariant,
    disabled: bool,
    compact_override: bool,
    theme: &AppTheme,
    density: &DensityTokens,
    on_press: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui::Stateful<Div> {
    let (bg, bg_hover, fg, border) = match variant {
        ButtonVariant::Primary => (
            theme.accent,
            theme.accent_hover,
            theme.accent_fg,
            theme.accent,
        ),
        ButtonVariant::Secondary => (
            theme.bg_surface_elevated,
            theme.bg_surface_hover,
            theme.text_primary,
            theme.border_strong,
        ),
        ButtonVariant::Danger => (
            theme.bg_surface_elevated,
            theme.bg_surface_hover,
            theme.danger,
            theme.border_strong,
        ),
        ButtonVariant::Subtle => (
            gpui::transparent_black(),
            theme.bg_surface_hover,
            theme.text_secondary,
            gpui::transparent_black(),
        ),
    };

    let px_pad = if compact_override { 8.0 } else { 13.0 };
    let text_size = if compact_override {
        density.font_caption
    } else {
        density.font_body
    };

    let mut base = div()
        .id(id)
        .tab_stop(!disabled)
        .flex()
        .items_center()
        .justify_center()
        .h(px(button_height(density, compact_override)))
        .px(px(px_pad))
        .rounded(px(8.0))
        .border_1()
        .border_color(if disabled {
            theme.border_subtle
        } else {
            border
        })
        .bg(if disabled && variant == ButtonVariant::Primary {
            theme.text_muted
        } else {
            bg
        })
        .text_color(if disabled && variant != ButtonVariant::Primary {
            theme.text_muted
        } else {
            fg
        })
        .text_size(px(text_size))
        .whitespace_nowrap();

    if !disabled {
        base = base
            .cursor_pointer()
            .hover(move |style| style.bg(bg_hover))
            .focus(|style| style.border_color(theme.border_focus))
            .on_mouse_down(MouseButton::Left, |_, window, _| {
                window.prevent_default();
            })
            .on_click(move |_, window, cx| {
                on_press(window, cx);
            });
    } else {
        base = base.opacity(0.6);
    }

    base
}

/// Renders a surface card container with consistent border radius, background, and padding.
pub fn card(theme: &AppTheme, density: &DensityTokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(density.item_gap))
        .p(px(density.card_pad))
        .rounded(px(12.0))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_surface)
}

/// Renders a subtle section title header inside pages and dialogs.
pub fn section_title(
    title: impl Into<SharedString>,
    subtitle: Option<SharedString>,
    theme: &AppTheme,
    density: &DensityTokens,
) -> Div {
    let mut wrap = div().flex().flex_col().gap(px(2.0)).child(
        div()
            .text_size(px(density.font_section))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.text_primary)
            .child(title.into()),
    );
    if let Some(sub) = subtitle {
        wrap = wrap.child(
            div()
                .text_size(px(density.font_caption))
                .text_color(theme.text_secondary)
                .line_height(rems(1.35))
                .child(sub),
        );
    }
    wrap
}

/// Renders a pill badge with custom colors.
pub fn badge(
    label: impl Into<SharedString>,
    fg: Hsla,
    bg: Hsla,
    border: Hsla,
    density: &DensityTokens,
) -> Div {
    div()
        .flex()
        .items_center()
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_size(px(density.font_caption))
        .font_weight(gpui::FontWeight::MEDIUM)
        .whitespace_nowrap()
        .child(label.into())
}

/// Renders an account or Discord bot status pill badge using canonical theme colors.
pub fn status_badge(
    raw_status: &str,
    label: impl Into<SharedString>,
    theme: &AppTheme,
    density: &DensityTokens,
) -> Div {
    let (bg, border, fg) = theme.status_colors(raw_status);
    badge(label, fg, bg, border, density)
}

/// Boxed click callback used by notification banners.
pub type BoxedAction = Box<dyn Fn(&mut Window, &mut App) + 'static>;

/// Renders a semantic feedback/notice banner with optional primary action and dismiss button.
pub fn banner(
    id: impl Into<ElementId>,
    tone: BannerTone,
    message: impl Into<SharedString>,
    action: Option<(&'static str, BoxedAction)>,
    on_dismiss: Option<BoxedAction>,
    theme: &AppTheme,
    density: &DensityTokens,
) -> AnyElement {
    let (bg, border, fg) = match tone {
        BannerTone::Info => (theme.info_bg, theme.info_border, theme.info),
        BannerTone::Success => (theme.success_bg, theme.success_border, theme.success),
        BannerTone::Warning => (theme.warning_bg, theme.warning_border, theme.warning),
        BannerTone::Danger => (theme.danger_bg, theme.danger_border, theme.danger),
    };

    let mut row = div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .px(px(14.0))
        .py(px(10.0))
        .rounded(px(9.0))
        .border_1()
        .border_color(border)
        .bg(bg)
        .child(
            div()
                .flex_1()
                .text_size(px(density.font_body))
                .text_color(fg)
                .child(message.into()),
        );

    let mut actions = div().flex().flex_row().items_center().gap(px(8.0));
    let mut has_actions = false;
    if let Some((act_label, act_fn)) = action {
        has_actions = true;
        actions = actions.child(button(
            "banner-action",
            act_label,
            ButtonVariant::Secondary,
            false,
            true,
            theme,
            density,
            act_fn,
        ));
    }
    if let Some(dismiss_fn) = on_dismiss {
        has_actions = true;
        actions = actions.child(button(
            "banner-dismiss",
            "Dismiss",
            ButtonVariant::Subtle,
            false,
            true,
            theme,
            density,
            dismiss_fn,
        ));
    }
    if has_actions {
        row = row.child(actions);
    }

    row.into_any_element()
}

/// Renders an accessible checkbox control with optional text label.
pub fn checkbox(
    id: impl Into<ElementId>,
    checked: bool,
    label: Option<SharedString>,
    disabled: bool,
    theme: &AppTheme,
    density: &DensityTokens,
    on_toggle: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let box_el = div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(18.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(if checked {
            theme.accent
        } else {
            theme.border_strong
        })
        .bg(if checked {
            theme.accent
        } else {
            theme.bg_input
        })
        .text_color(theme.accent_fg)
        .text_size(px(12.0))
        .font_weight(gpui::FontWeight::BOLD)
        .child(if checked { "✓" } else { "" });

    let mut row = div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(box_el);

    if let Some(lbl) = label {
        row = row.child(
            div()
                .text_size(px(density.font_body))
                .text_color(if disabled {
                    theme.text_muted
                } else {
                    theme.text_primary
                })
                .child(lbl),
        );
    }

    if !disabled {
        row = row
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(move |_, window, cx| on_toggle(window, cx));
    } else {
        row = row.opacity(0.6);
    }

    row.into_any_element()
}

/// Renders a horizontal group of mutually exclusive choice pills.
pub fn segmented_control(
    id_prefix: &'static str,
    options: &[(&str, &str)],
    selected_value: &str,
    disabled: bool,
    theme: &AppTheme,
    density: &DensityTokens,
    on_select: impl Fn(String, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let on_select = std::sync::Arc::new(on_select);
    let mut wrap = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .self_start()
        .max_w_full()
        .items_center()
        .gap(px(4.0))
        .p(px(3.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_input);

    for (idx, (value, label)) in options.iter().enumerate() {
        let active = *value == selected_value;
        let value = (*value).to_owned();
        let on_select = on_select.clone();
        wrap = wrap.child(
            button_surface(
                (id_prefix, idx),
                if active {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Subtle
                },
                disabled,
                false,
                theme,
                density,
                move |window, cx| {
                    if !active {
                        on_select(value.clone(), window, cx);
                    }
                },
            )
            .child((*label).to_owned()),
        );
    }

    wrap.into_any_element()
}

/// Renders a labeled stat card for Overview / Resources summaries.
pub fn stat_card(
    title: impl Into<SharedString>,
    value: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    accent_color: Option<Hsla>,
    theme: &AppTheme,
    density: &DensityTokens,
) -> Div {
    card(theme, density)
        .flex_1()
        .min_w(px(146.0))
        .gap(px(8.0))
        .child(
            div()
                .text_size(px(density.font_caption))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme.text_secondary)
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(density.font_title + 8.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(accent_color.unwrap_or(theme.text_primary))
                .child(value.into()),
        )
        .child(
            div()
                .text_size(px(density.font_caption))
                .text_color(theme.text_muted)
                .child(detail.into()),
        )
}
