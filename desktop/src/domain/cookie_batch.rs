//! Chunked, zeroized cookie batch parsing and supervisor import.

use std::collections::{BTreeMap, HashSet};
use std::future::Future;

use serde_json::Value;
use zeroize::Zeroizing;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchImportResult {
    pub imported: usize,
    pub errors: Vec<String>,
}

/// Parses one cookie per line, preserving order and removing exact duplicates.
pub fn parse_cookie_lines(text: &str) -> Vec<Zeroizing<String>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in text.split(['\r', '\n']) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(Zeroizing::new(trimmed.to_string()));
        }
    }
    out
}

/// Imports parsed cookies sequentially in chunks of at most 50 entries, clearing buffers on exit.
pub async fn import_batches<F, Fut>(
    mut cookies: Vec<Zeroizing<String>>,
    mut send: F,
) -> BatchImportResult
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<Value, BatchTransportError>>,
{
    if cookies.is_empty() || cookies.iter().all(|c| c.trim().is_empty()) {
        cookies.clear();
        return BatchImportResult {
            imported: 0,
            errors: vec!["Paste a session cookie first.".into()],
        };
    }

    let mut imported = 0usize;
    let mut errors = Vec::new();

    for offset in (0..cookies.len()).step_by(50) {
        let end = (offset + 50).min(cookies.len());
        let batch_len = end - offset;
        let batch_payload: Vec<String> = cookies[offset..end]
            .iter()
            .map(|c| c.as_str().to_string())
            .collect();

        let response = send(batch_payload).await;
        match response {
            Ok(value) => match validate_batch_response(&value, batch_len) {
                Ok((ok_count, indexed_errors)) => {
                    imported += ok_count;
                    for (pos, msg) in indexed_errors {
                        errors.push(format!("Entry {}: {msg}", offset + pos + 1));
                    }
                }
                Err(()) => {
                    errors.push(format!(
                        "Import stopped at entry {}. RoLauncher returned an invalid import response. Some accounts may have saved; check the refreshed list before retrying.",
                        offset + 1
                    ));
                    break;
                }
            },
            Err(kind) => {
                let reason = match kind {
                    BatchTransportError::Timeout => "Request timed out or was cancelled.",
                    BatchTransportError::InvalidResponse => {
                        "RoLauncher returned an invalid import response."
                    }
                    BatchTransportError::Failed => "Import request failed.",
                };
                errors.push(format!(
                    "Import stopped at entry {}. {reason} Some accounts may have saved; check the refreshed list before retrying.",
                    offset + 1
                ));
                break;
            }
        }
    }

    cookies.clear();
    BatchImportResult { imported, errors }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchTransportError {
    Timeout,
    InvalidResponse,
    Failed,
}

fn validate_batch_response(
    value: &Value,
    expected_len: usize,
) -> Result<(usize, BTreeMap<usize, String>), ()> {
    let obj = value.as_object().ok_or(())?;
    let accounts = obj.get("accounts").and_then(Value::as_array).ok_or(())?;
    let failures = obj.get("errors").and_then(Value::as_array).ok_or(())?;
    if accounts.len() + failures.len() != expected_len {
        return Err(());
    }
    if !accounts.iter().all(Value::is_object) {
        return Err(());
    }

    let mut indexed = BTreeMap::new();
    for failure in failures {
        let entry = failure.as_object().ok_or(())?;
        let index_val = entry.get("index").and_then(Value::as_u64).ok_or(())?;
        let pos = usize::try_from(index_val).map_err(|_| ())?;
        if pos >= expected_len {
            return Err(());
        }
        let message = entry
            .get("error")
            .and_then(Value::as_str)
            .ok_or(())?
            .to_string();
        if indexed.insert(pos, message).is_some() {
            return Err(());
        }
    }

    Ok((accounts.len(), indexed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_preserves_order_and_deduplicates() {
        let parsed = parse_cookie_lines("same\r\nexpired\n\nsame\n  other  ");
        let strs: Vec<&str> = parsed.iter().map(|s| s.as_str()).collect();
        assert_eq!(strs, vec!["same", "expired", "other"]);
    }

    #[test]
    fn batch_import_chunks_at_50_and_sanitizes_errors() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let inputs: Vec<Zeroizing<String>> = (1..=103)
            .map(|i| Zeroizing::new(format!("fixture-{i}")))
            .collect();
        let mut sizes = Vec::new();
        let result = rt.block_on(import_batches(inputs, |batch| {
            sizes.push(batch.len());
            let errors = vec![json!({"index": batch.len() - 1, "error": "Session expired"})];
            let accounts: Vec<Value> = (0..batch.len() - 1)
                .map(|_| json!({"id": "saved"}))
                .collect();
            async move { Ok(json!({ "accounts": accounts, "errors": errors })) }
        }));
        assert_eq!(sizes, vec![50, 50, 3]);
        assert_eq!(result.imported, 100);
        assert_eq!(
            result.errors,
            vec![
                "Entry 50: Session expired",
                "Entry 100: Session expired",
                "Entry 103: Session expired"
            ]
        );

        let sensitive = vec![Zeroizing::new("secret-not-for-diagnostics".into()); 51];
        let mut calls = 0usize;
        let failed = rt.block_on(import_batches(sensitive, |batch| {
            calls += 1;
            let current = calls;
            async move {
                if current == 2 {
                    Err(BatchTransportError::Failed)
                } else {
                    let accounts: Vec<Value> =
                        batch.iter().map(|_| json!({"id": "saved"})).collect();
                    Ok(json!({ "accounts": accounts, "errors": [] }))
                }
            }
        }));
        assert_eq!(failed.imported, 50);
        assert!(failed.errors[0].starts_with("Import stopped at entry 51."));
        assert!(!failed.errors[0].contains("secret"));
    }
}
