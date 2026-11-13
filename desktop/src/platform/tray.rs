//! Win32 system tray icon, double-click restore, and right-click context menu.

use crate::platform::process::to_wide_null;
use crate::platform::window::{find_app_icon_path, post_close, show_and_activate};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
    RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, HICON, IDI_APPLICATION,
    IMAGE_ICON, LR_LOADFROMFILE, LoadIconW, LoadImageW, MF_STRING, RegisterWindowMessageW,
    SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TrackPopupMenu, WM_LBUTTONDBLCLK,
    WM_NCDESTROY, WM_RBUTTONUP, WM_USER,
};

const TRAY_CALLBACK_MSG: u32 = WM_USER + 4;
const TRAY_SUBCLASS_ID: usize = 0x524F_5452;
const CMD_OPEN: i32 = 1;
const CMD_EXIT: i32 = 2;

struct TrayInner {
    hwnd: HWND,
    nid: NOTIFYICONDATAW,
    owns_icon: bool,
    exiting: Arc<AtomicBool>,
    taskbar_created_msg: u32,
    cleaned_up: AtomicBool,
}

/// Lifetime guard managing the Win32 `NOTIFYICONDATAW` tray entry and window subclass.
pub struct NativeTray {
    inner_ptr: *mut TrayInner,
}

impl NativeTray {
    /// Installs the RoLauncher system tray icon and subclasses `hwnd` for tray messages.
    pub fn install(hwnd: HWND, exiting: Arc<AtomicBool>) -> Option<Self> {
        if hwnd.is_null() {
            return None;
        }

        let (icon, owns_icon) = load_tray_icon();
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK_MSG,
            hIcon: icon,
            ..Default::default()
        };
        let tip = to_wide_null("RoLauncher");
        let len = tip.len().min(nid.szTip.len());
        nid.szTip[..len].copy_from_slice(&tip[..len]);

        let taskbar_msg_name = to_wide_null("TaskbarCreated");
        let taskbar_created_msg = unsafe { RegisterWindowMessageW(taskbar_msg_name.as_ptr()) };

        let inner = Box::new(TrayInner {
            hwnd,
            nid,
            owns_icon,
            exiting,
            taskbar_created_msg,
            cleaned_up: AtomicBool::new(false),
        });
        let raw_ptr = Box::into_raw(inner);

        let subclass_ok = unsafe {
            SetWindowSubclass(
                hwnd,
                Some(tray_subclass_proc),
                TRAY_SUBCLASS_ID,
                raw_ptr as usize,
            )
        };
        if subclass_ok == 0 {
            let reclaimed = unsafe { Box::from_raw(raw_ptr) };
            if reclaimed.owns_icon && !reclaimed.nid.hIcon.is_null() {
                unsafe { DestroyIcon(reclaimed.nid.hIcon) };
            }
            return None;
        }

        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &(*raw_ptr).nid) };
        if added == 0 {
            unsafe {
                RemoveWindowSubclass(hwnd, Some(tray_subclass_proc), TRAY_SUBCLASS_ID);
                let reclaimed = Box::from_raw(raw_ptr);
                if reclaimed.owns_icon && !reclaimed.nid.hIcon.is_null() {
                    DestroyIcon(reclaimed.nid.hIcon);
                }
            }
            return None;
        }

        Some(Self { inner_ptr: raw_ptr })
    }

    /// Removes the tray icon and detaches the subclass safely on application exit.
    pub fn remove(&mut self) {
        if self.inner_ptr.is_null() {
            return;
        }
        unsafe {
            let inner = &*self.inner_ptr;
            if !inner.cleaned_up.swap(true, Ordering::AcqRel) {
                Shell_NotifyIconW(NIM_DELETE, &inner.nid);
                RemoveWindowSubclass(inner.hwnd, Some(tray_subclass_proc), TRAY_SUBCLASS_ID);
                if inner.owns_icon && !inner.nid.hIcon.is_null() {
                    DestroyIcon(inner.nid.hIcon);
                }
            }
            let _ = Box::from_raw(self.inner_ptr);
        }
        self.inner_ptr = null_mut();
    }
}

impl Drop for NativeTray {
    fn drop(&mut self) {
        self.remove();
    }
}

fn load_tray_icon() -> (HICON, bool) {
    if let Some(path) = find_app_icon_path() {
        let wide = to_wide_null(&path.to_string_lossy());
        let loaded = unsafe {
            LoadImageW(
                null_mut(),
                wide.as_ptr(),
                IMAGE_ICON,
                16,
                16,
                LR_LOADFROMFILE,
            )
        };
        if !loaded.is_null() {
            return (loaded as HICON, true);
        }
    }
    let embedded = unsafe {
        let hmod = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(null());
        LoadImageW(hmod, std::ptr::dangling::<u16>(), IMAGE_ICON, 16, 16, 0)
    };
    if !embedded.is_null() {
        return (embedded as HICON, true);
    }
    let fallback = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
    (fallback, false)
}

unsafe fn show_tray_context_menu(inner: &TrayInner) {
    let menu = unsafe { CreatePopupMenu() };
    if menu.is_null() {
        return;
    }

    let open_label = to_wide_null("Open RoLauncher");
    let exit_label = to_wide_null("Exit (leave clients running)");
    unsafe {
        AppendMenuW(menu, MF_STRING, CMD_OPEN as usize, open_label.as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_EXIT as usize, exit_label.as_ptr());
        SetForegroundWindow(inner.hwnd);
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            inner.hwnd,
            null(),
        );
        DestroyMenu(menu);

        if chosen == CMD_OPEN {
            show_and_activate(inner.hwnd);
        } else if chosen == CMD_EXIT {
            inner.exiting.store(true, Ordering::Release);
            post_close(inner.hwnd);
        }
    }
}

unsafe extern "system" fn tray_subclass_proc(
    hwnd: HWND,
    umsg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uidsubclass: usize,
    dwrefdata: usize,
) -> LRESULT {
    let inner_ptr = dwrefdata as *mut TrayInner;
    if !inner_ptr.is_null() {
        let inner = unsafe { &*inner_ptr };
        if umsg == TRAY_CALLBACK_MSG {
            let mouse_msg = (lparam & 0xffff) as u32;
            if mouse_msg == WM_LBUTTONDBLCLK {
                show_and_activate(hwnd);
                return 0;
            }
            if mouse_msg == WM_RBUTTONUP {
                unsafe { show_tray_context_menu(inner) };
                return 0;
            }
        } else if inner.taskbar_created_msg != 0 && umsg == inner.taskbar_created_msg {
            if !inner.cleaned_up.load(Ordering::Acquire) {
                unsafe { Shell_NotifyIconW(NIM_ADD, &inner.nid) };
            }
        } else if umsg == WM_NCDESTROY && !inner.cleaned_up.swap(true, Ordering::AcqRel) {
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &inner.nid);
                RemoveWindowSubclass(hwnd, Some(tray_subclass_proc), TRAY_SUBCLASS_ID);
            }
        }
    }

    unsafe { DefSubclassProc(hwnd, umsg, wparam, lparam) }
}
