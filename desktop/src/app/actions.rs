//! Asynchronous API requests, polling, SSE subscriptions, file dialogs, and update handoff actions.

use crate::app::RoLauncherApp;
use crate::components::primitives::BannerTone;
use crate::components::thumbnail::ThumbnailCache;
use crate::dialogs::{account_editor, bulk_editor, game_editor, token};
use crate::domain::cookie_batch::{import_batches, parse_cookie_lines};
use crate::domain::drafts::EditorDraft;
use crate::domain::filter::matches_account;
use crate::domain::models::{LaunchProfile, ResourceSnapshotResponse, Snapshot, can_retry};
use crate::domain::resources::merge_history;
use crate::state::{ActiveModal, ConfirmAction, FeedbackMessage, NavTab};
use crate::updater::downloader::{
    ReqwestUpdateTransport, UpdateTransport, cached_verified_installer, download_verified_installer,
};
use crate::updater::handoff::start_update_handoff;
use gpui::{Context, PathPromptOptions, Window};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const MAX_IMPORT_FILE_BYTES: u64 = 1024 * 1024;

impl RoLauncherApp {
    /// Starts periodic polling (`1s`), initial refresh, and 6-hour automatic update checks.
    pub(super) fn start_background_tasks(&mut self, cx: &mut Context<Self>) {
        self.refresh_all(cx);

        // Periodic status + banner expiration ticker.
        cx.spawn(async move |this, cx| {
            let mut tick: u64 = 0;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let keep_going = this
                    .update(cx, |app, cx| {
                        if app.exiting.load(Ordering::Acquire) {
                            return false;
                        }
                        if app
                            .feedback
                            .as_ref()
                            .and_then(|f| f.expires_at)
                            .is_some_and(|exp| Instant::now() >= exp)
                        {
                            app.feedback = None;
                            cx.notify();
                        }
                        tick = tick.wrapping_add(1);
                        app.refresh_snapshot_tick(tick.is_multiple_of(5), cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();

        // Startup update check and 6-hour recurring check loop.
        cx.spawn(async move |this, cx| {
            let _ = this.update(cx, |app, cx| app.check_updates(false, cx));
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(6 * 3600))
                    .await;
                let keep_going = this
                    .update(cx, |app, cx| {
                        if app.exiting.load(Ordering::Acquire) {
                            return false;
                        }
                        app.check_updates(false, cx);
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }

    /// Triggers an initial full load of status, Discord state, backups, and activity.
    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.refresh_snapshot_tick(true, cx);
        self.refresh_backups(cx);
        self.refresh_activity(cx);
    }

    fn refresh_snapshot_tick(
        &mut self,
        include_activity_on_dashboard: bool,
        cx: &mut Context<Self>,
    ) {
        if self.refresh_in_flight || self.exiting.load(Ordering::Acquire) {
            return;
        }
        self.refresh_in_flight = true;
        let api = self.api.clone();
        let on_dashboard = self.active_tab == NavTab::Dashboard;
        cx.spawn(async move |this, cx| {
            let status_res = api.status().await;
            let discord_res = api.discord_settings().await;
            let activity_res = if include_activity_on_dashboard && on_dashboard {
                Some(api.activity(None).await)
            } else {
                None
            };

            let _ = this.update(cx, |app, cx| {
                app.refresh_in_flight = false;
                if let Ok(snap) = status_res {
                    app.apply_snapshot(snap, cx);
                }
                if let Ok(discord) = discord_res {
                    let sync_fields = !app.bot_loaded;
                    if let Some(bot) = discord.bot.as_ref().filter(|_| sync_fields) {
                        app.bot_enabled_draft = bot.enabled;
                        let guild = bot.guild_id.clone();
                        let users = bot.allowed_users.join(", ");
                        app.discord_guild_input
                            .update(cx, |ti, cx| ti.set_text(guild, cx));
                        app.discord_users_input
                            .update(cx, |ti, cx| ti.set_text(users, cx));
                        app.bot_loaded = true;
                    }
                    app.discord_view = Some(discord);
                }
                if let Some(Ok(acts)) = activity_res {
                    app.activities = acts;
                    app.activity_loaded = true;
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_snapshot(&mut self, snap: Snapshot, cx: &mut Context<Self>) {
        if snap.network_suspended && !self.network_suspended_notified {
            self.feedback = Some(FeedbackMessage::new(
                BannerTone::Warning,
                "No connection. Launches and rejoins paused.",
                Some(8),
            ));
        }
        self.network_suspended_notified = snap.network_suspended;

        let valid_ids: HashSet<String> = snap.accounts.iter().map(|a| a.id.clone()).collect();
        self.selected_account_ids
            .retain(|id| valid_ids.contains(id));
        self.preset_draft_account_ids
            .retain(|id| valid_ids.contains(id));

        if let Some(sel) = &self.selected_preset_id {
            if !snap.profiles.iter().any(|p| &p.id == sel) {
                self.selected_preset_id = snap.profiles.first().map(|p| p.id.clone());
            }
        } else if let Some(first) = snap.profiles.first() {
            self.selected_preset_id = Some(first.id.clone());
        }

        // Seed game metadata cache from saved game profiles and trigger missing lookups (max 4 concurrent).
        for gp in &snap.game_profiles {
            if !gp.game_name.trim().is_empty() {
                self.game_cache.resolved.insert(
                    gp.target.place_id,
                    crate::domain::models::GameLookupResponse {
                        name: gp.game_name.clone(),
                        thumbnail_url: gp.thumbnail_url.clone(),
                    },
                );
            }
        }
        let missing_places: Vec<u64> = snap
            .accounts
            .iter()
            .filter_map(|a| a.target.as_ref().map(|t| t.place_id))
            .filter(|&pid| self.game_cache.should_fetch(pid))
            .collect();
        for pid in missing_places.into_iter().take(4) {
            if self.game_cache.should_fetch(pid) {
                self.fetch_game_metadata_cached(pid, cx);
            }
        }

        self.snapshot = Some(snap);
        self.prune_hidden_selection(cx);
    }

    fn fetch_game_metadata_cached(&mut self, place_id: u64, cx: &mut Context<Self>) {
        if !self.game_cache.in_flight.insert(place_id) {
            return;
        }
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            let res = api.game_details(place_id).await;
            let _ = this.update(cx, |app, cx| {
                app.game_cache.in_flight.remove(&place_id);
                match res {
                    Ok(meta) if !meta.name.trim().is_empty() => {
                        app.game_cache.resolved.insert(place_id, meta);
                    }
                    _ => {
                        app.game_cache.failed_at.insert(place_id, Instant::now());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Starts or stops the supervisor `/v1/resources/events` SSE subscription based on `active_tab`.
    pub fn ensure_resource_stream(&mut self, cx: &mut Context<Self>) {
        if self.active_tab == NavTab::Resources {
            if self.resource_stop_tx.is_some() {
                return;
            }
            enum ResourceStreamMsg {
                Snapshot(Box<ResourceSnapshotResponse>),
                Status(Option<String>),
            }

            let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
            let (msg_tx, mut msg_rx) = tokio::sync::mpsc::unbounded_channel();
            self.resource_stop_tx = Some(stop_tx);
            self.resource_stream_error = None;
            let api = self.api.clone();
            let rt = api.runtime_handle();

            rt.spawn(async move {
                let mut delay = Duration::from_secs(1);
                loop {
                    if *stop_rx.borrow() {
                        break;
                    }
                    let snap_tx = msg_tx.clone();
                    let status_tx = msg_tx.clone();
                    api.stream_resources_events(
                        stop_rx.clone(),
                        move |snap| {
                            let _ = snap_tx.send(ResourceStreamMsg::Snapshot(Box::new(snap)));
                        },
                        move |status| {
                            let _ = status_tx.send(ResourceStreamMsg::Status(status));
                        },
                    )
                    .await;

                    if *stop_rx.borrow() {
                        break;
                    }
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(10));
                }
            });

            cx.spawn(async move |this, cx| {
                while let Some(msg) = msg_rx.recv().await {
                    let alive = this
                        .update(cx, |app, cx| match msg {
                            ResourceStreamMsg::Snapshot(snap) => {
                                app.resource_history =
                                    merge_history(&app.resource_history, &snap, 60);
                                app.resource_snapshot = Some(*snap);
                                app.resource_stream_error = None;
                                cx.notify();
                            }
                            ResourceStreamMsg::Status(status) => {
                                app.resource_stream_error = status;
                                cx.notify();
                            }
                        })
                        .is_ok();
                    if !alive {
                        break;
                    }
                }
            })
            .detach();
        } else {
            self.stop_resource_stream();
        }
    }

    pub(super) fn stop_resource_stream(&mut self) {
        if let Some(tx) = self.resource_stop_tx.take() {
            let _ = tx.send(true);
        }
    }

    /// Triggers validated HTTPS thumbnail loading if `url` is not yet cached or in flight.
    pub fn ensure_thumbnail_loaded(&mut self, url: String, cx: &mut Context<Self>) {
        if url.trim().is_empty()
            || !ThumbnailCache::is_allowed_thumbnail_url(&url)
            || self.thumbnails.get(&url).is_some()
        {
            return;
        }
        let rt = self.api.runtime_handle();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.thumbnails
            .request_load(url, &rt, move |done_url, image| {
                let _ = tx.send((done_url, image));
            });
        cx.spawn(async move |this, cx| {
            if let Ok((done_url, image)) = rx.await {
                let _ = this.update(cx, |app, cx| {
                    app.thumbnails.complete(done_url, image);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Refreshes resource metrics immediately via REST (`GET /v1/resources?history=true&seconds=60`).
    pub fn refresh_resources_once(&mut self, cx: &mut Context<Self>) {
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            let res = api.resources(true, 60).await;
            let _ = this.update(cx, |app, cx| {
                match res {
                    Ok(snap) => {
                        app.resource_history = merge_history(&app.resource_history, &snap, 60);
                        app.resource_snapshot = Some(snap);
                        app.resource_stream_error = None;
                    }
                    Err(err) => {
                        app.resource_stream_error = Some(err);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Deselects any account hidden by active search, status, rejoin, or group filters.
    pub fn prune_hidden_selection(&mut self, cx: &mut Context<Self>) {
        let query = self.account_search_input.read(cx).text().to_string();
        self.prune_hidden_selection_for_query(&query, cx);
    }

    pub(crate) fn prune_hidden_selection_for_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(snap) = &self.snapshot else {
            return;
        };
        let status = &self.account_status_filter;
        let rejoin = &self.account_rejoin_filter;
        let group = &self.account_group_filter;
        let group_opt = (!group.is_empty() && group != "__all__").then_some(group.as_str());
        let visible_ids: HashSet<&str> = snap
            .accounts
            .iter()
            .filter(|a| matches_account(a, query, status, rejoin, group_opt))
            .map(|a| a.id.as_str())
            .collect();
        let before = self.selected_account_ids.len();
        self.selected_account_ids
            .retain(|id| visible_ids.contains(id.as_str()));
        if self.selected_account_ids.len() != before {
            cx.notify();
        }
    }

    /// Selects every visible account matching current Accounts search & filter controls.
    pub fn select_visible_accounts(&mut self, cx: &mut Context<Self>) {
        let query = self.account_search_input.read(cx).text().to_string();
        let status = self.account_status_filter.clone();
        let rejoin = self.account_rejoin_filter.clone();
        let group = self.account_group_filter.clone();
        let group_opt = (!group.is_empty() && group != "__all__").then_some(group.as_str());
        let Some(snap) = &self.snapshot else {
            return;
        };
        let visible_ids: Vec<String> = snap
            .accounts
            .iter()
            .filter(|a| matches_account(a, &query, &status, &rejoin, group_opt))
            .map(|a| a.id.clone())
            .collect();
        self.selected_account_ids.clear();
        for id in visible_ids {
            self.selected_account_ids.insert(id);
        }
        cx.notify();
    }

    /// Selects every account in the workspace.
    pub fn select_all_accounts(&mut self, cx: &mut Context<Self>) {
        let Some(snap) = &self.snapshot else {
            return;
        };
        self.selected_account_ids.clear();
        for a in &snap.accounts {
            self.selected_account_ids.insert(a.id.clone());
        }
        cx.notify();
    }

    /// Opens single-account or bulk editor depending on selection count.
    pub fn open_selected_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snap) = self.snapshot.clone() else {
            return;
        };
        let chosen: Vec<String> = snap
            .accounts
            .iter()
            .filter(|a| self.selected_account_ids.contains(&a.id))
            .map(|a| a.id.clone())
            .collect();
        if chosen.is_empty() {
            self.show_info("Select an account first.", Some(4), cx);
            return;
        }
        self.open_menu = None;
        if chosen.len() == 1 {
            account_editor::open_for_account(self, &chosen[0], window, cx);
        } else {
            bulk_editor::open_for_ids(self, chosen, window, cx);
        }
    }

    /// Opens the WebView2 sign-in window (`POST /v1/login`).
    pub fn start_browser_login(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            let res = api.browser_login().await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(()) => {
                        app.show_info("Sign in to Roblox in the opened browser.", Some(6), cx)
                    }
                    Err(e) => app.show_error(e, cx),
                }
            });
        })
        .detach();
    }

    /// Imports newline-separated `.ROBLOSECURITY` cookies from the cookie batch textbox.
    pub fn submit_cookie_textbox_import(&mut self, cx: &mut Context<Self>) {
        let raw = self.cookie_batch_input.read(cx).text().to_string();
        let cookies = parse_cookie_lines(&raw);
        if cookies.is_empty() {
            self.show_info(
                "Paste one .ROBLOSECURITY cookie per line first.",
                Some(4),
                cx,
            );
            return;
        }
        self.run_cookie_import(cookies, true, cx);
    }

    /// Opens native `.txt` picker (`<= 1 MiB`), parses cookies, and imports in batches of 50.
    pub fn import_cookies_from_file(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose cookie text file (.txt)".into()),
        });
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let len = std::fs::metadata(&path)
                .map(|m| m.len())
                .unwrap_or(u64::MAX);
            if len > MAX_IMPORT_FILE_BYTES {
                entity.update(cx, |app, cx| {
                    app.show_error("Cookie file is too large (max 1 MiB).", cx);
                });
                return;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let cookies = parse_cookie_lines(&text);
                    if cookies.is_empty() {
                        entity.update(cx, |app, cx| {
                            app.show_info("No cookies found in selected file.", Some(4), cx);
                        });
                        return;
                    }
                    entity.update(cx, |app, cx| {
                        app.run_cookie_import(cookies, false, cx);
                    });
                }
                Err(err) => {
                    entity.update(cx, |app, cx| {
                        app.show_error(format!("Unable to read cookie file: {err}"), cx);
                    });
                }
            }
        })
        .detach();
    }

    pub(crate) fn run_cookie_import(
        &mut self,
        cookies: Vec<Zeroizing<String>>,
        from_textbox: bool,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            let result = import_batches(cookies, |batch| {
                let api = api.clone();
                async move { api.import_cookies_batch(batch).await }
            })
            .await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                if from_textbox && result.errors.is_empty() {
                    app.cookie_batch_input
                        .update(cx, |ti, cx| ti.set_text("", cx));
                }
                if result.errors.is_empty() {
                    app.show_info(
                        format!("Imported {} account(s).", result.imported),
                        Some(5),
                        cx,
                    );
                } else {
                    app.show_error(
                        format!(
                            "Imported {} account(s); {} failed ({})",
                            result.imported,
                            result.errors.len(),
                            result.errors.first().cloned().unwrap_or_default()
                        ),
                        cx,
                    );
                }
                app.refresh_snapshot_tick(false, cx);
            });
        })
        .detach();
    }

    /// Runs `start` / `stop` / `restart` / `retry` for a single account ID.
    pub fn run_account_command(&mut self, id: &str, action: &str, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let ids = vec![id.to_string()];
        self.dispatch_account_commands(ids, action, cx);
    }

    /// Runs `start` / `stop` / `restart` across all currently selected accounts.
    pub fn run_selected_account_command(&mut self, action: &str, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let ids: Vec<String> = self.selected_account_ids.iter().cloned().collect();
        if ids.is_empty() {
            self.show_info("Select at least one account first.", Some(4), cx);
            return;
        }
        self.open_menu = None;
        self.dispatch_account_commands(ids, action, cx);
    }

    /// Retries selected accounts currently waiting in retry backoff.
    pub fn run_selected_retry(&mut self, cx: &mut Context<Self>) {
        let Some(snap) = &self.snapshot else {
            return;
        };
        let ids: Vec<String> = snap
            .accounts
            .iter()
            .filter(|a| self.selected_account_ids.contains(&a.id) && can_retry(a))
            .map(|a| a.id.clone())
            .collect();
        if ids.is_empty() {
            self.show_info("No selected accounts have a scheduled retry.", Some(4), cx);
            return;
        }
        self.open_menu = None;
        self.dispatch_account_commands(ids, "retry", cx);
    }

    /// Starts account repair sign-in for a single selected account (`POST /v1/accounts/{id}/repair`).
    pub fn repair_selected_account(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.selected_account_ids.len() != 1 {
            self.show_info("Select one account to sign in again.", Some(4), cx);
            return;
        }
        let Some(id) = self.selected_account_ids.iter().next().cloned() else {
            return;
        };
        let username = self
            .snapshot
            .as_ref()
            .and_then(|s| s.accounts.iter().find(|a| a.id == id))
            .map(|a| a.username.clone())
            .unwrap_or_default();
        self.open_menu = None;
        self.busy = true;
        cx.notify();
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            let res = api.repair_account(&id).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(()) => app.show_info(
                        format!("Sign in as @{username}; other accounts are rejected. Settings stay saved."),
                        Some(8),
                        cx,
                    ),
                    Err(e) => app.show_error(e, cx),
                }
            });
        })
        .detach();
    }

    fn dispatch_account_commands(
        &mut self,
        ids: Vec<String>,
        action: &str,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        cx.notify();
        let api = self.api.clone();
        let action_owned = action.to_string();
        cx.spawn(async move |this, cx| {
            let mut failed = Vec::new();
            for id in &ids {
                match api.account_command(id, &action_owned).await {
                    Ok(op) => {
                        if op.state == "failed" || op.error.is_some() {
                            failed.push(
                                op.error
                                    .unwrap_or_else(|| "Account command failed.".to_string()),
                            );
                        }
                    }
                    Err(err) => failed.push(err),
                }
            }
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                if !failed.is_empty() {
                    app.show_error(failed.join(" "), cx);
                }
                app.refresh_snapshot_tick(false, cx);
            });
        })
        .detach();
    }

    /// Looks up live place metadata & thumbnail preview inside AccountEditor or GameEditor.
    pub fn fetch_game_preview_for_modal(&mut self, place_id: u64, cx: &mut Context<Self>) {
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            let res = api.game_details(place_id).await;
            let _ = this.update(cx, |app, cx| match res {
                Ok(meta) => {
                    app.game_cache.resolved.insert(place_id, meta.clone());
                    match &mut app.modal {
                        ActiveModal::AccountEditor(ae) => {
                            ae.lookup_preview = Some(meta);
                            ae.error = None;
                        }
                        ActiveModal::GameEditor(ge) => {
                            if ge.name_input.read(cx).text().trim().is_empty() {
                                ge.name_input
                                    .update(cx, |ti, cx| ti.set_text(meta.name.clone(), cx));
                            }
                            ge.lookup = Some(meta);
                            ge.error = None;
                        }
                        _ => {}
                    }
                    cx.notify();
                }
                Err(err) => {
                    match &mut app.modal {
                        ActiveModal::AccountEditor(ae) => ae.error = Some(err),
                        ActiveModal::GameEditor(ge) => ge.error = Some(err),
                        _ => {}
                    }
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Validates and saves edits from the single-account editor dialog.
    pub fn submit_account_editor(&mut self, clear_target: bool, cx: &mut Context<Self>) {
        let ActiveModal::AccountEditor(ae) = &self.modal else {
            return;
        };
        let account_id = ae.account_id.clone();
        let draft = EditorDraft {
            alias: ae.alias_input.read(cx).text().to_string(),
            place: ae.place_input.read(cx).text().to_string(),
            job: ae.job_input.read(cx).text().to_string(),
            private: ae.private_input.read(cx).text().to_string(),
        };
        let group = ae.group_input.read(cx).text().to_string();
        let payload = match account_editor::validate_account_editor_patch(
            &draft,
            &group,
            ae.game_profile_id.as_deref(),
            ae.auto_recovery,
            &ae.fallback_policy,
            clear_target,
        ) {
            Ok((json, _)) => json,
            Err(err) => {
                if let ActiveModal::AccountEditor(ae) = &mut self.modal {
                    ae.error = Some(err);
                }
                cx.notify();
                return;
            }
        };

        self.saving = true;
        self.busy = true;
        cx.notify();
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            let res = api.patch_account(&account_id, payload).await;
            let _ = this.update(cx, |app, cx| {
                app.saving = false;
                app.busy = false;
                match res {
                    Ok(_) => {
                        app.close_modal_clearing_secrets(cx);
                        app.show_info("Account settings saved.", Some(4), cx);
                        app.refresh_snapshot_tick(false, cx);
                    }
                    Err(err) => {
                        if let ActiveModal::AccountEditor(ae) = &mut app.modal {
                            ae.error = Some(err);
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Validates and saves edits from the bulk-account editor dialog.
    pub fn submit_bulk_editor(&mut self, cx: &mut Context<Self>) {
        let ActiveModal::BulkEditor(bm) = &self.modal else {
            return;
        };
        let (ids, payload) = match bulk_editor::build_bulk_payload(bm, cx) {
            Ok(v) => v,
            Err(err) => {
                if let ActiveModal::BulkEditor(bm) = &mut self.modal {
                    bm.error = Some(err);
                }
                cx.notify();
                return;
            }
        };
        self.saving = true;
        self.busy = true;
        cx.notify();
        let api = self.api.clone();
        let count = ids.len();
        cx.spawn(async move |this, cx| {
            let res = api.bulk_patch(ids, payload).await;
            let _ = this.update(cx, |app, cx| {
                app.saving = false;
                app.busy = false;
                match res {
                    Ok(_) => {
                        app.close_modal_clearing_secrets(cx);
                        app.show_info(format!("Updated {count} selected accounts."), Some(4), cx);
                        app.refresh_snapshot_tick(false, cx);
                    }
                    Err(err) => {
                        if let ActiveModal::BulkEditor(bm) = &mut app.modal {
                            bm.error = Some(err);
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Validates and saves a created or edited game profile.
    pub fn submit_game_editor(&mut self, cx: &mut Context<Self>) {
        let ActiveModal::GameEditor(ge) = &self.modal else {
            return;
        };
        let profile = match game_editor::build_game_profile(ge, cx) {
            Ok(p) => p,
            Err(err) => {
                if let ActiveModal::GameEditor(ge) = &mut self.modal {
                    ge.error = Some(err);
                }
                cx.notify();
                return;
            }
        };
        self.saving = true;
        self.busy = true;
        cx.notify();
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            let res = api.save_game_profile(&profile).await;
            let _ = this.update(cx, |app, cx| {
                app.saving = false;
                app.busy = false;
                match res {
                    Ok(_) => {
                        app.close_modal_clearing_secrets(cx);
                        app.show_info("Saved game updated.", Some(4), cx);
                        app.refresh_snapshot_tick(false, cx);
                    }
                    Err(err) => {
                        if let ActiveModal::GameEditor(ge) = &mut app.modal {
                            ge.error = Some(err);
                        }
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Fetches persistent 1,000-entry activity history (`GET /v1/activity`).
    pub fn refresh_activity(&mut self, cx: &mut Context<Self>) {
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(acts) = api.activity(None).await {
                let _ = this.update(cx, |app, cx| {
                    app.activities = acts;
                    app.activity_loaded = true;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Saves or replaces a roster preset (`POST /v1/profiles`).
    pub fn save_preset_by_id(
        &mut self,
        id: Option<String>,
        name: String,
        account_ids: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.open_menu = None;
        cx.notify();
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            let res = api.save_profile(id.as_deref(), &name, account_ids).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(saved) => {
                        app.selected_preset_id = Some(saved.id);
                        app.preset_name_input
                            .update(cx, |ti, cx| ti.set_text("", cx));
                        app.show_info("Preset saved.", Some(4), cx);
                        app.refresh_snapshot_tick(false, cx);
                    }
                    Err(err) => app.show_error(err, cx),
                }
            });
        })
        .detach();
    }

    /// Applies preset settings (`apply`) or applies + launches (`start`).
    pub fn run_preset_action(
        &mut self,
        id: &str,
        action: &str,
        close_modal: bool,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        if close_modal {
            self.modal = ActiveModal::None;
        }
        cx.notify();
        let api = self.api.clone();
        let id_owned = id.to_string();
        let action_owned = action.to_string();
        cx.spawn(async move |this, cx| {
            let res = api.profile_action(&id_owned, &action_owned).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(items) => {
                        if action_owned == "start" {
                            let failures: Vec<String> = items
                                .iter()
                                .filter_map(|i| {
                                    i.error
                                        .as_ref()
                                        .map(|e| format!("Account {}: {e}", i.account_id))
                                })
                                .collect();
                            let ok = items.len().saturating_sub(failures.len());
                            if failures.is_empty() {
                                app.show_info(
                                    format!("Preset queued: {ok}/{} accounts.", items.len()),
                                    Some(5),
                                    cx,
                                );
                            } else {
                                app.feedback = Some(FeedbackMessage::new(
                                    BannerTone::Warning,
                                    format!(
                                        "Preset queued: {ok}/{} accounts. {}",
                                        items.len(),
                                        failures.join(" ")
                                    ),
                                    Some(8),
                                ));
                                cx.notify();
                            }
                        } else {
                            app.show_info("Preset settings applied.", Some(4), cx);
                        }
                        app.refresh_snapshot_tick(false, cx);
                    }
                    Err(err) => app.show_error(err, cx),
                }
            });
        })
        .detach();
    }

    /// Imports saved presets from a user-selected JSON file (`<= 1 MiB`).
    pub fn import_presets_from_file(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_menu = None;
        let entity = cx.entity();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import presets JSON".into()),
        });
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let len = std::fs::metadata(&path)
                .map(|m| m.len())
                .unwrap_or(u64::MAX);
            if len > MAX_IMPORT_FILE_BYTES {
                entity.update(cx, |app, cx| {
                    app.show_error("Preset file is too large (max 1 MiB).", cx);
                });
                return;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(err) => {
                    entity.update(cx, |app, cx| {
                        app.show_error(format!("Unable to read preset file: {err}"), cx);
                    });
                    return;
                }
            };
            let profiles: Vec<LaunchProfile> = match serde_json::from_str(&text) {
                Ok(p) => p,
                Err(_) => {
                    entity.update(cx, |app, cx| {
                        app.show_error("Invalid preset JSON file.", cx);
                    });
                    return;
                }
            };
            entity.update(cx, |app, cx| {
                let api = app.api.clone();
                app.busy = true;
                cx.notify();
                cx.spawn(async move |this, cx| {
                    let res = api.import_profiles(&profiles).await;
                    let _ = this.update(cx, |app, cx| {
                        app.busy = false;
                        match res {
                            Ok(count) => {
                                app.show_info(format!("Imported {count} preset(s)."), Some(4), cx);
                                app.refresh_snapshot_tick(false, cx);
                            }
                            Err(err) => app.show_error(err, cx),
                        }
                    });
                })
                .detach();
            });
        })
        .detach();
    }

    fn export_presets_to_file(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let api = self.api.clone();
        let dir = default_export_dir(&self.data_dir);
        let receiver = cx.prompt_for_new_path(&dir, Some("rolauncher-profiles.json"));
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(dest))) = receiver.await else {
                return;
            };
            let snap = match api.status().await {
                Ok(s) => s,
                Err(err) => {
                    entity.update(cx, |app, cx| app.show_error(err, cx));
                    return;
                }
            };
            match serde_json::to_string_pretty(&snap.profiles) {
                Ok(json) => {
                    if let Err(err) = std::fs::write(&dest, json) {
                        entity.update(cx, |app, cx| {
                            app.show_error(format!("Unable to save presets export: {err}"), cx);
                        });
                    } else {
                        entity.update(cx, |app, cx| {
                            app.show_info("Preset export saved.", Some(4), cx);
                        });
                    }
                }
                Err(err) => {
                    entity.update(cx, |app, cx| app.show_error(err.to_string(), cx));
                }
            }
        })
        .detach();
    }

    /// Exports sanitized diagnostics JSON (`GET /v1/diagnostics`) to a user-selected path.
    pub fn export_diagnostics_report(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let api = self.api.clone();
        let dir = default_export_dir(&self.data_dir);
        let receiver = cx.prompt_for_new_path(&dir, Some("rolauncher-support.json"));
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(dest))) = receiver.await else {
                return;
            };
            let report = match api.diagnostics().await {
                Ok(val) => val,
                Err(err) => {
                    entity.update(cx, |app, cx| app.show_error(err, cx));
                    return;
                }
            };
            match serde_json::to_string_pretty(&report) {
                Ok(json) => {
                    if let Err(err) = std::fs::write(&dest, json) {
                        entity.update(cx, |app, cx| {
                            app.show_error(format!("Unable to save diagnostics report: {err}"), cx);
                        });
                    } else {
                        entity.update(cx, |app, cx| {
                            app.show_info("Diagnostics report exported.", Some(4), cx);
                        });
                    }
                }
                Err(err) => {
                    entity.update(cx, |app, cx| app.show_error(err.to_string(), cx));
                }
            }
        })
        .detach();
    }

    /// Opens the controlled Local API bearer token reveal modal.
    pub fn open_api_token_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        token::open(self, window, cx);
    }

    /// Dispatches whichever action was confirmed inside [`ActiveModal::Confirm`].
    pub fn execute_confirmed_action(
        &mut self,
        action: ConfirmAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.modal = ActiveModal::None;
        match action {
            ConfirmAction::DeleteSelectedAccounts(ids) => {
                self.busy = true;
                cx.notify();
                let api = self.api.clone();
                cx.spawn(async move |this, cx| {
                    let mut errs = Vec::new();
                    for id in &ids {
                        if let Err(e) = api.delete_account(id).await {
                            errs.push(e);
                        }
                    }
                    let _ = this.update(cx, |app, cx| {
                        app.busy = false;
                        for id in &ids {
                            app.selected_account_ids.remove(id);
                        }
                        if errs.is_empty() {
                            app.show_info("Removed selected account(s).", Some(4), cx);
                        } else {
                            app.show_error(errs.join(" "), cx);
                        }
                        app.refresh_snapshot_tick(false, cx);
                    });
                })
                .detach();
            }
            ConfirmAction::DeleteSingleAccount(id) => {
                self.busy = true;
                cx.notify();
                let api = self.api.clone();
                cx.spawn(async move |this, cx| {
                    let res = api.delete_account(&id).await;
                    let _ = this.update(cx, |app, cx| {
                        app.busy = false;
                        app.selected_account_ids.remove(&id);
                        match res {
                            Ok(()) => {
                                app.show_info("Account removed.", Some(4), cx);
                                app.refresh_snapshot_tick(false, cx);
                            }
                            Err(e) => app.show_error(e, cx),
                        }
                    });
                })
                .detach();
            }
            ConfirmAction::StopAllRoblox => {
                self.busy = true;
                cx.notify();
                let api = self.api.clone();
                cx.spawn(async move |this, cx| {
                    let res = api.close_all_instances().await;
                    let _ = this.update(cx, |app, cx| {
                        app.busy = false;
                        match res {
                            Ok(closed) => {
                                app.show_info(
                                    format!("Stopped automatic rejoin and closed {closed} Roblox process(es)."),
                                    Some(5),
                                    cx,
                                );
                                app.refresh_snapshot_tick(false, cx);
                            }
                            Err(e) => app.show_error(e, cx),
                        }
                    });
                })
                .detach();
            }
            ConfirmAction::DeleteGameProfile(id) => {
                self.busy = true;
                cx.notify();
                let api = self.api.clone();
                cx.spawn(async move |this, cx| {
                    let res = api.delete_game_profile(&id).await;
                    let _ = this.update(cx, |app, cx| {
                        app.busy = false;
                        match res {
                            Ok(()) => {
                                app.show_info("Saved game removed.", Some(4), cx);
                                app.refresh_snapshot_tick(false, cx);
                            }
                            Err(e) => app.show_error(e, cx),
                        }
                    });
                })
                .detach();
            }
            ConfirmAction::DeletePreset(id) => {
                self.busy = true;
                cx.notify();
                let api = self.api.clone();
                cx.spawn(async move |this, cx| {
                    let res = api.delete_profile(&id).await;
                    let _ = this.update(cx, |app, cx| {
                        app.busy = false;
                        match res {
                            Ok(()) => {
                                app.show_info("Preset removed.", Some(4), cx);
                                app.refresh_snapshot_tick(false, cx);
                            }
                            Err(e) => app.show_error(e, cx),
                        }
                    });
                })
                .detach();
            }
            ConfirmAction::ExportPresetsNotice => {
                self.export_presets_to_file(cx);
            }
            ConfirmAction::RestoreBackup(name) => {
                self.busy = true;
                cx.notify();
                let api = self.api.clone();
                cx.spawn(async move |this, cx| {
                    let res = api.restore_backup(&name).await;
                    let _ = this.update(cx, |app, cx| {
                        app.busy = false;
                        match res {
                            Ok(()) => {
                                app.show_info(
                                    "Backup restored. Accounts stay stopped.",
                                    Some(6),
                                    cx,
                                );
                                app.refresh_all(cx);
                            }
                            Err(e) => app.show_error(e, cx),
                        }
                    });
                })
                .detach();
            }
            ConfirmAction::InstallUpdate => {
                self.apply_downloaded_update(window, cx);
            }
        }
    }

    // ---- Settings & Support Actions ----

    pub fn set_close_to_tray(&mut self, tray: bool, cx: &mut Context<Self>) {
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.set_window_settings(tray).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                if let Err(e) = res {
                    app.show_error(e, cx);
                }
                app.refresh_snapshot_tick(false, cx);
            });
        })
        .detach();
    }

    pub fn set_game_settings(
        &mut self,
        server_selection: String,
        join_type: String,
        cx: &mut Context<Self>,
    ) {
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.set_game_settings(&server_selection, &join_type).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                if let Err(e) = res {
                    app.show_error(e, cx);
                }
                app.refresh_snapshot_tick(false, cx);
            });
        })
        .detach();
    }

    pub fn set_multi_instance(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.set_multi_instance(enabled).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                if let Err(e) = res {
                    app.show_error(e, cx);
                }
                app.refresh_snapshot_tick(false, cx);
            });
        })
        .detach();
    }

    pub fn set_startup_cleanup(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.set_startup_cleanup(enabled).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                if let Err(e) = res {
                    app.show_error(e, cx);
                }
                app.refresh_snapshot_tick(false, cx);
            });
        })
        .detach();
    }

    pub fn set_discord_preferences(
        &mut self,
        enabled: bool,
        notify_recovery: bool,
        cx: &mut Context<Self>,
    ) {
        let api = self.api.clone();
        let patch = serde_json::json!({
            "enabled": enabled,
            "notify_recovery": notify_recovery,
        });
        cx.spawn(async move |this, cx| {
            let res = api.update_discord(patch).await;
            let _ = this.update(cx, |app, cx| match res {
                Ok(view) => {
                    app.discord_view = Some(view);
                    cx.notify();
                }
                Err(e) => app.show_error(e, cx),
            });
        })
        .detach();
    }

    pub fn save_discord_webhook(&mut self, cx: &mut Context<Self>) {
        let url = self
            .discord_webhook_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let patch = if url.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::json!({ "webhook_url": url })
        };
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.update_discord(patch).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(view) => {
                        app.discord_webhook_input
                            .update(cx, |ti, cx| ti.set_text("", cx));
                        app.discord_view = Some(view);
                        app.show_info("Webhook settings saved.", Some(4), cx);
                    }
                    Err(e) => app.show_error(e, cx),
                }
            });
        })
        .detach();
    }

    pub fn test_discord_webhook(&mut self, cx: &mut Context<Self>) {
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.test_discord().await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(()) => {
                        app.show_info(
                            "Test queued. See Recent activity for delivery.",
                            Some(5),
                            cx,
                        );
                        app.refresh_snapshot_tick(false, cx);
                    }
                    Err(e) => app.show_error(e, cx),
                }
            });
        })
        .detach();
    }

    pub fn remove_discord_webhook(&mut self, cx: &mut Context<Self>) {
        let api = self.api.clone();
        let patch = serde_json::json!({ "webhook_url": "", "enabled": false });
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.update_discord(patch).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(view) => {
                        app.discord_webhook_input
                            .update(cx, |ti, cx| ti.set_text("", cx));
                        app.discord_view = Some(view);
                        app.show_info("Webhook removed. Notifications off.", Some(4), cx);
                    }
                    Err(e) => app.show_error(e, cx),
                }
            });
        })
        .detach();
    }

    pub fn save_discord_bot(&mut self, cx: &mut Context<Self>) {
        let token = self
            .discord_bot_token_input
            .read(cx)
            .text()
            .trim()
            .to_string();
        let guild_id = self.discord_guild_input.read(cx).text().trim().to_string();
        let users_raw = self.discord_users_input.read(cx).text().to_string();
        let allowed_users: Vec<String> = users_raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();

        let mut bot = serde_json::Map::new();
        bot.insert("enabled".into(), serde_json::json!(self.bot_enabled_draft));
        bot.insert("guild_id".into(), serde_json::json!(guild_id));
        bot.insert("allowed_users".into(), serde_json::json!(allowed_users));
        if !token.is_empty() {
            bot.insert("token".into(), serde_json::json!(token));
        }
        let patch = serde_json::json!({ "bot": bot });
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.update_discord(patch).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(view) => {
                        app.discord_bot_token_input
                            .update(cx, |ti, cx| ti.set_text("", cx));
                        app.bot_loaded = false;
                        app.discord_view = Some(view);
                        app.show_info("Bot settings saved.", Some(4), cx);
                        app.refresh_snapshot_tick(false, cx);
                    }
                    Err(e) => app.show_error(e, cx),
                }
            });
        })
        .detach();
    }

    pub fn control_discord_bot(&mut self, action: &'static str, cx: &mut Context<Self>) {
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.bot_control(action).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(view) => {
                        app.discord_view = Some(view);
                        cx.notify();
                    }
                    Err(e) => app.show_error(e, cx),
                }
            });
        })
        .detach();
    }

    pub fn remove_discord_bot(&mut self, cx: &mut Context<Self>) {
        let patch = serde_json::json!({
            "bot": {
                "enabled": false,
                "token": "",
                "guild_id": "",
                "allowed_users": []
            }
        });
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.update_discord(patch).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(view) => {
                        app.discord_bot_token_input
                            .update(cx, |ti, cx| ti.set_text("", cx));
                        app.discord_guild_input
                            .update(cx, |ti, cx| ti.set_text("", cx));
                        app.discord_users_input
                            .update(cx, |ti, cx| ti.set_text("", cx));
                        app.bot_enabled_draft = false;
                        app.bot_loaded = false;
                        app.discord_view = Some(view);
                        app.show_info("Discord bot configuration removed.", Some(4), cx);
                    }
                    Err(e) => app.show_error(e, cx),
                }
            });
        })
        .detach();
    }

    pub fn refresh_backups(&mut self, cx: &mut Context<Self>) {
        let api = self.api.clone();
        cx.spawn(async move |this, cx| {
            if let Ok(list) = api.backups().await {
                let _ = this.update(cx, |app, cx| {
                    app.backups = list;
                    if let Some(sel) = &app.selected_backup {
                        if !app.backups.contains(sel) {
                            app.selected_backup = app.backups.first().cloned();
                        }
                    } else {
                        app.selected_backup = app.backups.first().cloned();
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn create_backup(&mut self, cx: &mut Context<Self>) {
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.create_backup().await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(name) => {
                        app.selected_backup = Some(name);
                        app.show_info("Encrypted backup created.", Some(4), cx);
                        app.refresh_backups(cx);
                    }
                    Err(e) => app.show_error(e, cx),
                }
            });
        })
        .detach();
    }

    // ---- Updater Actions ----

    pub fn set_beta_updates(&mut self, include_beta: bool, cx: &mut Context<Self>) {
        if self.busy || self.update_checking || self.update_downloading || self.update_installing {
            return;
        }
        let api = self.api.clone();
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let res = api.set_beta_updates(include_beta).await;
            let _ = this.update(cx, |app, cx| {
                app.busy = false;
                match res {
                    Ok(()) => {
                        app.update_view = None;
                        app.cached_installer = None;
                        app.refresh_snapshot_tick(false, cx);
                        app.check_updates(true, cx);
                    }
                    Err(e) => app.show_error(e, cx),
                }
            });
        })
        .detach();
    }

    pub fn check_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.update_checking || self.update_downloading || self.update_installing {
            return;
        }
        self.update_checking = true;
        self.update_status_text = "Checking official releases…".to_string();
        cx.notify();

        let api = self.api.clone();
        let current_version = self.bootstrap.version.clone();
        let cache_dir = self.update_cache_dir.clone();
        let rt = api.runtime_handle();
        let (tx, rx) = tokio::sync::oneshot::channel();

        rt.spawn(async move {
            let check_res = api.check_official_updates().await;
            let resolved: Result<
                (
                    crate::domain::models::UpdateView,
                    Option<crate::updater::downloader::CachedInstaller>,
                ),
                String,
            > = match check_res {
                Ok(view) => {
                    if view.available {
                        let cached = match ReqwestUpdateTransport::new() {
                            Ok(t) => {
                                let transport: Arc<dyn UpdateTransport> = Arc::new(t);
                                cached_verified_installer(
                                    transport,
                                    &view,
                                    &current_version,
                                    &cache_dir,
                                )
                                .await
                                .unwrap_or(None)
                            }
                            Err(_) => None,
                        };
                        Ok((view, cached))
                    } else {
                        Ok((view, None))
                    }
                }
                Err(err) => Err(err),
            };
            let _ = tx.send(resolved);
        });

        let current_version = self.bootstrap.version.clone();
        cx.spawn(async move |this, cx| {
            let Ok(resolved) = rx.await else {
                return;
            };
            let _ = this.update(cx, |app, cx| {
                app.update_checking = false;
                match resolved {
                    Ok((view, cached)) => {
                        if view.available {
                            app.update_status_text = if cached.is_some() {
                                "Verified. Relaunch to install; games stay open.".to_string()
                            } else {
                                "Download, then Relaunch to install.".to_string()
                            };
                        } else {
                            app.update_status_text = format!("Up to date · v{current_version}");
                            if manual {
                                app.show_info("Up to date.", Some(4), cx);
                            }
                        }
                        app.cached_installer = cached;
                        app.update_view = Some(view);
                    }
                    Err(_) => {
                        app.update_status_text =
                            "Update check failed. Will retry automatically.".to_string();
                        if manual {
                            app.feedback = Some(FeedbackMessage::new(
                                BannerTone::Warning,
                                app.update_status_text.clone(),
                                Some(6),
                            ));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn handle_update_primary_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_downloading {
            if let Some(cancel) = self.update_download_cancel.take() {
                let _ = cancel.send(true);
            }
            return;
        }
        if self.update_installing || self.update_checking {
            return;
        }
        let Some(view) = self.update_view.clone().filter(|u| u.available) else {
            return;
        };
        if self.cached_installer.is_some() {
            if self.saving || !matches!(self.modal, ActiveModal::None) {
                self.show_info("Finish editing before updating.", Some(4), cx);
                return;
            }
            self.modal = ActiveModal::Confirm(crate::state::ConfirmModal {
                title: format!("Install RoLauncher v{}?", view.version),
                body: "Creates an encrypted backup, prepares the installer, and closes RoLauncher to apply the update. Running Roblox games stay open.".to_string(),
                confirm_label: "Relaunch & install".to_string(),
                dangerous: false,
                action: ConfirmAction::InstallUpdate,
            });
            let _ = window;
            cx.notify();
            return;
        }

        let transport: Arc<dyn UpdateTransport> = match ReqwestUpdateTransport::new() {
            Ok(t) => Arc::new(t),
            Err(err) => {
                self.show_error(err, cx);
                return;
            }
        };

        let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
        self.update_download_cancel = Some(cancel_tx);
        self.update_downloading = true;
        self.update_download_pct = 0;
        self.update_status_text = "Downloading… 0%".to_string();
        cx.notify();

        let current_version = self.bootstrap.version.clone();
        let cache_dir = self.update_cache_dir.clone();
        let rt = self.api.runtime_handle();
        enum DownloadMsg {
            Progress(u8),
            Finished(Result<crate::updater::downloader::CachedInstaller, String>),
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        rt.spawn(async move {
            let prog_tx = tx.clone();
            let dl_fut = download_verified_installer(
                transport,
                &view,
                &current_version,
                &cache_dir,
                move |pct| {
                    let _ = prog_tx.send(DownloadMsg::Progress(pct));
                },
            );

            let outcome = tokio::select! {
                _ = cancel_rx.changed() => Err("__cancelled__".to_string()),
                res = dl_fut => res,
            };
            let _ = tx.send(DownloadMsg::Finished(outcome));
        });

        cx.spawn(async move |this, cx| {
            while let Some(msg) = rx.recv().await {
                let keep_going = this
                    .update(cx, |app, cx| match msg {
                        DownloadMsg::Progress(pct) => {
                            if app.update_downloading {
                                app.update_download_pct = pct;
                                app.update_status_text = format!("Downloading… {pct}%");
                                cx.notify();
                            }
                            true
                        }
                        DownloadMsg::Finished(outcome) => {
                            app.update_downloading = false;
                            app.update_download_cancel = None;
                            match outcome {
                                Ok(cached) => {
                                    app.cached_installer = Some(cached);
                                    app.update_status_text =
                                        "Verified. Relaunch to install; games stay open."
                                            .to_string();
                                    app.show_info(
                                        "Update downloaded and SHA-256 verified.",
                                        Some(5),
                                        cx,
                                    );
                                }
                                Err(ref msg) if msg == "__cancelled__" => {
                                    app.update_status_text =
                                        "Download cancelled. Current version still running."
                                            .to_string();
                                }
                                Err(err) => {
                                    app.update_status_text =
                                        "Download or verification failed. Try Download again."
                                            .to_string();
                                    app.show_error(err, cx);
                                }
                            }
                            cx.notify();
                            false
                        }
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }

    fn apply_downloaded_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.update_installing || !matches!(self.modal, ActiveModal::None) || self.saving {
            self.show_info("Finish editing before updating.", Some(4), cx);
            return;
        }
        let Some(cached) = self.cached_installer.clone() else {
            return;
        };
        self.update_installing = true;
        self.update_status_text = "Backing up and preparing installer…".to_string();
        cx.notify();

        let api = self.api.clone();
        let bootstrap = self.bootstrap.clone();
        let cache_dir = self.update_cache_dir.clone();
        let window_handle = window.window_handle();
        let rt = api.runtime_handle();
        let (tx, rx) = tokio::sync::oneshot::channel();

        rt.spawn(async move {
            let outcome: Result<(), String> = async {
                let _ = api.create_backup().await?;
                start_update_handoff(&bootstrap, &cached, &cache_dir).await
            }
            .await;
            let _ = tx.send(outcome);
        });

        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.await else {
                return;
            };
            let _ = this.update(cx, |app, cx| {
                app.update_installing = false;
                match outcome {
                    Ok(()) => {
                        app.exiting.store(true, Ordering::Release);
                        app.cleanup_on_exit();
                        let _ = window_handle.update(cx, |_, window, _| window.remove_window());
                        cx.quit();
                    }
                    Err(err) => {
                        app.update_status_text =
                            "Update preparation failed. App still running; try Relaunch again."
                                .to_string();
                        app.show_error(err, cx);
                    }
                }
            });
        })
        .detach();
    }
}

fn default_export_dir(fallback: &Path) -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Documents"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| fallback.to_path_buf())
}
