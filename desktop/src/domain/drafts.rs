//! Validation and patch generation for individual and bulk account editors.

use serde_json::{Map, Value, json};
use url::Url;
use uuid::Uuid;

use crate::domain::models::{Account, Target};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EditorDraft {
    pub alias: String,
    pub place: String,
    pub job: String,
    pub private: String,
}

impl EditorDraft {
    /// Validates Place ID, optional hyphenated Job ID, or HTTPS Roblox private server link.
    pub fn try_target(&self) -> Result<Target, &'static str> {
        let place_trimmed = self.place.trim();
        let place: u64 = place_trimmed
            .parse::<u64>()
            .ok()
            .filter(|p| *p > 0 && place_trimmed.bytes().all(|b| b.is_ascii_digit()))
            .ok_or("Enter a Place ID greater than zero.")?;

        let job = self.job.trim();
        let link = self.private.trim();
        if !job.is_empty() && !link.is_empty() {
            return Err("Choose a Job ID or a private server link.");
        }
        if !job.is_empty() {
            let valid_uuid = job.len() == 36
                && job.bytes().filter(|b| *b == b'-').count() == 4
                && Uuid::parse_str(job).is_ok();
            if !valid_uuid {
                return Err("Enter a valid Job ID.");
            }
        }
        if !link.is_empty() {
            validate_private_server_link(link, place)?;
        }

        Ok(Target {
            place_id: place,
            job_id: (!job.is_empty()).then(|| job.to_string()),
            private_server_link: (!link.is_empty()).then(|| link.to_string()),
        })
    }
}

fn validate_private_server_link(link: &str, place: u64) -> Result<(), &'static str> {
    const ERR: &str = "Enter a Roblox private server link matching the Place ID.";
    if link.len() > 2048 {
        return Err(ERR);
    }
    let url = Url::parse(link).map_err(|_| ERR)?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(ERR);
    }
    if url.port().is_some() {
        return Err(ERR);
    }
    if !matches!(url.host_str(), Some("www.roblox.com" | "roblox.com")) {
        return Err(ERR);
    }

    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };

    if get("code").is_some() && get("type") != Some("Server") {
        return Err(ERR);
    }

    let private_code = get("privateServerLinkCode");
    if private_code.is_some() {
        let mut segments = url.path_segments().ok_or(ERR)?;
        if segments.next() != Some("games") {
            return Err(ERR);
        }
        let path_place: u64 = segments
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(ERR)?;
        if path_place != place {
            return Err(ERR);
        }
    }

    let code = private_code.or_else(|| get("code")).ok_or(ERR)?;
    if code.is_empty()
        || code.len() > 512
        || !code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(ERR);
    }

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BulkDraft {
    pub fields: EditorDraft,
    pub apply_alias: bool,
    pub apply_target: bool,
    pub clear_target: bool,
    pub game_profile_id: Option<String>,
    pub apply_rejoin: bool,
    pub rejoin: Option<bool>,
    pub apply_group: bool,
    pub group: String,
    pub apply_fallback: bool,
    pub fallback: Option<String>,
}

impl Default for BulkDraft {
    fn default() -> Self {
        Self {
            fields: EditorDraft::default(),
            apply_alias: false,
            apply_target: false,
            clear_target: false,
            game_profile_id: None,
            apply_rejoin: false,
            rejoin: None,
            apply_group: false,
            group: String::new(),
            apply_fallback: false,
            fallback: Some("allow_public".to_string()),
        }
    }
}

impl BulkDraft {
    pub fn try_patch(&self) -> Result<Map<String, Value>, &'static str> {
        let mut patch = Map::new();
        if self.apply_alias {
            if self.fields.alias.trim().is_empty()
                || self.fields.alias.chars().count() > 100
                || self.fields.alias.chars().any(char::is_control)
            {
                return Err("Enter an alias or pattern, up to 100 characters.");
            }
            patch.insert("alias".into(), Value::String(self.fields.alias.clone()));
        }
        if self.apply_target {
            if self.clear_target {
                patch.insert("clear_target".into(), Value::Bool(true));
            } else {
                let target = self.fields.try_target()?;
                patch.insert("target".into(), json!(target));
                if let Some(profile_id) = &self.game_profile_id {
                    patch.insert("game_profile_id".into(), Value::String(profile_id.clone()));
                }
            }
        }
        if self.apply_rejoin {
            let value = self
                .rejoin
                .ok_or("Choose on/off for rejoin and a fallback policy where applied.")?;
            patch.insert("auto_recovery".into(), Value::Bool(value));
        }
        if self.apply_group {
            if self.group.chars().count() > 100 || self.group.chars().any(char::is_control) {
                return Err("Group: max 100 characters; no control characters.");
            }
            patch.insert("group".into(), Value::String(self.group.clone()));
        }
        if self.apply_fallback {
            let policy = self
                .fallback
                .as_ref()
                .ok_or("Choose on/off for rejoin and a fallback policy where applied.")?;
            patch.insert("fallback_policy".into(), Value::String(policy.clone()));
        }
        if patch.is_empty() {
            return Err("Check at least one Apply setting.");
        }
        Ok(patch)
    }
}

pub fn common_field<'a>(accounts: &'a [Account], f: impl Fn(&'a Account) -> &'a str) -> String {
    if accounts.is_empty() {
        return String::new();
    }
    let first = f(&accounts[0]);
    if accounts.iter().all(|a| f(a) == first) {
        first.to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_validation_matches_winui_rules() {
        let valid_public = EditorDraft {
            place: "1818".into(),
            ..Default::default()
        };
        assert_eq!(valid_public.try_target().unwrap().place_id, 1818);

        let both = EditorDraft {
            place: "1818".into(),
            job: "11111111-1111-1111-1111-111111111111".into(),
            private: "https://www.roblox.com/games/1818?privateServerLinkCode=abc-123".into(),
            ..Default::default()
        };
        assert!(both.try_target().is_err());

        let bad_place_link = EditorDraft {
            place: "1818".into(),
            private: "https://www.roblox.com/games/9999?privateServerLinkCode=abc-123".into(),
            ..Default::default()
        };
        assert!(bad_place_link.try_target().is_err());

        let share_link = EditorDraft {
            place: "1818".into(),
            private: "https://www.roblox.com/share?code=abc123&type=Server".into(),
            ..Default::default()
        };
        assert!(share_link.try_target().is_ok());

        let bad_scheme = EditorDraft {
            place: "1818".into(),
            private: "http://www.roblox.com/share?code=abc123&type=Server".into(),
            ..Default::default()
        };
        assert!(bad_scheme.try_target().is_err());

        let bad_job = EditorDraft {
            place: "1818".into(),
            job: "11111111111111111111111111111111".into(),
            ..Default::default()
        };
        assert!(bad_job.try_target().is_err());
    }

    #[test]
    fn bulk_draft_requires_apply_and_validates_mixed_rejoin() {
        let mut draft = BulkDraft::default();
        assert_eq!(
            draft.try_patch().unwrap_err(),
            "Check at least one Apply setting."
        );

        draft.apply_rejoin = true;
        assert!(draft.try_patch().is_err());

        draft.rejoin = Some(false);
        draft.apply_alias = true;
        draft.fields.alias = "Team {index}".into();
        let patch = draft.try_patch().unwrap();
        assert_eq!(patch.get("alias").unwrap(), "Team {index}");
        assert_eq!(patch.get("auto_recovery").unwrap(), false);

        draft.fields.alias = "bad\nline".into();
        assert!(draft.try_patch().is_err());
    }
}
