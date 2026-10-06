//! Capture cookies only from our own ephemeral WebKit browser, over anonymous pipes.
use crate::engine::Engine;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use zeroize::Zeroizing;

static LOGIN_OPEN: AtomicBool = AtomicBool::new(false);
struct OpenLogin;
impl Drop for OpenLogin {
    fn drop(&mut self) {
        LOGIN_OPEN.store(false, Ordering::Release);
    }
}

pub fn wait_closed() {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while LOGIN_OPEN.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn run_for(
    engine: Engine,
    runtime: tokio::runtime::Handle,
    expected: Option<String>,
) -> Result<(), String> {
    if LOGIN_OPEN.swap(true, Ordering::AcqRel) {
        return Err("Browser sign-in is already open".into());
    }
    let _open = OpenLogin;
    let mut child = Command::new("python3")
        .args(["-c", include_str!("../scripts/login-linux.py")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "Browser sign-in requires Python 3, PyGObject and WebKitGTK 4.1")?;
    let mut input = child.stdin.take().ok_or("Cannot initialize sign-in pipe")?;
    let output = child
        .stdout
        .take()
        .ok_or("Cannot initialize sign-in pipe")?;
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(output);
        loop {
            let mut line = Zeroizing::new(Vec::new());
            // Bound all pipe messages, including malformed helper output.
            let result = std::io::Read::take(&mut reader, 32769).read_until(b'\n', &mut line);
            if !matches!(result, Ok(n) if n > 0 && n <= 32768 && line.last() == Some(&b'\n')) {
                break;
            }
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let result = loop {
        if engine.is_shutdown() {
            break Ok(());
        }
        match receive.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                #[derive(serde::Deserialize)]
                struct Login {
                    cookie: Option<String>,
                    error: Option<String>,
                }
                let Ok(mut login) = serde_json::from_slice::<Login>(&line) else {
                    break Err("Invalid browser sign-in response".into());
                };
                if login.error.take().is_some() {
                    break Err(
                        "Browser sign-in requires Python 3, PyGObject and WebKitGTK 4.1".into(),
                    );
                }
                let Some(cookie) = login.cookie.take() else {
                    break Err("Invalid browser sign-in response".into());
                };
                let cookie = Zeroizing::new(cookie);
                let saved = runtime.block_on(engine.import_for(&cookie, expected.as_deref()));
                let reply = match saved {
                    Ok(_) => serde_json::json!({"saved":true}),
                    Err(error) => {
                        let close = error
                            == "Sign-in account does not match the account being repaired"
                            || error == "Account was removed while sign-in was open";
                        serde_json::json!({"error":error, "close":close})
                    }
                };
                if writeln!(input, "{reply}").is_err() {
                    break Ok(());
                }
                // serde values from the response never contain the session cookie.
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // EOF can arrive a moment before normal helper teardown completes.
                // Allow that exit to finish before reporting an installation error.
                let deadline = std::time::Instant::now() + Duration::from_secs(1);
                while matches!(child.try_wait(), Ok(None)) && std::time::Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(20));
                }
                break match child.try_wait() {
                    Ok(Some(status)) if status.success() => Ok(()),
                    _ => Err(
                        "Browser sign-in closed unexpectedly; check WebKitGTK 4.1 installation"
                            .into(),
                    ),
                };
            }
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    drop(receive);
    let _ = reader.join();
    result
}
