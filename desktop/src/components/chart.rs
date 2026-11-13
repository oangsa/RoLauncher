//! Efficient GPU-painted charts and meters for Resource Monitor and Dashboard views.

use crate::domain::models::ResourceHistoryPoint;
use crate::theme::{AppTheme, DensityTokens};
use gpui::{AnyElement, Bounds, Hsla, canvas, div, fill, point, prelude::*, px, size};

/// Single vertical sample pair plotted inside [`dual_series_bar_chart`].
#[derive(Debug, Clone, Copy)]
pub struct ChartSample {
    /// Background reference value in `0.0..=100.0` (e.g. total system utilization).
    pub primary_pct: f32,
    /// Foreground managed value in `0.0..=100.0` (e.g. managed Roblox utilization).
    pub secondary_pct: f32,
}

/// Converts [`ResourceHistoryPoint`] CPU samples into normalized chart pairs.
pub fn cpu_chart_samples(points: &[ResourceHistoryPoint]) -> Vec<ChartSample> {
    points
        .iter()
        .map(|p| ChartSample {
            primary_pct: p.system_cpu_percent.unwrap_or(0.0).clamp(0.0, 100.0) as f32,
            secondary_pct: p.managed_cpu_percent.clamp(0.0, 100.0) as f32,
        })
        .collect()
}

/// Converts [`ResourceHistoryPoint`] RAM samples into normalized chart pairs.
pub fn ram_chart_samples(
    points: &[ResourceHistoryPoint],
    total_system_ram_bytes: u64,
) -> Vec<ChartSample> {
    let total_f = total_system_ram_bytes.max(1) as f64;
    points
        .iter()
        .map(|p| {
            let sys_pct = p.ram_utilization_percent.unwrap_or(0.0).clamp(0.0, 100.0) as f32;
            let managed_pct =
                ((p.managed_ram_bytes as f64 / total_f) * 100.0).clamp(0.0, 100.0) as f32;
            ChartSample {
                primary_pct: sys_pct,
                secondary_pct: managed_pct,
            }
        })
        .collect()
}

/// Renders a fixed-capacity dual-series vertical bar chart using a single GPUI `canvas` pass.
#[allow(clippy::too_many_arguments)]
pub fn dual_series_bar_chart(
    title: &'static str,
    primary_label: &'static str,
    secondary_label: &'static str,
    primary_val_text: String,
    secondary_val_text: String,
    samples: &[ChartSample],
    capacity: usize,
    theme: &AppTheme,
    density: &DensityTokens,
) -> AnyElement {
    let slots = capacity.max(1);
    let data = samples.to_vec();
    let grid_color = theme.chart_grid;
    let sys_color = theme.chart_sys;
    let managed_color = theme.chart_managed;

    div()
        .flex()
        .flex_1()
        .flex_col()
        .gap(px(8.0))
        .p(px(density.card_pad))
        .rounded(px(10.0))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_surface)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(
                    div()
                        .text_size(px(density.font_body))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_primary)
                        .child(title),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(12.0))
                        .text_size(px(density.font_caption))
                        .child(legend_item(
                            primary_label,
                            primary_val_text,
                            sys_color,
                            theme,
                        ))
                        .child(legend_item(
                            secondary_label,
                            secondary_val_text,
                            managed_color,
                            theme,
                        )),
                ),
        )
        .child(
            div()
                .relative()
                .w_full()
                .h(px(136.0))
                .p(px(6.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(theme.border_subtle)
                .bg(theme.bg_input)
                .child(
                    canvas(
                        move |_bounds, _, _| (),
                        move |bounds, (), window, _| {
                            let w = f32::from(bounds.size.width);
                            let h = f32::from(bounds.size.height);
                            if w <= 4.0 || h <= 4.0 {
                                return;
                            }

                            // Horizontal 25%, 50%, 75% reference lines.
                            for frac in [0.25_f32, 0.50, 0.75] {
                                let y = bounds.top() + px(h * (1.0 - frac));
                                window.paint_quad(fill(
                                    Bounds::new(point(bounds.left(), y), size(px(w), px(1.0))),
                                    grid_color,
                                ));
                            }

                            let count = data.len().min(slots);
                            if count == 0 {
                                return;
                            }
                            let slot_w = w / (slots as f32);
                            let bar_w = (slot_w - 2.0).max(2.0);
                            let start_slot = slots.saturating_sub(count);

                            for (idx, sample) in data.iter().rev().take(slots).enumerate() {
                                let slot = start_slot + (count - 1 - idx);
                                let x = f32::from(bounds.left()) + (slot as f32) * slot_w + 1.0;

                                let sys_h = (h * (sample.primary_pct / 100.0))
                                    .max(if sample.primary_pct > 0.0 { 2.0 } else { 0.0 });
                                if sys_h > 0.0 {
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            point(px(x), px(f32::from(bounds.bottom()) - sys_h)),
                                            size(px(bar_w), px(sys_h)),
                                        ),
                                        sys_color,
                                    ));
                                }

                                let mgr_h = (h * (sample.secondary_pct / 100.0))
                                    .max(if sample.secondary_pct > 0.0 { 2.0 } else { 0.0 });
                                if mgr_h > 0.0 {
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            point(px(x), px(f32::from(bounds.bottom()) - mgr_h)),
                                            size(px(bar_w), px(mgr_h)),
                                        ),
                                        managed_color,
                                    ));
                                }
                            }
                        },
                    )
                    .size_full(),
                )
                .when(samples.is_empty(), |wrap| {
                    wrap.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(density.font_caption))
                            .text_color(theme.text_muted)
                            .child("Collecting live samples…"),
                    )
                }),
        )
        .into_any_element()
}

