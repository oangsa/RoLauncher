//! Floating menus shared by workspace controls.

use crate::app::RoLauncherApp;
use crate::components::primitives::icon;
use crate::state::OpenMenu;
use crate::theme::{AppTheme, DensityTokens};
use gpui::{Context, Window, anchored, container_query, deferred, div, prelude::*, px};
use std::rc::Rc;

#[allow(clippy::too_many_arguments)]
pub fn settings_dropdown(
    menu: OpenMenu,
    open_menu: Option<OpenMenu>,
    options: &[(&str, &str)],
    value: &str,
    disabled: bool,
    theme: &AppTheme,
    density: &DensityTokens,
    cx: &Context<RoLauncherApp>,
    on_select: impl Fn(&mut RoLauncherApp, String, &mut Window, &mut Context<RoLauncherApp>) + 'static,
) -> impl IntoElement {
    let current = options
        .iter()
        .find(|(key, _)| *key == value)
        .or_else(|| options.first())
        .map_or("No backups available", |(_, label)| *label)
        .to_owned();
    let open = !disabled && open_menu == Some(menu);
    div()
        .id(format!("{menu:?}"))
        .relative()
        .w_full()
        .child(
            div()
                .id("dropdown-trigger")
                .tab_stop(!disabled)
                .flex()
                .items_center()
                .justify_between()
                .w_full()
                .h(px(density.control_h))
                .gap(px(8.0))
                .px(px(12.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(if open {
                    theme.border_focus
                } else {
                    theme.border_subtle
                })
                .bg(theme.bg_input)
                .when(disabled, |el| el.text_color(theme.text_muted))
                .when(!disabled, |el| {
                    el.cursor_pointer()
                        .hover(|style| style.bg(theme.bg_surface_hover))
                        .focus(|style| style.border_color(theme.border_focus))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            cx.stop_propagation();
                            app.open_menu = (app.open_menu != Some(menu)).then_some(menu);
                            cx.notify();
                        }))
                })
                .child(div().min_w_0().truncate().child(current))
                .child(icon("\u{e70d}", 10.0).flex_shrink_0()),
        )
        .when(open, |field| {
            let on_select = Rc::new(on_select);
            let options_menu = popover_box(theme)
                .max_h(px(280.0))
                .overflow_y_scroll()
                .children(options.iter().enumerate().map(|(index, &(key, label))| {
                    let selected = key == value;
                    let key = key.to_owned();
                    let on_select = Rc::clone(&on_select);
                    menu_item(
                        ("dropdown-option", index),
                        label.to_owned(),
                        false,
                        theme,
                        density,
                        cx.listener(move |app, _, window, cx| {
                            cx.stop_propagation();
                            app.open_menu = None;
                            cx.notify();
                            if !selected {
                                on_select(app, key.clone(), window, cx);
                            }
                        }),
                    )
                    .when(selected, |el| el.bg(theme.accent_subtle))
                }));
            field.child(
                container_query(move |size, _, _| {
                    deferred(
                        anchored()
                            .snap_to_window()
                            .child(options_menu.w(size.width)),
                    )
                })
                .absolute()
                .left_0()
                .top_full()
                .mt(px(4.0))
                .h(px(0.0)),
            )
        })
}

pub fn popover(align_right: bool, menu: impl IntoElement) -> impl IntoElement {
    deferred(
        div()
            .absolute()
            .top_full()
            .mt(px(4.0))
            .when(align_right, |el| el.right_0())
            .when(!align_right, |el| el.left_0())
            .child(anchored().snap_to_window().child(menu)),
    )
}

pub fn popover_box(theme: &crate::theme::AppTheme) -> gpui::Stateful<gpui::Div> {
    div()
        .id("menu")
        .w(px(230.0))
        .p(px(4.0))
        .occlude()
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.bg_surface)
        .shadow_md()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .on_click(|_, _, cx| cx.stop_propagation())
}

pub fn menu_item(
    id: impl Into<gpui::ElementId>,
    label: String,
    disabled: bool,
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let mut item = div()
        .id(id)
        .px(px(10.0))
        .py(px(6.0))
        .rounded(px(5.0))
        .text_size(px(density.font_body))
        .text_color(if disabled {
            theme.text_muted
        } else {
            theme.text_primary
        })
        .child(label);
    if !disabled {
        item = item
            .tab_stop(true)
            .cursor_pointer()
            .hover(|s| s.bg(theme.bg_surface_hover))
            .on_click(on_click);
    }
    item
}
