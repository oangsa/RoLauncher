//! Root GPUI application state, navigation shell, keyboard bindings, tray integration, and lifecycle.

mod actions;

use crate::api::ApiClient;
use crate::bootstrap::Bootstrap;
use crate::components::primitives::{BannerTone, ButtonVariant, banner, button, icon, icon_button};
use crate::components::text_input::{TextInput, TextInputOptions, UiStyleGlobal};
use crate::components::thumbnail::ThumbnailCache;
use crate::dialogs;
use crate::domain::models::{
    Activity, DiscordView, ResourceHistoryPoint, ResourceSnapshotResponse, Snapshot, UpdateView,
};
use crate::pages;
use crate::platform::appearance::{
    AppearancePreference, animations_enabled, load_appearance, os_high_contrast_enabled,
    save_appearance,
};
use crate::platform::tray::NativeTray;
use crate::platform::window::{apply_window_icon, hide_to_tray, hwnd_from_window, post_close};
use crate::state::{
    ActiveModal, FeedbackMessage, GameMetadataCache, NavTab, OpenMenu, UiPreferences,
};
use crate::theme::{AppTheme, DensityTokens};
use crate::updater::downloader::{CachedInstaller, update_cache_dir};
use crate::updater::handoff::consume_recent_handoff_results;
use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, KeyBinding, Render, ScrollHandle,
    Window, actions, div, prelude::*, px,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

actions!(
    rolauncher_shell,
    [SelectAllAccounts, FocusAccountSearch, DismissOverlay]
);

/// Binds a parameterless UI control callback (`button`, `checkbox`, `modal_frame`, `banner`) to [`RoLauncherApp`].
pub(crate) fn on_action(
    cx: &Context<RoLauncherApp>,
    f: impl Fn(&mut RoLauncherApp, &mut Window, &mut Context<RoLauncherApp>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let entity = cx.entity();
    move |window, cx| {
        entity.update(cx, |this, cx| f(this, window, cx));
    }
}

/// Binds a [`segmented_control`] value-change callback to [`RoLauncherApp`].
pub(crate) fn on_select(
    cx: &Context<RoLauncherApp>,
    f: impl Fn(&mut RoLauncherApp, String, &mut Window, &mut Context<RoLauncherApp>) + 'static,
) -> impl Fn(String, &mut Window, &mut App) + 'static {
    let entity = cx.entity();
    move |value, window, cx| {
        entity.update(cx, |this, cx| f(this, value, window, cx));
    }
}

/// Registers top-level RoLauncher shell keyboard shortcuts.
pub fn bind_shell_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-a", SelectAllAccounts, Some("RoLauncherShell")),
        KeyBinding::new("ctrl-f", FocusAccountSearch, Some("RoLauncherShell")),
        KeyBinding::new("escape", DismissOverlay, Some("RoLauncherShell")),
    ]);
}

/// Primary desktop presentation state backed by the supervisor REST/SSE local API.
pub struct RoLauncherApp {
    pub bootstrap: Bootstrap,
    pub api: ApiClient,
    pub snapshot: Option<Snapshot>,
    pub discord_view: Option<DiscordView>,
    pub bot_loaded: bool,
    pub bot_enabled_draft: bool,
    pub update_view: Option<UpdateView>,
    pub cached_installer: Option<CachedInstaller>,
    pub update_checking: bool,
    pub update_downloading: bool,
    pub update_download_pct: u8,
    pub update_download_cancel: Option<tokio::sync::watch::Sender<bool>>,
    pub update_installing: bool,
    pub update_status_text: String,
    pub active_tab: NavTab,
    pub tab_anim_progress: f32,
    pub tab_anim_started: Option<Instant>,
    pub tab_scroll_handles: HashMap<NavTab, ScrollHandle>,
    pub modal: ActiveModal,
    pub open_menu: Option<OpenMenu>,
    pub busy: bool,
    pub saving: bool,
    pub exiting: Arc<AtomicBool>,
    pub network_suspended_notified: bool,
    pub prefs: UiPreferences,
    pub thumbnails: ThumbnailCache,
    pub game_cache: GameMetadataCache,
    pub selected_account_ids: HashSet<String>,
    pub selected_preset_id: Option<String>,
    pub preset_draft_account_ids: HashSet<String>,
    pub activities: Vec<Activity>,
    pub activity_loaded: bool,
    pub dashboard_account_filter: String,
    pub dashboard_activity_page: usize,
    pub dashboard_notice_page: usize,
    pub backups: Vec<String>,
    pub selected_backup: Option<String>,
    pub resource_snapshot: Option<ResourceSnapshotResponse>,
    pub resource_history: Vec<ResourceHistoryPoint>,
    pub resource_sort_by: String,
    pub resource_sort_desc: bool,
    pub resource_stream_error: Option<String>,
    pub resource_stop_tx: Option<tokio::sync::watch::Sender<bool>>,
    pub account_status_filter: String,
    pub account_rejoin_filter: String,
    pub account_group_filter: String,
    pub feedback: Option<FeedbackMessage>,
    pub account_search_input: Entity<TextInput>,
    pub cookie_batch_input: Entity<TextInput>,
    pub preset_name_input: Entity<TextInput>,
    pub discord_webhook_input: Entity<TextInput>,
    pub discord_bot_token_input: Entity<TextInput>,
    pub discord_guild_input: Entity<TextInput>,
    pub discord_users_input: Entity<TextInput>,
    pub tray: Option<NativeTray>,
    pub data_dir: PathBuf,
    pub update_cache_dir: PathBuf,
    pub focus_handle: FocusHandle,
    pub refresh_in_flight: bool,
}

