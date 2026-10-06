use crate::{model::Database, platform};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Reuse an existing installation's database and backups without copying credentials.
pub fn default_directory(local_app_data: &Path) -> PathBuf {
    let current = local_app_data.join("RoLauncher");
    let legacy = local_app_data.join("RbxTools");
    // Download caches can create the new brand's folder without an account database.
    // Keep using legacy state until the new folder actually contains a database.
    if !current.join("accounts.json").exists() && legacy.is_dir() {
        legacy
    } else {
        current
    }
}

#[derive(Clone)]
pub struct Store {
    pub directory: PathBuf,
}
impl Store {
    pub fn new(directory: PathBuf) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        platform::private_directory(&directory)?;
        std::fs::create_dir_all(&directory)
            .map_err(|_| "Unable to create application data folder")?;
        Ok(Self { directory })
    }
    pub fn load(&self) -> Result<Database, String> {
        let path = self.directory.join("accounts.json");
        if !path.exists() {
            return Ok(Database {
                version: 2,
                ..Default::default()
            });
        }
        let bytes = std::fs::read(path).map_err(|_| "Unable to read account database")?;
        let db: Database = serde_json::from_slice(&bytes).map_err(
            |_| "Account database is invalid; restore a backup rather than overwriting it",
        )?;
        if !matches!(db.version, 1 | 2) {
            return Err("Unsupported account database version".into());
        }
        Ok(db)
    }
    pub fn save(&self, database: &Database) -> Result<(), String> {
        let path = self.directory.join("accounts.json");
        let temp = self.directory.join("accounts.json.tmp");
        let bytes = serde_json::to_vec_pretty(database)
            .map_err(|_| "Unable to serialize account database")?;
        if path.exists() {
            let old = std::fs::read(&path).map_err(|_| "Unable to read database before backup")?;
            if old == bytes {
                return Ok(());
            }
            // Preserve the previous valid state before the first upgrade write, then at most
            // once per five minutes. Backups are whole-file DPAPI envelopes.
            let previous: Database = serde_json::from_slice(&old)
                .map_err(|_| "Refusing to overwrite an invalid database")?;
            let latest = self.backups()?.first().cloned();
            let recent = latest
                .and_then(|name| std::fs::metadata(self.directory.join("backups").join(name)).ok())
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() < 300);
            if !recent || previous.version != database.version {
                self.backup(&previous)?;
            }
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(&temp)
            .map_err(|_| "Unable to write account database")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Unable to flush account database")?;
        drop(file);
        platform::replace_file(&temp, &path)
    }
    pub fn backups(&self) -> Result<Vec<String>, String> {
        let directory = self.directory.join("backups");
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut names = std::fs::read_dir(directory)
            .map_err(|_| "Unable to list backups")?
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| valid_backup_name(n))
            .collect::<Vec<_>>();
        names.sort_by(|a, b| b.cmp(a));
        Ok(names)
    }
    pub fn backup(&self, database: &Database) -> Result<String, String> {
        let directory = self.directory.join("backups");
        std::fs::create_dir_all(&directory).map_err(|_| "Unable to create backup folder")?;
        let json = zeroize::Zeroizing::new(
            serde_json::to_string(database).map_err(|_| "Unable to serialize backup")?,
        );
        let encrypted = platform::protect(&json)?;
        let name = format!(
            "{}-{}.rolbackup",
            chrono::Utc::now().format("%Y%m%dT%H%M%S%9f"),
            uuid::Uuid::new_v4()
        );
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(&name))
            .map_err(|_| "Unable to create backup")?;
        file.write_all(encrypted.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|_| "Unable to flush backup")?;
        for old in self.backups()?.into_iter().skip(10) {
            std::fs::remove_file(directory.join(old)).map_err(|_| "Unable to rotate old backup")?;
        }
        Ok(name)
    }
    pub fn read_backup(&self, name: &str) -> Result<Database, String> {
        if !valid_backup_name(name) {
            return Err("Invalid backup name".into());
        }
        let path = self.directory.join("backups").join(name);
        if std::fs::metadata(&path)
            .map_err(|_| "Backup not found")?
            .len()
            > 16 * 1024 * 1024
        {
            return Err("Backup is too large".into());
        }
        let encrypted = std::fs::read_to_string(path).map_err(|_| "Unable to read backup")?;
        let json = platform::unprotect(&encrypted)?;
        let mut db: Database =
            serde_json::from_str(&json).map_err(|_| "Invalid backup contents")?;
        if !matches!(db.version, 1 | 2)
            || db.accounts.len() > 50
            || db.profiles.len() > 50
            || db.activity.len() > 1000
        {
            return Err("Unsupported backup schema or size".into());
        }
        let mut ids = std::collections::HashSet::new();
        for saved in &mut db.accounts {
            if !ids.insert(saved.account.id.clone()) || saved.account.id.parse::<u64>().is_err() {
                return Err("Invalid backup account IDs".into());
            }
            if let Some(t) = &saved.account.target {
                t.validate()?;
            }
            platform::unprotect(&saved.encrypted_session)?;
            let a = &mut saved.account;
            a.desired_running = false;
            a.process = None;
            a.tracker = None;
            a.ownership_deadline = None;
            a.status = crate::model::Status::Stopped;
            a.generation = uuid::Uuid::new_v4();
            a.operation_id = None;
            a.next_retry = None;
            a.connected_since = None;
            a.disconnected_since = None;
            a.failures = 0;
            a.public_fallback_active = false;
            a.last_error = None;
            a.recovery_reason.clear();
        }
        db.retired_launches.clear();
        db.version = 2;
        for profile in &db.profiles {
            if profile.name.trim().is_empty()
                || profile.name.len() > 100
                || profile.entries.is_empty()
                || profile.entries.len() > 50
            {
                return Err("Invalid backup profile".into());
            }
            let mut members = std::collections::HashSet::new();
            for e in &profile.entries {
                if !ids.contains(&e.account_id)
                    || !members.insert(&e.account_id)
                    || e.alias.trim().is_empty()
                    || e.alias.len() > 100
                    || e.group.len() > 100
                    || e.alias.chars().any(char::is_control)
                    || e.group.chars().any(char::is_control)
                {
                    return Err("Invalid backup profile membership or settings".into());
                }
                e.target
                    .as_ref()
                    .ok_or("Missing backup profile destination")?
                    .validate()?;
            }
        }
        if !db.discord.encrypted_webhook.is_empty() {
            crate::discord::validate_webhook(&platform::unprotect(&db.discord.encrypted_webhook)?)?;
        }
        Ok(db)
    }
    /// Explicit offline recovery also works when the current JSON cannot be parsed.
    pub fn recover(&self, name: &str) -> Result<(), String> {
        let mut db = self.read_backup(name)?;
        db.encrypted_api_token.clear();
        let path = self.directory.join("accounts.json");
        if path.exists() {
            let saved = self.directory.join(format!(
                "accounts-before-restore-{}.json",
                uuid::Uuid::new_v4()
            ));
            std::fs::copy(&path, saved).map_err(|_| "Unable to preserve the current database")?;
        }
        let temp = self.directory.join("accounts.restore.tmp");
        let mut file =
            std::fs::File::create(&temp).map_err(|_| "Unable to write restored database")?;
        file.write_all(
            &serde_json::to_vec_pretty(&db).map_err(|_| "Unable to serialize restored database")?,
        )
        .and_then(|_| file.sync_all())
        .map_err(|_| "Unable to flush restored database")?;
        drop(file);
        platform::replace_file(&temp, &path)
    }
}

