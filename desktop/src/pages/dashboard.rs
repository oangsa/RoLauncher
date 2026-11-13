//! Dashboard overview: status breakdown, longest streak, 24h histogram, recovery alerts, and paginated activity logs.

use crate::app::{RoLauncherApp, on_action};
use crate::components::chart::{activity_8x3h_chart, progress_bar};
use crate::components::primitives::{ButtonVariant, button, card, stat_card, status_badge};
use crate::domain::models::{
    format_account_status, format_duration, format_fallback, format_group, format_recovery_reason,
    format_retry, longest_seconds,
};
use chrono::{DateTime, Duration as ChronoDuration, Local, Utc};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

const PAGE_SIZE: usize = 10;

pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let snapshot = app.snapshot.as_ref();
    let accounts = snapshot.map(|s| s.accounts.as_slice()).unwrap_or(&[]);
    let total = accounts.len();

    let running = accounts.iter().filter(|a| a.status == "running").count();
    let launching = accounts
        .iter()
        .filter(|a| matches!(a.status.as_str(), "queued" | "launching"))
        .count();
    let reconnecting = accounts
        .iter()
        .filter(|a| matches!(a.status.as_str(), "reconnecting" | "backoff"))
        .count();
    let stopped = accounts
        .iter()
        .filter(|a| matches!(a.status.as_str(), "stopped" | "stopping" | "unknown"))
        .count();
    let attention = accounts
        .iter()
        .filter(|a| a.status == "needs_attention")
        .count();
    let covered = accounts
        .iter()
        .filter(|a| a.auto_recovery && a.target.is_some())
        .count();
    let with_dest = accounts.iter().filter(|a| a.target.is_some()).count();
    let now = Utc::now();
    let longest = accounts.iter().max_by_key(|a| longest_seconds(a, now));
    let best_streak = longest.map_or(0, |a| longest_seconds(a, now));
    let suspended = snapshot.is_some_and(|s| s.network_suspended);

    // Account totals
    let kpi_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(12.0))
        .child(stat_card(
            "Saved accounts",
            total.to_string(),
            format!(
                "{} saved games",
                snapshot.map_or(0, |s| s.game_profiles.len())
            ),
            None,
            &theme,
            &density,
        ))
        .child(stat_card(
            "Running",
            running.to_string(),
            format!("{launching} queued or launching"),
            None,
            &theme,
            &density,
        ))
        .child(stat_card(
            "Rejoining",
            reconnecting.to_string(),
            if suspended {
                "Connectivity suspended".to_string()
            } else {
                "Automatic rejoin".to_string()
            },
            None,
            &theme,
            &density,
        ))
        .child(stat_card(
            "Needs attention",
            attention.to_string(),
            "Review below",
            None,
            &theme,
            &density,
        ));
    let longest_card = stat_card(
        "Longest streak",
        if best_streak > 0 {
            format_duration(best_streak)
        } else {
            "—".into()
        },
        longest
            .filter(|_| best_streak > 0)
            .map(|a| format!("{} · @{}", a.display_name(), a.username))
            .unwrap_or_else(|| "No streak recorded".to_owned()),
        None,
        &theme,
        &density,
    )
    .child(
        div()
            .text_size(px(density.font_caption))
            .text_color(theme.text_muted)
            .child("Includes rejoins. Stops and manual restarts end the streak."),
    );

    // Status breakdown & coverage
    let breakdown_card = card(&theme, &density)
        .flex_1()
        .min_w(px(340.0))
        .child(
            div()
                .text_size(px(density.font_section))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Account status"),
        )
        .child(meter_row("Running", running, total, &theme, &density))
        .child(meter_row(
            "Queued / Launching",
            launching,
            total,
            &theme,
            &density,
        ))
        .child(meter_row(
            "Rejoining / Backoff",
            reconnecting,
            total,
            &theme,
            &density,
        ))
        .child(meter_row(
            "Stopped / Stopping",
            stopped,
            total,
            &theme,
            &density,
        ))
        .child(meter_row(
            "Needs attention",
            attention,
            total,
            &theme,
            &density,
        ));
    let coverage = (covered * 100).checked_div(total).unwrap_or(0) as u8;
    let coverage_card = card(&theme, &density)
        .flex_1()
        .min_w(px(300.0))
        .child(
            div()
                .text_size(px(density.font_section))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Rejoin coverage"),
        )
        .child(
            div()
                .text_size(px(density.font_title + 8.0))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(format!("{coverage}%")),
        )
        .child(progress_bar(coverage, &theme))
        .child(format!("{covered}/{total} accounts ready for rejoin"))
        .child(
            div()
                .text_size(px(density.font_caption))
                .text_color(theme.text_muted)
                .child(format!("{} without a destination", total - with_dest)),
        )
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(if suspended {
                    "Network unavailable · launches and rejoins paused"
                } else {
                    "Network available"
                }),
        );

    // 24h Activity histogram (8 x 3h buckets matching account filter)
    let filter_acc = app.dashboard_account_filter.clone();
    let filtered_activities: Vec<_> = app
        .activities
        .iter()
        .filter(|act| filter_acc.is_empty() || act.account_id == filter_acc)
        .cloned()
        .collect();
    let buckets = compute_8x3h_buckets(&filtered_activities, now);

    let mut acct_filter_chips = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(4.0))
        .child(button(
            "dash-flt-all",
            "All accounts",
            if filter_acc.is_empty() {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            },
            false,
            true,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.dashboard_account_filter.clear();
                this.dashboard_activity_page = 0;
                this.refresh_activity(cx);
            }),
        ));
    for (idx, a) in accounts.iter().take(12).enumerate() {
        let active = filter_acc == a.id;
        let aid = a.id.clone();
        acct_filter_chips = acct_filter_chips.child(button(
            ("dash-flt-acc", idx),
            a.display_name().to_string(),
            if active {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            },
            false,
            true,
            &theme,
            &density,
            on_action(cx, move |this, _w, cx| {
                this.dashboard_account_filter = aid.clone();
                this.dashboard_activity_page = 0;
                this.refresh_activity(cx);
            }),
        ));
    }

    let chart_card = card(&theme, &density)
        .child(
            div()
                .text_size(px(density.font_section))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_primary)
                .child("Activity · last 24 hours"),
        )
        .child(acct_filter_chips)
        .child(activity_8x3h_chart(&buckets, &theme, &density));

    // Needs your attention list
    let troubled: Vec<_> = accounts
        .iter()
        .filter(|a| {
            a.status == "needs_attention"
                || a.status == "backoff"
                || a.failures > 0
                || a.last_error.is_some()
        })
        .cloned()
        .collect();

    let mut attention_card = card(&theme, &density).child(
        div()
            .text_size(px(density.font_section))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.text_primary)
            .child(format!("Needs your attention ({})", troubled.len())),
    );
    if troubled.is_empty() {
        attention_card = attention_card.child(
            div()
                .text_size(px(density.font_body))
                .text_color(theme.text_muted)
                .child("All managed accounts look healthy."),
        );
    } else {
        for (idx, a) in troubled.iter().enumerate() {
            let aid = a.id.clone();
            let reason = if a.recovery_reason.is_empty() {
                String::new()
            } else {
                format_recovery_reason(a)
            };
            let retry = format_retry(a, now);
            let detail = format!(
                "{} · {} · Failures {}/5{}{}",
                format_group(&a.group),
                format_fallback(a),
                a.failures,
                if reason.is_empty() {
                    "".to_string()
                } else {
                    format!(" · {reason}")
                },
                if retry.is_empty() {
                    "".to_string()
                } else {
                    format!(" · {retry}")
                }
            );
            attention_card = attention_card.child(
                div()
                    .id(("dash-attn", idx))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(10.0))
                    .p(px(8.0))
                    .rounded(px(7.0))
                    .border_1()
                    .border_color(theme.border_subtle)
                    .bg(theme.bg_surface_elevated)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        crate::dialogs::account_editor::open_for_account(this, &aid, window, cx);
                    }))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(density.font_body))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme.text_primary)
                                    .child(format!("{} (@{})", a.display_name(), a.username)),
                            )
                            .child(
                                div()
                                    .text_size(px(density.font_caption))
                                    .text_color(theme.text_secondary)
                                    .child(detail),
                            ),
                    )
                    .child(status_badge(
                        &a.status,
                        format_account_status(&a.status),
                        &theme,
                        &density,
                    )),
            );
        }
    }

    // Activity history paginated card
    let act_total_pages = filtered_activities.len().div_ceil(PAGE_SIZE).max(1);
    let act_page = app.dashboard_activity_page.min(act_total_pages - 1);
    let act_start = act_page * PAGE_SIZE;
    let act_slice: Vec<_> = filtered_activities
        .iter()
        .skip(act_start)
        .take(PAGE_SIZE)
        .cloned()
        .collect();

    let mut act_card = card(&theme, &density)
        .flex_1()
        .min_w(px(340.0))
        .child(pagination_header(
            "Activity history",
            act_page,
            act_total_pages,
            "dash-act",
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.dashboard_activity_page = this.dashboard_activity_page.saturating_sub(1);
                cx.notify();
            }),
            on_action(cx, move |this, _w, cx| {
                if this.dashboard_activity_page + 1 < act_total_pages {
                    this.dashboard_activity_page += 1;
                }
                cx.notify();
            }),
        ));
    if act_slice.is_empty() {
        act_card = act_card.child(
            div()
                .text_size(px(density.font_body))
                .text_color(theme.text_muted)
                .child("No recorded activity events yet."),
        );
    } else {
        for item in act_slice {
            let account_label = accounts
                .iter()
                .find(|a| a.id == item.account_id)
                .map(|a| a.display_name().to_string())
                .unwrap_or_else(|| item.account_id.clone());
            let stamp = item
                .timestamp
                .with_timezone(&Local)
                .format("%H:%M:%S")
                .to_string();
            let detail = if item.message.is_empty() {
                format!("Failures {}/5", item.failures)
            } else {
                item.message
            };
            act_card = act_card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .py(px(4.0))
                    .border_t_1()
                    .border_color(theme.border_subtle)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(density.font_body))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme.text_primary)
                                    .child(format!(
                                        "{} · {}",
                                        account_label,
                                        format_account_status(&item.status)
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(density.font_caption))
                                    .text_color(theme.text_muted)
                                    .child(stamp),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(density.font_caption))
                            .text_color(theme.text_secondary)
                            .child(detail),
                    ),
            );
        }
    }

    // Recent Discord / session notices card
    let notices = app
        .discord_view
        .as_ref()
        .map(|d| d.recent.clone())
        .unwrap_or_default();
    let notice_total_pages = notices.len().div_ceil(PAGE_SIZE).max(1);
    let notice_page = app.dashboard_notice_page.min(notice_total_pages - 1);
    let notice_slice: Vec<_> = notices
        .iter()
        .skip(notice_page * PAGE_SIZE)
        .take(PAGE_SIZE)
        .cloned()
        .collect();

    let mut notice_card =
        card(&theme, &density)
            .flex_1()
            .min_w(px(340.0))
            .child(pagination_header(
                "Recent Discord notices",
                notice_page,
                notice_total_pages,
                "dash-not",
                &theme,
                &density,
                on_action(cx, |this, _w, cx| {
                    this.dashboard_notice_page = this.dashboard_notice_page.saturating_sub(1);
                    cx.notify();
                }),
                on_action(cx, move |this, _w, cx| {
                    if this.dashboard_notice_page + 1 < notice_total_pages {
                        this.dashboard_notice_page += 1;
                    }
                    cx.notify();
                }),
            ));
    if notice_slice.is_empty() {
        notice_card = notice_card.child(
            div()
                .text_size(px(density.font_body))
                .text_color(theme.text_muted)
                .child("No Discord notifications sent in this session."),
        );
    } else {
        for n in notice_slice {
            notice_card = notice_card.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .py(px(4.0))
                    .border_t_1()
                    .border_color(theme.border_subtle)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(density.font_body))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme.text_primary)
                                    .child(format!("{} · {}", n.account, n.title)),
                            )
                            .child(
                                div()
                                    .text_size(px(density.font_caption))
                                    .text_color(theme.text_muted)
                                    .child(
                                        n.timestamp
                                            .with_timezone(&Local)
                                            .format("%H:%M:%S")
                                            .to_string(),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(density.font_caption))
                            .text_color(theme.text_secondary)
                            .child(n.message),
                    ),
            );
        }
    }

    div()
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(kpi_row)
        .child(longest_card)
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(density.section_gap))
                .child(breakdown_card)
                .child(coverage_card),
        )
        .child(chart_card)
        .child(attention_card)
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(density.section_gap))
                .child(act_card)
                .child(notice_card),
        )
        .into_any_element()
}