impl RoLauncherApp {
    /// Initializes root shell state, text inputs, tray icon, close behavior, and pollers.
    pub fn new(
        bootstrap: Bootstrap,
        api: ApiClient,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let data_dir = PathBuf::from(&bootstrap.data_directory);
        let prefs = UiPreferences {
            appearance: load_appearance(&data_dir),
        };
        cx.set_global(UiStyleGlobal {
            appearance: prefs.appearance,
        });

        let cache_dir = resolve_update_cache_dir();
        let initial_feedback = match consume_recent_handoff_results(&cache_dir) {
            Some(result) if result.status == "failed" => Some(FeedbackMessage::new(
                BannerTone::Warning,
                format!(
                    "Previous update to v{} did not finish: {}. Running v{}.",
                    result.version,
                    if !result.message.trim().is_empty() {
                        result.message.trim().to_string()
                    } else if !result.stage.trim().is_empty() {
                        format!("stage '{}'", result.stage.trim())
                    } else {
                        "installer error".to_string()
                    },
                    bootstrap.version
                ),
                None,
            )),
            Some(result) => Some(FeedbackMessage::new(
                BannerTone::Success,
                format!("Updated to RoLauncher v{}.", result.version),
                Some(6),
            )),
            None => None,
        };

        let mut tab_scroll_handles = HashMap::new();
        for tab in NavTab::ALL {
            tab_scroll_handles.insert(tab, ScrollHandle::new());
        }

        let exiting = Arc::new(AtomicBool::new(false));
        let hwnd = hwnd_from_window(window);
        if let Some(h) = hwnd {
            apply_window_icon(h);
        }
        let tray = hwnd.and_then(|h| NativeTray::install(h, exiting.clone()));
        if let Some(h) = hwnd {
            let exiting_parent = exiting.clone();
            let h_usize = h as usize;
            crate::platform::process::watch_parent_exit(bootstrap.parent_id, move || {
                exiting_parent.store(true, Ordering::Release);
                post_close(h_usize as windows_sys::Win32::Foundation::HWND);
            });
        }

        let entity_close = cx.entity();
        window.on_window_should_close(cx, move |window, cx| {
            entity_close.update(cx, |this, cx| this.handle_window_should_close(window, cx))
        });

        let account_search_input = cx.new(|cx| {
            TextInput::new(
                cx,
                TextInputOptions {
                    placeholder: "Search accounts…".into(),
                    ..Default::default()
                },
            )
        });
        let weak_search = cx.entity().downgrade();
        account_search_input.update(cx, |ti, _cx| {
            ti.set_on_change(move |query, _window, cx| {
                let query = query.to_string();
                let _ = weak_search.update(cx, |app, cx| {
                    app.prune_hidden_selection_for_query(&query, cx);
                });
            });
        });
        let cookie_batch_input = cx.new(|cx| {
            TextInput::new(
                cx,
                TextInputOptions {
                    placeholder: "Session cookies, one per line".into(),
                    multiline: true,
                    height_px: Some(80.0),
                    max_len: Some(1_048_576),
                    ..Default::default()
                },
            )
        });
        let preset_name_input = cx.new(|cx| {
            TextInput::new(
                cx,
                TextInputOptions {
                    placeholder: "Evening team".into(),
                    max_len: Some(100),
                    ..Default::default()
                },
            )
        });
        let discord_webhook_input = cx.new(|cx| {
            TextInput::new(
                cx,
                TextInputOptions {
                    placeholder: "Discord webhook URL".into(),
                    masked: true,
                    max_len: Some(512),
                    ..Default::default()
                },
            )
        });
        let discord_bot_token_input = cx.new(|cx| {
            TextInput::new(
                cx,
                TextInputOptions {
                    placeholder: "Bot token".into(),
                    masked: true,
                    max_len: Some(256),
                    ..Default::default()
                },
            )
        });
        let discord_guild_input = cx.new(|cx| {
            TextInput::new(
                cx,
                TextInputOptions {
                    placeholder: "Server ID".into(),
                    max_len: Some(20),
                    ..Default::default()
                },
            )
        });
        let discord_users_input = cx.new(|cx| {
            TextInput::new(
                cx,
                TextInputOptions {
                    placeholder: "123456789012345678, …".into(),
                    max_len: Some(2200),
                    ..Default::default()
                },
            )
        });

        let mut app = Self {
            bootstrap,
            api,
            snapshot: None,
            discord_view: None,
            bot_loaded: false,
            bot_enabled_draft: false,
            update_view: None,
            cached_installer: None,
            update_checking: false,
            update_downloading: false,
            update_download_pct: 0,
            update_download_cancel: None,
            update_installing: false,
            update_status_text: "Automatic checks enabled.".to_string(),
            active_tab: NavTab::Accounts,
            tab_anim_progress: 1.0,
            tab_anim_started: None,
            tab_scroll_handles,
            modal: ActiveModal::None,
            open_menu: None,
            busy: false,
            saving: false,
            exiting,
            network_suspended_notified: false,
            prefs,
            thumbnails: ThumbnailCache::new(),
            game_cache: GameMetadataCache::new(),
            selected_account_ids: HashSet::new(),
            selected_preset_id: None,
            preset_draft_account_ids: HashSet::new(),
            activities: Vec::new(),
            activity_loaded: false,
            dashboard_account_filter: String::new(),
            dashboard_activity_page: 0,
            dashboard_notice_page: 0,
            backups: Vec::new(),
            selected_backup: None,
            resource_snapshot: None,
            resource_history: Vec::new(),
            resource_sort_by: "cpu".to_string(),
            resource_sort_desc: true,
            resource_stream_error: None,
            resource_stop_tx: None,
            account_status_filter: String::new(),
            account_rejoin_filter: String::new(),
            account_group_filter: String::new(),
            feedback: initial_feedback,
            account_search_input,
            cookie_batch_input,
            preset_name_input,
            discord_webhook_input,
            discord_bot_token_input,
            discord_guild_input,
            discord_users_input,
            tray,
            data_dir,
            update_cache_dir: cache_dir,
            focus_handle: cx.focus_handle(),
            refresh_in_flight: false,
        };

        app.start_background_tasks(cx);
        #[cfg(feature = "ui-smoke")]
        crate::smoke::maybe_start(window, cx);
        app
    }

