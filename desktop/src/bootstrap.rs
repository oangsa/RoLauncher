//! Supervisor-to-desktop pipe bootstrap parsing and validation.

use std::io::BufRead;

use serde::Deserialize;
use zeroize::Zeroizing;

/// Startup configuration delivered once over inherited standard input.
#[derive(Clone, Debug, Deserialize)]
pub struct Bootstrap {
    pub port: u16,
    #[serde(deserialize_with = "deserialize_zeroizing")]
    pub token: Zeroizing<String>,
    pub version: String,
    pub parent_id: u32,
    #[serde(default)]
    pub data_directory: String,
}

impl Bootstrap {
    /// Parses and validates a single JSON bootstrap line.
    pub fn parse_line(line: &str) -> Option<Self> {
        let parsed: Self = serde_json::from_str(line.trim()).ok()?;
        if parsed.port == 0 || parsed.token.len() < 32 {
            return None;
        }
        Some(parsed)
    }

    /// Reads one line from standard input without retaining raw input buffers.
    pub fn read_from_stdin() -> Option<Self> {
        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        let mut line = Zeroizing::new(String::new());
        let bytes = handle.read_line(&mut line).ok()?;
        if bytes == 0 {
            return None;
        }
        Self::parse_line(&line)
    }
}

fn deserialize_zeroizing<'de, D>(deserializer: D) -> Result<Zeroizing<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    String::deserialize(deserializer).map(Zeroizing::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_validation_rejects_short_token_or_zero_port() {
        let short = r#"{"port":38471,"token":"short","version":"2.0.0","parent_id":42}"#;
        assert!(Bootstrap::parse_line(short).is_none());

        let zero_port = format!(
            r#"{{"port":0,"token":"{}","version":"2.0.0","parent_id":42}}"#,
            "a".repeat(48)
        );
        assert!(Bootstrap::parse_line(&zero_port).is_none());

        let valid = format!(
            r#"{{"port":38471,"token":"{}","version":"2.0.0-dev.0123456789ab","parent_id":42}}"#,
            "a".repeat(48)
        );
        let parsed = Bootstrap::parse_line(&valid).expect("valid bootstrap");
        assert_eq!(parsed.port, 38471);
        assert_eq!(parsed.parent_id, 42);
        assert_eq!(parsed.data_directory, "");
    }
}
