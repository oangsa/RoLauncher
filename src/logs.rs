use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Connected,
    Disconnected,
    ConnectionFailed,
    JoinRequestFailed,
    SessionLost(Option<u32>),
    TargetUnavailable,
    PermissionDenied,
}

// Preserve exhaustive matches in existing log consumers: updates use the existing code-bearing signal.
impl Signal {
    #[allow(non_upper_case_globals)]
    pub const ClientUpdateRequired: Self = Self::SessionLost(Some(280));
}

/// Conservative external-log signatures. An attempted join is NOT a connection.
pub fn parse_line(line: &str) -> Option<Signal> {
    // The first log channel is authoritative; script output must not spoof a network tag.
    let tagged = line
        .split_once('[')
        .and_then(|(_, tail)| tail.split_once(']'));
    let channel = tagged.map(|(tag, _)| tag);
    // Most log traffic is game output or assets. Reject it before scanning messages.
    let network = match channel {
        Some(
            "FLog::Network" | "DFLog::Network" | "FLog::NetworkClient" | "DFLog::NetworkClient",
        ) => true,
        Some(
            "FLog::HttpTraceError"
            | "DFLog::HttpTraceError"
            | "FLog::RbxTransportDummyClient"
            | "DFLog::RbxTransportDummyClient"
            | "FLog::Error"
            | "DFLog::Error"
            | "FLog::GameJoinUtil"
            | "DFLog::GameJoinUtil",
        ) => false,
        _ => return None,
    };
    // A join HTTP failure can leave the error dialog open without GameJoinFailed
    // or any network connection. Ignore asset, telemetry and other HTTP traffic.
    if matches!(
        channel,
        Some("FLog::HttpTraceError" | "DFLog::HttpTraceError")
    ) && let Some((metadata, request)) = tagged
        .and_then(|(_, message)| message.trim_start().strip_prefix("HttpResponse("))
        .and_then(|message| message.split_once(" url:{ \""))
        && let Some((uri, _)) = request.split_once('"')
        && let Ok(url) = url::Url::parse(uri)
        && url.scheme() == "https"
        && url.host_str() == Some("gamejoin.roblox.com")
        && matches!(
            url.path(),
            "/v1/join-game" | "/v1/join-game-instance" | "/v1/join-private-game"
        )
    {
        let number = |key| {
            metadata.split_once(key).and_then(|(_, value)| {
                value
                    .trim_start()
                    .split(|c: char| !c.is_ascii_digit())
                    .next()?
                    .parse::<u32>()
                    .ok()
            })
        };
        if (number(" error:").is_some_and(|code| code != 0)
            && metadata.contains(" message:HttpError:"))
            || number("status:").is_some_and(|status| (400..=599).contains(&status))
        {
            return Some(Signal::JoinRequestFailed);
        }
    }
    if network && line.contains("Failed to connect to server") && line.contains("no response") {
        return Some(Signal::ConnectionFailed);
    }
    // Native kick dialogs can omit the numeric 267 disconnect message entirely.
    if network
        && let Some((label, reason)) = tagged.and_then(|(_, message)| message.split_once(':'))
        && label.trim() == "Lost connection with reason"
        && reason.trim() == "You have been kicked from the game"
    {
        return Some(Signal::SessionLost(Some(267)));
    }
    if network && let Some((_, reason)) = line.split_once("Sending disconnect with reason:") {
        let code = reason
            .trim()
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|n| n.parse::<u32>().ok());
        // 0/282 can be intentional shutdown/transfer. Do not turn them into
        // immediate error alerts. Preserve confirmed Roblox failure codes.
        if let Some(code) = code {
            match code {
                279 => return Some(Signal::ConnectionFailed),
                280 => return Some(Signal::ClientUpdateRequired),
                524 | 533 | 600 | 773 => return Some(Signal::PermissionDenied),
                264 | 266 | 267 | 268 | 273 | 277 | 285 => {
                    return Some(Signal::SessionLost(Some(code)));
                }
                _ => {}
            }
        }
    }
    if network
        && (line.contains("Client:Disconnect")
            || line.contains("Sending disconnect with reason:")
            || line.contains("Connection lost")
            || line.contains("ID_CONNECTION_LOST")
            || line.contains("ID_DISCONNECTION_NOTIFICATION"))
    {
        return Some(Signal::Disconnected);
    }
    if matches!(
        channel,
        Some("DFLog::RbxTransportDummyClient" | "FLog::RbxTransportDummyClient")
    ) && line.contains("Connection closed")
    {
        return Some(Signal::Disconnected);
    }
    if network && (line.contains("Connection accepted") || line.contains("Replicator created")) {
        return Some(Signal::Connected);
    }
    if matches!(
        channel,
        Some("FLog::Error" | "FLog::GameJoinUtil" | "DFLog::Error" | "DFLog::GameJoinUtil")
    ) && line.contains("GameJoinFailed")
    {
        let code = line.split_once("errorCode:").and_then(|(_, value)| {
            value
                .trim_start()
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse::<u32>()
                .ok()
        });
        return Some(match code {
            Some(279) => Signal::ConnectionFailed,
            Some(280) => Signal::ClientUpdateRequired,
            Some(524 | 533 | 600 | 773) => Signal::PermissionDenied,
            Some(528 | 772) => Signal::TargetUnavailable,
            _ => Signal::SessionLost(code),
        });
    }
    None
}

