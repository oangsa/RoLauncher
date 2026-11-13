//! External installer handoff compatible with `scripts/apply-update.ps1`.

use crate::bootstrap::Bootstrap;
use crate::updater::downloader::{
    CachedInstaller, MAX_CHECKSUMS_BYTES, verify_existing_installer_file,
};
use serde::{Deserialize, Serialize};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const EPOCH_TO_NET_TICKS: u64 = 504_911_232_000_000_000;
const APPLY_UPDATE_SCRIPT: &[u8] = include_bytes!("../../../scripts/apply-update.ps1");

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct HandoffRequest<'a> {
    installer: String,
    sha256: &'a str,
    version: &'a str,
    install_directory: String,
    data_directory: &'a str,
    port: u16,
    parent_id: u32,
    parent_started: u64,
    desktop_id: u32,
    desktop_started: u64,
}

/// Recorded result written by `apply-update.ps1` after updater execution.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateHandoffResult {
    /// `"installed"` or `"failed"`.
    pub status: String,
    /// Stage identifier if failed.
    #[serde(default)]
    pub stage: String,
    /// Target version attempted.
    #[serde(default)]
    pub version: String,
    /// Sanitized outcome message from `apply-update.ps1`.
    #[serde(default)]
    pub message: String,
}

/// Consumes any previous `handoff-*/result.json` left by the updater script and prunes old handoff directories.
pub fn consume_recent_handoff_results(cache_dir: &Path) -> Option<UpdateHandoffResult> {
    let entries = std::fs::read_dir(cache_dir).ok()?;
    let mut latest: Option<(std::time::SystemTime, UpdateHandoffResult)> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("handoff-") || !path.is_dir() {
            continue;
        }
        let result_path = path.join("result.json");
        if result_path.is_file() {
            if let Ok(bytes) = std::fs::read(&result_path)
                && bytes.len() <= MAX_CHECKSUMS_BYTES
            {
                let text = String::from_utf8_lossy(&bytes);
                let trimmed = text.trim_start_matches('\u{feff}');
                if let Ok(parsed) = serde_json::from_str::<UpdateHandoffResult>(trimmed) {
                    let mtime = result_path
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    if latest.as_ref().is_none_or(|(prev, _)| mtime > *prev) {
                        latest = Some((mtime, parsed));
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&path);
        } else if let Ok(meta) = path.metadata()
            && let Ok(modified) = meta.modified()
            && modified.elapsed().unwrap_or_default() > std::time::Duration::from_secs(300)
        {
            let _ = std::fs::remove_dir_all(&path);
        }
    }

    latest.map(|(_, r)| r)
}

/// Returns the `.NET DateTime.UtcNow.Ticks`-compatible start timestamp for an open process handle.
pub unsafe fn process_start_ticks(handle: HANDLE) -> Result<u64, String> {
    let mut creation = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut kernel = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut user = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let ok = unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    if ok == 0 {
        return Err("Unable to read process creation timestamp".to_string());
    }
    let filetime = ((creation.dwHighDateTime as u64) << 32) | (creation.dwLowDateTime as u64);
    Ok(filetime + EPOCH_TO_NET_TICKS)
}

unsafe fn query_process_image_path(handle: HANDLE) -> Result<PathBuf, String> {
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) };
    if ok == 0 || len == 0 {
        return Err("Unable to query supervisor executable path".to_string());
    }
    let path_str = String::from_utf16_lossy(&buf[..len as usize]);
    Ok(PathBuf::from(path_str))
}

