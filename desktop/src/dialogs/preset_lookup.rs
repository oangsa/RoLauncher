//! Account table and filters for choosing accounts for a preset.

use crate::app::{RoLauncherApp, on_action};
use crate::components::menu::{menu_item, popover, popover_box};
use crate::components::modal::modal_frame;
use crate::components::primitives::{ButtonVariant, button, icon};
use crate::domain::filter::{REJOIN_FILTER_OPTIONS, STATUS_FILTER_OPTIONS, matches_account};
use crate::domain::models::{format_account_status, format_group};
use crate::state::{ActiveModal, OpenMenu, PresetLookupModal};
use crate::theme::{AppTheme, DensityTokens};
use gpui::{AnyElement, Context, Div, Window, div, prelude::*, px};
use std::collections::BTreeSet;

pub fn open(app: &mut RoLauncherApp, window: &mut Window, cx: &mut Context<RoLauncherApp>) {
    let search_input = app.new_text_input("Search accounts…", false, false, false, None, cx);
    let owner = cx.entity().downgrade();
    search_input.update(cx, |input, cx| {
        input.set_on_change(move |_, _, cx| {
            let _ = owner.update(cx, |_, cx| cx.notify());
        });
        input.focus_and_select_all(window, cx);
    });
    app.open_menu = None;
    app.modal = ActiveModal::PresetLookup(PresetLookupModal {
        search_input,
        status_filter: String::new(),
        rejoin_filter: String::new(),
        group_filter: None,
        chosen_ids: app.preset_draft_account_ids.clone(),
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
    let ActiveModal::PresetLookup(lookup) = &app.modal else {
        return div().into_any_element();
    };
    let accounts = app
        .snapshot
        .as_ref()
        .map_or(&[][..], |s| s.accounts.as_slice());
    let query = lookup.search_input.read(cx).text();
    let visible: Vec<_> = accounts
        .iter()
        .filter(|account| {
            matches_account(
                account,
                query,
                &lookup.status_filter,
                &lookup.rejoin_filter,
                lookup.group_filter.as_deref(),
            )
        })
        .collect();
    let selected = accounts
        .iter()
        .filter(|account| lookup.chosen_ids.contains(&account.id))
        .count();
    let visible_selected = visible
        .iter()
        .filter(|account| lookup.chosen_ids.contains(&account.id))
        .count();
    let hidden = selected - visible_selected;
    let shown = visible.len();
    let total = accounts.len();
    let busy = app.busy;
    let open_menu = app.open_menu;
    let visible_ids: Vec<_> = visible.iter().map(|account| account.id.clone()).collect();
    let groups = accounts
        .iter()
        .map(|account| account.group.as_str())
        .collect::<BTreeSet<_>>();
    let group_options = std::iter::once((None, "All groups")).chain(
        groups
            .into_iter()
            .map(|group| (Some(group), format_group(group))),
    );
    let search = lookup.search_input.clone();
    let status = lookup.status_filter.clone();
    let rejoin = lookup.rejoin_filter.clone();
    let group = lookup.group_filter.clone();

    let header = columns(["Alias", "Username", "Group", "Game", "Rejoin", "Status"])
        .min_h(px(36.0))
        .rounded(px(8.0))
        .bg(theme.bg_table_header)
        .text_size(px(density.font_caption))
        .text_color(theme.text_muted);
    let mut rows = div()
        .id("lookup-table-rows")
        .flex()
        .flex_col()
        .max_h(px(300.0))
        .overflow_y_scroll();
    for account in visible {
        let id = account.id.clone();
        let checked = lookup.chosen_ids.contains(&id);
        let game = account.target.as_ref().map_or_else(
            || "—".to_owned(),
            |target| {
                app.game_cache
                    .display_name(target.place_id)
                    .map_or_else(|| format!("Place {}", target.place_id), ToOwned::to_owned)
            },
        );
        let row = columns([
            account.display_name().to_owned(),
            format!("@{}", account.username),
            format_group(&account.group).to_owned(),
            game,
            if account.auto_recovery { "On" } else { "Off" }.to_owned(),
            format_account_status(&account.status).to_string(),
        ]);
        rows = rows.child(
            row.id(format!("lookup-row-{id}"))
                .relative()
                .min_h(px(44.0))
                .rounded(px(8.0))
                .when(checked, |row| {
                    row.bg(theme.bg_selected).child(
                        div()
                            .absolute()
                            .left(px(10.0))
                            .text_color(theme.nav_marker)
                            .child(icon("\u{e73e}", 14.0)),
                    )
                })
                .tab_stop(!busy)
                .when(!busy, |row| {
                    row.cursor_pointer()
                        .hover(|style| style.bg(theme.bg_surface_hover))
                        .focus(|style| style.bg(theme.bg_surface_hover))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let ActiveModal::PresetLookup(lookup) = &mut app.modal {
                                if !lookup.chosen_ids.remove(&id) {
                                    lookup.chosen_ids.insert(id.clone());
                                }
                                cx.notify();
                            }
                        }))
                }),
        );
    }

    let body = div().id("lookup-body").flex().flex_col().gap(px(12.0))
        .on_click(cx.listener(|app, _, _, cx| {
            if app.open_menu.take().is_some() {
                cx.notify();
            }
        }))
        .child(div().flex().flex_wrap().items_center().gap(px(8.0))
            .child(div().flex_1().min_w(px(180.0)).child(search.clone()))
            .child(div().w(px(180.0)).child(filter_picker(
                "lookup-group", OpenMenu::LookupGroup, open_menu, group.as_deref(),
                group_options, &theme, &density, cx,
            ))))
        .child(div().flex().flex_wrap().items_center().gap(px(8.0))
            .child(div().flex_1().min_w(px(145.0)).child(filter_picker(
                "lookup-status", OpenMenu::LookupStatus, open_menu, Some(status.as_str()),
                STATUS_FILTER_OPTIONS.iter().map(|&(value, label)| (Some(value), label)),
                &theme, &density, cx,
            )))
            .child(div().flex_1().min_w(px(160.0)).child(filter_picker(
                "lookup-rejoin", OpenMenu::LookupRejoin, open_menu, Some(rejoin.as_str()),
                REJOIN_FILTER_OPTIONS.iter().map(|&(value, label)| (Some(value), label)),
                &theme, &density, cx,
            )))
            .child(button("lookup-clear-filters", "Clear filters", ButtonVariant::Secondary,
                false, false, &theme, &density, on_action(cx, move |app, _, cx| {
                    search.update(cx, |input, cx| input.set_text("", cx));
                    if let ActiveModal::PresetLookup(lookup) = &mut app.modal {
                        lookup.status_filter.clear();
                        lookup.rejoin_filter.clear();
                        lookup.group_filter = None;
                    }
                    cx.notify();
                }))))
        .child(div().flex().flex_wrap().gap(px(8.0))
            .child(button("lookup-select-visible", "Select visible", ButtonVariant::Secondary,
                busy || shown == 0, false, &theme, &density, on_action(cx, move |app, _, cx| {
                    if let ActiveModal::PresetLookup(lookup) = &mut app.modal {
                        lookup.chosen_ids.extend(visible_ids.iter().cloned());
                        cx.notify();
                    }
                })))
            .child(button("lookup-clear-selection", "Clear selection", ButtonVariant::Secondary,
                busy || selected == 0, false, &theme, &density, on_action(cx, |app, _, cx| {
                    if let ActiveModal::PresetLookup(lookup) = &mut app.modal {
                        lookup.chosen_ids.clear();
                        cx.notify();
                    }
                }))))
        .child(div().id("lookup-table").w_full().overflow_x_scroll()
            .child(div().flex().flex_col().gap(px(4.0)).w_full().min_w(px(640.0))
                .child(header).child(rows)))
        .when(shown == 0, |body| body.child(div().text_color(theme.text_muted)
            .text_size(px(density.font_caption)).child("No matching accounts.")))
        .child(div().text_size(px(density.font_caption)).text_color(theme.text_muted)
            .child(if hidden > 0 { format!("{shown} of {total} shown · {selected} selected ({hidden} hidden by filters)") }
                else { format!("{shown} of {total} shown · {selected} selected") }))
        .into_any_element();

    let footer = div()
        .flex()
        .gap(px(8.0))
        .child(button(
            "lookup-apply",
            "Use selection",
            ButtonVariant::Primary,
            busy,
            false,
            &theme,
            &density,
            on_action(cx, |app, _, cx| {
                if let ActiveModal::PresetLookup(lookup) = &app.modal {
                    app.preset_draft_account_ids = app
                        .snapshot
                        .iter()
                        .flat_map(|s| &s.accounts)
                        .filter(|account| lookup.chosen_ids.contains(&account.id))
                        .map(|account| account.id.clone())
                        .collect();
                }
                close(app, cx);
            }),
        ))
        .child(button(
            "lookup-cancel",
            "Cancel",
            ButtonVariant::Secondary,
            false,
            false,
            &theme,
            &density,
            on_action(cx, |app, _, cx| close(app, cx)),
        ))
        .into_any_element();

    modal_frame(
        "preset-lookup-modal",
        "Choose accounts",
        None,
        864.0,
        true,
        &theme,
        &density,
        on_action(cx, |app, _, cx| {
            if app.open_menu.take().is_some() {
                cx.notify();
            } else {
                close(app, cx);
            }
        }),
        body,
        footer,
    )
}