    /// Resolves active palette from persisted preference, window OS theme, and Windows high-contrast flag.
    pub fn theme(&self, window: &Window) -> AppTheme {
        AppTheme::resolve(
            self.prefs.appearance,
            window.appearance(),
            os_high_contrast_enabled(),
        )
    }

    /// Returns layout spacing and typography tokens for the desktop layout.
    pub fn density(&self) -> DensityTokens {
        DensityTokens::default()
    }

    /// Creates a new configured `TextInput` child entity with optional monospace typography.
    pub fn new_text_input(
        &self,
        placeholder: &str,
        masked: bool,
        read_only: bool,
        multiline: bool,
        monospace: Option<bool>,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        let placeholder = placeholder.to_string();
        cx.new(|cx| {
            TextInput::new(
                cx,
                TextInputOptions {
                    placeholder,
                    masked,
                    read_only,
                    multiline,
                    monospace: monospace.unwrap_or(false),
                    max_len: None,
                    height_px: None,
                },
            )
        })
    }

    /// Shows an informational banner above workspace pages.
    pub fn show_info(
        &mut self,
        text: impl Into<String>,
        secs: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        self.feedback = Some(FeedbackMessage::new(BannerTone::Success, text, secs));
        cx.notify();
    }

    /// Shows an error banner above workspace pages.
    pub fn show_error(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.feedback = Some(FeedbackMessage::new(BannerTone::Danger, text, Some(8)));
        cx.notify();
    }

