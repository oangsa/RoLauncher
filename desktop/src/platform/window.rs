//! Native Win32 window handle extraction, clamped sizing, icons, and show/hide helpers.

use gpui::{App, Bounds, Pixels, WindowBounds, px, size};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::path::PathBuf;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ICON_BIG, ICON_SMALL, IMAGE_ICON, IsIconic, LR_DEFAULTSIZE, LR_LOADFROMFILE, LoadImageW,
    SW_HIDE, SW_RESTORE, SW_SHOW, SendMessageW, SetForegroundWindow, ShowWindow, WM_CLOSE,
    WM_SETICON,
};

use crate::platform::process::to_wide_null;

/// Extracts the Win32 `HWND` backing a GPUI `Window`.
pub fn hwnd_from_window(window: &gpui::Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(win32) => Some(win32.hwnd.get() as HWND),
        _ => None,
    }
}

/// Computes initial centered window bounds clamped cleanly inside the active display bounds.
pub fn initial_window_bounds(cx: &mut App) -> (WindowBounds, gpui::Size<Pixels>) {
    let (avail_w, avail_h) = cx
        .primary_display()
        .map(|d| {
            (
                f32::from(d.bounds().size.width),
                f32::from(d.bounds().size.height),
            )
        })
        .unwrap_or((1920.0, 1080.0));

    let width = 1180.0_f32.min((avail_w - 48.0).max(760.0));
    let height = 940.0_f32.min((avail_h - 48.0).max(600.0));

    let min_w = 1020.0_f32.min((avail_w - 48.0).max(720.0));
    let min_h = 740.0_f32.min((avail_h - 48.0).max(560.0));

    let bounds = WindowBounds::Windowed(Bounds::centered(None, size(px(width), px(height)), cx));
    (bounds, size(px(min_w), px(min_h)))
}

/// Resolves `RoLauncher.ico` beside the current executable or development workspace paths.
pub fn find_app_icon_path() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidates = [
            dir.join("RoLauncher.ico"),
            dir.join("assets\\RoLauncher.ico"),
            dir.join("..\\..\\assets\\RoLauncher.ico"),
            dir.join("..\\..\\..\\assets\\RoLauncher.ico"),
        ];
        for c in candidates {
            if c.is_file() {
                return Some(c);
            }
        }
    }
    let cwd_candidates = [
        PathBuf::from("assets\\RoLauncher.ico"),
        PathBuf::from("RoLauncher.ico"),
    ];
    cwd_candidates.into_iter().find(|p| p.is_file())
}

/// Applies `RoLauncher.ico` (file or embedded `.exe` resource ID 1) as the Win32 window icon.
pub fn apply_window_icon(hwnd: HWND) {
    let file_path = find_app_icon_path();
    let (hinst, name_ptr, flags, _wide_buf): (
        *mut core::ffi::c_void,
        *const u16,
        u32,
        Option<Vec<u16>>,
    ) = if let Some(path) = file_path {
        let wide = to_wide_null(&path.to_string_lossy());
        let ptr = wide.as_ptr();
        (std::ptr::null_mut(), ptr, LR_LOADFROMFILE, Some(wide))
    } else {
        // Embedded icon resource #1 placed by winresource::WindowsResource::set_icon.
        (std::ptr::null_mut(), std::ptr::dangling::<u16>(), 0, None)
    };
    unsafe {
        let hmodule = if hinst.is_null() && flags == 0 {
            GetModuleHandleW(std::ptr::null())
        } else {
            hinst
        };
        let small = LoadImageW(hmodule, name_ptr, IMAGE_ICON, 16, 16, flags);
        if !small.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
        }
        let big = LoadImageW(
            hmodule,
            name_ptr,
            IMAGE_ICON,
            32,
            32,
            flags | LR_DEFAULTSIZE,
        );
        if !big.is_null() {
            SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, big as isize);
        }
    }
}

/// Restores (if minimized) and activates the window from the system tray.
pub fn show_and_activate(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        SetForegroundWindow(hwnd);
    }
}

/// Hides the window into the system tray without terminating the supervisor.
pub fn hide_to_tray(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    unsafe {
        ShowWindow(hwnd, SW_HIDE);
    }
}

/// Requests GPUI / Win32 window closure by posting `WM_CLOSE` to `hwnd`.
pub fn post_close(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(hwnd, WM_CLOSE, 0, 0);
    }
}
