//! Local, bounded decision journal. Only explicitly selected fields may enter this log.
use crate::model::{Account, Status};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};
use uuid::Uuid;

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const FILE_COUNT: usize = 5;

pub(crate) struct Journal {
    directory: PathBuf,
    run_id: Uuid,
    started: Instant,
    writer: Mutex<Writer>,
}
#[derive(Default)]
struct Writer {
    file: Option<File>,
    bytes: u64,
    sequence: u64,
    write_failures: u64,
    last_error: Option<io::ErrorKind>,
}
impl Journal {
    pub(crate) fn new(data: &Path) -> Self {
        let log = Self {
            directory: data.join("logs"),
            run_id: Uuid::new_v4(),
            started: Instant::now(),
            writer: Mutex::new(Writer::default()),
        };
        log.record("session_started", json!({}));
        log
    }
    fn path(&self, index: usize) -> PathBuf {
        self.directory.join(if index == 0 {
            "recovery.jsonl".into()
        } else {
            format!("recovery.{index}.jsonl")
        })
    }
    // Callers must never pass raw errors, HTTP bodies, command lines, URLs or Account/Database.
    pub(crate) fn record(&self, event: &'static str, details: Value) {
        let mut writer = self.writer.lock().unwrap();
        writer.sequence += 1;
        let mut line = serde_json::to_vec(&json!({
            "schema": 1, "timestamp": chrono::Utc::now(),
            "elapsed_ms": self.started.elapsed().as_millis(), "run_id": self.run_id,
            "sequence": writer.sequence, "launcher_pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"), "event": event, "details": details,
        }))
        .expect("journal fields are JSON values");
        line.push(b'\n');
        if let Err(error) = self.write(&mut writer, &line, MAX_BYTES) {
            writer.file = None;
            writer.write_failures += 1;
            writer.last_error = Some(error.kind());
        }
    }
    fn write(&self, writer: &mut Writer, line: &[u8], limit: u64) -> io::Result<()> {
        if line.len() as u64 > limit {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        if writer.file.is_none() {
            #[cfg(target_os = "linux")]
            crate::platform::private_directory(&self.directory).map_err(io::Error::other)?;
            std::fs::create_dir_all(&self.directory)?;
            let mut options = OpenOptions::new();
            options.create(true).append(true).read(true);
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
            }
            let mut file = options.open(self.path(0))?;
            writer.bytes = file.metadata()?.len();
            if writer.bytes > 0 {
                file.seek(SeekFrom::End(-1))?;
                let mut last = [0];
                file.read_exact(&mut last)?;
                if last[0] != b'\n' {
                    file.write_all(b"\n")?;
                    writer.bytes += 1;
                }
            }
            writer.file = Some(file);
        }
        if writer.bytes + line.len() as u64 > limit && writer.bytes > 0 {
            writer.file = None;
            match std::fs::remove_file(self.path(FILE_COUNT - 1)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            for index in (0..FILE_COUNT - 1).rev() {
                match std::fs::rename(self.path(index), self.path(index + 1)) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            writer.bytes = 0;
            return self.write(writer, line, limit);
        }
        // Unbuffered writes make each completed record immediately readable by support tools.
        writer.file.as_mut().unwrap().write_all(line)?;
        writer.bytes += line.len() as u64;
        Ok(())
    }
    pub(crate) fn health(&self) -> Value {
        let writer = self.writer.lock().unwrap();
        json!({"directory": self.directory, "active_file": "recovery.jsonl",
            "run_id": self.run_id, "recording": writer.file.is_some(),
            "write_failures": writer.write_failures,
            "last_io_error": writer.last_error.map(|e| format!("{e:?}")),
            "max_file_bytes": MAX_BYTES, "retained_files": FILE_COUNT})
    }
    pub(crate) fn recent(&self) -> Vec<Value> {
        // Export is bounded independently of retention; a partial crash record is skipped.
        let _writer = self.writer.lock().unwrap();
        let mut rows = std::collections::VecDeque::new();
        let mut bytes = 0;
        for index in (0..FILE_COUNT).rev() {
            let Ok(file) = File::open(self.path(index)) else {
                continue;
            };
            for line in BufReader::new(file.take(MAX_BYTES))
                .lines()
                .map_while(Result::ok)
            {
                if let Ok(row) = serde_json::from_str::<Value>(&line) {
                    bytes += line.len();
                    rows.push_back((line.len(), row));
                    while rows.len() > 500 || bytes > 512 * 1024 {
                        bytes -= rows.pop_front().unwrap().0;
                    }
                }
            }
        }
        rows.into_iter().map(|(_, row)| row).collect()
    }
}

/// Never serialize Account directly: it contains aliases and private destination links.
pub(crate) fn account(a: &Account) -> Value {
    json!({"account_id": a.id.parse::<u64>().ok(), "generation": a.generation,
        "operation_id": a.operation_id, "status": a.status,
        "desired_running": a.desired_running, "auto_recovery": a.auto_recovery,
        "tracker": a.tracker.as_ref().and_then(|s| s.parse::<u64>().ok()),
        "process": a.process.as_ref().map(process), "ownership_deadline": a.ownership_deadline,
        "failures": a.failures, "next_retry": a.next_retry,
        "connected_since": a.connected_since, "disconnected_since": a.disconnected_since,
        "recovery_evidence": format!("{:?}", a.recovery_evidence),
        "recovery_reason": match a.recovery_reason.as_str() {
            "Network" | "RateLimit" | "Auth" | "Permission" | "TargetUnavailable" |
            "Unsupported" | "Other" | "SessionLost" | "ConnectionFailed" |
            "ConfirmedTerminalDisconnect" | "NormalExit" => a.recovery_reason.as_str(),
            _ => "unspecified",
        },
        "reason": reason(a.last_error.as_deref().unwrap_or("")),
        "error_code": error_code(a.last_error.as_deref().unwrap_or("")),
        "retry_limit_reached": a.failures >= 5,
        "rejoin_disabled": a.status == Status::NeedsAttention && !a.auto_recovery,
        "close_intent": a.close_intent, "fallback_policy": a.fallback_policy,
        "public_fallback_active": a.public_fallback_active,
        "place_id": a.target.as_ref().map(|t| t.place_id),
        "destination_kind": a.target.as_ref().map(|t| if t.private_server_link.is_some() {
            "private"
        } else if t.job_id.is_some() { "specific_server" } else { "public" })})
}

pub(crate) fn process(p: &crate::model::ProcessIdentity) -> Value {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hash = DefaultHasher::new();
    p.creation_time.hash(&mut hash);
    json!({"pid": p.pid, "creation_fingerprint": format!("{:016x}", hash.finish()),
        "generation": p.generation, "tracker": p.tracker.parse::<u64>().ok()})
}

pub(crate) fn error_code(message: &str) -> Option<u32> {
    if message.starts_with("Connection failed:") {
        return Some(279);
    }
    ["Windows error ", "Session lost (code ", "HTTP "]
        .iter()
        .find_map(|prefix| {
            message.split_once(prefix).and_then(|(_, rest)| {
                rest.split(|c: char| !c.is_ascii_digit())
                    .next()?
                    .parse()
                    .ok()
            })
        })
}

/// Stable allowlisted reasons, rather than redaction of arbitrary secret-bearing text.
pub(crate) fn reason(message: &str) -> &'static str {
    for (prefix, code) in [
        (
            "Managed client close blocked: verified process close failed",
            "close_effect_failed",
        ),
        (
            "Managed client close blocked: replacement readiness changed",
            "close_readiness_changed",
        ),
        (
            "Managed client close blocked:",
            "close_policy_or_ownership_denied",
        ),
        (
            "Client preserved: no explicit close intent",
            "close_intent_missing",
        ),
        ("Late client preserved:", "retired_close_unverified"),
        ("Late client close failed", "retired_close_effect_failed"),
        (
            "Launch tracker has no verified instance",
            "ownership_timeout",
        ),
        ("Multiple primary instances", "ambiguous_launch_ownership"),
        ("Automatic rejoin disabled", "rejoin_disabled"),
        (
            "Destination unavailable; fallback policy",
            "fallback_requires_attention",
        ),
        ("Game denied access", "game_access_denied"),
        ("Roblox is out of date", "client_update_required"),
        (
            "Roblox rejected client behavior",
            "client_behavior_rejected",
        ),
        ("Session lost", "session_lost"),
        ("Connection failed:", "connection_failed"),
        ("Game connection lost", "connection_lost"),
        ("Unverified RAM disconnect", "unverified_memory_signal"),
        ("Unconfirmed connection event", "unconfirmed_disk_signal"),
        ("Log unavailable", "log_unavailable"),
        ("Process metadata inaccessible", "process_inaccessible"),
        ("PID verified", "waiting_for_connection"),
        (
            "Managed client exit confirmed",
            "exit_confirmed_recovery_resumed",
        ),
        ("Managed client exited", "process_exited"),
        ("Client closed normally", "normal_exit"),
        ("Multiple instances disabled", "single_instance_wait"),
        ("Waiting for a same-preset account", "peer_join_pending"),
        (
            "Roblox launch coordination is not ready",
            "launch_coordination_unavailable",
        ),
        (
            "Multi-instance initialization failed",
            "launch_coordination_unavailable",
        ),
        ("Session cannot be decrypted", "session_decryption_failed"),
        ("Session expired", "session_expired"),
        ("Roblox could not be reached", "network_unreachable"),
        (
            "Roblox requires authentication or permission",
            "authentication_or_permission_required",
        ),
        ("Roblox rate limit", "rate_limited"),
        ("Account validation:", "account_validation_failed"),
        ("Authentication ticket:", "authentication_ticket_failed"),
        ("Game lookup:", "game_lookup_failed"),
        (
            "Private share-link resolution:",
            "private_link_resolution_failed",
        ),
        ("Private-server page:", "private_server_page_failed"),
        (
            "Roblox launch protocol unavailable",
            "protocol_handler_unavailable",
        ),
        ("Roblox protocol handler", "protocol_handler_failed"),
        ("Unable to launch Roblox", "protocol_handler_failed"),
        ("Roblox identity response", "identity_response_unrecognized"),
        (
            "Roblox private-server page format",
            "private_destination_unrecognized",
        ),
        ("Server search exceeded", "server_search_timeout"),
        (
            "Previous launch is unresolved",
            "previous_launch_unresolved",
        ),
        ("A cancelled launch is still", "retired_launch_pending"),
        ("A launch is already pending", "launch_already_pending"),
        ("Connectivity is suspended", "network_suspended"),
        ("The rate-limit cooldown", "rate_limit_cooldown"),
    ] {
        if message.starts_with(prefix) {
            return code;
        }
    }
    if message.is_empty() {
        "none"
    } else {
        "details_omitted"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_rotates_bounds_storage_and_keeps_order_across_restarts() {
        let path = std::env::temp_dir().join(format!("journal-{}", Uuid::new_v4()));
        let log = Journal::new(&path);
        for index in 0..12 {
            let mut writer = log.writer.lock().unwrap();
            let line = format!("{{\"index\":{index}}}\n");
            log.write(&mut writer, line.as_bytes(), 16).unwrap();
        }
        assert_eq!(
            std::fs::read_dir(path.join("logs")).unwrap().count(),
            FILE_COUNT
        );
        assert_eq!(
            std::fs::read_to_string(log.path(0)).unwrap(),
            "{\"index\":11}\n"
        );
        drop(log);
        let reopened = Journal::new(&path);
        reopened.record("check", json!({}));
        let rows: Vec<Value> = std::fs::read_to_string(reopened.path(0))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        let last = rows.last().unwrap();
        assert_eq!(last["sequence"], 2);
        assert_eq!(last["run_id"], reopened.run_id.to_string());
        drop(reopened);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn journal_failure_is_visible_and_recovers_without_stopping_the_engine() {
        let path = std::env::temp_dir().join(format!("journal-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("logs"), "blocked").unwrap();
        let log = Journal::new(&path);
        assert_eq!(log.health()["recording"], false);
        assert_eq!(log.health()["write_failures"], 1);
        std::fs::remove_file(path.join("logs")).unwrap();
        log.record("recovered", json!({}));
        assert_eq!(log.health()["recording"], true);
        drop(log);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn account_fields_and_errors_cannot_export_secrets() {
        let mut a = Account::new("1".into(), "SECRET_NAME".into());
        a.last_error = Some("Session lost (code 273); SECRET_COOKIE".into());
        a.recovery_reason = "SECRET_REASON".into();
        a.target = Some(crate::model::Target {
            place_id: 1,
            job_id: None,
            private_server_link: Some("https://roblox.com/?code=SECRET_LINK".into()),
        });
        let value = account(&a);
        assert_eq!(value["error_code"], 273);
        assert_eq!(value["reason"], "session_lost");
        assert!(!value.to_string().contains("SECRET"));
    }
    #[test]
    fn concurrent_records_are_ordered_and_partial_crash_rows_do_not_hide_new_events() {
        let path = std::env::temp_dir().join(format!("journal-{}", Uuid::new_v4()));
        let log = std::sync::Arc::new(Journal::new(&path));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let log = log.clone();
                std::thread::spawn(move || {
                    for _ in 0..25 {
                        log.record("parallel", json!({}));
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let rows = log.recent();
        assert_eq!(rows.len(), 101);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(row["sequence"], index + 1);
        }
        let active = log.path(0);
        drop(log);
        OpenOptions::new()
            .append(true)
            .open(active)
            .unwrap()
            .write_all(b"{partial")
            .unwrap();
        let log = Journal::new(&path);
        assert_eq!(log.recent().len(), 102);
        assert_eq!(log.recent().last().unwrap()["event"], "session_started");
        drop(log);
        std::fs::remove_dir_all(path).unwrap();
    }
}
