//! Official release updates: beta opt-in, check, verified download with SHA-256 progress, and relaunch handoff.

use crate::app::{RoLauncherApp, on_action};
use crate::components::chart::progress_bar;
use crate::components::primitives::{ButtonVariant, badge, button, card, checkbox, section_title};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

/// Renders the Updates settings page.
pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;
    let include_beta = app
        .snapshot
        .as_ref()
        .is_some_and(|s| s.include_beta_updates);
    let update_available = app.update_view.as_ref().is_some_and(|u| u.available);
    let has_release_url = app
        .update_view
        .as_ref()
        .is_some_and(|u| !u.release_url.is_empty());
    let repo_label = app
        .snapshot
        .as_ref()
        .map(|s| s.update_repository.as_str())
        .filter(|r| !r.is_empty())
        .unwrap_or("oangsa/RoLauncher");

    let primary_label = if app.update_installing {
        "Installing…"
    } else if app.update_downloading {
        "Cancel download"
    } else if app.cached_installer.is_some() {
        "Relaunch"
    } else {
        "Download"
    };

    let disable_check = busy
        || app.update_checking
        || app.update_downloading
        || app.update_installing
        || app.snapshot.is_none();
    let disable_primary = !app.update_downloading
        && (busy || app.update_checking || app.update_installing || !update_available);

    let release_notes_url = app
        .update_view
        .as_ref()
        .map(|u| u.release_url.clone())
        .unwrap_or_default();

    let updates_card = card(&theme, &density)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(section_title(
                    "Official updates",
                    Some("Checks at startup and every 6 hours. Download, then Relaunch to install. Data is backed up; games stay open.".into()),
                    &theme,
                    &density,
                ))
                .child(badge(
                    format!("v{}", app.bootstrap.version),
                    theme.text_secondary,
                    theme.bg_badge,
                    theme.border_subtle,
                    &density,
                )),
        )
        .child(checkbox(
            "updates-beta-check",
            include_beta,
            Some("Include beta updates".into()),
            disable_check,
            &theme,
            &density,
            on_action(cx, move |this, _w, cx| {
                this.set_beta_updates(!include_beta, cx);
            }),
        ))
        .child(
            div()
                .pl(px(24.0))
                .text_size(px(density.font_caption))
                .text_color(theme.text_muted)
                .child(format!(
                    "Receive features still being tested. Verified against official GitHub releases ({repo_label})."
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(8.0))
                .child(button(
                    "updates-check-btn",
                    if app.update_checking {
                        "Checking…"
                    } else {
                        "Check updates"
                    },
                    ButtonVariant::Secondary,
                    disable_check,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.check_updates(true, cx)),
                ))
                .child(button(
                    "updates-primary-btn",
                    primary_label,
                    if app.update_downloading {
                        ButtonVariant::Danger
                    } else {
                        ButtonVariant::Primary
                    },
                    disable_primary,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, window, cx| {
                        this.handle_update_primary_action(window, cx);
                    }),
                ))
                .child(button(
                    "updates-notes-btn",
                    "Release notes",
                    ButtonVariant::Secondary,
                    !has_release_url,
                    false,
                    &theme,
                    &density,
                    move |_window, cx| {
                        if !release_notes_url.is_empty() {
                            cx.open_url(&release_notes_url);
                        }
                    },
                )),
        )
        .when(app.update_downloading, |c| {
            c.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(progress_bar(app.update_download_pct, &theme)),
            )
        })
        .child(
            div()
                .text_size(px(density.font_body))
                .text_color(theme.text_secondary)
                .child(app.update_status_text.clone()),
        );

    div()
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(updates_card)
        .into_any_element()
}