#[derive(Clone, Copy, Debug)]
pub struct LogEvent {
    pub signal: Signal,
    pub confirmed_terminal: bool,
}

/// Disk event semantics; the caller must separately establish process ownership.
pub fn parse_event(line: &str) -> Option<LogEvent> {
    let signal = parse_line(line)?;
    let confirmed_terminal = match signal {
        Signal::Connected => false,
        Signal::JoinRequestFailed => false, // One request can fail while the client retries.
        Signal::Disconnected => {
            // Bare local disconnect/peer-close events can accompany an intentional transfer.
            // Explicit connection-lost events establish a lost gameplay connection.
            line.contains("Connection lost") || line.contains("ID_CONNECTION_LOST")
        }
        // Only known failed-session codes authorize shutdown. Unknown codes need evidence.
        Signal::SessionLost(code) => {
            matches!(code, Some(264 | 266 | 267 | 268 | 273 | 277 | 280 | 529))
        }
        Signal::ConnectionFailed | Signal::TargetUnavailable | Signal::PermissionDenied => true,
    };
    Some(LogEvent {
        signal,
        confirmed_terminal,
    })
}

pub struct Tail {
    pub path: PathBuf,
    pub offset: u64,
    pending: Vec<u8>,
    discard_line: bool,
    pub last_signal_time: Option<chrono::DateTime<chrono::Utc>>,
    #[cfg(target_os = "linux")]
    inode: (u64, u64),
}
impl Tail {
    pub fn new(path: PathBuf, from_end: bool) -> std::io::Result<Self> {
        let offset = if from_end {
            std::fs::metadata(&path)?.len()
        } else {
            0
        };
        Ok(Self {
            path: path.clone(),
            offset,
            pending: Vec::new(),
            discard_line: false,
            last_signal_time: None,
            #[cfg(target_os = "linux")]
            inode: {
                use std::os::unix::fs::MetadataExt;
                let metadata = std::fs::metadata(&path)?;
                (metadata.dev(), metadata.ino())
            },
        })
    }
    pub fn read(&mut self) -> std::io::Result<Vec<Signal>> {
        self.read_events()
            .map(|events| events.into_iter().map(|event| event.signal).collect())
    }
    pub fn read_events(&mut self) -> std::io::Result<Vec<LogEvent>> {
        let mut f = File::open(&self.path)?;
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = f.metadata()?;
            if (metadata.dev(), metadata.ino()) != self.inode {
                return Err(std::io::Error::other(
                    "Log descriptor changed; rediscover ownership",
                ));
            }
        }
        let length = f.metadata()?.len();
        if length < self.offset {
            self.offset = 0;
            self.pending.clear();
            self.discard_line = false;
        }
        // Current state matters more than replaying megabytes of old game output.
        // Drop the partial first line after a seek so embedded tags cannot become authoritative.
        let skipped = length.saturating_sub(self.offset) > 256 * 1024;
        if skipped {
            self.offset = length.saturating_sub(256 * 1024);
            self.pending.clear();
            self.discard_line = true;
        }
        f.seek(SeekFrom::Start(self.offset))?;
        let previous = self.pending.len();
        f.take(256 * 1024).read_to_end(&mut self.pending)?;
        self.offset += (self.pending.len() - previous) as u64;
        if self.discard_line {
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                self.pending.drain(..=end);
                self.discard_line = false;
            } else {
                self.pending.clear();
            }
        }
        let mut signals = Vec::new();
        if let Some(end) = self.pending.iter().rposition(|b| *b == b'\n') {
            for line in self.pending[..=end].split(|b| *b == b'\n') {
                let line = String::from_utf8_lossy(line);
                if let Some(event) = parse_event(&line) {
                    if let Some(time) = timestamp(&line) {
                        self.last_signal_time =
                            Some(self.last_signal_time.map_or(time, |last| last.max(time)));
                    }
                    signals.push(event);
                }
            }
            self.pending.drain(..=end);
        }
        if self.pending.len() > 256 * 1024 {
            self.pending.clear();
            // Keep discarding across reads until this oversized line ends.
            self.discard_line = true;
        }
        Ok(signals)
    }
}

