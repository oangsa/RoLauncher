use super::*;
use std::os::windows::{io::AsRawHandle, process::CommandExt};

#[test]
fn consent_and_conflict_are_independent_requirements() {
    assert_eq!(close_startup_background_players(&[], false, false), Ok(0));
    assert_eq!(close_startup_background_players(&[], true, false), Ok(0));
    let message = close_startup_background_players(&[], false, true).unwrap_err();
    assert!(message.contains("Settings") && message.contains("No external clients closed"));
    let name = format!("startup-conflict-{}", Uuid::new_v4());
    assert!(!startup_launch_conflict_named(&name).unwrap());
    let guard = MultiInstanceGuard::acquire_named(&name).unwrap();
    assert!(startup_launch_conflict_named(&name).unwrap());
    drop(guard);
    assert!(!startup_launch_conflict_named(&name).unwrap());
}

#[test]
#[ignore = "child process fixture, invoked only by startup cleanup regression"]
fn window_fixture() {
    let Some(ready) = std::env::var_os("ROLAUNCHER_STARTUP_FIXTURE_READY") else {
        return;
    };
    let visible = std::env::var_os("ROLAUNCHER_STARTUP_FIXTURE_VISIBLE").is_some();
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide("RoLauncher cleanup test").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            100,
            100,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!hwnd.is_null());
    if visible {
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
        }
    }
    std::fs::write(&ready, b"ready").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while unsafe { IsWindow(hwnd) } != 0 && std::time::Instant::now() < deadline {
        let mut message = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn external_cleanup_protects_visible_owned_and_changed_processes() {
    let directory = std::env::temp_dir().join(format!("rbx-consent-{}", Uuid::new_v4()));
    let versions = directory.join("Roblox").join("Versions");
    std::fs::create_dir_all(&versions).unwrap();
    let executable = versions.join("RobloxPlayerBeta.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let spawn = |visible: bool, name: &str| {
        let ready = directory.join(name);
        let mut command = std::process::Command::new(&executable);
        command
            .args([
                "--exact",
                "platform::imp::startup_tests::window_fixture",
                "--ignored",
                "--nocapture",
            ])
            .creation_flags(0x08000000)
            .env("ROLAUNCHER_STARTUP_FIXTURE_READY", &ready)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if visible {
            command.env("ROLAUNCHER_STARTUP_FIXTURE_VISIBLE", "1");
        }
        let mut child = command.spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !ready.exists() {
            assert!(
                child.try_wait().unwrap().is_none(),
                "fixture exited before creating window"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "fixture window not ready"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child
    };
    let mut visible = spawn(true, "visible");
    let mut managed = spawn(false, "managed");
    let mut external = spawn(false, "external");
    let identity = |child: &std::process::Child| {
        Player {
            pid: child.id(),
            creation_time: process_time(child.as_raw_handle() as _).unwrap(),
            tracker: String::new(),
        }
        .identity(Uuid::nil())
    };
    let discovered = all_players().unwrap();
    for child in [&visible, &managed, &external] {
        assert!(discovered.iter().any(|p| p.pid == child.id() && p.tracker.is_empty()),
            "global discovery must include visible and hidden untracked clients");
    }
    let managed_id = identity(&managed);
    let external_id = identity(&external);
    let mut session = 0;
    assert_ne!(
        unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session) },
        0
    );
    let root = versions.to_string_lossy().replace('/', "\\").to_lowercase() + "\\";
    let mut stale = external_id.clone();
    stale.creation_time = "0".into();
    assert!(close_startup_candidate(&stale, &root, session, true).is_err());
    assert!(close_startup_candidate(&external_id, "z:\\other\\", session, true).is_err());
    assert!(close_startup_candidate(&external_id, &root, session + 1, true).is_err());
    assert!(external.try_wait().unwrap().is_none());
    assert!(
        close_startup_background_players(std::slice::from_ref(&managed_id), false, true).is_err()
    );
    assert_eq!(
        close_startup_background_players(std::slice::from_ref(&managed_id), true, false),
        Ok(0)
    );
    assert!(external.try_wait().unwrap().is_none());
    assert_eq!(
        close_startup_background_players_at(&[managed_id], &versions).unwrap(),
        1
    );
    assert!(visible.try_wait().unwrap().is_none());
    assert!(managed.try_wait().unwrap().is_none());
    assert!(external.try_wait().unwrap().is_some());
    let mut invalid = external_id;
    invalid.pid = 0;
    assert!(close_startup_candidate(&invalid, &root, session, true).is_err());
    // Explicit close does not depend on a window, tracker, or launcher ownership.
    for child in [&visible, &managed] {
        close(&identity(child), true).unwrap();
    }
    for child in [&mut visible, &mut managed, &mut external] {
        let _ = child.kill();
        child.wait().unwrap();
    }
    std::fs::remove_dir_all(directory).unwrap();
}
