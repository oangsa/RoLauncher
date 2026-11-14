//! Accounts workspace page: cookie/browser onboarding, multi-filter search, bulk selection, and 11-column account table.

use crate::app::{RoLauncherApp, on_action};
use crate::components::menu::{menu_item, popover, popover_box};
use crate::components::primitives::{
    ButtonVariant, button, card, checkbox, icon, icon_button, status_badge,
};
use crate::domain::filter::{REJOIN_FILTER_OPTIONS, STATUS_FILTER_OPTIONS, matches_account};
use crate::domain::models::{
    can_retry, format_account_status, format_destination_kind, format_duration, format_error,
    format_group, format_retry, longest_seconds, running_seconds,
};
use crate::state::{ConfirmAction, ConfirmModal, OpenMenu};
use chrono::Utc;
use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use std::collections::BTreeSet;

pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let busy = app.busy;
    let open_menu = app.open_menu;
    let accounts = app
        .snapshot
        .as_ref()
        .map(|s| s.accounts.clone())
        .unwrap_or_default();

    let search_text = app.account_search_input.read(cx).text().to_string();
    let status_filter = app.account_status_filter.clone();
    let rejoin_filter = app.account_rejoin_filter.clone();
    let group_filter = app.account_group_filter.clone();

    let groups = accounts
        .iter()
        .map(|account| account.group.as_str())
        .filter(|group| !group.is_empty())
        .collect::<BTreeSet<_>>();

    let visible: Vec<_> = accounts
        .iter()
        .filter(|a| {
            let gf = (!group_filter.is_empty()).then_some(group_filter.as_str());
            matches_account(a, &search_text, &status_filter, &rejoin_filter, gf)
        })
        .cloned()
        .collect();

    let selected_ids: Vec<String> = accounts
        .iter()
        .filter(|a| app.selected_account_ids.contains(&a.id))
        .map(|a| a.id.clone())
        .collect();
    let sel_count = selected_ids.len();
    let total_count = accounts.len();
    let visible_count = visible.len();

    let caption = |text: &'static str| {
        div()
            .text_size(px(density.font_caption))
            .text_color(theme.text_muted)
            .child(text)
    };
    let heading = |text: &'static str| {
        div()
            .text_size(px(density.font_section))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(text)
    };

    let add_card = card(&theme, &density)
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(16.0))
                .child(heading("Add account"))
                .child(caption("Sign in or import cookies.")),
        )
        .child(app.cookie_batch_input.clone())
        .child(caption("Cookies are visible until import starts."))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.0))
                .child(button(
                    "acct-submit-cookies",
                    "Import cookies",
                    ButtonVariant::Secondary,
                    busy,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _, cx| this.submit_cookie_textbox_import(cx)),
                ))
                .child(button(
                    "acct-import-txt",
                    "Import file…",
                    ButtonVariant::Secondary,
                    busy,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, window, cx| {
                        this.import_cookies_from_file(window, cx)
                    }),
                ))
                .child(button(
                    "acct-browser-login",
                    "Sign in",
                    ButtonVariant::Primary,
                    busy,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _, cx| this.start_browser_login(cx)),
                )),
        );

    let filters = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(12.0))
        .child(
            div()
                .flex_grow(1.0)
                .flex_basis(px(220.0))
                .child(app.account_search_input.clone()),
        )
        .child(filter_picker(
            OpenMenu::AccountStatus,
            open_menu,
            &status_filter,
            "All statuses",
            STATUS_FILTER_OPTIONS
                .iter()
                .skip(1)
                .map(|&(value, label)| (value, label.to_owned())),
            &theme,
            &density,
            cx,
        ))
        .child(filter_picker(
            OpenMenu::AccountRejoin,
            open_menu,
            &rejoin_filter,
            "All rejoin settings",
            REJOIN_FILTER_OPTIONS
                .iter()
                .skip(1)
                .map(|&(value, label)| (value, label.to_owned())),
            &theme,
            &density,
            cx,
        ))
        .child(button(
            "acct-clear-filters",
            "Clear",
            ButtonVariant::Secondary,
            false,
            false,
            &theme,
            &density,
            on_action(cx, |this, _, cx| {
                this.account_search_input
                    .update(cx, |ti, cx| ti.set_text("", cx));
                this.account_status_filter.clear();
                this.account_rejoin_filter.clear();
                this.account_group_filter.clear();
                this.open_menu = None;
                this.prune_hidden_selection(cx);
                cx.notify();
            }),
        ));

    // Toolbar
    let can_retry_sel = accounts
        .iter()
        .any(|a| app.selected_account_ids.contains(&a.id) && can_retry(a));
    let can_repair_sel = sel_count == 1
        && accounts
            .iter()
            .find(|a| a.id == selected_ids[0])
            .is_some_and(|a| a.status == "needs_attention");

    let toolbar = div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(heading("Accounts"))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_muted)
                        .child(if total_count == 0 {
                            "0 accounts".to_owned()
                        } else {
                            format!("{visible_count}/{total_count} accounts · {sel_count} selected")
                        }),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(button(
                    "acct-tb-start",
                    "Start",
                    ButtonVariant::Primary,
                    busy || sel_count == 0,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _, cx| {
                        this.run_selected_account_command("start", cx)
                    }),
                ))
                .child(button(
                    "acct-tb-stop",
                    "Stop",
                    ButtonVariant::Secondary,
                    busy || sel_count == 0,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _, cx| {
                        this.run_selected_account_command("stop", cx)
                    }),
                ))
                .child(
                    div()
                        .relative()
                        .child(icon_button(
                            "acct-tb-more-menu",
                            "\u{e712}",
                            "More actions",
                            ButtonVariant::Secondary,
                            busy,
                            false,
                            &theme,
                            &density,
                            on_action(cx, |this, _, cx| {
                                toggle_menu(this, OpenMenu::AccountsMore, cx);
                            }),
                        ))
                        .when(open_menu == Some(OpenMenu::AccountsMore), |wrap| {
                            wrap.child(popover(
                                true,
                                more_popover(
                                    sel_count,
                                    can_retry_sel,
                                    can_repair_sel,
                                    total_count,
                                    &theme,
                                    &density,
                                    cx,
                                ),
                            ))
                        }),
                ),
        );

    let selection_toolbar = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.0))
        .child(filter_picker(
            OpenMenu::AccountGroup,
            open_menu,
            &group_filter,
            "All groups",
            groups.into_iter().map(|group| (group, group.to_owned())),
            &theme,
            &density,
            cx,
        ))
        .child(button(
            "acct-tb-edit",
            "Edit selected",
            ButtonVariant::Secondary,
            busy || sel_count == 0,
            false,
            &theme,
            &density,
            on_action(cx, |this, window, cx| this.open_selected_editor(window, cx)),
        ))
        .child(
            div()
                .relative()
                .child(button(
                    "acct-tb-sel-menu",
                    "Selection",
                    ButtonVariant::Secondary,
                    false,
                    false,
                    &theme,
                    &density,
                    on_action(cx, |this, _, cx| {
                        toggle_menu(this, OpenMenu::AccountsSelection, cx)
                    }),
                ))
                .when(open_menu == Some(OpenMenu::AccountsSelection), |wrap| {
                    wrap.child(popover(
                        false,
                        selection_popover(
                            visible_count,
                            total_count,
                            sel_count,
                            &theme,
                            &density,
                            cx,
                        ),
                    ))
                }),
        );

    let table_header = table_header(&theme, &density, visible_count, sel_count, cx);
    let mut table_rows = div().flex().flex_col().w_full().gap(px(4.0));
    if visible.is_empty() {
        table_rows = table_rows.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .min_h(px(170.0))
                .gap(px(8.0))
                .text_color(theme.text_muted)
                .child(icon("\u{e77b}", 28.0))
                .child(
                    div()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .child(if total_count == 0 {
                            "No accounts yet"
                        } else {
                            "No matching accounts"
                        }),
                )
                .child(caption(if total_count == 0 {
                    "Add an account above."
                } else {
                    "Clear search or filters to see more accounts."
                })),
        );
    } else {
        for (idx, acct) in visible.iter().enumerate() {
            let is_sel = app.selected_account_ids.contains(&acct.id);
            let resolved_game = acct
                .target
                .as_ref()
                .and_then(|t| app.game_cache.display_name(t.place_id))
                .map(ToOwned::to_owned);

            let game_label = match (&acct.target, resolved_game) {
                (Some(_), Some(name)) => name,
                (Some(t), None) => format!("Place {}", t.place_id),
                (None, _) => "Choose a game".to_string(),
            };
            let dest_kind = format_destination_kind(acct.target.as_ref());
            let pid_str = acct
                .process
                .as_ref()
                .map(|p| p.pid.to_string())
                .unwrap_or_else(|| "—".to_string());
            let now = Utc::now();
            let run_secs = running_seconds(acct, now);
            let run_str = if run_secs > 0 {
                format_duration(run_secs)
            } else {
                "—".to_string()
            };
            let long_str = {
                let l = longest_seconds(acct, now);
                if l > 0 {
                    format_duration(l)
                } else {
                    "—".to_string()
                }
            };
            let err_text = if acct.next_retry.is_some() || acct.status == "reconnecting" {
                format_retry(acct, now)
            } else {
                format_error(acct.last_error.as_deref())
            };
            let aid_chk = acct.id.clone();
            let aid_row = acct.id.clone();
            let aid_start = acct.id.clone();
            let aid_stop = acct.id.clone();
            let aid_restart = acct.id.clone();
            let aid_edit = acct.id.clone();
            let aid_del = acct.id.clone();
            let aid_retry = acct.id.clone();
            let alias_del = acct.display_name().to_string();
            let retryable = can_retry(acct);

            let row = div()
                .id(("acct-row", idx))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .px(px(density.row_px))
                .py(px(density.row_py))
                .rounded(px(8.0))
                .cursor_pointer()
                .bg(if is_sel {
                    theme.bg_selected
                } else {
                    gpui::transparent_black()
                })
                .hover(|s| {
                    s.bg(if is_sel {
                        theme.bg_selected
                    } else {
                        theme.bg_surface_hover
                    })
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_menu = None;
                    crate::dialogs::account_editor::open_for_account(this, &aid_row, window, cx);
                }))
                .child(div().id("account-check-cell").w(px(40.0)).on_click(|_, _, cx| cx.stop_propagation()).child(checkbox(
                    ("acct-chk", idx),
                    is_sel,
                    None,
                    false,
                    &theme,
                    &density,
                    on_action(cx, move |this, _w, cx| {
                        if this.selected_account_ids.contains(&aid_chk) {
                            this.selected_account_ids.remove(&aid_chk);
                        } else {
                            this.selected_account_ids.insert(aid_chk.clone());
                        }
                        cx.notify();
                    }),
                )))
                .child(
                    div()
                        .w(px(142.0))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(density.font_body))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme.text_primary)
                                .truncate()
                                .child(acct.display_name().to_string()),
                        )
                        .child(
                            div()
                                .text_size(px(density.font_caption))
                                .text_color(theme.text_muted)
                                .truncate()
                                 .child(format!("@{}", acct.username)),
                        )
                        .child(div().text_size(px(density.font_caption)).text_color(theme.text_muted)
                            .truncate().child(format_group(&acct.group).to_owned())),
                )
                .child(cell_text(pid_str, 45.0, theme.text_secondary, &density))
                .child(
                    div().w(px(110.0)).flex().child(status_badge(
                        &acct.status,
                        format_account_status(&acct.status),
                        &theme,
                        &density,
                    )),
                )
                .child(
                    div()
                        .w(px(131.0))
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .text_size(px(density.font_body))
                                .text_color(theme.text_primary)
                                .truncate()
                                .child(game_label),
                        )
                        .child(
                            div()
                                .text_size(px(density.font_caption))
                                .text_color(theme.text_muted)
                                .truncate()
                                .child(dest_kind),
                        ),
                )
                .child(cell_text(acct.failures.to_string(), 50.0, theme.text_secondary, &density))
                .child(cell_text(
                    if acct.auto_recovery { "On" } else { "Off" }.to_string(),
                    45.0,
                    theme.text_secondary,
                    &density,
                ))
                .child(
                    div()
                        .w(px(100.0))
                        .text_size(px(density.font_caption))
                        .text_color(if acct.last_error.is_some() {
                            theme.warning
                        } else {
                            theme.text_muted
                        })
                        .truncate()
                        .child(err_text),
                )
                .child(cell_text(run_str, 120.0, theme.text_secondary, &density))
                .child(cell_text(long_str, 120.0, theme.text_secondary, &density))
                .child(
                    div()
                        .id("account-actions")
                        .flex_1()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.0))
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(if retryable {
                            icon_button(
                                ("acct-row-retry", idx), "\u{e777}", "Retry this account",
                                ButtonVariant::Subtle,
                                busy,
                                true,
                                &theme,
                                &density,
                                on_action(cx, move |this, _w, cx| {
                                    this.run_account_command(&aid_retry, "retry", cx);
                                }),
                            )
                        } else {
                            icon_button(
                                ("acct-row-start", idx), "\u{e768}", "Start this account",
                                ButtonVariant::Subtle,
                                busy,
                                true,
                                &theme,
                                &density,
                                on_action(cx, move |this, _w, cx| {
                                    this.run_account_command(&aid_start, "start", cx);
                                }),
                            )
                        })
                        .child(icon_button(
                                ("acct-row-stop", idx), "\u{e71a}", "Stop this account",
                            ButtonVariant::Subtle,
                            busy,
                            true,
                            &theme,
                            &density,
                            on_action(cx, move |this, _w, cx| {
                                this.run_account_command(&aid_stop, "stop", cx);
                            }),
                        ))
                        .child(icon_button(
                                ("acct-row-restart", idx), "\u{e777}", "Restart this account",
                            ButtonVariant::Subtle,
                            busy,
                            true,
                            &theme,
                            &density,
                            on_action(cx, move |this, _w, cx| {
                                this.run_account_command(&aid_restart, "restart", cx);
                            }),
                        ))
                        .child(icon_button(
                                ("acct-row-edit", idx), "\u{e70f}", "Edit account",
                            ButtonVariant::Subtle,
                            busy,
                            true,
                            &theme,
                            &density,
                            on_action(cx, move |this, window, cx| {
                                crate::dialogs::account_editor::open_for_account(this, &aid_edit, window, cx);
                            }),
                        ))
                        .child(icon_button(
                                ("acct-row-del", idx), "\u{e74d}", "Remove account",
                            ButtonVariant::Danger,
                            busy,
                            true,
                            &theme,
                            &density,
                            on_action(cx, move |this, _w, cx| {
                                this.modal = crate::state::ActiveModal::Confirm(ConfirmModal {
                                    title: "Remove account".into(),
                                    body: format!(
                                        "Remove {alias_del} from RoLauncher? Running Roblox clients stay open unless stopped first."
                                    ),
                                    confirm_label: "Remove".into(),
                                    dangerous: true,
                                    action: ConfirmAction::DeleteSingleAccount(aid_del.clone()),
                                });
                                cx.notify();
                            }),
                        )),
                );
            table_rows = table_rows.child(row);
        }
    }

    let table = div()
        .id("acct-table-hscroll")
        .w_full()
        .overflow_x_scroll()
        .restrict_scroll_to_axis()
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .w_full()
                .min_w(px(1220.0))
                .child(table_header)
                .child(
                    div()
                        .id("acct-table-vscroll")
                        .min_h(px(170.0))
                        .max_h(px(340.0))
                        .overflow_y_scroll()
                        .restrict_scroll_to_axis()
                        .child(table_rows),
                ),
        );

    div()
        .id("accounts-page-root")
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(add_card)
        .child(
            card(&theme, &density)
                .gap(px(density.section_gap))
                .child(toolbar)
                .child(filters)
                .child(selection_toolbar)
                .child(caption(
                    "Click a row to edit. Use checkboxes for bulk actions.",
                ))
                .child(table),
        )
        .into_any_element()
}