pub fn timestamp(line: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(line.split_once(',')?.0)
        .ok()
        .map(|t| t.to_utc())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_kick_reason_confirms_only_the_authoritative_network_message() {
        for channel in ["FLog::Network", "DFLog::NetworkClient"] {
            let line = format!(
                "[{channel}] Lost connection with reason : You have been kicked from the game"
            );
            let event = parse_event(&line).expect("native kick must be recognized");
            assert_eq!(event.signal, Signal::SessionLost(Some(267)));
            assert!(event.confirmed_terminal);
            assert!(parse_event(&format!("[FLog::Output] {line}")).is_none());
            assert!(parse_event(&format!("[{channel}] chat: Lost connection with reason : You have been kicked from the game")).is_none());
        }
        for line in [
            "[FLog::Output] Lost connection with reason : You have been kicked from the game",
            "[FLog::Error] Lost connection with reason : You have been kicked from the game",
            "[FLog::Network] Lost connection with reason : unknown",
            "[FLog::Network] Lost connection with reason : You have been kicked from the game is sample text",
            "[FLog::Network] Players service requests player disconnect",
        ] {
            assert!(parse_event(line).is_none(), "{line}");
        }
    }
    #[test]
    fn terminal_policy_uses_event_semantics_not_only_valid_log_format() {
        for line in [
            "[FLog::Network] Connection lost",
            "[FLog::NetworkClient] ID_CONNECTION_LOST",
            "[FLog::Network] Sending disconnect with reason: 273",
            "[FLog::GameJoinUtil] GameJoinFailed errorCode: 279",
            "[FLog::GameJoinUtil] GameJoinFailed errorCode: 529",
        ] {
            assert!(parse_event(line).unwrap().confirmed_terminal, "{line}");
        }
        for line in [
            "[FLog::Network] Sending disconnect with reason: 0",
            "[FLog::Network] Sending disconnect with reason: 282",
            "[FLog::Network] Sending disconnect with reason: 20",
            "[FLog::Network] Sending disconnect with reason: 285",
            "[FLog::Network] Client:Disconnect",
            "[FLog::Network] ID_DISCONNECTION_NOTIFICATION",
            "[FLog::RbxTransportDummyClient] Connection closed",
            "[FLog::GameJoinUtil] GameJoinFailed errorCode: 999",
            "[FLog::GameJoinUtil] GameJoinFailed",
            "[FLog::Network] Connection accepted",
            "[FLog::HttpTraceError] HttpResponse(#1) status:503 Error url:{ \"https://gamejoin.roblox.com/v1/join-game\" }",
        ] {
            assert!(!parse_event(line).unwrap().confirmed_terminal, "{line}");
        }
        for line in [
            "[FLog::Output] [FLog::Network] Connection lost",
            "[FLog::HttpTraceError] unrelated network error",
            "[FLog::Output] GameJoinFailed errorCode: 279",
        ] {
            assert!(parse_event(line).is_none());
        }
    }
    #[test]
    #[ignore = "release-build backlog replay benchmark"]
    fn hotfix_benchmark_log_backlog() {
        let path = std::env::temp_dir().join(format!("rbx-backlog-bench-{}", uuid::Uuid::new_v4()));
        let mut contents = "[FLog::Output] unrelated game output\n".repeat(100_000);
        contents.push_str("[FLog::Network] Sending disconnect with reason: 273\n");
        std::fs::write(&path, &contents).unwrap();
        let mut reports = Vec::new();
        for old in [true, false] {
            let start = std::time::Instant::now();
            let mut polls = 0;
            for _ in 0..50 {
                if old {
                    // v1.7.0 Tail::read, same 256 KiB reads and partial-line handling.
                    let mut offset = 0;
                    let mut pending = Vec::new();
                    loop {
                        let mut file = File::open(&path).unwrap();
                        file.seek(SeekFrom::Start(offset)).unwrap();
                        let mut bytes = Vec::new();
                        file.take(256 * 1024).read_to_end(&mut bytes).unwrap();
                        offset += bytes.len() as u64;
                        pending.extend(bytes);
                        polls += 1;
                        let mut detected = false;
                        if let Some(end) = pending.iter().rposition(|b| *b == b'\n') {
                            for line in pending[..=end].split(|b| *b == b'\n') {
                                detected |= parse_line(&String::from_utf8_lossy(line))
                                    == Some(Signal::SessionLost(Some(273)));
                            }
                            pending.drain(..=end);
                        }
                        if detected {
                            break;
                        }
                        assert!(offset < contents.len() as u64);
                    }
                } else {
                    let mut tail = Tail::new(path.clone(), false).unwrap();
                    assert_eq!(
                        tail.read().unwrap().last(),
                        Some(&Signal::SessionLost(Some(273)))
                    );
                    polls += 1;
                }
            }
            reports.push(serde_json::json!({"mode":if old {"baseline"} else {"new"},"elapsed_ms":start.elapsed().as_secs_f64()*1000.0,"polls_per_client":polls / 50}));
        }
        std::fs::remove_file(path).unwrap();
        println!(
            "HOTFIX_BENCH {}",
            serde_json::json!({"workload":"50 sequential replays of the same cached log; no live Roblox clients","log_bytes":contents.len(),"reports":reports})
        );
    }
    #[test]
    fn large_backlog_detects_latest_273_on_first_poll() {
        let path = std::env::temp_dir().join(format!("rbx-backlog-{}", uuid::Uuid::new_v4()));
        let mut contents = "[FLog::Output] unrelated game output\n".repeat(100_000);
        contents.push_str("[FLog::Network] Sending disconnect with reason: 273\n");
        std::fs::write(&path, contents).unwrap();
        let mut tail = Tail::new(path.clone(), false).unwrap();
        let signals = tail.read().unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(signals.last(), Some(&Signal::SessionLost(Some(273))));
    }
    #[test]
    fn failed_join_http_requests_are_recoverable_without_game_join_failed() {
        // Redacted from a client stuck on Join Error 529 before any network connection.
        let line = "2026-10-15T11:58:05.018Z,24.018595,4648,12 [DFLog::HttpTraceError] HttpResponse(#19 0x123) time:21052.3ms (net:21052.1ms timeInRetryQueue:0.0ms) error:5 message:HttpError: Timedout url:{ \"https://gamejoin.roblox.com/v1/join-game-instance\" } ip: external:0 numberOfTimesRetried:0";
        assert_eq!(parse_line(line), Some(Signal::JoinRequestFailed));
        for path in ["join-game", "join-game-instance", "join-private-game"] {
            for status in [400, 403, 429, 500, 503, 529] {
                assert_eq!(
                    parse_line(&format!(
                        "[FLog::HttpTraceError] HttpResponse(#1) status:{status} Error url:{{ \"https://gamejoin.roblox.com/v1/{path}\" }}"
                    )),
                    Some(Signal::JoinRequestFailed)
                );
            }
        }
    }
    #[test]
    fn unrelated_http_failures_and_spoofed_join_messages_are_ignored() {
        for line in [
            "[DFLog::HttpTraceError] HttpResponse(#1) error:5 message:HttpError: Timedout url:{ \"https://assetdelivery.roblox.com/v1/asset\" }",
            "[FLog::Error] Error making AccountSettingsEmailRequest. Received HTTPErrorCode 5, status code -1.",
            "[FLog::Output] [DFLog::HttpTraceError] HttpResponse(#1) error:5 message:HttpError: Timedout url:{ \"https://gamejoin.roblox.com/v1/join-game-instance\" }",
            "[DFLog::HttpTraceError] HttpResponse(#1) status:200 OK url:{ \"https://gamejoin.roblox.com/v1/join-game-instance\" }",
            "[DFLog::HttpTraceError] HttpResponse(#1) error:0 message:HttpError: OK url:{ \"https://gamejoin.roblox.com/v1/join-game-instance\" }",
            "[DFLog::HttpTraceError] HttpResponse(#1) status:503 Error url:{ \"https://gamejoin.roblox.com.evil.test/v1/join-game\" }",
            "[DFLog::HttpTraceError] HttpResponse(#1) status:503 Error url:{ \"https://gamejoin.roblox.com/v1/unrelated?next=/v1/join-game\" }",
            "[DFLog::HttpTraceError] HttpResponse(#1) status:503 Error url:{ \"invalid\" }",
            "[FLog::Output] GameJoinFailed errorCode: 529",
        ] {
            assert_eq!(parse_line(line), None);
        }
    }
    #[test]
    fn server_no_response_is_recoverable_but_unrelated_network_failures_are_ignored() {
        assert_eq!(
            parse_line(
                "Error [DFLog::NetworkClient] Failed to connect to server at 127.0.0.1|12345, no response."
            ),
            Some(Signal::ConnectionFailed)
        );
        assert_eq!(
            parse_line("[DFLog::GameJoinUtil] GameJoinFailed errorCode: 279"),
            Some(Signal::ConnectionFailed)
        );
        assert_eq!(
            parse_line("[DFLog::NetworkClient] Client:Disconnect"),
            Some(Signal::Disconnected)
        );
        assert_eq!(
            parse_line("[DFLog::NetworkClient] Connection accepted"),
            Some(Signal::Connected)
        );
        for line in [
            "[DFLog::HttpTraceError] Failed to connect to server, no response",
            "[DFLog::AssetProvider] Failed to connect to server, no response",
            "[FLog::Output] Failed to connect to server, no response",
            "[DFLog::NetworkClient] will connect to server",
        ] {
            assert_eq!(parse_line(line), None);
        }
    }
    #[test]
    fn kicks_and_unknown_join_failures_retry_without_permission_or_fallback_guesses() {
        for code in [267, 277, 268, 2790, 999] {
            assert_eq!(
                parse_line(&format!(
                    "[DFLog::GameJoinUtil] GameJoinFailed errorCode: {code}"
                )),
                Some(Signal::SessionLost(Some(code)))
            );
        }
        assert_eq!(
            parse_line("[FLog::GameJoinUtil] GameJoinFailed"),
            Some(Signal::SessionLost(None))
        );
        for code in [524, 533, 773] {
            assert_eq!(
                parse_line(&format!(
                    "[FLog::GameJoinUtil] GameJoinFailed errorCode: {code}"
                )),
                Some(Signal::PermissionDenied)
            );
        }
        assert_eq!(
            parse_line("[FLog::Network] Connection lost"),
            Some(Signal::Disconnected)
        );
        assert_eq!(
            parse_line("[DFLog::RbxTransportDummyClient] Connection closed"),
            Some(Signal::Disconnected)
        );
        assert_eq!(
            parse_line("[FLog::Output] [DFLog::NetworkClient] Client:Disconnect"),
            None
        );
        assert_eq!(
            parse_line("[FLog::Output] [FLog::GameJoinUtil] GameJoinFailed errorCode: 267"),
            None
        );
    }
    #[test]
    fn join_attempt_and_unrelated_text_do_not_trigger_recovery() {
        assert_eq!(
            parse_line("[FLog::Output] ! Joining game 'foo' place 1 at 0.0.0.0"),
            None
        );
        assert_eq!(
            parse_line("chat: Sending disconnect with reason: 267"),
            None
        );
        assert_eq!(
            parse_line("[FLog::Network] Sending disconnect with reason: 20"),
            Some(Signal::Disconnected)
        );
    }
    #[test]
    fn partial_lines_and_truncation_are_handled() {
        let p = std::env::temp_dir().join(format!("rbx-tail-{}", uuid::Uuid::new_v4()));
        std::fs::write(&p, "[FLog::Network] Sending disconnect with reason:").unwrap();
        let mut t = Tail::new(p.clone(), false).unwrap();
        assert!(t.read().unwrap().is_empty());
        use std::io::Write;
        writeln!(
            std::fs::OpenOptions::new().append(true).open(&p).unwrap(),
            " 20"
        )
        .unwrap();
        assert_eq!(t.read().unwrap(), vec![Signal::Disconnected]);
        std::fs::write(&p, "[FLog::Network] Replicator created\n").unwrap();
        assert_eq!(t.read().unwrap(), vec![Signal::Connected]);
        std::fs::remove_file(p).unwrap();
    }
}
