#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    if let Err(error) = run() {
        rolauncher::ui::message(&error);
    }
}
#[cfg(windows)]
fn run() -> Result<(), String> {
    use rolauncher::{engine::Engine, platform, store::Store};
    let mut args = std::env::args().skip(1);
    let mut port = 38471;
    let mut headless = false;
    let local_app_data = std::path::PathBuf::from(
        std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable")?,
    );
    let mut directory = rolauncher::store::default_directory(&local_app_data);
    let mut diagnostic = false;
    let mut restore_backup = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--headless" => headless = true,
            "--data-dir" => directory = args.next().ok_or("--data-dir requires a folder")?.into(),
            "--port" => {
                port = args
                    .next()
                    .ok_or("--port requires a number")?
                    .parse()
                    .map_err(|_| "Invalid port")?
            }
            "--diagnose" => diagnostic = true,
            "--restore-backup" => {
                restore_backup = Some(
                    args.next()
                        .ok_or("--restore-backup requires a backup filename")?,
                )
            }
            _ => {
                return Err(
                    "Arguments: --headless --data-dir FOLDER --port NUMBER --diagnose --restore-backup NAME".into(),
                );
            }
        }
    }
    if diagnostic {
        let players = platform::players();
        let report = serde_json::json!({"platform":"Windows","memory_access":"none","metadata_available":players.is_ok(),"identified_player_count":players.as_ref().map(|p|p.len()).unwrap_or(0),"live_account_tests":"not performed"});
        std::fs::create_dir_all(&directory).map_err(|_| "Cannot create diagnostics directory")?;
        std::fs::write(
            directory.join("compatibility.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .map_err(|_| "Cannot save diagnostics")?;
        return Ok(());
    }
    let _single = platform::InstanceGuard::acquire()?;
    if let Some(name) = restore_backup {
        if !platform::players()?.is_empty() {
            return Err("Close Roblox clients before offline database recovery".into());
        }
        Store::new(directory)?.recover(&name)?;
        rolauncher::ui::message(
            "Backup restored. Accounts are stopped; start RoLauncher again. The API token was rotated.",
        );
        return Ok(());
    }
    let multi = platform::MultiInstanceGuard::acquire();
    let compatibility = if multi.is_ok() {
        "OS multi-instance mutex acquired; Roblox launch, mapping and log compatibility require live validation".into()
    } else {
        multi.as_ref().err().unwrap().clone()
    };
    let engine = Engine::open(Store::new(directory)?, compatibility)?;
    engine.set_launch_allowed(multi.is_ok());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|_| "Unable to start worker runtime")?;
    let _enter = runtime.enter();
    // Reserve the loopback port before passing credentials to the desktop shell.
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind((
            std::net::Ipv4Addr::LOCALHOST,
            port,
        )))
        .map_err(|_| "Local API port is unavailable")?;
    engine.spawn();
    let api_engine = engine.clone();
    runtime.spawn(async move {
        if let Err(e) = rolauncher::api::serve_on(api_engine.clone(), listener).await {
            api_engine.shutdown();
            rolauncher::ui::message(&e);
        }
    });
    let ui_result = if headless {
        runtime.block_on(async {
            let _ = tokio::signal::ctrl_c().await;
        });
        Ok(())
    } else {
        rolauncher::ui::run(engine.clone(), runtime.handle().clone(), port)
    };
    engine.shutdown();
    rolauncher::login::wait_closed();
    runtime.shutdown_timeout(std::time::Duration::from_secs(3));
    drop(multi);
    ui_result
}
#[cfg(not(windows))]
fn main() {
    eprintln!("RoLauncher requires Windows 10/11 x64");
}