fn meter_row(
    label: &'static str,
    count: usize,
    total: usize,
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
) -> gpui::Div {
    let pct = (count * 100).checked_div(total).unwrap_or(0).min(100) as u8;
    div()
        .flex()
        .flex_col()
        .gap(px(3.0))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .text_size(px(density.font_caption))
                .child(div().text_color(theme.text_secondary).child(label))
                .child(
                    div()
                        .text_color(theme.text_primary)
                        .child(format!("{count} / {total}")),
                ),
        )
        .child(progress_bar(pct, theme))
}

#[allow(clippy::too_many_arguments)]
fn pagination_header(
    title: &'static str,
    page: usize,
    total_pages: usize,
    id_prefix: &'static str,
    theme: &crate::theme::AppTheme,
    density: &crate::theme::DensityTokens,
    on_newer: impl Fn(&mut Window, &mut gpui::App) + 'static,
    on_older: impl Fn(&mut Window, &mut gpui::App) + 'static,
) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(
            div()
                .text_size(px(density.font_section))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_primary)
                .child(title),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(button(
                    (id_prefix, 1usize),
                    "Newer",
                    ButtonVariant::Secondary,
                    page == 0,
                    true,
                    theme,
                    density,
                    on_newer,
                ))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_muted)
                        .child(format!("{}/{total_pages}", page + 1)),
                )
                .child(button(
                    (id_prefix, 2usize),
                    "Older",
                    ButtonVariant::Secondary,
                    page + 1 >= total_pages,
                    true,
                    theme,
                    density,
                    on_older,
                )),
        )
}

/// Groups UTC activity timestamps into 8 three-hour buckets covering the past 24 hours.
pub fn compute_8x3h_buckets(
    activities: &[crate::domain::models::Activity],
    now: DateTime<Utc>,
) -> [u32; 8] {
    let mut buckets = [0u32; 8];
    let window_start = now - ChronoDuration::hours(24);
    for act in activities {
        let utc = act.timestamp;
        if utc >= window_start && utc <= now {
            let hours_since = (utc - window_start).num_hours().clamp(0, 23) as usize;
            let idx = (hours_since / 3).min(7);
            buckets[idx] += 1;
        }
    }
    buckets
}
