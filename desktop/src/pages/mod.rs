//! Top-level page views dispatched by the active navigation tab.

pub mod accounts;
pub mod dashboard;
pub mod games;
pub mod presets;
pub mod resources;
pub mod settings_backups;
pub mod settings_discord;
pub mod settings_general;
pub mod settings_joining;
pub mod settings_updates;
pub mod support;

use crate::app::RoLauncherApp;
use crate::state::NavTab;
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

/// Renders the currently selected sidebar page.
pub fn render_active_page(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let tab = app.active_tab;
    let content = match tab {
        NavTab::Accounts => accounts::render(app, window, cx),
        NavTab::Games => games::render(app, window, cx),
        NavTab::Dashboard => dashboard::render(app, window, cx),
        NavTab::Resources => resources::render(app, window, cx),
        NavTab::Presets => presets::render(app, window, cx),
        NavTab::General => settings_general::render(app, window, cx),
        NavTab::Joining => settings_joining::render(app, window, cx),
        NavTab::Discord => settings_discord::render(app, window, cx),
        NavTab::Backups => settings_backups::render(app, window, cx),
        NavTab::Updates => settings_updates::render(app, window, cx),
        NavTab::Support => support::render(app, window, cx),
    };
    if matches!(tab, NavTab::Accounts | NavTab::Games) {
        return content;
    }
    let density = app.density();
    let heading = if matches!(tab, NavTab::Resources | NavTab::Dashboard) {
        density.font_title + 2.0
    } else {
        density.font_title
    };
    div()
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(
            div()
                .text_size(px(heading))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(tab.label()),
        )
        .child(content)
        .into_any_element()
}
