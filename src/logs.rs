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
    SessionLost(Option<u32>),
    TargetUnavailable,
    PermissionDenied,
}

/// Conservative external-log signatures. An attempted join is NOT a connection.
pub fn parse_line(line: &str) -> Option<Signal> {
    // The first log channel is authoritative; script output must not spoof a network tag.
    let channel = line
        .split_once('[')
        .and_then(|(_, tail)| tail.split_once(']'))
        .map(|(tag, _)| tag);
    let network = [
        "[FLog::Network]",
        "[DFLog::Network]",
        "[FLog::NetworkClient]",
        "[DFLog::NetworkClient]",
    ]
    .iter()
    .any(|tag| channel == Some(tag.trim_matches(['[', ']'])));
    if network && line.contains("Failed to connect to server") && line.contains("no response") {
        return Some(Signal::ConnectionFailed);
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
    if [
        "[FLog::Error]",
        "[FLog::GameJoinUtil]",
        "[DFLog::Error]",
        "[DFLog::GameJoinUtil]",
    ]
    .iter()
    .any(|tag| channel == Some(tag.trim_matches(['[', ']'])))
        && line.contains("GameJoinFailed")
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
            Some(524 | 533 | 773) => Signal::PermissionDenied,
            Some(528 | 772) => Signal::TargetUnavailable,
            _ => Signal::SessionLost(code),
        });
    }
    None
}

pub struct Tail {
    pub path: PathBuf,
    pub offset: u64,
    pending: Vec<u8>,
}
impl Tail {
    pub fn new(path: PathBuf, from_end: bool) -> std::io::Result<Self> {
        let offset = if from_end {
            std::fs::metadata(&path)?.len()
        } else {
            0
        };
        Ok(Self {
            path,
            offset,
            pending: Vec::new(),
        })
    }
    pub fn read(&mut self) -> std::io::Result<Vec<Signal>> {
        let mut f = File::open(&self.path)?;
        if f.metadata()?.len() < self.offset {
            self.offset = 0;
            self.pending.clear();
        }
        f.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        f.take(256 * 1024).read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        self.pending.extend(bytes);
        let mut signals = Vec::new();
        if let Some(end) = self.pending.iter().rposition(|b| *b == b'\n') {
            for line in self.pending[..=end].split(|b| *b == b'\n') {
                if let Some(signal) = parse_line(&String::from_utf8_lossy(line)) {
                    signals.push(signal);
                }
            }
            self.pending.drain(..=end);
        }
        if self.pending.len() > 256 * 1024 {
            self.pending.clear();
        }
        Ok(signals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
