//! Design tokens, light/dark/high-contrast palettes, density sizing, and status colors.

use crate::platform::appearance::AppearancePreference;
use gpui::{Hsla, WindowAppearance, hsla, rgb};

/// Semantic color palette resolved from theme preference, OS appearance, and high-contrast mode.
#[derive(Debug, Clone, Copy)]
pub struct AppTheme {
    pub bg_app: Hsla,
    pub bg_sidebar: Hsla,
    pub bg_surface: Hsla,
    pub bg_surface_elevated: Hsla,
    pub bg_surface_hover: Hsla,
    pub bg_selected: Hsla,
    pub bg_input: Hsla,
    pub bg_table_header: Hsla,
    pub bg_dialog: Hsla,
    pub nav_marker: Hsla,
    pub bg_badge: Hsla,
    pub border_subtle: Hsla,
    pub border_strong: Hsla,
    pub border_focus: Hsla,
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub accent_subtle: Hsla,
    pub accent_fg: Hsla,
    pub success: Hsla,
    pub success_bg: Hsla,
    pub success_border: Hsla,
    pub warning: Hsla,
    pub warning_bg: Hsla,
    pub warning_border: Hsla,
    pub danger: Hsla,
    pub danger_bg: Hsla,
    pub danger_border: Hsla,
    pub info: Hsla,
    pub info_bg: Hsla,
    pub info_border: Hsla,
    pub chart_sys: Hsla,
    pub chart_managed: Hsla,
    pub chart_grid: Hsla,
    pub scrim: Hsla,
}

