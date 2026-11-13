//! Persisted appearance settings plus Windows animation and high-contrast queries.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SPI_GETHIGHCONTRAST, SystemParametersInfoW,
};

/// Theme selection persisted in `appearance.txt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppearancePreference {
    /// Follow the Windows OS color mode.
    #[default]
    System,
    /// Force light palette (unless OS High Contrast overrides it).
    Light,
    /// Force dark palette (unless OS High Contrast overrides it).
    Dark,
}

impl AppearancePreference {
    /// Parses a serialized `appearance.txt` tag.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }

    /// Canonical lowercase storage key matching WinUI `appearance.txt`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

fn appearance_path(data_dir: &Path) -> PathBuf {
    data_dir.join("appearance.txt")
}

/// Loads saved theme preference from `data_dir/appearance.txt`.
pub fn load_appearance(data_dir: &Path) -> AppearancePreference {
    std::fs::read_to_string(appearance_path(data_dir))
        .map(|text| AppearancePreference::parse(&text))
        .unwrap_or(AppearancePreference::System)
}

/// Saves theme preference to `data_dir/appearance.txt`.
pub fn save_appearance(data_dir: &Path, pref: AppearancePreference) -> Result<(), String> {
    std::fs::create_dir_all(data_dir)
        .map_err(|err| format!("Unable to access settings folder: {err}"))?;
    std::fs::write(appearance_path(data_dir), pref.as_str())
        .map_err(|err| format!("Unable to save theme setting: {err}"))
}

/// Queries Windows `SPI_GETCLIENTAREAANIMATION` to honor reduced-motion preferences.
pub fn os_client_animations_enabled() -> bool {
    let mut enabled: i32 = 1;
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast::<c_void>(),
            0,
        )
    };
    ok == 0 || enabled != 0
}

/// Queries Windows `SPI_GETHIGHCONTRAST` to detect system high-contrast accessibility mode.
pub fn os_high_contrast_enabled() -> bool {
    let mut hc = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        dwFlags: 0,
        lpszDefaultScheme: std::ptr::null_mut(),
    };
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            (&mut hc as *mut HIGHCONTRASTW).cast::<c_void>(),
            0,
        )
    };
    ok != 0 && (hc.dwFlags & HCF_HIGHCONTRASTON) != 0
}

/// Returns whether subtle navigation/card transitions should animate.
pub fn animations_enabled() -> bool {
    os_client_animations_enabled() && !os_high_contrast_enabled()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_round_trip() {
        let dir = std::env::temp_dir().join(format!("rolauncher-pref-{}", uuid::Uuid::new_v4()));
        assert_eq!(load_appearance(&dir), AppearancePreference::System);

        save_appearance(&dir, AppearancePreference::Dark).unwrap();
        assert_eq!(load_appearance(&dir), AppearancePreference::Dark);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