fn toggle_menu(app: &mut RoLauncherApp, menu: OpenMenu, cx: &mut Context<RoLauncherApp>) {
    app.open_menu = (app.open_menu != Some(menu)).then_some(menu);
    cx.notify();
}

#[allow(clippy::too_many_arguments)]
fn filter_picker<'a>(
    menu: OpenMenu,
    open_menu: Option<OpenMenu>,
    value: &str,
    placeholder: &'static str,
    options: impl Iterator<Item = (&'a str, String)>,
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
    cx: &mut Context<RoLauncherApp>,
) -> gpui::Div {
    let options = std::iter::once(("", placeholder.to_owned()))
        .chain(options)
        .collect::<Vec<_>>();
    let label = options
        .iter()
        .find(|(key, _)| *key == value)
        .map(|(_, label)| label.as_str())
        .unwrap_or(placeholder)
        .to_owned();
    let (id, width) = match menu {
        OpenMenu::AccountStatus => ("acct-status-filter", 162.0),
        OpenMenu::AccountRejoin => ("acct-rejoin-filter", 162.0),
        OpenMenu::AccountGroup => ("acct-group-filter", 160.0),
        _ => unreachable!(),
    };
    let mut field = div().relative().min_w(px(width));
    if menu != OpenMenu::AccountGroup {
        field = field.flex_grow(1.0).flex_basis(px(width));
    }
    field
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
                .border_color(theme.border_strong)
                .bg(theme.bg_surface_elevated)
                .cursor_pointer()
                .hover(|style| style.bg(theme.bg_surface_hover))
                .focus(|style| style.border_color(theme.border_focus))
                .on_click(cx.listener(move |app, _, _, cx| toggle_menu(app, menu, cx)))
                .child(div().truncate().child(label))
                .child(icon("\u{e70d}", 10.0)),
        )
        .when(open_menu == Some(menu), |field| {
            let options = options.into_iter().enumerate().map(|(idx, (key, label))| {
                let key = key.to_owned();
                let selected = key == value;
                menu_item(
                    ("filter-option", idx),
                    label,
                    false,
                    theme,
                    density,
                    cx.listener(move |app, _, _, cx| {
                        let filter = match menu {
                            OpenMenu::AccountStatus => &mut app.account_status_filter,
                            OpenMenu::AccountRejoin => &mut app.account_rejoin_filter,
                            OpenMenu::AccountGroup => &mut app.account_group_filter,
                            _ => unreachable!(),
                        };
                        filter.clone_from(&key);
                        app.open_menu = None;
                        app.prune_hidden_selection(cx);
                        cx.notify();
                    }),
                )
                .when(selected, |row| row.bg(theme.bg_selected))
            });
            field.child(popover(false, popover_box(theme).children(options)))
        })
}

