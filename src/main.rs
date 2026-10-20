#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(any(windows, target_os = "linux"))]
fn main() {
    if let Err(error) = run() {
        rolauncher::ui::message(&error);
    }
}
#[cfg(any(windows, target_os = "linux"))]
fn run() -> Result<(), String> {
    use rolauncher::{engine::Engine, platform, store::Store};
    let mut args = std::env::args().skip(1);
    let mut port = 38471;
    let mut headless = false;
    #[cfg(windows)]
    let local_app_data = std::path::PathBuf::from(
        std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA unavailable")?,
    );
    #[cfg(windows)]
    let mut directory = rolauncher::store::default_directory(&local_app_data);
    #[cfg(target_os = "linux")]
    let mut directory = platform::data_directory()?;
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
    #[cfg(target_os = "linux")]
    platform::initialize(&directory)?;
    if diagnostic {
        let players = platform::players();
        let logs = players.as_ref().ok().map(|players| {
            let identities: Vec<_> = players
                .iter()
                .map(|player| player.identity(uuid::Uuid::nil()))
                .collect();
            platform::owned_logs(&identities)
        });
        let report = serde_json::json!({"platform":std::env::consts::OS,"memory_access":"none","metadata_available":players.is_ok(),"identified_player_count":players.as_ref().map(|p|p.len()).unwrap_or(0),"log_metadata_available":logs.as_ref().is_some_and(|logs| logs.is_ok()),"verified_log_count":logs.as_ref().and_then(|logs| logs.as_ref().ok()).map(|logs|logs.len()).unwrap_or(0),"live_account_tests":"not performed"});
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
    let store = Store::new(directory)?;
    let saved = store.load()?;
    let multi = if saved.multi_instance_enabled {
        platform::MultiInstanceGuard::acquire().map(Some)
    } else {
        Ok(None)
    };
    // Runs under InstanceGuard, before Engine/API launch workers exist.
    #[cfg(windows)]
    let startup_cleanup = (|| -> Result<usize, String> {
        let conflict = if saved.multi_instance_enabled {
            multi
                .as_ref()
                .ok()
                .and_then(|guard| guard.as_ref())
                .is_some_and(|guard| {
                    !guard
                        .launch_gate()
                        .load(std::sync::atomic::Ordering::Acquire)
                })
        } else {
            platform::startup_launch_conflict()?
        };
        if conflict
            && (!saved.retired_launches.is_empty()
                || saved.accounts.iter().any(|s| s.account.tracker.is_some()))
        {
            return Err("Startup conflict: saved launches still need inspection. No external clients closed; quit conflicting Roblox clients and restart RoLauncher.".into());
        }
        let protected: Vec<_> = saved
            .accounts
            .iter()
            .filter_map(|s| s.account.process.clone())
            .collect();
        platform::close_startup_background_players(
            &protected,
            saved.external_background_cleanup_enabled,
            conflict,
        )
    })();
    #[cfg(windows)]
    let compatibility = {
        let coordination = if !saved.multi_instance_enabled {
            "Multiple instances disabled; one managed client at a time".into()
        } else if let Err(error) = &multi {
            error.clone()
        } else {
            "OS multi-instance coordination initialized; read-only RAM log fallback is best effort"
                .into()
        };
        match startup_cleanup {
            Ok(0) => coordination,
            Ok(count) => format!(
                "{coordination}; authorized startup cleanup closed {count} conflicting background clients"
            ),
            Err(error) => format!("{coordination}; {error}"),
        }
    };
    #[cfg(target_os = "linux")]
    let compatibility = match &multi {
        Ok(Some(_)) => "Linux / Sober: built-in per-account runtime/IPC isolation; concurrent game sessions and UI parity require live validation".into(),
        Ok(None) => "Multiple instances disabled; one managed Sober client at a time".into(),
        Err(error) => error.clone(),
    };
    #[cfg(windows)]
    let launch_gate = multi
        .as_ref()
        .map(|guard| {
            guard
                .as_ref()
                .map(|guard| guard.launch_gate())
                .unwrap_or_else(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)))
        })
        .unwrap_or_else(|_| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)));
    #[cfg(target_os = "linux")]
    let launch_gate = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(multi.is_ok()));
    let engine = Engine::open_with_launch_gate(store, compatibility, launch_gate)?;
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
#[cfg(not(any(windows, target_os = "linux")))]
fn main() {
    eprintln!("RoLauncher requires Windows 10/11 x64 or Linux x64");
}