    /// Closes any active modal and explicitly scrubs secret fields held by modals.
    pub fn close_modal_clearing_secrets(&mut self, cx: &mut Context<Self>) {
        if let ActiveModal::BulkEditor(bm) = &mut self.modal {
            bm.private_input.update(cx, |ti, cx| ti.set_text("", cx));
        }
        if let ActiveModal::GameEditor(ge) = &mut self.modal {
            ge.private_input.update(cx, |ti, cx| ti.set_text("", cx));
        }
        if let ActiveModal::AccountEditor(ae) = &mut self.modal {
            ae.private_input.update(cx, |ti, cx| ti.set_text("", cx));
        }
        if let ActiveModal::Token(tm) = &mut self.modal {
            tm.token_input.update(cx, |ti, cx| ti.set_text("", cx));
        }
        self.modal = ActiveModal::None;
        cx.notify();
    }

    /// Persists appearance preference (`System` / `Light` / `Dark`) to `<data_dir>/appearance.txt`.
    pub fn set_appearance(
        &mut self,
        pref: AppearancePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prefs.appearance = pref;
        cx.set_global(UiStyleGlobal {
            appearance: self.prefs.appearance,
        });
        if let Err(err) = save_appearance(&self.data_dir, pref) {
            self.feedback = Some(FeedbackMessage::new(BannerTone::Warning, err, Some(5)));
        }
        window.refresh();
        cx.notify();
    }