fn cell_text(
    text: String,
    width_px: f32,
    color: gpui::Hsla,
    density: &crate::theme::DensityTokens,
) -> gpui::Div {
    div()
        .w(px(width_px))
        .text_size(px(density.font_caption))
        .text_color(color)
        .truncate()
        .child(text)
}

fn table_header(
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
    visible_count: usize,
    sel_count: usize,
    cx: &mut Context<RoLauncherApp>,
) -> gpui::Div {
    let all_vis_checked = visible_count > 0 && sel_count >= visible_count;
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .px(px(density.row_px))
        .min_h(px(density.control_h + 12.0))
        .rounded(px(6.0))
        .bg(theme.bg_table_header)
        .text_size(px(density.font_caption))
        .text_color(theme.text_secondary)
        .child(div().w(px(40.0)).child(checkbox(
            "acct-hdr-chk",
            all_vis_checked,
            None,
            visible_count == 0,
            theme,
            density,
            on_action(cx, move |this, _w, cx| {
                if all_vis_checked {
                    this.selected_account_ids.clear();
                } else {
                    this.select_visible_accounts(cx);
                }
                cx.notify();
            }),
        )))
        .child(div().w(px(142.0)).child("Account"))
        .child(div().w(px(45.0)).child("PID"))
        .child(div().w(px(110.0)).child("Status"))
        .child(div().w(px(131.0)).child("Game"))
        .child(div().w(px(50.0)).child("Failures"))
        .child(div().w(px(45.0)).child("Rejoin"))
        .child(div().w(px(100.0)).child("Last error"))
        .child(div().w(px(120.0)).child("Running time"))
        .child(div().w(px(120.0)).child("Longest online"))
        .child(div().flex_1().child("Actions"))
}

