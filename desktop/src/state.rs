//! Shared application navigation, modal state, menus, and metadata cache structures.

use crate::components::primitives::BannerTone;
use crate::components::text_input::TextInput;
use crate::domain::models::{GameLookupResponse, LaunchProfile};
use crate::platform::appearance::AppearancePreference;
use gpui::Entity;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// Top-level navigation destinations mirroring WinUI grouped sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavTab {
    Accounts,
    Games,
    Dashboard,
    Resources,
    Presets,
    General,
    Joining,
    Discord,
    Backups,
    Updates,
    Support,
}

impl NavTab {
    /// Ordered navigation items across the fixed sidebar.
    pub const ALL: [NavTab; 11] = [
        NavTab::Accounts,
        NavTab::Games,
        NavTab::Dashboard,
        NavTab::Resources,
        NavTab::Presets,
        NavTab::General,
        NavTab::Joining,
        NavTab::Discord,
        NavTab::Backups,
        NavTab::Updates,
        NavTab::Support,
    ];

    /// Sidebar group heading this destination belongs to.
    pub fn section(self) -> &'static str {
        match self {
            Self::Accounts | Self::Games | Self::Dashboard | Self::Resources => "WORKSPACE",
            Self::Presets => "ACCOUNTS",
            Self::General | Self::Joining | Self::Discord | Self::Backups | Self::Updates => {
                "SETTINGS"
            }
            Self::Support => "TOOLS",
        }
    }

    /// Sidebar and header title.
    pub fn label(self) -> &'static str {
        match self {
            Self::Accounts => "Accounts",
            Self::Games => "Games",
            Self::Dashboard => "Dashboard",
            Self::Resources => "Resources",
            Self::Presets => "Account presets",
            Self::General => "General",
            Self::Joining => "Game joining",
            Self::Discord => "Discord",
            Self::Backups => "Backups",
            Self::Updates => "Updates",
            Self::Support => "Support",
        }
    }

    /// Segoe Fluent Icons glyphs used by the desktop navigation.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Accounts => "\u{e716}",
            Self::Games => "\u{e7fc}",
            Self::Dashboard => "\u{e9d9}",
            Self::Resources => "\u{e9d2}",
            Self::Presets | Self::Backups => "\u{e8f1}",
            Self::General => "\u{e713}",
            Self::Joining => "\u{e768}",
            Self::Discord => "\u{e8f2}",
            Self::Updates => "\u{e895}",
            Self::Support => "\u{e90f}",
        }
    }
}

/// Auto-dismissing or sticky status banner shown above page content.
#[derive(Debug, Clone)]
pub struct FeedbackMessage {
    pub tone: BannerTone,
    pub text: String,
    pub expires_at: Option<Instant>,
}

impl FeedbackMessage {
    pub fn new(tone: BannerTone, text: impl Into<String>, auto_dismiss_secs: Option<u64>) -> Self {
        Self {
            tone,
            text: text.into(),
            expires_at: auto_dismiss_secs.map(|s| Instant::now() + Duration::from_secs(s)),
        }
    }
}

/// Dropdown menu currently open in the desktop shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMenu {
    AccountsMore,
    AccountsSelection,
    AccountStatus,
    AccountRejoin,
    AccountGroup,
    PresetsMore,
    LookupGroup,
    LookupStatus,
    LookupRejoin,
    SettingsTheme,
    SettingsClose,
    SettingsJoinType,
    SettingsServer,
    SettingsBackup,
}

/// Currently active modal dialog overlay.
pub enum ActiveModal {
    None,
    AccountEditor(AccountEditorModal),
    BulkEditor(BulkEditorModal),
    GameEditor(GameEditorModal),
    PresetLookup(PresetLookupModal),
    PresetReview(PresetReviewModal),
    Changelog,
    Token(TokenModal),
    Confirm(ConfirmModal),
}

