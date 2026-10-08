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
fn capture(identity: &ProcessIdentity) -> Option<Vec<u8>> {
    use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
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
        let mut rect: RECT = std::mem::zeroed();
        if GetClientRect(*hwnd, &mut rect) == 0 {
            return None;
        }
        let (width, height) = (rect.right, rect.bottom);
        if width <= 0 || height <= 0 || width > 4096 || height > 4096 {
            return None;
        }
        let dc = GetDC(*hwnd);
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
            ReleaseDC(*hwnd, dc);
            return None;
        }
        let previous = SelectObject(mem, bitmap);
        // PW_CLIENTONLY | PW_RENDERFULLCONTENT. This never copies the desktop
        // or another window even when the client is obscured.
        let mut owner = 0;
        GetWindowThreadProcessId(*hwnd, &mut owner);
        let ok = owner == identity.pid
            && windows_sys::Win32::Storage::Xps::PrintWindow(*hwnd, mem, 3) != 0;
        GetWindowThreadProcessId(*hwnd, &mut owner);
        let ok = ok && owner == identity.pid;
        SelectObject(mem, previous);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
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
        ReleaseDC(*hwnd, dc);
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