fn selection_popover(
    visible_count: usize,
    total_count: usize,
    sel_count: usize,
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
    cx: &mut Context<RoLauncherApp>,
) -> gpui::Stateful<gpui::Div> {
    popover_box(theme)
        .child(menu_item(
            "sel-visible",
            format!("Select visible ({visible_count}) · Ctrl+A"),
            visible_count == 0,
            theme,
            density,
            cx.listener(|this, _ev, _w, cx| {
                this.open_menu = None;
                this.select_visible_accounts(cx);
            }),
        ))
        .child(menu_item(
            "sel-all",
            format!("Select all accounts ({total_count})"),
            total_count == 0,
            theme,
            density,
            cx.listener(|this, _ev, _w, cx| {
                this.open_menu = None;
                this.select_all_accounts(cx);
            }),
        ))
        .child(menu_item(
            "sel-edit-all",
            "Edit all accounts…".to_string(),
            total_count == 0,
            theme,
            density,
            cx.listener(|this, _ev, window, cx| {
                this.open_menu = None;
                this.select_all_accounts(cx);
                this.open_selected_editor(window, cx);
            }),
        ))
        .child(menu_item(
            "sel-clear",
            "Clear selection".to_string(),
            sel_count == 0,
            theme,
            density,
            cx.listener(|this, _ev, _w, cx| {
                this.open_menu = None;
                this.selected_account_ids.clear();
                cx.notify();
            }),
        ))
}

