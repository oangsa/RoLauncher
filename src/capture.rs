//! Best-effort capture of one verified client window. Never capture the desktop.
use crate::model::ProcessIdentity;

pub async fn instance_async(identity: ProcessIdentity) -> Option<Vec<u8>> {
    // PrintWindow can block inside an unresponsive application. A detached,
    // single-flight OS thread cannot hold up Tokio/runtime shutdown.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("instance-capture".into())
        .spawn(move || {
            let _ = sender.send(instance(&identity));
        })
        .ok()?;
    tokio::time::timeout(std::time::Duration::from_secs(4), receiver)
        .await
        .ok()?
        .ok()?
}

pub fn instance(identity: &ProcessIdentity) -> Option<Vec<u8>> {
    static CAPTURE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = CAPTURE.try_lock().ok()?;
    if !crate::platform::alive(identity).ok()? {
        return None;
    }
    let image = capture(identity)?;
    // The process may have exited or its PID been reused during capture.
    if !crate::platform::alive(identity).ok()? || image.len() > 8 * 1024 * 1024 {
        return None;
    }
    Some(image)
}

#[cfg(windows)]
struct PhysicalPixels(windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);

#[cfg(windows)]
impl PhysicalPixels {
    fn enter() -> Option<Self> {
        use windows_sys::Win32::UI::HiDpi::*;
        // The supervisor and its worker threads do not inherit the WinUI
        // process's manifest. Measure and render in the same physical units,
        // including after a window moves to a monitor with different scaling.
        let previous =
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        (!previous.is_null()).then_some(Self(previous))
    }
}

#[cfg(windows)]
impl Drop for PhysicalPixels {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.0) };
    }
}

#[cfg(any(windows, test))]
fn pixel_buffer_len(width: i32, height: i32) -> Option<usize> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let bytes = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    // Bound allocation rather than imposing a monitor-resolution ceiling.
    // Covers 2.8K, 4K, ultrawide and 8K clients; PNG delivery stays capped at 8 MiB.
    (bytes <= 128 * 1024 * 1024).then_some(bytes)
}

#[cfg(windows)]
fn capture(identity: &ProcessIdentity) -> Option<Vec<u8>> {
    use windows_sys::Win32::{Foundation::*, UI::WindowsAndMessaging::*};
    struct Search {
        pid: u32,
        windows: Vec<HWND>,
    }
    unsafe extern "system" fn find(hwnd: HWND, data: LPARAM) -> windows_sys::core::BOOL {
        unsafe {
            let search = &mut *(data as *mut Search);
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == search.pid
                && IsWindowVisible(hwnd) != 0
                && IsIconic(hwnd) == 0
                && GetWindow(hwnd, GW_OWNER).is_null()
            {
                search.windows.push(hwnd);
            }
            1
        }
    }
    let mut search = Search {
        pid: identity.pid,
        windows: Vec::new(),
    };
    unsafe {
        EnumWindows(Some(find), &mut search as *mut Search as isize);
        let [hwnd] = search.windows.as_slice() else {
            return None;
        };
        capture_window(*hwnd, identity.pid)
    }
}

#[cfg(windows)]
fn capture_window(hwnd: windows_sys::Win32::Foundation::HWND, pid: u32) -> Option<Vec<u8>> {
    use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
    let _dpi = PhysicalPixels::enter()?;
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        if GetClientRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let (width, height) = (rect.right, rect.bottom);
        let buffer_len = pixel_buffer_len(width, height)?;
        let dc = GetDC(hwnd);
        if dc.is_null() {
            return None;
        }
        let mem = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, width, height);
        if mem.is_null() || bitmap.is_null() {
            if !mem.is_null() {
                DeleteDC(mem);
            }
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            ReleaseDC(hwnd, dc);
            return None;
        }
        let previous = SelectObject(mem, bitmap);
        // PW_CLIENTONLY | PW_RENDERFULLCONTENT. Never copy the desktop or another window.
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, &mut owner);
        let ok = owner == pid && windows_sys::Win32::Storage::Xps::PrintWindow(hwnd, mem, 3) != 0;
        GetWindowThreadProcessId(hwnd, &mut owner);
        // A resize during PrintWindow can otherwise deliver a cropped frame.
        let mut after: RECT = std::mem::zeroed();
        let ok = ok
            && owner == pid
            && GetClientRect(hwnd, &mut after) != 0
            && after.right == width
            && after.bottom == height;
        SelectObject(mem, previous);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut pixels = vec![0u8; buffer_len];
        let rows = if ok {
            GetDIBits(
                dc,
                bitmap,
                0,
                height as u32,
                pixels.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        DeleteObject(bitmap);
        DeleteDC(mem);
        ReleaseDC(hwnd, dc);
        if rows != height {
            return None;
        }
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        // GPU/protected surfaces can return a black image despite success.
        if pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3] == [0, 0, 0])
        {
            return None;
        }
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, width as u32, height as u32);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .ok()?
                .write_image_data(&pixels)
                .ok()?;
        }
        Some(bytes)
    }
}