/// Stages `apply-update.ps1` + `request.json`, waits for handoff acknowledgement, and commits the update.
pub async fn start_update_handoff(
    bootstrap: &Bootstrap,
    cached: &CachedInstaller,
    cache_dir: &Path,
) -> Result<(), String> {
    verify_existing_installer_file(&cached.installer_path, &cached.sha256).await?;

    let (parent_path, parent_started, desktop_id, desktop_started) = {
        let parent_handle =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, bootstrap.parent_id) };
        if parent_handle.is_null() {
            return Err(
                "Unable to inspect parent RoLauncher process for update handoff".to_string(),
            );
        }
        let parent_res = unsafe {
            let path = query_process_image_path(parent_handle);
            let started = process_start_ticks(parent_handle);
            CloseHandle(parent_handle);
            (path, started)
        };
        let parent_path = parent_res.0?;
        let parent_started = parent_res.1?;
        let desktop_handle = unsafe { GetCurrentProcess() };
        let desktop_started = unsafe { process_start_ticks(desktop_handle) }?;
        let desktop_id = unsafe { GetCurrentProcessId() };
        (parent_path, parent_started, desktop_id, desktop_started)
    };

    let parent_name = parent_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Unexpected supervisor executable path".to_string())?;
    if !parent_name.eq_ignore_ascii_case("RoLauncher.exe") {
        return Err(
            "Update handoff aborted: unexpected supervisor executable identity".to_string(),
        );
    }
    let install_dir = parent_path
        .parent()
        .ok_or_else(|| "Unable to resolve installation folder".to_string())?;

    let handoff_dir = cache_dir.join(format!("handoff-{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&handoff_dir)
        .await
        .map_err(|err| format!("Unable to create handoff folder: {err}"))?;

    let script_path = handoff_dir.join("apply-update.ps1");
    let manifest_path = handoff_dir.join("request.json");
    let ready_path = handoff_dir.join("ready");
    let commit_path = handoff_dir.join("commit");

    let write_res: Result<(), String> = async {
        tokio::fs::write(&script_path, APPLY_UPDATE_SCRIPT)
            .await
            .map_err(|err| format!("Unable to write updater script: {err}"))?;

        let request = HandoffRequest {
            installer: cached.installer_path.to_string_lossy().into_owned(),
            sha256: &cached.sha256,
            version: &cached.version,
            install_directory: install_dir.to_string_lossy().into_owned(),
            data_directory: &bootstrap.data_directory,
            port: bootstrap.port,
            parent_id: bootstrap.parent_id,
            parent_started,
            desktop_id,
            desktop_started,
        };
        let json = serde_json::to_vec_pretty(&request)
            .map_err(|err| format!("Unable to encode handoff request: {err}"))?;
        tokio::fs::write(&manifest_path, json)
            .await
            .map_err(|err| format!("Unable to write handoff request: {err}"))?;

        let mut child = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script_path)
            .arg("-Manifest")
            .arg(&manifest_path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|err| format!("Unable to launch Windows PowerShell updater: {err}"))?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            if tokio::fs::try_exists(&ready_path).await.unwrap_or(false) {
                break;
            }
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!(
                    "Updater helper exited prematurely before handoff (code {})",
                    status.code().unwrap_or(-1)
                ));
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                return Err("Timed out waiting for updater handoff acknowledgement".to_string());
            }
            tokio::time::sleep(std::time::Duration::from_millis(75)).await;
        }

        tokio::fs::write(&commit_path, b"Apply")
            .await
            .map_err(|err| format!("Unable to confirm updater commit: {err}"))?;
        Ok(())
    }
    .await;

    if let Err(err) = write_res {
        let _ = tokio::fs::remove_dir_all(&handoff_dir).await;
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_start_ticks_matches_expected_offset_and_handoff_prunes() {
        let handle = unsafe { GetCurrentProcess() };
        let ticks = unsafe { process_start_ticks(handle) }.expect("current process ticks readable");
        // Year 2020 in .NET ticks is ~637_134_336_000_000_000.
        assert!(ticks > 637_000_000_000_000_000);

        let dir = std::env::temp_dir().join(format!("rolauncher-handoff-{}", uuid::Uuid::new_v4()));
        let old = dir.join("handoff-1");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(
            old.join("result.json"),
            r#"{"status":"installed","version":"2.0.0"}"#,
        )
        .unwrap();
        let consumed = consume_recent_handoff_results(&dir).expect("should read result.json");
        assert_eq!(consumed.status, "installed");
        assert_eq!(consumed.version, "2.0.0");
        assert!(
            !old.exists(),
            "consumed handoff folder should be cleaned up"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
