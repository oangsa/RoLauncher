//! Account filtering matching the desktop account search, status, rejoin, and group controls.

use crate::domain::models::Account;

/// Rejoin filter options shared across Accounts toolbar and Preset Lookup modal.
pub const REJOIN_FILTER_OPTIONS: &[(&str, &str)] = &[
    ("", "All rejoin settings"),
    ("on", "Rejoin on"),
    ("off", "Rejoin off"),
];

/// Status filter options shared across Accounts toolbar and Preset Lookup modal.
pub const STATUS_FILTER_OPTIONS: &[(&str, &str)] = &[
    ("", "All statuses"),
    ("running", "Running"),
    ("stopped", "Stopped"),
    ("stopping", "Stopping"),
    ("queued", "Queued"),
    ("launching", "Launching"),
    ("reconnecting", "Reconnecting"),
    ("backoff", "Backoff"),
    ("needs_attention", "Needs attention"),
    ("unknown", "Unknown"),
];

/// Returns whether `account` matches the active filter criteria.
pub fn matches_account(
    account: &Account,
    search: &str,
    status: &str,
    rejoin: &str,
    group: Option<&str>,
) -> bool {
    let query = search.trim();
    let username_query = query.trim_start_matches('@');
    let name_matches = query.is_empty()
        || contains_ascii_ci(&account.alias, query)
        || contains_ascii_ci(&account.username, username_query);
    let status_matches = status.is_empty() || account.status == status;
    let rejoin_matches = rejoin.is_empty() || account.auto_recovery == (rejoin == "on");
    let group_matches = group.is_none_or(|target| account.group == target);
    name_matches && status_matches && rejoin_matches && group_matches
}

fn contains_ascii_ci(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle.is_ascii() {
        return haystack
            .as_bytes()
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()));
    }
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::Target;

    fn sample(id: &str, username: &str, alias: &str, status: &str, auto_recovery: bool) -> Account {
        Account {
            id: id.into(),
            username: username.into(),
            alias: alias.into(),
            target: Some(Target {
                place_id: 1818,
                job_id: None,
                private_server_link: None,
            }),
            auto_recovery,
            status: status.into(),
            process: None,
            failures: 0,
            last_error: None,
            group: "Alpha".into(),
            fallback_policy: "allow_public".into(),
            next_retry: None,
            recovery_reason: String::new(),
            public_fallback_active: false,
            desired_running: false,
            running_since: None,
            longest_streak_seconds: 0,
            game_profile_id: None,
        }
    }

    #[test]
    fn search_status_rejoin_and_group_filtering() {
        let a1 = sample("1", "builder_one", "Main", "running", true);
        let mut a2 = sample("2", "alt_two", "Farmer", "stopped", false);
        a2.group = String::new();

        assert!(matches_account(&a1, "", "", "", None));
        assert!(matches_account(&a1, "@BUILDER", "", "", None));
        assert!(matches_account(&a1, "main", "running", "on", Some("Alpha")));
        assert!(!matches_account(&a1, "main", "stopped", "", None));
        assert!(!matches_account(&a1, "main", "", "off", None));
        assert!(matches_account(&a2, "alt", "stopped", "off", Some("")));
        assert!(!matches_account(&a2, "alt", "", "", Some("Alpha")));
    }
}