/// Fields and entities held while editing a single account.
pub struct AccountEditorModal {
    pub account_id: String,
    pub username: String,
    pub alias_input: Entity<TextInput>,
    pub place_input: Entity<TextInput>,
    pub job_input: Entity<TextInput>,
    pub private_input: Entity<TextInput>,
    pub group_input: Entity<TextInput>,
    pub game_profile_id: Option<String>,
    pub auto_recovery: bool,
    pub fallback_policy: String,
    pub lookup_preview: Option<GameLookupResponse>,
    pub error: Option<String>,
}

/// Fields and entities held while editing multiple selected accounts.
pub struct BulkEditorModal {
    pub account_ids: Vec<String>,
    pub account_labels: Vec<String>,
    pub show_included: bool,
    pub apply_alias: bool,
    pub alias_input: Entity<TextInput>,
    pub apply_target: bool,
    pub clear_target: bool,
    pub game_profile_id: Option<String>,
    pub place_input: Entity<TextInput>,
    pub job_input: Entity<TextInput>,
    pub private_input: Entity<TextInput>,
    pub apply_rejoin: bool,
    pub rejoin: bool,
    pub apply_fallback: bool,
    pub fallback: String,
    pub apply_group: bool,
    pub group_input: Entity<TextInput>,
    pub error: Option<String>,
}

/// Fields held while creating or editing a saved game profile.
pub struct GameEditorModal {
    pub id: String,
    pub is_new: bool,
    pub name_input: Entity<TextInput>,
    pub place_input: Entity<TextInput>,
    pub job_input: Entity<TextInput>,
    pub private_input: Entity<TextInput>,
    pub lookup: Option<GameLookupResponse>,
    pub error: Option<String>,
}

/// Interactive account picker modal used while building a preset.
pub struct PresetLookupModal {
    pub search_input: Entity<TextInput>,
    pub status_filter: String,
    pub rejoin_filter: String,
    pub group_filter: Option<String>,
    pub chosen_ids: HashSet<String>,
}

/// Detailed pre-launch confirmation dialog for an account preset.
pub struct PresetReviewModal {
    pub preset: LaunchProfile,
}

/// Controlled Local API token inspection modal (token explicitly zeroized on close).
pub struct TokenModal {
    pub token_input: Entity<TextInput>,
    pub raw_token: Zeroizing<String>,
}

/// Reusable confirmation dialog action identifier.
#[derive(Debug, Clone)]
pub enum ConfirmAction {
    DeleteSelectedAccounts(Vec<String>),
    DeleteSingleAccount(String),
    StopAllRoblox,
    DeleteGameProfile(String),
    DeletePreset(String),
    ExportPresetsNotice,
    RestoreBackup(String),
    InstallUpdate,
}

/// Reusable yes/no confirmation modal state.
#[derive(Debug, Clone)]
pub struct ConfirmModal {
    pub title: String,
    pub body: String,
    pub confirm_label: String,
    pub dangerous: bool,
    pub action: ConfirmAction,
}

/// Cached experience metadata for place IDs referenced by accounts and saved games.
pub struct GameMetadataCache {
    pub resolved: HashMap<u64, GameLookupResponse>,
    pub failed_at: HashMap<u64, Instant>,
    pub in_flight: HashSet<u64>,
}

impl GameMetadataCache {
    pub fn new() -> Self {
        Self {
            resolved: HashMap::new(),
            failed_at: HashMap::new(),
            in_flight: HashSet::new(),
        }
    }

    pub fn should_fetch(&self, place_id: u64) -> bool {
        if place_id == 0
            || self.resolved.contains_key(&place_id)
            || self.in_flight.contains(&place_id)
        {
            return false;
        }
        if self.in_flight.len() >= 4 {
            return false;
        }
        if let Some(failed) = self.failed_at.get(&place_id) {
            return failed.elapsed() >= Duration::from_secs(300);
        }
        true
    }

    pub fn display_name(&self, place_id: u64) -> Option<&str> {
        self.resolved
            .get(&place_id)
            .map(|r| r.name.trim())
            .filter(|n| !n.is_empty())
    }
}

/// Persisted UI preferences synced with `<data_directory>/appearance.txt`.
#[derive(Debug, Clone, Copy)]
pub struct UiPreferences {
    pub appearance: AppearancePreference,
}
