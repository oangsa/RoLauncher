//! Resource formatting helpers, sorting, bounded history merging, and row formatting.

use crate::domain::models::{
    ManagedInstanceResource, ResourceHistoryPoint, ResourceSnapshotResponse, format_account_status,
    format_duration,
};
use std::cmp::Ordering;

/// Human-friendly label and detail copy shown when the Resource Monitor table has no rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceEmptyState {
    /// Whether the empty-state panel should be displayed.
    pub visible: bool,
    /// Primary heading describing the empty state.
    pub title: &'static str,
    /// Actionable guidance for resolving the empty state.
    pub hint: &'static str,
}

/// Pre-formatted cell values for one row in the per-instance resource table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceInstanceRow {
    /// Account identifier.
    pub account_id: String,
    /// Account display alias (falls back to username in backend snapshots).
    pub display_name: String,
    /// Formatted `@username`.
    pub username: String,
    /// Combined account lifecycle and process availability status label.
    pub status: String,
    /// Process ID string or `"—"`.
    pub pid: String,
    /// Formatted normalized machine CPU percentage, `"Sampling…"`, or `"—"`.
    pub cpu: String,
    /// Formatted resident working set bytes or `"—"`.
    pub ram: String,
    /// Formatted private working set bytes or `"—"`.
    pub private_ram: String,
    /// Formatted GPU utilization percentage, `"Sampling…"`, or `"Unavailable"`.
    pub gpu: String,
    /// Formatted process uptime or `"—"`.
    pub uptime: String,
}

impl ResourceInstanceRow {
    /// Formats one [`ManagedInstanceResource`] sample into UI-ready strings.
    pub fn from_item(item: &ManagedInstanceResource) -> Self {
        let cpu = match item.availability.as_str() {
            "available" => item
                .cpu_percent
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "—".to_string()),
            "pending" => "Sampling…".to_string(),
            _ => "—".to_string(),
        };
        let gpu = match item.gpu_availability.as_str() {
            "available" => item
                .gpu_percent
                .map(|v| format!("{v:.1}%"))
                .unwrap_or_else(|| "Unavailable".to_string()),
            "pending" => "Sampling…".to_string(),
            _ => "Unavailable".to_string(),
        };

        Self {
            account_id: item.account_id.clone(),
            display_name: item.display_name.clone(),
            username: format!("@{}", item.username),
            status: format!(
                "{} · {}",
                format_account_status(&item.account_status),
                format_process_availability(&item.availability)
            ),
            pid: item
                .pid
                .map(|p| p.to_string())
                .unwrap_or_else(|| "—".to_string()),
            cpu,
            ram: item
                .working_set_bytes
                .map(format_bytes)
                .unwrap_or_else(|| "—".to_string()),
            private_ram: item
                .private_memory_bytes
                .map(format_bytes)
                .unwrap_or_else(|| "—".to_string()),
            gpu,
            uptime: item
                .uptime_seconds
                .map(format_duration)
                .unwrap_or_else(|| "—".to_string()),
        }
    }
}

/// Formats a byte count using base-1024 units matching the WinUI formatter.
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    if bytes < 1024 * 1024 {
        return format!("{:.1} KB", bytes as f64 / 1024.0);
    }
    if bytes < 1024 * 1024 * 1024 {
        return format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0));
    }
    format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// Formats an optional percentage metric together with its backend availability state.
pub fn format_percent_metric(
    value: Option<f64>,
    availability: &str,
    unavailable_label: &str,
) -> String {
    match availability {
        "available" => value
            .map(|pct| format!("{pct:.1}%"))
            .unwrap_or_else(|| unavailable_label.to_string()),
        "pending" => "Sampling…".to_string(),
        _ => unavailable_label.to_string(),
    }
}

/// Maps backend process availability tags to concise UI labels.
pub fn format_process_availability(availability: &str) -> &'static str {
    match availability {
        "available" => "Live",
        "pending" => "Sampling",
        "exited" => "Exited",
        "inaccessible" => "Inaccessible",
        "ownership_mismatch" => "Identity mismatch",
        _ => "Unavailable",
    }
}

fn compare_nullable<T: PartialOrd>(a: Option<T>, b: Option<T>, desc: bool) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            if desc {
                y.partial_cmp(&x).unwrap_or(Ordering::Equal)
            } else {
                x.partial_cmp(&y).unwrap_or(Ordering::Equal)
            }
        }
    }
}

fn compare_ascii_ci(a: &str, b: &str) -> Ordering {
    a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
}

