//! "What's new" release notes dialog displaying the latest two entries from CHANGELOG.md.

use crate::app::{RoLauncherApp, on_action};
use crate::components::modal::modal_frame;
use crate::components::primitives::{ButtonVariant, button};
use crate::state::ActiveModal;
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

const EMBEDDED_CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// Extracts up to the first `limit` version sections from `CHANGELOG.md`.
pub fn parse_latest_changelog_sections(markdown: &str, limit: usize) -> Vec<(String, Vec<String>)> {
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    for line in markdown.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            if sections.len() >= limit {
                break;
            }
            sections.push((heading.trim().to_string(), Vec::new()));
            continue;
        }
        if let Some((_, lines)) = sections.last_mut()
            && !trimmed.is_empty()
            && !trimmed.starts_with('#')
        {
            lines.push(trimmed.to_string());
        }
    }
    sections
}

/// Reads local `CHANGELOG.md` adjacent to the executable when available, falling back to the embedded copy.
pub fn load_changelog_text() -> String {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for candidate in [
            dir.join("CHANGELOG.md"),
            dir.join("..\\..\\..\\CHANGELOG.md"),
        ] {
            if let Ok(text) = std::fs::read_to_string(candidate) {
                return text;
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string("CHANGELOG.md") {
        return text;
    }
    EMBEDDED_CHANGELOG.to_string()
}

pub fn render(
    app: &mut RoLauncherApp,
    window: &mut Window,
    cx: &mut Context<RoLauncherApp>,
) -> AnyElement {
    let theme = app.theme(window);
    let density = app.density();
    let sections = parse_latest_changelog_sections(&load_changelog_text(), 2);

    let mut body = div().flex().flex_col().gap(px(14.0));
    for (heading, lines) in sections {
        let mut sec = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .p(px(12.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme.border_subtle)
            .bg(theme.bg_surface_elevated)
            .child(
                div()
                    .text_size(px(density.font_section))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.text_primary)
                    .child(heading),
            );
        for line in lines {
            sec = sec.child(
                div()
                    .text_size(px(density.font_body))
                    .text_color(theme.text_secondary)
                    .child(line),
            );
        }
        body = body.child(sec);
    }

    let footer = div()
        .flex()
        .flex_row()
        .gap(px(8.0))
        .child(button(
            "changelog-open-full",
            "Open full CHANGELOG.md",
            ButtonVariant::Secondary,
            false,
            false,
            &theme,
            &density,
            |_w, cx| {
                cx.open_url("https://github.com/oangsa/RoLauncher/blob/main/CHANGELOG.md");
            },
        ))
        .child(button(
            "changelog-close",
            "Close",
            ButtonVariant::Primary,
            false,
            false,
            &theme,
            &density,
            on_action(cx, |this, _w, cx| {
                this.modal = ActiveModal::None;
                cx.notify();
            }),
        ))
        .into_any_element();

    modal_frame(
        "changelog-modal",
        "What's new in RoLauncher",
        Some(format!("Running v{}", app.bootstrap.version).into()),
        600.0,
        true,
        &theme,
        &density,
        on_action(cx, |this, _w, cx| {
            this.modal = ActiveModal::None;
            cx.notify();
        }),
        body.into_any_element(),
        footer,
    )
}
