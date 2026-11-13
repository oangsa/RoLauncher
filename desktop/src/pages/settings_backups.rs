//! Encrypted configuration backups: create, inspect, and restore snapshots safely.

use crate::app::{RoLauncherApp, on_action};
use crate::components::menu::settings_dropdown;
use crate::components::primitives::{ButtonVariant, button, card, section_title};
use crate::state::{ActiveModal, ConfirmAction, ConfirmModal, OpenMenu};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

/// Renders the Backups page.
pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;

    let selected = app
        .selected_backup
        .as_ref()
        .filter(|name| app.backups.contains(*name))
        .cloned()
        .or_else(|| app.backups.first().cloned());

    let options: Vec<_> = app
        .backups
        .iter()
        .map(|name| (name.as_str(), name.as_str()))
        .collect();
    let backup_picker = settings_dropdown(
        OpenMenu::SettingsBackup,
        app.open_menu,
        &options,
        selected.as_deref().unwrap_or_default(),
        busy || options.is_empty(),
        &theme,
        &density,
        cx,
        |this, name, _, _| {
            this.selected_backup = Some(name);
        },
    );

    let restore_target = selected.clone();

    let backup_card = card(&theme, &density)
        .child(section_title(
            "Encrypted backups",
            Some("Keeps 10 encrypted backups for this OS user. Stop all accounts before restoring; restored accounts stay stopped.".into()),
            &theme,
            &density,
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(8.0))
                .child(button(
                    "backups-create",
                    "Create backup",
                    ButtonVariant::Primary,
                    busy,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.create_backup(cx)),
                ))
                .child(button(
                    "backups-restore",
                    "Restore backup",
                    ButtonVariant::Danger,
                    busy || restore_target.is_none(),
                    false,
                    &theme,
                    &density,
                    on_action(cx, move |this, _w, cx| {
                        let Some(name) = restore_target.clone() else {
                            this.show_info("Choose a backup first.", Some(4), cx);
                            return;
                        };
                        this.modal = ActiveModal::Confirm(ConfirmModal {
                            title: "Restore backup?".to_string(),
                            body: format!(
                                "Restore '{name}'? Replaces accounts, presets, activity, and Discord settings after backing up current data. Restored accounts stay stopped. Your API token is kept."
                            ),
                            confirm_label: "Restore".to_string(),
                            dangerous: true,
                            action: ConfirmAction::RestoreBackup(name),
                        });
                        cx.notify();
                    }),
                ))
                .child(button(
                    "backups-refresh",
                    "Refresh",
                    ButtonVariant::Secondary,
                    busy,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _w, cx| this.refresh_backups(cx)),
                )),
        )
        .child(div().flex().flex_col().gap(px(4.0)).child(
            div().text_size(px(density.font_caption)).text_color(theme.text_secondary).child("Backup"),
        ).child(backup_picker));

    div()
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(backup_card)
        .into_any_element()
}