fn valid_backup_name(name: &str) -> bool {
    name.len() == 71
        && (name.ends_with(".rolbackup") || name.ends_with(".rbxbackup"))
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renamed_app_reuses_legacy_data_despite_a_new_update_cache() {
        let root = std::env::temp_dir().join(format!("rolauncher-data-{}", uuid::Uuid::new_v4()));
        let current = root.join("RoLauncher");
        let legacy = root.join("RbxTools");
        assert_eq!(default_directory(&root), current);
        assert!(!root.exists());
        std::fs::create_dir_all(&legacy).unwrap();
        let saved = legacy.join("accounts.json");
        std::fs::write(&saved, b"existing encrypted database fixture").unwrap();
        assert_eq!(default_directory(&root), legacy);
        assert_eq!(
            std::fs::read(&saved).unwrap(),
            b"existing encrypted database fixture"
        );
        assert!(!current.exists());
        std::fs::create_dir_all(current.join("updates")).unwrap();
        std::fs::write(current.join("updates").join("download.partial"), b"fixture").unwrap();
        assert_eq!(default_directory(&root), legacy);
        assert_eq!(
            std::fs::read(&saved).unwrap(),
            b"existing encrypted database fixture"
        );
        std::fs::write(current.join("accounts.json"), b"new database fixture").unwrap();
        assert_eq!(default_directory(&root), current);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backup_names_support_the_new_brand_and_existing_backups() {
        let stem = "20261005T092000123456789-00000000-0000-0000-0000-000000000000";
        assert!(valid_backup_name(&format!("{stem}.rolbackup")));
        assert!(valid_backup_name(&format!("{stem}.rbxbackup")));
        assert!(!valid_backup_name(&format!("../{stem}.rolbackup")));
        assert!(!valid_backup_name(&format!("{stem}.json")));
    }
}