#[cfg(target_os = "linux")]
fn capture(identity: &ProcessIdentity) -> Option<Vec<u8>> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    // X11/XWayland requires a host PID reported by XRes, not the ambiguous
    // sandbox-local _NET_WM_PID. Native Wayland windows may refuse capture.
    let mut child = Command::new("python3")
        .args([
            "-c",
            include_str!("../scripts/capture-linux.py"),
            &identity.pid.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    let success = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Err(_) => break false,
            _ => {}
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break false;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let bytes = reader.join().ok()?;
    (success && bytes.starts_with(b"\x89PNG\r\n\x1a\n")).then_some(bytes)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn capture(_: &ProcessIdentity) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_capture_sizes_are_bounded_by_memory() {
        for (width, height) in [
            (1920, 1080),
            (2880, 1800),
            (3840, 2160),
            (5120, 1440),
            (7680, 4320),
        ] {
            assert_eq!(
                pixel_buffer_len(width, height),
                Some(width as usize * height as usize * 4)
            );
        }
        for (width, height) in [(0, 1800), (2880, -1), (i32::MAX, i32::MAX), (16384, 16384)] {
            assert_eq!(pixel_buffer_len(width, height), None);
        }
    }

    #[cfg(windows)]
    #[test]
    fn physical_client_geometry_and_dpi_restoration_from_unaware_thread() {
        use windows_sys::Win32::{
            Foundation::*,
            UI::{HiDpi::*, WindowsAndMessaging::*},
        };
        // Measure only our hidden test window; no desktop or Roblox access.
        std::thread::spawn(|| unsafe {
            let original = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                2880,
                1800,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!hwnd.is_null());
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE);
            for (width, height) in [(2880, 1800), (5120, 1440)] {
                {
                    let _dpi = PhysicalPixels::enter().unwrap();
                    assert_ne!(
                        SetWindowPos(
                            hwnd,
                            std::ptr::null_mut(),
                            0,
                            0,
                            width,
                            height,
                            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
                        ),
                        0
                    );
                    let mut rect: RECT = std::mem::zeroed();
                    assert_ne!(GetClientRect(hwnd, &mut rect), 0);
                    assert_eq!((rect.right, rect.bottom), (width, height));
                    assert_ne!(
                        AreDpiAwarenessContextsEqual(
                            GetThreadDpiAwarenessContext(),
                            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
                        ),
                        0
                    );
                }
                assert_ne!(
                    AreDpiAwarenessContextsEqual(
                        GetThreadDpiAwarenessContext(),
                        DPI_AWARENESS_CONTEXT_UNAWARE
                    ),
                    0
                );
            }
            // Ownership rejection must restore DPI context on early returns too.
            assert!(capture_window(hwnd, pid.wrapping_add(1)).is_none());
            assert_ne!(
                AreDpiAwarenessContextsEqual(
                    GetThreadDpiAwarenessContext(),
                    DPI_AWARENESS_CONTEXT_UNAWARE
                ),
                0
            );
            DestroyWindow(hwnd);
            SetThreadDpiAwarenessContext(original);
        })
        .join()
        .unwrap();
    }
}