fn more_popover(
    sel_count: usize,
    can_retry_sel: bool,
    can_repair_sel: bool,
    total_count: usize,
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
    cx: &mut Context<RoLauncherApp>,
) -> gpui::Stateful<gpui::Div> {
    popover_box(theme)
        .child(menu_item(
            "more-restart",
            "Restart selected".to_string(),
            sel_count == 0,
            theme,
            density,
            cx.listener(|this, _ev, _w, cx| {
                this.open_menu = None;
                this.run_selected_account_command("restart", cx);
            }),
        ))
        .child(menu_item(
            "more-retry",
            "Retry selected".to_string(),
            !can_retry_sel,
            theme,
            density,
            cx.listener(|this, _ev, _w, cx| {
                this.open_menu = None;
                this.run_selected_retry(cx);
            }),
        ))
        .child(menu_item(
            "more-repair",
            "Sign in again (repair cookie)".to_string(),
            !can_repair_sel,
            theme,
            density,
            cx.listener(|this, _ev, _w, cx| {
                this.open_menu = None;
                this.repair_selected_account(cx);
            }),
        ))
        .child(menu_item(
            "more-remove",
            "Remove selected…".to_string(),
            sel_count == 0,
            theme,
            density,
            cx.listener(|this, _ev, _w, cx| {
                this.open_menu = None;
                let ids: Vec<String> = this.selected_account_ids.iter().cloned().collect();
                let count = ids.len();
                this.modal = crate::state::ActiveModal::Confirm(ConfirmModal {
                    title: "Remove selected accounts".into(),
                    body: format!("Remove {count} selected account(s) from RoLauncher?"),
                    confirm_label: "Remove".into(),
                    dangerous: true,
                    action: ConfirmAction::DeleteSelectedAccounts(ids),
                });
                cx.notify();
            }),
        ))
        .child(menu_item(
            "more-close-all",
            "Stop all Roblox processes…".to_string(),
            total_count == 0,
            theme,
            density,
            cx.listener(|this, _ev, _w, cx| {
                this.open_menu = None;
                this.modal = crate::state::ActiveModal::Confirm(ConfirmModal {
                    title: "Stop all Roblox processes".into(),
                    body: "Stop every running Roblox process (including unmanaged instances) and mark managed accounts stopped?".into(),
                    confirm_label: "Stop all".into(),
                    dangerous: true,
                    action: ConfirmAction::StopAllRoblox,
                });
                cx.notify();
            }),
        ))
}
