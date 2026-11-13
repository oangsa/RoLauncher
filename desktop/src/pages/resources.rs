//! Live Phase 1 Resource Monitor UI consuming authenticated SSE updates from `/v1/resources/events`.

use crate::app::{RoLauncherApp, on_action, on_select};
use crate::components::chart::{cpu_chart_samples, dual_series_bar_chart, ram_chart_samples};
use crate::components::primitives::{
    BannerTone, ButtonVariant, banner, button, card, segmented_control, stat_card, status_badge,
};
use crate::domain::resources::{
    ResourceInstanceRow, empty_state_copy, format_bytes, format_percent_metric, sort_instances,
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let total_accounts = app.snapshot.as_ref().map(|s| s.accounts.len()).unwrap_or(0);
    let stream_err = app.resource_stream_error.clone();
    let sort_by = app.resource_sort_by.clone();
    let sort_desc = app.resource_sort_desc;
    let history = app.resource_history.clone();

    let mut top_bar = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(10.0));

    let Some(res) = app.resource_snapshot.as_ref() else {
        top_bar = top_bar
            .child(
                div()
                    .text_size(px(density.font_body))
                    .text_color(theme.text_secondary)
                    .child("Connecting to sampler…"),
            )
            .child(button(
                "res-refresh-init",
                "Refresh",
                ButtonVariant::Secondary,
                false,
                false,
                &theme,
                &density,
                on_action(cx, |this, _w, cx| {
                    this.refresh_resources_once(cx);
                }),
            ));

        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(density.section_gap))
            .child(top_bar);
        if let Some(err) = stream_err {
            col = col.child(banner(
                "res-stream-err",
                BannerTone::Warning,
                err,
                None,
                None,
                &theme,
                &density,
            ));
        } else {
            col = col.child(
                card(&theme, &density).child(
                    div()
                        .text_size(px(density.font_body))
                        .text_color(theme.text_muted)
                        .child("Waiting for the first resource sample."),
                ),
            );
        }
        return col.into_any_element();
    };

    let sampled_local = res
        .timestamp
        .with_timezone(&chrono::Local)
        .format("%H:%M:%S");
    let status_text = format!(
        "Updated {sampled_local} · Sample age {} ms",
        res.sample_age_ms
    );

    top_bar = top_bar
        .child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap(px(8.0))
                .child(status_badge(
                    if stream_err.is_some() {
                        "backoff"
                    } else if res.stale {
                        "needs_attention"
                    } else {
                        "running"
                    },
                    if stream_err.is_some() {
                        "Reconnecting"
                    } else if res.stale {
                        "Stale"
                    } else {
                        "Live"
                    },
                    &theme,
                    &density,
                ))
                .child(
                    div()
                        .text_size(px(density.font_caption))
                        .text_color(theme.text_secondary)
                        .child(status_text),
                ),
        )
        .child(button(
            "res-refresh-btn",
            "Refresh",
            ButtonVariant::Secondary,
            false,
            false,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.refresh_resources_once(cx);
            }),
        ));

    // KPI row
    let gpu_sys_note = format!(
        "{} logical CPUs · GPU {}",
        res.system.logical_processors,
        format_percent_metric(
            res.system.gpu_percent,
            &res.system.gpu_availability,
            "Unavailable"
        )
    );
    let ram_note = format!(
        "{} / {} used ({} free)",
        res.system
            .used_ram_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "Unavailable".into()),
        res.system
            .total_ram_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "Unavailable".into()),
        res.system
            .available_ram_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "Unavailable".into())
    );
    let mgr_note = format!(
        "WS {} · Priv {} · GPU {}",
        format_bytes(res.aggregates.working_set_bytes),
        res.aggregates
            .private_memory_bytes
            .map(format_bytes)
            .unwrap_or_else(|| "Unavailable".into()),
        format_percent_metric(
            res.aggregates.gpu_percent,
            &res.aggregates.gpu_availability,
            "Unavailable"
        )
    );
    let inst_note = format!(
        "{} unmanaged · {} stale/exited",
        res.aggregates.unmanaged_roblox_processes.unwrap_or(0),
        res.aggregates.exited_or_stale_instances
    );

    let kpi_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(10.0))
        .child(stat_card(
            "Running instances",
            format!(
                "{} / {}",
                res.aggregates.running_instances, res.aggregates.total_accounts
            ),
            inst_note,
            None,
            &theme,
            &density,
        ))
        .child(stat_card(
            "System CPU",
            format_percent_metric(
                res.system.cpu_percent,
                &res.system.cpu_availability,
                "Unavailable",
            ),
            gpu_sys_note,
            None,
            &theme,
            &density,
        ))
        .child(stat_card(
            "System RAM",
            format_percent_metric(
                res.system.ram_utilization_percent,
                &res.system.ram_availability,
                "Unavailable",
            ),
            ram_note,
            None,
            &theme,
            &density,
        ))
        .child(stat_card(
            "Managed Roblox",
            format_percent_metric(
                Some(res.aggregates.cpu_percent),
                &res.aggregates.cpu_availability,
                "Unavailable",
            ),
            mgr_note,
            Some(theme.chart_managed),
            &theme,
            &density,
        ));

    let cpu_samples = cpu_chart_samples(&history);
    let ram_samples = ram_chart_samples(&history, res.system.total_ram_bytes.unwrap_or(0));
    let charts_row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap(px(density.section_gap))
        .child(dual_series_bar_chart(
            "CPU utilization (60s)",
            "System",
            "Managed",
            format_percent_metric(
                res.system.cpu_percent,
                &res.system.cpu_availability,
                "Unavailable",
            ),
            format_percent_metric(
                Some(res.aggregates.cpu_percent),
                &res.aggregates.cpu_availability,
                "Unavailable",
            ),
            &cpu_samples,
            res.metadata.history_capacity.max(60),
            &theme,
            &density,
        ))
        .child(dual_series_bar_chart(
            "Physical RAM (60s)",
            "System",
            "Managed",
            format_percent_metric(
                res.system.ram_utilization_percent,
                &res.system.ram_availability,
                "Unavailable",
            ),
            format_bytes(res.aggregates.working_set_bytes),
            &ram_samples,
            res.metadata.history_capacity.max(60),
            &theme,
            &density,
        ));

    // Sort controls
    let sort_bar = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .child(segmented_control(
            "res-sort-by",
            &[
                ("account", "Account"),
                ("cpu", "CPU %"),
                ("ram", "Physical RAM"),
                ("uptime", "Uptime"),
                ("pid", "PID"),
                ("status", "Status"),
            ],
            &sort_by,
            false,
            &theme,
            &density,
            on_select(cx, |this, val, _w, cx| {
                this.resource_sort_by = val;
                cx.notify();
            }),
        ))
        .child(button(
            "res-sort-dir",
            if sort_desc {
                "Direction: High → Low"
            } else {
                "Direction: Low → High"
            },
            ButtonVariant::Secondary,
            false,
            true,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.resource_sort_desc = !this.resource_sort_desc;
                cx.notify();
            }),
        ));

    let sorted = sort_instances(&res.accounts, &sort_by, sort_desc);
    let empty_state = empty_state_copy(
        sorted.len(),
        total_accounts.max(res.aggregates.total_accounts),
    );

    let mut rows_col = div().flex().flex_col().w(px(940.0)).child(
        div()
            .flex()
            .flex_row()
            .gap(px(8.0))
            .px(px(density.row_px))
            .py(px(8.0))
            .bg(theme.bg_surface_elevated)
            .text_size(px(density.font_caption))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(theme.text_secondary)
            .child(div().w(px(190.0)).child("Account"))
            .child(div().w(px(155.0)).child("Status"))
            .child(div().w(px(74.0)).child("PID"))
            .child(div().w(px(80.0)).child("CPU %"))
            .child(div().w(px(96.0)).child("RAM"))
            .child(div().w(px(96.0)).child("Private"))
            .child(div().w(px(90.0)).child("GPU"))
            .child(div().flex_1().child("Uptime")),
    );

    if empty_state.visible {
        rows_col = rows_col.child(
            div()
                .p(px(28.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(density.font_section))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .child(empty_state.title),
                )
                .child(
                    div()
                        .text_size(px(density.font_body))
                        .text_color(theme.text_secondary)
                        .child(empty_state.hint),
                ),
        );
    } else {
        for item in &sorted {
            let formatted = ResourceInstanceRow::from_item(item);
            rows_col = rows_col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(density.row_px))
                    .py(px(density.row_py))
                    .border_t_1()
                    .border_color(theme.border_subtle)
                    .child(
                        div()
                            .w(px(190.0))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(density.font_body))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme.text_primary)
                                    .truncate()
                                    .child(formatted.display_name),
                            )
                            .child(
                                div()
                                    .text_size(px(density.font_caption))
                                    .text_color(theme.text_muted)
                                    .truncate()
                                    .child(formatted.username),
                            ),
                    )
                    .child(
                        div()
                            .w(px(155.0))
                            .text_size(px(density.font_caption))
                            .text_color(theme.text_secondary)
                            .truncate()
                            .child(formatted.status),
                    )
                    .child(cell(formatted.pid, 74.0, theme.text_secondary, &density))
                    .child(cell(formatted.cpu, 80.0, theme.text_primary, &density))
                    .child(cell(formatted.ram, 96.0, theme.text_secondary, &density))
                    .child(cell(
                        formatted.private_ram,
                        96.0,
                        theme.text_secondary,
                        &density,
                    ))
                    .child(cell(formatted.gpu, 90.0, theme.text_secondary, &density))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(density.font_caption))
                            .text_color(theme.text_secondary)
                            .child(formatted.uptime),
                    ),
            );
        }
    }

    let mut page = div()
        .flex()
        .flex_col()
        .gap(px(density.section_gap))
        .child(top_bar);

    if let Some(err) = stream_err {
        page = page.child(banner(
            "res-stream-warn",
            BannerTone::Warning,
            err,
            None,
            None,
            &theme,
            &density,
        ));
    } else if res.stale
        || res.availability != "available"
        || res.aggregates.inaccessible_roblox_processes.unwrap_or(0) > 0
    {
        let notice = if res.stale {
            "Resource snapshot is stale; awaiting updated supervisor metrics."
        } else if res.aggregates.inaccessible_roblox_processes.unwrap_or(0) > 0 {
            "Some Roblox processes were inaccessible due to OS permissions; metrics may be partial."
        } else {
            "Some system or per-process metrics reported partial/pending availability."
        };
        page = page.child(banner(
            "res-availability-note",
            BannerTone::Info,
            notice,
            None,
            None,
            &theme,
            &density,
        ));
    }

    page.child(kpi_row)
        .child(charts_row)
        .child(sort_bar)
        .child(
            card(&theme, &density).p(px(0.0)).overflow_hidden().child(
                div()
                    .id("res-table-hscroll")
                    .w_full()
                    .overflow_x_scroll()
                    .child(rows_col),
            ),
        )
        .into_any_element()
}

fn cell(
    value: String,
    w: f32,
    color: gpui::Hsla,
    density: &crate::theme::DensityTokens,
) -> gpui::Div {
    div()
        .w(px(w))
        .text_size(px(density.font_caption))
        .text_color(color)
        .truncate()
        .child(value)
}