fn close(app: &mut RoLauncherApp, cx: &mut Context<RoLauncherApp>) {
    app.open_menu = None;
    app.modal = ActiveModal::None;
    cx.notify();
}

fn columns<T: Into<gpui::SharedString>>(labels: [T; 6]) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .pl(px(36.0))
        .pr(px(12.0))
        .children(labels.into_iter().enumerate().map(|(index, label)| {
            let cell = div().min_w(px(0.0)).truncate().child(label.into());
            match index {
                4 => cell.w(px(45.0)).flex_shrink_0(),
                5 => cell.w(px(95.0)).flex_shrink_0(),
                _ => cell
                    .flex_basis(px(0.0))
                    .flex_grow([1.2, 1.2, 1.0, 1.3][index]),
            }
        }))
}

#[allow(clippy::too_many_arguments)]
fn filter_picker<'a>(
    id: &'static str,
    menu: OpenMenu,
    open_menu: Option<OpenMenu>,
    value: Option<&str>,
    options: impl Iterator<Item = (Option<&'a str>, &'a str)>,
    theme: &AppTheme,
    density: &DensityTokens,
    cx: &mut Context<RoLauncherApp>,
) -> impl IntoElement {
    let options: Vec<_> = options.collect();
    let current = options
        .iter()
        .find(|(key, _)| *key == value)
        .or_else(|| options.first())
        .map_or("", |(_, label)| *label)
        .to_owned();
    div()
        .relative()
        .child(
            div()
                .id(id)
                .tab_stop(true)
                .flex()
                .items_center()
                .justify_between()
                .h(px(density.control_h))
                .gap(px(8.0))
                .px(px(12.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.border_subtle)
                .bg(theme.bg_input)
                .cursor_pointer()
                .hover(|style| style.bg(theme.bg_surface_hover))
                .focus(|style| style.border_color(theme.border_focus))
                .on_click(cx.listener(move |app, _, _, cx| {
                    cx.stop_propagation();
                    app.open_menu = (app.open_menu != Some(menu)).then_some(menu);
                    cx.notify();
                }))
                .child(div().truncate().child(current))
                .child(icon("\u{e70d}", 10.0)),
        )
        .when(open_menu == Some(menu), |field| {
            field.child(popover(
                false,
                popover_box(theme)
                    .max_h(px(300.0))
                    .overflow_y_scroll()
                    .children(
                        options
                            .into_iter()
                            .enumerate()
                            .map(|(index, (key, label))| {
                                let active = key == value;
                                let key = key.map(ToOwned::to_owned);
                                menu_item(
                                    ("lookup-option", index),
                                    label.to_owned(),
                                    false,
                                    theme,
                                    density,
                                    cx.listener(move |app, _, _, cx| {
                                        if let ActiveModal::PresetLookup(lookup) = &mut app.modal {
                                            match menu {
                                                OpenMenu::LookupGroup => {
                                                    lookup.group_filter.clone_from(&key)
                                                }
                                                OpenMenu::LookupStatus => {
                                                    lookup.status_filter =
                                                        key.clone().unwrap_or_default()
                                                }
                                                OpenMenu::LookupRejoin => {
                                                    lookup.rejoin_filter =
                                                        key.clone().unwrap_or_default()
                                                }
                                                _ => unreachable!(),
                                            }
                                        }
                                        app.open_menu = None;
                                        cx.notify();
                                    }),
                                )
                                .when(active, |item| item.bg(theme.bg_selected))
                            }),
                    ),
            ))
        })
}