/// Sorts managed resource instances while keeping missing metrics ordered after live values.
pub fn sort_instances(
    source: &[ManagedInstanceResource],
    sort_by: &str,
    descending: bool,
) -> Vec<ManagedInstanceResource> {
    let mut list = source.to_vec();
    list.sort_by(|left, right| {
        let primary = match sort_by {
            "cpu" => compare_nullable(left.cpu_percent, right.cpu_percent, descending),
            "ram" => compare_nullable(left.working_set_bytes, right.working_set_bytes, descending),
            "uptime" => compare_nullable(left.uptime_seconds, right.uptime_seconds, descending),
            "pid" => compare_nullable(left.pid, right.pid, descending),
            "status" => {
                let ls = format_account_status(&left.account_status);
                let rs = format_account_status(&right.account_status);
                if descending {
                    compare_ascii_ci(rs, ls)
                } else {
                    compare_ascii_ci(ls, rs)
                }
            }
            _ => {
                if descending {
                    compare_ascii_ci(&right.display_name, &left.display_name)
                } else {
                    compare_ascii_ci(&left.display_name, &right.display_name)
                }
            }
        };
        if primary != Ordering::Equal {
            return primary;
        }
        let by_name = compare_ascii_ci(&left.display_name, &right.display_name);
        if by_name != Ordering::Equal {
            by_name
        } else {
            left.account_id.cmp(&right.account_id)
        }
    });
    list
}

/// Appends the latest sample onto the bounded client-side history ring buffer.
pub fn merge_history(
    current: &[ResourceHistoryPoint],
    snapshot: &ResourceSnapshotResponse,
    capacity: usize,
) -> Vec<ResourceHistoryPoint> {
    let mut points = if let Some(seeded) = snapshot.history.as_ref().filter(|h| !h.is_empty()) {
        seeded.clone()
    } else {
        current.to_vec()
    };

    let next = ResourceHistoryPoint {
        timestamp: snapshot.timestamp,
        system_cpu_percent: snapshot.system.cpu_percent,
        ram_utilization_percent: snapshot.system.ram_utilization_percent,
        managed_cpu_percent: snapshot.aggregates.cpu_percent,
        managed_ram_bytes: snapshot.aggregates.working_set_bytes,
        managed_gpu_percent: snapshot.aggregates.gpu_percent,
        running_instances: snapshot.aggregates.running_instances,
    };

    if snapshot.history.as_ref().is_none_or(|h| h.is_empty())
        && points
            .last()
            .is_none_or(|last| last.timestamp != next.timestamp)
    {
        points.push(next);
    }

    while points.len() > capacity {
        points.remove(0);
    }
    points
}

