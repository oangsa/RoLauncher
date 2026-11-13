//! General settings page: appearance theme and close-to-tray behavior.

use crate::app::{RoLauncherApp, on_action};
use crate::components::menu::settings_dropdown;
use crate::components::primitives::{ButtonVariant, button, card, section_title};
use crate::platform::appearance::AppearancePreference;
use crate::state::OpenMenu;
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

/// Renders the General settings tab.
pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;
    let close_to_tray = app.snapshot.as_ref().is_none_or(|s| s.close_to_tray);
    let appearance = app.prefs.appearance;

    let appearance_card = card(&theme, &density)
        .child(section_title("Appearance", None, &theme, &density))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme.text_secondary)
                        .child("Theme"),
                )
                .child(settings_dropdown(
                    OpenMenu::SettingsTheme,
                    app.open_menu,
                    &[("system", "System"), ("light", "Light"), ("dark", "Dark")],
                    appearance.as_str(),
                    false,
                    &theme,
                    &density,
                    cx,
                    move |this, value, window, cx| {
                        let next = AppearancePreference::parse(&value);
                        this.set_appearance(next, window, cx);
                    },
                )),
        )
        .child(section_title("Window", None, &theme, &density))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(theme.text_secondary)
                        .child("On close"),
                )
                .child(settings_dropdown(
                    OpenMenu::SettingsClose,
                    app.open_menu,
                    &[("tray", "Minimize to tray"), ("exit", "Close completely")],
                    if close_to_tray { "tray" } else { "exit" },
                    busy || app.snapshot.is_none(),
                    &theme,
                    &density,
                    cx,
                    move |this, value, _window, cx| {
                        let tray = value == "tray";
                        this.set_close_to_tray(tray, cx);
                    },
                ))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_muted)
                        .child("Exiting stops automatic rejoin; running Roblox windows stay open."),
                ),
        )
        .child(div().flex().flex_row().pt(px(4.0)).child(button(
            "general-exit-btn",
            "Exit RoLauncher",
            ButtonVariant::Danger,
            busy,
            false,
            &theme,
            &density,
            on_action(cx, |this, window, cx| {
                this.request_exit(window, cx);
            }),
        )));

    div()
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(appearance_card)
        .into_any_element()
}
