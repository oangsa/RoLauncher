//! Supervisor process exit watcher, startup error dialog, and `.exe` image path lookup.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Threading::{INFINITE, OpenProcess, WaitForSingleObject};
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

const SYNCHRONIZE: u32 = 0x0010_0000;

/// Encodes a Rust string into a NUL-terminated UTF-16 buffer for Win32 `*W` APIs.
pub fn to_wide_null(text: &str) -> Vec<u16> {
    OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Shows a native modal Win32 error box (used only for fatal pre-window bootstrap failures).
pub fn show_fatal_message_box(title: &str, message: &str) {
    let wide_title = to_wide_null(title);
    let wide_msg = to_wide_null(message);
    unsafe {
        MessageBoxW(
            null_mut(),
            wide_msg.as_ptr(),
            wide_title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// Spawns an OS wait thread watching `parent_pid` and invoking `on_exit` when the supervisor exits.
pub fn watch_parent_exit<F>(parent_pid: u32, on_exit: F)
where
    F: FnOnce() + Send + 'static,
{
    if parent_pid == 0 {
        return;
    }
    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, parent_pid) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        on_exit();
        return;
    }

    let handle_usize = handle as usize;
    let _ = std::thread::Builder::new()
        .name("parent-watch".to_string())
        .spawn(move || {
            let raw = handle_usize as windows_sys::Win32::Foundation::HANDLE;
            unsafe {
                WaitForSingleObject(raw, INFINITE);
                CloseHandle(raw);
            }
            on_exit();
        });
}