impl AppTheme {
    /// Resolves active colors from persisted theme preference, GPUI OS appearance, and Windows high-contrast flag.
    pub fn resolve(
        pref: AppearancePreference,
        os_appearance: WindowAppearance,
        high_contrast: bool,
    ) -> Self {
        let os_dark = matches!(
            os_appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let dark = match pref {
            AppearancePreference::System => os_dark,
            AppearancePreference::Dark => true,
            AppearancePreference::Light => false,
        };
        if high_contrast {
            if os_dark {
                Self::high_contrast_dark()
            } else {
                Self::high_contrast_light()
            }
        } else if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    /// Dark WinUI surface colors on a neutral workspace.
    pub fn dark() -> Self {
        Self {
            bg_app: rgb(0x1b1e28).into(),
            bg_sidebar: rgb(0x1c1f28).into(),
            bg_surface: rgb(0x262a35).into(),
            bg_surface_elevated: rgb(0x323642).into(),
            bg_surface_hover: rgb(0x3a3f4b).into(),
            bg_selected: rgb(0x292d37).into(),
            bg_input: rgb(0x323642).into(),
            bg_table_header: rgb(0x303034).into(),
            bg_dialog: rgb(0x292d37).into(),
            nav_marker: rgb(0xafcbff).into(),
            bg_badge: rgb(0x39393b).into(),
            border_subtle: rgba_hex(0xffffff18),
            border_strong: rgba_hex(0xffffff28),
            border_focus: rgb(0x60a5fa).into(),
            text_primary: rgb(0xfafafa).into(),
            text_secondary: rgb(0xd4d4d8).into(),
            text_muted: rgb(0xa1a1aa).into(),
            accent: rgb(0x4cc2ff).into(),
            accent_hover: rgb(0x45afe5).into(),
            accent_subtle: hsla(217.0 / 360.0, 0.91, 0.60, 0.16),
            accent_fg: rgb(0x18181b).into(),
            success: rgb(0x4ade80).into(),
            success_bg: hsla(142.0 / 360.0, 0.71, 0.45, 0.14),
            success_border: hsla(142.0 / 360.0, 0.71, 0.45, 0.36),
            warning: rgb(0xfbbf24).into(),
            warning_bg: hsla(38.0 / 360.0, 0.92, 0.50, 0.14),
            warning_border: hsla(38.0 / 360.0, 0.92, 0.50, 0.38),
            danger: rgb(0xff9494).into(),
            danger_bg: hsla(0.0, 0.84, 0.60, 0.14),
            danger_border: hsla(0.0, 0.84, 0.60, 0.38),
            info: rgb(0x60a5fa).into(),
            info_bg: hsla(217.0 / 360.0, 0.91, 0.60, 0.14),
            info_border: hsla(217.0 / 360.0, 0.91, 0.60, 0.36),
            chart_sys: rgb(0xa1a1aa).into(),
            chart_managed: rgb(0xafcbff).into(),
            chart_grid: rgba_hex(0xffffff12),
            scrim: rgba_hex(0x00000090),
        }
    }

    /// Light WinUI surface colors on a neutral workspace.
    pub fn light() -> Self {
        Self {
            bg_app: rgb(0xf0f1f4).into(),
            bg_sidebar: rgb(0xf5f6f8).into(),
            bg_surface: rgb(0xf9f9fa).into(),
            bg_surface_elevated: rgb(0xffffff).into(),
            bg_surface_hover: rgb(0xeeeeef).into(),
            bg_selected: rgb(0xededee).into(),
            bg_input: rgb(0xffffff).into(),
            bg_table_header: rgb(0xf5f5f6).into(),
            bg_dialog: rgb(0xfafafa).into(),
            nav_marker: rgb(0x2563eb).into(),
            bg_badge: rgb(0xf0f0f2).into(),
            border_subtle: rgba_hex(0x00000014),
            border_strong: rgba_hex(0x00000024),
            border_focus: rgb(0x2563eb).into(),
            text_primary: rgb(0x18181b).into(),
            text_secondary: rgb(0x52525b).into(),
            text_muted: rgb(0x71717a).into(),
            accent: rgb(0x0067b8).into(),
            accent_hover: rgb(0x005fa9).into(),
            accent_subtle: hsla(217.0 / 360.0, 0.91, 0.53, 0.10),
            accent_fg: rgb(0xffffff).into(),
            success: rgb(0x15803d).into(),
            success_bg: hsla(142.0 / 360.0, 0.76, 0.36, 0.11),
            success_border: hsla(142.0 / 360.0, 0.76, 0.36, 0.34),
            warning: rgb(0xb45309).into(),
            warning_bg: hsla(32.0 / 360.0, 0.95, 0.44, 0.12),
            warning_border: hsla(32.0 / 360.0, 0.95, 0.44, 0.38),
            danger: rgb(0xb91c1c).into(),
            danger_bg: hsla(0.0, 0.74, 0.42, 0.11),
            danger_border: hsla(0.0, 0.74, 0.42, 0.34),
            info: rgb(0x1d4ed8).into(),
            info_bg: hsla(217.0 / 360.0, 0.91, 0.53, 0.10),
            info_border: hsla(217.0 / 360.0, 0.91, 0.53, 0.34),
            chart_sys: rgb(0xa1a1aa).into(),
            chart_managed: rgb(0x315ca8).into(),
            chart_grid: rgba_hex(0x0f172a14),
            scrim: rgba_hex(0x0f172a66),
        }
    }

    /// High-contrast dark palette with crisp borders and AAA-level contrast.
    pub fn high_contrast_dark() -> Self {
        Self {
            bg_app: rgb(0x000000).into(),
            bg_sidebar: rgb(0x000000).into(),
            bg_surface: rgb(0x080808).into(),
            bg_surface_elevated: rgb(0x121212).into(),
            bg_surface_hover: rgb(0x1f1f1f).into(),
            bg_selected: rgb(0x1a365d).into(),
            bg_input: rgb(0x000000).into(),
            bg_table_header: rgb(0x000000).into(),
            bg_dialog: rgb(0x000000).into(),
            nav_marker: rgb(0xffff00).into(),
            bg_badge: rgb(0x1a1a1a).into(),
            border_subtle: rgb(0xffffff).into(),
            border_strong: rgb(0xffffff).into(),
            border_focus: rgb(0xffff00).into(),
            text_primary: rgb(0xffffff).into(),
            text_secondary: rgb(0xffffff).into(),
            text_muted: rgb(0xe5e5e5).into(),
            accent: rgb(0x1aebff).into(),
            accent_hover: rgb(0x7af5ff).into(),
            accent_subtle: rgb(0x003b44).into(),
            accent_fg: rgb(0x000000).into(),
            success: rgb(0x3ff23f).into(),
            success_bg: rgb(0x002800).into(),
            success_border: rgb(0x3ff23f).into(),
            warning: rgb(0xffff00).into(),
            warning_bg: rgb(0x332b00).into(),
            warning_border: rgb(0xffff00).into(),
            danger: rgb(0xff5f5f).into(),
            danger_bg: rgb(0x380000).into(),
            danger_border: rgb(0xff5f5f).into(),
            info: rgb(0x1aebff).into(),
            info_bg: rgb(0x002633).into(),
            info_border: rgb(0x1aebff).into(),
            chart_sys: rgb(0xffffff).into(),
            chart_managed: rgb(0x1aebff).into(),
            chart_grid: rgb(0x888888).into(),
            scrim: rgba_hex(0x000000cc),
        }
    }

    /// High-contrast light palette with crisp ink borders and maximum foreground separation.
    pub fn high_contrast_light() -> Self {
        Self {
            bg_app: rgb(0xffffff).into(),
            bg_sidebar: rgb(0xffffff).into(),
            bg_surface: rgb(0xffffff).into(),
            bg_surface_elevated: rgb(0xf3f3f3).into(),
            bg_surface_hover: rgb(0xe5e5e5).into(),
            bg_selected: rgb(0xd0e3ff).into(),
            bg_input: rgb(0xffffff).into(),
            bg_table_header: rgb(0xffffff).into(),
            bg_dialog: rgb(0xffffff).into(),
            nav_marker: rgb(0x00009f).into(),
            bg_badge: rgb(0xededed).into(),
            border_subtle: rgb(0x000000).into(),
            border_strong: rgb(0x000000).into(),
            border_focus: rgb(0x00009f).into(),
            text_primary: rgb(0x000000).into(),
            text_secondary: rgb(0x000000).into(),
            text_muted: rgb(0x262626).into(),
            accent: rgb(0x003399).into(),
            accent_hover: rgb(0x002266).into(),
            accent_subtle: rgb(0xd8e6ff).into(),
            accent_fg: rgb(0xffffff).into(),
            success: rgb(0x005a00).into(),
            success_bg: rgb(0xe6ffe6).into(),
            success_border: rgb(0x005a00).into(),
            warning: rgb(0x7a3e00).into(),
            warning_bg: rgb(0xfff4d6).into(),
            warning_border: rgb(0x7a3e00).into(),
            danger: rgb(0x9b0000).into(),
            danger_bg: rgb(0xffe5e5).into(),
            danger_border: rgb(0x9b0000).into(),
            info: rgb(0x003399).into(),
            info_bg: rgb(0xd8e6ff).into(),
            info_border: rgb(0x003399).into(),
            chart_sys: rgb(0x333333).into(),
            chart_managed: rgb(0x003399).into(),
            chart_grid: rgb(0x666666).into(),
            scrim: rgba_hex(0x00000099),
        }
    }

    /// Returns `(background, border, foreground)` pill colors for an account lifecycle or bot status.
    pub fn status_colors(&self, status: &str) -> (Hsla, Hsla, Hsla) {
        match status {
            "running" | "connected" => (self.success_bg, self.success_border, self.success),
            "reconnecting" | "launching" | "queued" | "stopping" | "backoff" | "connecting" => {
                (self.warning_bg, self.warning_border, self.warning)
            }
            "needs_attention" | "error" => (self.danger_bg, self.danger_border, self.danger),
            _ => (self.bg_badge, self.border_subtle, self.text_secondary),
        }
    }
}

/// Spacing and typography for the desktop layout.
#[derive(Debug, Clone, Copy)]
pub struct DensityTokens {
    pub sidebar_width: f32,
    pub card_pad: f32,
    pub row_px: f32,
    pub row_py: f32,
    pub section_gap: f32,
    pub item_gap: f32,
    pub control_h: f32,
    pub font_title: f32,
    pub font_section: f32,
    pub font_body: f32,
    pub font_caption: f32,
}

impl Default for DensityTokens {
    fn default() -> Self {
        Self {
            sidebar_width: 244.0,
            card_pad: 20.0,
            row_px: 12.0,
            row_py: 12.0,
            section_gap: 16.0,
            item_gap: 12.0,
            control_h: 40.0,
            font_title: 24.0,
            font_section: 18.0,
            font_body: 14.0,
            font_caption: 12.0,
        }
    }
}

fn rgba_hex(hex: u32) -> Hsla {
    gpui::rgba(hex).into()
}
