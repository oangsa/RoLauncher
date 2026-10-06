//! The desktop is a separate process; the Rust supervisor retains account ownership.
use crate::engine::Engine;
use std::{io::Write, process::Command};

pub fn run(engine: Engine, _runtime: tokio::runtime::Handle, port: u16) -> Result<(), String> {
    let root = std::env::current_exe()
        .map_err(|_| "Cannot locate the application folder")?
        .parent()
        .ok_or("Cannot locate the application folder")?
        .to_path_buf();
    #[cfg(windows)]
    let shell = root.join("desktop").join("RoLauncher.Desktop.exe");
    #[cfg(target_os = "linux")]
    let shell = root.join("desktop").join("RoLauncher.Desktop");
    run_shell(engine, port, &shell)
}
fn run_shell(engine: Engine, port: u16, shell: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    if !shell.is_file() {
        return Err("Desktop files are missing. Extract the entire release package, or run the platform build script.".into());
    }
    let mut command = Command::new(shell);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW: no console during startup.
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "Unable to start the desktop shell")?;
    let bootstrap = zeroize::Zeroizing::new(
        serde_json::json!({
            "port": port, "token": engine.token(), "version": env!("CARGO_PKG_VERSION"),
            "parent_id": std::process::id(), "data_directory": engine.data_directory()
        })
        .to_string(),
    );
    let mut input = child
        .stdin
        .take()
        .ok_or("Cannot initialize desktop connection")?;
    if writeln!(input, "{}", bootstrap.as_str()).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Unable to initialize desktop connection".into());
    }
    drop(input);
    let status = child
        .wait()
        .map_err(|_| "Unable to wait for the desktop shell")?;
    if status.success() {
        Ok(())
    } else {
        Err("The desktop shell closed unexpectedly".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Requires the compiled UI_SMOKE shell; run after scripts/build-desktop.ps1 -Smoke"]
    fn winui_bootstrap_pipe_connects_to_real_rust_api_and_exits() {
        let shell =
            std::env::var_os("ROLAUNCHER_DESKTOP_TEST_EXE").expect("Set the smoke shell path");
        let path = std::env::temp_dir().join(format!("rbx-winui-bridge-{}", uuid::Uuid::new_v4()));
        let engine = Engine::open(
            crate::store::Store::new(path.clone()).unwrap(),
            "UI bridge test".into(),
        )
        .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind((
                std::net::Ipv4Addr::LOCALHOST,
                0,
            )))
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let server_engine = engine.clone();
        let server = runtime.spawn(crate::api::serve_on(server_engine, listener));
        assert!(run_shell(engine.clone(), port, &std::path::PathBuf::from(shell)).is_ok());
        engine.shutdown();
        runtime.block_on(server).unwrap().unwrap();
        drop(engine);
        drop(runtime);
        let result = std::fs::read_to_string(
            std::path::PathBuf::from(std::env::var_os("ROLAUNCHER_UI_SMOKE_DIR").unwrap())
                .join("result.txt"),
        )
        .unwrap();
        assert!(result.starts_with("WinUI bridge passed"), "{result}");
        std::fs::remove_dir_all(path).unwrap();
    }
}