    /// Selects a navigation tab, updates SSE stream subscription, and triggers restrained entrance motion.
    pub fn switch_tab(&mut self, tab: NavTab, cx: &mut Context<Self>) {
        if self.active_tab == tab {
            return;
        }
        self.open_menu = None;
        self.active_tab = tab;
        if let Some(handle) = self.tab_scroll_handles.get(&tab) {
            handle.set_offset(gpui::point(px(0.0), px(0.0)));
        }
        self.ensure_resource_stream(cx);

        if tab == NavTab::Dashboard {
            self.refresh_activity(cx);
        } else if tab == NavTab::Backups {
            self.refresh_backups(cx);
        }

        if animations_enabled() {
            self.tab_anim_progress = 0.0;
            self.tab_anim_started = Some(Instant::now());
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                    let keep_going = this
                        .update(cx, |app, cx| {
                            let Some(started) = app.tab_anim_started else {
                                app.tab_anim_progress = 1.0;
                                return false;
                            };
                            let elapsed = started.elapsed().as_secs_f32() / 0.14;
                            if elapsed >= 1.0 {
                                app.tab_anim_progress = 1.0;
                                app.tab_anim_started = None;
                                cx.notify();
                                false
                            } else {
                                app.tab_anim_progress = 1.0 - (1.0 - elapsed).powi(3);
                                cx.notify();
                                true
                            }
                        })
                        .unwrap_or(false);
                    if !keep_going {
                        break;
                    }
                }
            })
            .detach();
        } else {
            self.tab_anim_progress = 1.0;
            self.tab_anim_started = None;
        }
        cx.notify();
    }

    /// Initiates clean shutdown across tray, SSE streams, and window loop.
    pub fn request_exit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.exiting.load(Ordering::Acquire) || self.saving {
            return;
        }
        self.exiting.store(true, Ordering::Release);
        self.cleanup_on_exit();
        window.remove_window();
        cx.quit();
    }

    fn handle_window_should_close(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> bool {
        if self.exiting.load(Ordering::Acquire) {
            self.cleanup_on_exit();
            return true;
        }
        let close_to_tray = self.snapshot.as_ref().is_none_or(|s| s.close_to_tray);
        if close_to_tray && self.tray.is_some() {
            if let Some(hwnd) = hwnd_from_window(window) {
                hide_to_tray(hwnd);
            }
            return false;
        }
        self.exiting.store(true, Ordering::Release);
        self.cleanup_on_exit();
        true
    }

    pub(crate) fn cleanup_on_exit(&mut self) {
        self.stop_resource_stream();
        if let Some(tx) = self.update_download_cancel.take() {
            let _ = tx.send(true);
        }
        if let Some(mut tray) = self.tray.take() {
            tray.remove();
        }
    }

    fn on_select_all_action(
        &mut self,
        _: &SelectAllAccounts,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab == NavTab::Accounts && matches!(self.modal, ActiveModal::None) {
            self.select_all_accounts(cx);
        }
    }

    fn on_focus_search_action(
        &mut self,
        _: &FocusAccountSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &self.modal {
            ActiveModal::PresetLookup(pl) => {
                let search = pl.search_input.clone();
                search.update(cx, |ti, cx| ti.focus_and_select_all(window, cx));
            }
            ActiveModal::None => {
                if self.active_tab != NavTab::Accounts {
                    self.switch_tab(NavTab::Accounts, cx);
                }
                let search = self.account_search_input.clone();
                search.update(cx, |ti, cx| ti.focus_and_select_all(window, cx));
            }
            _ => {}
        }
    }

    fn on_dismiss_overlay_action(
        &mut self,
        _: &DismissOverlay,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.open_menu.is_some() {
            self.open_menu = None;
            cx.notify();
            return;
        }
        if !matches!(self.modal, ActiveModal::None) && !self.saving {
            self.close_modal_clearing_secrets(cx);
        }
    }

    fn render_sidebar(
        &self,
        theme: &AppTheme,
        density: &DensityTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut nav = div().flex().flex_col().gap(px(4.0));
        let mut last_section = "";
        for (idx, tab) in NavTab::ALL.iter().copied().enumerate() {
            let section = tab.section();
            if section != last_section {
                last_section = section;
                nav = nav.child(
                    div()
                        .pt(px(16.0))
                        .pb(px(8.0))
                        .px(px(12.0))
                        .text_size(px(density.font_caption))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_muted)
                        .child(section),
                );
            }
            let active = self.active_tab == tab;
            nav = nav.child(
                div()
                    .id(("nav-tab", idx))
                    .tab_index((idx + 1) as isize)
                    .tab_stop(true)
                    .relative()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .h(px(density.control_h))
                    .px(px(13.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(if active {
                        theme.border_subtle
                    } else {
                        gpui::transparent_black()
                    })
                    .when(active, |row| row.bg(theme.bg_selected))
                    .hover(|row| {
                        if active {
                            row
                        } else {
                            row.bg(theme.bg_surface_hover)
                        }
                    })
                    .focus(|row| row.border_color(theme.border_focus))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.switch_tab(tab, cx)))
                    .when(active, |row| {
                        row.child(
                            div()
                                .absolute()
                                .left(px(3.0))
                                .top(gpui::relative(0.5))
                                .mt(px(-9.0))
                                .w(px(3.0))
                                .h(px(18.0))
                                .rounded(px(2.0))
                                .bg(theme.nav_marker),
                        )
                    })
                    .child(
                        div()
                            .w(px(20.0))
                            .flex()
                            .justify_center()
                            .child(icon(tab.glyph(), 16.0)),
                    )
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(if active {
                                gpui::FontWeight::SEMIBOLD
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .child(tab.label()),
                    ),
            );
        }

        div()
            .w(px(density.sidebar_width))
            .flex_shrink_0()
            .h_full()
            .p(px(12.0))
            .child(
                div()
                    .id("sidebar-nav-scroll")
                    .h_full()
                    .px(px(10.0))
                    .py(px(12.0))
                    .rounded(px(16.0))
                    .border_1()
                    .border_color(theme.border_subtle)
                    .bg(theme.bg_sidebar)
                    .overflow_y_scroll()
                    .child(nav),
            )
            .into_any_element()
    }

    fn render_update_notice(
        &self,
        theme: &AppTheme,
        density: &DensityTokens,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let update = self.update_view.as_ref().filter(|u| u.available)?;
        let action_label = if self.update_installing {
            "Installing…"
        } else if self.update_downloading {
            "Cancel download"
        } else if self.cached_installer.is_some() {
            "Relaunch"
        } else {
            "Download"
        };
        let detail = if self.update_downloading {
            format!("Downloading… {}%", self.update_download_pct)
        } else if self.cached_installer.is_some() {
            "Verified. Relaunch to install; running games stay open.".to_string()
        } else {
            "Download, then Relaunch to install.".to_string()
        };

        Some(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(10.0))
                .px(px(14.0))
                .py(px(10.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(theme.info_border)
                .bg(theme.info_bg)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(density.font_body))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme.text_primary)
                                .child(format!(
                                    "RoLauncher {}{} available",
                                    update.version,
                                    if update.prerelease { " beta" } else { "" }
                                )),
                        )
                        .child(
                            div()
                                .text_size(px(density.font_caption))
                                .text_color(theme.text_secondary)
                                .child(detail),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(button(
                            "update-notice-action",
                            action_label,
                            if self.update_downloading {
                                ButtonVariant::Danger
                            } else {
                                ButtonVariant::Primary
                            },
                            !self.update_downloading
                                && (self.update_checking || self.update_installing),
                            true,
                            theme,
                            density,
                            on_action(cx, |this, window, cx| {
                                this.handle_update_primary_action(window, cx);
                            }),
                        ))
                        .child(button(
                            "update-notice-tab",
                            "Updates tab",
                            ButtonVariant::Secondary,
                            false,
                            true,
                            theme,
                            density,
                            on_action(cx, |this, _w, cx| {
                                this.switch_tab(NavTab::Updates, cx);
                            }),
                        )),
                )
                .into_any_element(),
        )
    }
}

