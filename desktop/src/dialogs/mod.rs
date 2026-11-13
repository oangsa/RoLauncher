//! Modal dialog overlays and shared action dispatch.

pub mod account_editor;
pub mod bulk_editor;
pub mod changelog;
pub mod confirm;
pub mod game_editor;
pub mod preset_lookup;
pub mod preset_review;
pub mod token;

use crate::app::RoLauncherApp;
use crate::state::ActiveModal;
use gpui::{AnyElement, Context, Window};

/// Renders whichever modal dialog is currently active, if any.
pub fn render_active_modal(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> Option<AnyElement> {
    match &app.modal {
        ActiveModal::None => None,
        ActiveModal::AccountEditor(_) => Some(account_editor::render(app, window, cx)),
        ActiveModal::BulkEditor(_) => Some(bulk_editor::render(app, window, cx)),
        ActiveModal::GameEditor(_) => Some(game_editor::render(app, window, cx)),
        ActiveModal::PresetLookup(_) => Some(preset_lookup::render(app, window, cx)),
        ActiveModal::PresetReview(_) => Some(preset_review::render(app, window, cx)),
        ActiveModal::Changelog => Some(changelog::render(app, window, cx)),
        ActiveModal::Token(_) => Some(token::render(app, window, cx)),
        ActiveModal::Confirm(_) => Some(confirm::render(app, window, cx)),
    }
}