fn legend_item(label: &str, value: String, color: Hsla, theme: &AppTheme) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(5.0))
        .child(div().size(px(8.0)).rounded(px(2.0)).bg(color))
        .child(
            div()
                .text_color(theme.text_secondary)
                .child(format!("{label}: {value}")),
        )
}

/// Renders an 8 x 3-hour activity histogram (`24h` window) matching the WinUI Dashboard chart.
pub fn activity_8x3h_chart(
    buckets: &[u32; 8],
    theme: &AppTheme,
    density: &DensityTokens,
) -> AnyElement {
    let max_val = buckets.iter().copied().max().unwrap_or(1).max(1) as f32;
    let bars = *buckets;
    let bar_color = theme.accent;
    let empty_color = theme.border_subtle;

    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .w_full()
                .h(px(68.0))
                .p(px(4.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(theme.border_subtle)
                .bg(theme.bg_input)
                .child(
                    canvas(
                        move |_bounds, _, _| (),
                        move |bounds, (), window, _| {
                            let w = f32::from(bounds.size.width);
                            let h = f32::from(bounds.size.height);
                            if w <= 4.0 || h <= 4.0 {
                                return;
                            }
                            let slot_w = w / 8.0;
                            let bar_w = (slot_w - 8.0).max(4.0);
                            for (i, &count) in bars.iter().enumerate() {
                                let x = f32::from(bounds.left()) + (i as f32) * slot_w + 4.0;
                                let bar_h = if count == 0 {
                                    3.0
                                } else {
                                    ((count as f32 / max_val) * (h - 2.0)).max(5.0)
                                };
                                let color = if count == 0 { empty_color } else { bar_color };
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(px(x), px(f32::from(bounds.bottom()) - bar_h)),
                                        size(px(bar_w), px(bar_h)),
                                    ),
                                    color,
                                ));
                            }
                        },
                    )
                    .size_full(),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .justify_between()
                .text_size(px(density.font_caption))
                .text_color(theme.text_muted)
                .child("24h ago")
                .child("12h ago")
                .child("Now"),
        )
        .into_any_element()
}

/// Renders a linear progress meter clamped to `0..=100` percent.
pub fn progress_bar(percent: u8, theme: &AppTheme) -> AnyElement {
    let clamped = (percent.min(100) as f32) / 100.0;
    div()
        .w_full()
        .h(px(8.0))
        .rounded(px(999.0))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_input)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(gpui::relative(clamped))
                .rounded(px(999.0))
                .bg(theme.accent),
        )
        .into_any_element()
}