impl Focusable for RoLauncherApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for RoLauncherApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme(window);
        let density = self.density();
        let active_tab = self.active_tab;
        let scroll_handle = self
            .tab_scroll_handles
            .entry(active_tab)
            .or_default()
            .clone();

        let anim_p = self.tab_anim_progress.clamp(0.0, 1.0);
        let slide_y = (1.0 - anim_p) * 6.0;
        let opacity = 0.78 + 0.22 * anim_p;

        let sidebar = self.render_sidebar(&theme, &density, cx);
        let update_notice = self.render_update_notice(&theme, &density, cx);
        let feedback_banner = self.feedback.as_ref().map(|fb| {
            banner(
                "main-feedback-banner",
                fb.tone,
                fb.text.clone(),
                None,
                Some(Box::new(on_action(cx, |this, _w, cx| {
                    this.feedback = None;
                    cx.notify();
                }))),
                &theme,
                &density,
            )
        });

        let page_content = pages::render_active_page(self, window, cx);
        let modal_overlay = dialogs::render_active_modal(self, window, cx);

        let main_col = div()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(28.0))
                    .pt(px(22.0))
                    .pb(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(16.0))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(28.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("RoLauncher"),
                    )
                    .child(
                        div()
                            .text_size(px(density.font_caption))
                            .text_color(theme.text_muted)
                            .child(format!("v{}", self.bootstrap.version)),
                    )
                    .child(icon_button(
                        "whats-new",
                        "\u{e946}",
                        "What’s new",
                        ButtonVariant::Secondary,
                        false,
                        false,
                        &theme,
                        &density,
                        on_action(cx, |this, _, cx| {
                            this.open_menu = None;
                            this.modal = ActiveModal::Changelog;
                            cx.notify();
                        }),
                    )),
            )
            .when_some(update_notice, |main, notice| {
                main.child(div().px(px(28.0)).pb(px(18.0)).child(notice))
            })
            .when_some(feedback_banner, |main, feedback| {
                main.child(div().px(px(28.0)).pb(px(12.0)).child(feedback))
            })
            .child(
                div()
                    .id("main-page-scroll")
                    .track_scroll(&scroll_handle)
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .px(px(28.0))
                    .pb(px(8.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(density.section_gap))
                            .child(div().pt(px(slide_y)).opacity(opacity).child(page_content)),
                    ),
            );

        div()
            .id("rolauncher-shell")
            .key_context("RoLauncherShell")
            .track_focus(&self.focus_handle)
            .relative()
            .flex()
            .flex_row()
            .size_full()
            .bg(theme.bg_app)
            .text_color(theme.text_primary)
            .font_family("Segoe UI Variable Text")
            .text_size(px(density.font_body))
            .on_action(cx.listener(Self::on_select_all_action))
            .on_action(cx.listener(Self::on_focus_search_action))
            .on_action(cx.listener(Self::on_dismiss_overlay_action))
            .on_click(cx.listener(|this, _, _, cx| {
                if matches!(
                    this.open_menu,
                    Some(
                        OpenMenu::SettingsTheme
                            | OpenMenu::SettingsClose
                            | OpenMenu::SettingsJoinType
                            | OpenMenu::SettingsServer
                            | OpenMenu::SettingsBackup
                    )
                ) {
                    this.open_menu = None;
                    cx.notify();
                }
            }))
            .child(sidebar)
            .child(main_col)
            .when_some(modal_overlay, |root, modal| root.child(modal))
    }
}

fn resolve_update_cache_dir() -> PathBuf {
    if let Ok(smoke_dir) = std::env::var("ROLAUNCHER_UI_SMOKE_DIR") {
        let trimmed = smoke_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("update-cache");
        }
    }
    update_cache_dir().unwrap_or_else(|_| std::env::temp_dir().join("RoLauncher").join("updates"))
}
