//! Login uses only WebView2's own cookie API. It never touches another browser's data.
use crate::{engine::Engine, platform::wide};
use std::{
    cell::RefCell,
    ptr::{null, null_mut},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};
use webview2_com::{
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler,
    GetCookiesCompletedHandler, Microsoft::Web::WebView2::Win32::*,
};
use windows::{
    Win32::Foundation::{HWND as ComHwnd, RECT as ComRect},
    Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize},
    core::{Interface, PCWSTR, PWSTR},
};
use windows_sys::Win32::{
    Foundation::*, System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*,
};

static LOGIN_OPEN: AtomicBool = AtomicBool::new(false);
pub fn wait_closed() {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while LOGIN_OPEN.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
const SAVED: u32 = WM_APP + 3;
struct LoginData {
    controller: Option<ICoreWebView2Controller>,
    manager: Option<ICoreWebView2CookieManager>,
    busy: bool,
    closing: bool,
    engine: Engine,
    runtime: tokio::runtime::Handle,
    expected_account: Option<String>,
}
type Shared = Rc<RefCell<LoginData>>;
fn com_string(pointer: PWSTR) -> String {
    if pointer.is_null() {
        return String::new();
    }
    let result = unsafe { pointer.to_string() }.unwrap_or_default();
    unsafe {
        CoTaskMemFree(Some(pointer.0 as _));
    }
    result
}
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = unsafe { &*(l as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
        }
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Shared;
    if !pointer.is_null() {
        let shared = unsafe { &*pointer };
        match msg {
            WM_TIMER => {
                let mut data = shared.borrow_mut();
                if data.engine.is_shutdown() {
                    drop(data);
                    unsafe {
                        DestroyWindow(hwnd);
                    }
                    return 0;
                }
                if data.busy || data.closing {
                    return 0;
                }
                let Some(manager) = data.manager.clone() else {
                    return 0;
                };
                data.busy = true;
                drop(data);
                let captured = shared.clone();
                let window = hwnd as usize;
                let callback = GetCookiesCompletedHandler::create(Box::new(
                    move |result, cookies| {
                        let mut data = captured.borrow_mut();
                        data.busy = false;
                        if data.closing {
                            return Ok(());
                        }
                        result?;
                        let Some(cookies) = cookies else {
                            return Ok(());
                        };
                        let mut count = 0;
                        unsafe {
                            cookies.Count(&mut count)?;
                        }
                        for index in 0..count {
                            let cookie = unsafe { cookies.GetValueAtIndex(index)? };
                            let mut name = PWSTR::null();
                            unsafe {
                                cookie.Name(&mut name)?;
                            }
                            if com_string(name) != ".ROBLOSECURITY" {
                                continue;
                            }
                            let mut value = PWSTR::null();
                            unsafe {
                                cookie.Value(&mut value)?;
                            }
                            let session = zeroize::Zeroizing::new(com_string(value));
                            if session.is_empty() {
                                continue;
                            }
                            data.busy = true;
                            let engine = data.engine.clone();
                            let runtime = data.runtime.clone();
                            let expected = data.expected_account.clone();
                            runtime.spawn(async move {
                                match engine.import_for(&session, expected.as_deref()).await {
                                    Ok(_) => unsafe {
                                        PostMessageW(window as HWND, SAVED, 0, 0);
                                    },
                                    Err(e) => {
                                        let close = e == "Sign-in account does not match the account being repaired" || e == "Account was removed while sign-in was open";
                                        crate::ui::message(&e);
                                        unsafe {
                                            PostMessageW(window as HWND, SAVED, if close { 2 } else { 1 }, 0);
                                        }
                                    }
                                }
                            });
                            break;
                        }
                        Ok(())
                    },
                ));
                if unsafe {
                    manager.GetCookies(PCWSTR(wide("https://www.roblox.com").as_ptr()), &callback)
                }
                .is_err()
                {
                    shared.borrow_mut().busy = false;
                }
                return 0;
            }
            WM_SIZE => {
                if let Some(controller) = shared.borrow().controller.clone() {
                    let mut bounds: RECT = unsafe { std::mem::zeroed() };
                    unsafe {
                        GetClientRect(hwnd, &mut bounds);
                        let _ = controller.SetBounds(ComRect {
                            left: bounds.left,
                            top: bounds.top,
                            right: bounds.right,
                            bottom: bounds.bottom,
                        });
                    }
                }
                return 0;
            }
            SAVED => {
                if w == 0 || w == 2 {
                    unsafe {
                        DestroyWindow(hwnd);
                    }
                } else {
                    shared.borrow_mut().busy = false;
                }
                return 0;
            }
            WM_CLOSE => {
                unsafe {
                    DestroyWindow(hwnd);
                }
                return 0;
            }
            WM_DESTROY => {
                let mut data = shared.borrow_mut();
                data.closing = true;
                data.manager = None;
                let controller = data.controller.take();
                drop(data);
                if let Some(controller) = controller {
                    unsafe {
                        let _ = controller.Close();
                    }
                }
                unsafe {
                    KillTimer(hwnd, 1);
                    PostQuitMessage(0);
                }
                return 0;
            }
            _ => {}
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}
pub fn run(engine: Engine, runtime: tokio::runtime::Handle) -> Result<(), String> {
    run_for(engine, runtime, None)
}
pub fn run_for(
    engine: Engine,
    runtime: tokio::runtime::Handle,
    expected_account: Option<String>,
) -> Result<(), String> {
    if LOGIN_OPEN.swap(true, Ordering::AcqRel) {
        return Err("A browser sign-in window is already open".into());
    }
    struct Open;
    impl Drop for Open {
        fn drop(&mut self) {
            LOGIN_OPEN.store(false, Ordering::Release);
        }
    }
    let _open = Open;
    let profile = std::env::temp_dir().join(format!("RoLauncher-login-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&profile).map_err(|_| "Unable to create temporary login profile")?;
    let result = run_window(engine, runtime, &profile, expected_account);
    // The directory is generated here and checked before recursive cleanup.
    let root = std::env::temp_dir();
    if profile.parent() == Some(root.as_path())
        && profile
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("RoLauncher-login-"))
    {
        let mut removed = false;
        for _ in 0..20 {
            if std::fs::remove_dir_all(&profile).is_ok() {
                removed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        if !removed {
            return Err("Login finished, but WebView2 has not released its temporary profile. Close the browser window and remove the RoLauncher-login folder from your Windows temp directory".into());
        }
    }
    result
}
fn run_window(
    engine: Engine,
    runtime: tokio::runtime::Handle,
    profile: &std::path::Path,
    expected_account: Option<String>,
) -> Result<(), String> {
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|_| "Unable to initialize browser COM apartment")?;
    }
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    let _com = Com;
    let shared = Rc::new(RefCell::new(LoginData {
        controller: None,
        manager: None,
        busy: false,
        closing: false,
        engine,
        runtime,
        expected_account,
    }));
    let window = unsafe {
        let class = wide("RoLauncherLogin");
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(procedure);
        wc.hInstance = GetModuleHandleW(null());
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.lpszClassName = class.as_ptr();
        RegisterClassW(&wc);
        CreateWindowExW(
            0,
            class.as_ptr(),
            wide("Sign in to Roblox — account saves automatically").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1024,
            768,
            null_mut(),
            null_mut(),
            wc.hInstance,
            &shared as *const Shared as *const _,
        )
    };
    if window.is_null() {
        return Err("Unable to open browser login window".into());
    }
    let captured = shared.clone();
    let window_id = window as usize;
    let environment_callback = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |result, environment| {
            if captured.borrow().closing {
                return Ok(());
            }
            if result.is_err() || environment.is_none() {
                crate::ui::message(
                    "WebView2 Runtime is unavailable. Install Microsoft Edge WebView2 Runtime or use cookie import",
                );
                unsafe {
                    DestroyWindow(window_id as HWND);
                }
                return Ok(());
            }
            let captured = captured.clone();
            let controller_callback = CreateCoreWebView2ControllerCompletedHandler::create(
                Box::new(move |result, controller| {
                    if captured.borrow().closing {
                        if let Some(c) = controller {
                            unsafe {
                                let _ = c.Close();
                            }
                        }
                        return Ok(());
                    }
                    result?;
                    let Some(controller) = controller else {
                        return Ok(());
                    };
                    unsafe {
                        let webview = controller.CoreWebView2()?;
                        let version: ICoreWebView2_2 = webview.cast()?;
                        let manager = version.CookieManager()?;
                        let mut bounds: RECT = std::mem::zeroed();
                        GetClientRect(window_id as HWND, &mut bounds);
                        controller.SetBounds(ComRect {
                            left: 0,
                            top: 0,
                            right: bounds.right,
                            bottom: bounds.bottom,
                        })?;
                        controller.SetIsVisible(true)?;
                        // Profile is unique to this login. No hooks, script injection or password capture.
                        webview.Navigate(PCWSTR(wide("https://www.roblox.com/login").as_ptr()))?;
                        let mut data = captured.borrow_mut();
                        data.controller = Some(controller);
                        data.manager = Some(manager);
                        drop(data);
                        SetTimer(window_id as HWND, 1, 2000, None);
                    }
                    Ok(())
                }),
            );
            unsafe {
                environment.unwrap().CreateCoreWebView2Controller(
                    ComHwnd(window_id as HWND),
                    &controller_callback,
                )?;
            }
            Ok(())
        },
    ));
    let profile = wide(&profile.to_string_lossy());
    if unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            PCWSTR(profile.as_ptr()),
            None,
            &environment_callback,
        )
    }
    .is_err()
    {
        unsafe {
            DestroyWindow(window);
        }
        return Err(
            "WebView2 could not start; install the Microsoft WebView2 Runtime or import cookies"
                .into(),
        );
    }
    unsafe {
        ShowWindow(window, SW_SHOWNORMAL);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    drop(environment_callback);
    drop(shared);
    Ok(())
}