/// Computes the empty-state copy for the per-instance Resource Monitor list.
pub fn empty_state_copy(row_count: usize, total_accounts: usize) -> ResourceEmptyState {
    if row_count > 0 {
        return ResourceEmptyState {
            visible: false,
            title: "",
            hint: "",
        };
    }
    if total_accounts == 0 {
        ResourceEmptyState {
            visible: true,
            title: "No saved accounts",
            hint: "Add an account to monitor its managed Roblox instance.",
        }
    } else {
        ResourceEmptyState {
            visible: true,
            title: "No running managed Roblox instances",
            hint: "Start an account on Accounts to view live CPU, RAM, and GPU metrics.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> ResourceSnapshotResponse {
        let json = r#"{
          "timestamp": "2026-11-13T00:00:00Z",
          "sample_age_ms": 250,
          "stale": false,
          "availability": "partial",
          "system": {
            "logical_processors": 8,
            "cpu_percent": 31.25,
            "cpu_availability": "available",
            "total_ram_bytes": 17179869184,
            "available_ram_bytes": 8589934592,
            "used_ram_bytes": 8589934592,
            "ram_utilization_percent": 50.0,
            "ram_availability": "available",
            "gpu_percent": null,
            "gpu_availability": "unavailable"
          },
          "aggregates": {
            "running_instances": 1,
            "total_accounts": 3,
            "cpu_percent": 12.5,
            "cpu_availability": "available",
            "working_set_bytes": 536870912,
            "private_memory_bytes": 419430400,
            "gpu_percent": null,
            "gpu_availability": "unavailable",
            "unmanaged_roblox_processes": 1,
            "inaccessible_roblox_processes": 0,
            "exited_or_stale_instances": 1
          },
          "accounts": [
            {
              "account_id": "1",
              "display_name": "Alpha",
              "username": "alpha",
              "account_status": "running",
              "pid": 1001,
              "creation_time": "100",
              "cpu_percent": 12.5,
              "working_set_bytes": 536870912,
              "private_memory_bytes": 419430400,
              "gpu_percent": null,
              "gpu_availability": "unavailable",
              "uptime_seconds": 3665,
              "sampled_at": "2026-11-13T00:00:00Z",
              "availability": "available"
            },
            {
              "account_id": "2",
              "display_name": "Beta",
              "username": "beta",
              "account_status": "stopped",
              "pid": null,
              "creation_time": null,
              "cpu_percent": null,
              "working_set_bytes": null,
              "private_memory_bytes": null,
              "gpu_percent": null,
              "gpu_availability": "unavailable",
              "uptime_seconds": null,
              "sampled_at": "2026-11-13T00:00:00Z",
              "availability": "exited"
            },
            {
              "account_id": "3",
              "display_name": "Gamma",
              "username": "gamma",
              "account_status": "running",
              "pid": 3003,
              "creation_time": "200",
              "cpu_percent": 18.0,
              "working_set_bytes": 268435456,
              "private_memory_bytes": null,
              "gpu_percent": 9.5,
              "gpu_availability": "available",
              "uptime_seconds": 60,
              "sampled_at": "2026-11-13T00:00:00Z",
              "availability": "available"
            }
          ],
          "metadata": {
            "cpu_normalization": "machine_percent",
            "cpu_formula": "delta_cpu_time / (elapsed_wall_time * logical_processors) * 100",
            "memory_units": "bytes",
            "gpu_normalization": "gpu_engine_percent_sum_capped_100",
            "sample_interval_ms": 1000,
            "history_capacity": 60,
            "stale_threshold_ms": 5000,
            "sampling_mode": "active",
            "last_sample_duration_ms": 4
          },
          "history": [
            {
              "timestamp": "2026-11-13T00:00:00Z",
              "system_cpu_percent": 31.25,
              "ram_utilization_percent": 50.0,
              "managed_cpu_percent": 12.5,
              "managed_ram_bytes": 536870912,
              "managed_gpu_percent": null,
              "running_instances": 1
            }
          ]
        }"#;
        serde_json::from_str(json).expect("sample resource snapshot should deserialize")
    }

    #[test]
    fn resource_formatting_and_sorting_match_winui_contract() {
        let parsed = sample_snapshot();
        assert_eq!(parsed.metadata.cpu_normalization, "machine_percent");
        assert_eq!(parsed.aggregates.running_instances, 1);
        assert_eq!(format_bytes(536_870_912), "512.0 MB");
        assert_eq!(format_bytes(16 * 1024 * 1024 * 1024), "16.00 GB");
        assert_eq!(
            format_percent_metric(None, "unavailable", "Unavailable"),
            "Unavailable"
        );
        assert_eq!(
            format_percent_metric(None, "pending", "Unavailable"),
            "Sampling…"
        );

        let by_cpu_desc: Vec<_> = sort_instances(&parsed.accounts, "cpu", true)
            .into_iter()
            .map(|a| a.account_id)
            .collect();
        assert_eq!(by_cpu_desc, vec!["3", "1", "2"]);

        let by_ram_asc: Vec<_> = sort_instances(&parsed.accounts, "ram", false)
            .into_iter()
            .map(|a| a.account_id)
            .collect();
        assert_eq!(by_ram_asc, vec!["3", "1", "2"]);

        let alpha = ResourceInstanceRow::from_item(&parsed.accounts[0]);
        let beta = ResourceInstanceRow::from_item(&parsed.accounts[1]);
        let gamma = ResourceInstanceRow::from_item(&parsed.accounts[2]);
        assert_eq!(alpha.cpu, "12.5%");
        assert_eq!(alpha.gpu, "Unavailable");
        assert_eq!(alpha.uptime, "1h 1m 5s");
        assert_eq!(beta.cpu, "—");
        assert!(beta.status.contains("Exited"));
        assert_eq!(gamma.gpu, "9.5%");

        let mut history = merge_history(&[], &parsed, 60);
        for s in 1..=65 {
            let mut next = parsed.clone();
            next.history = None;
            let ts = format!("2026-11-13T00:{:02}:{:02}Z", s / 60, s % 60);
            next.timestamp = chrono::DateTime::parse_from_rfc3339(&ts)
                .unwrap()
                .with_timezone(&chrono::Utc);
            history = merge_history(&history, &next, 60);
        }
        assert_eq!(history.len(), 60);
        assert_eq!(
            history[0]
                .timestamp
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "2026-11-13T00:00:06Z"
        );

        let no_saved = empty_state_copy(0, 0);
        let none_running = empty_state_copy(0, 3);
        let has_rows = empty_state_copy(1, 3);
        assert!(no_saved.visible && no_saved.title == "No saved accounts");
        assert!(
            none_running.visible && none_running.title == "No running managed Roblox instances"
        );
        assert!(!has_rows.visible);
    }
}
