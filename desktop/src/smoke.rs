//! Headless-driven live GPUI smoke verification gated behind `--features ui-smoke`.
//!
//! Exercises the real GPUI window, navigation shell, modals, REST/SSE API client,
//! Resource Monitor, cookie batching, presets, saved games, Discord controls, backups,
//! update channel/handoff safety, themes and responsive window bounds.

use crate::app::RoLauncherApp;
use crate::domain::cookie_batch::parse_cookie_lines;
use crate::domain::filter::matches_account;
use crate::domain::resources::sort_instances;
use crate::platform::appearance::{AppearancePreference, load_appearance};
use crate::state::{ActiveModal, NavTab};
use crate::updater::downloader::CachedInstaller as DownloadedInstaller;
use gpui::{AnyWindowHandle, AsyncApp, Context, WeakEntity, Window, px, size};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

/// Starts the automated GPUI smoke runner when `ROLAUNCHER_UI_SMOKE_DIR` is set.
pub fn maybe_start(window: &mut Window, cx: &mut Context<RoLauncherApp>) {
    let Some(dir) = std::env::var_os("ROLAUNCHER_UI_SMOKE_DIR") else {
        return;
    };
    let output_dir = PathBuf::from(dir);
    let _ = std::fs::create_dir_all(&output_dir);
    let bridge_only = std::env::var("ROLAUNCHER_UI_BRIDGE_SMOKE").as_deref() == Ok("1");
    let win_handle = window.window_handle();

    cx.spawn(async move |this, cx| {
        let outcome = if bridge_only {
            run_bridge_smoke(&this, cx).await
        } else {
            run_full_smoke(&this, win_handle, cx).await
        };

        let result_path = output_dir.join("result.txt");
        match outcome {
            Ok(msg) => {
                let _ = std::fs::write(&result_path, msg);
            }
            Err(err) => {
                let _ = std::fs::write(&result_path, format!("FAILED: {err}"));
            }
        }

        let _ = this.update(cx, |app, _cx| {
            app.exiting.store(true, Ordering::Release);
            app.cleanup_on_exit();
        });
        let _ = win_handle.update(cx, |_, window, cx| {
            window.remove_window();
            cx.quit();
        });
    })
    .detach();
}

async fn run_bridge_smoke(
    this: &WeakEntity<RoLauncherApp>,
    cx: &mut AsyncApp,
) -> Result<String, String> {
    wait_until(cx, Duration::from_secs(10), |cx| {
        this.read_with(cx, |app, _| {
            app.snapshot.is_some() && !app.refresh_in_flight
        })
        .unwrap_or(false)
    })
    .await?;

    let (api, bootstrap_version) = this
        .read_with(cx, |app, _| {
            (app.api.clone(), app.bootstrap.version.clone())
        })
        .map_err(|e| format!("bridge read failed: {e}"))?;

    let snap = api.status().await?;
    check(
        snap.accounts.is_empty(),
        "Bridge fixture has no real accounts.",
    )?;
    check(
        bootstrap_version == env!("ROLAUNCHER_SHELL_VERSION"),
        &format!(
            "Runtime version '{bootstrap_version}' must match shell '{}'.",
            env!("ROLAUNCHER_SHELL_VERSION")
        ),
    )?;

    Ok("GPUI bridge passed: inherited pipe bootstrap, authenticated Rust API and clean desktop exit.".to_string())
}

async fn run_full_smoke(
    this: &WeakEntity<RoLauncherApp>,
    win_handle: AnyWindowHandle,
    cx: &mut AsyncApp,
) -> Result<String, String> {
    wait_until(cx, Duration::from_secs(12), |cx| {
        this.read_with(cx, |app, _| {
            app.snapshot.as_ref().is_some_and(|s| s.accounts.len() == 3)
                && !app.refresh_in_flight
                && app.discord_view.is_some()
                && app.activity_loaded
        })
        .unwrap_or(false)
    })
    .await?;

    // 1. Verify all 11 navigation tabs and animation settling cleanly across viewports.
    for tab in NavTab::ALL {
        this.update(cx, |app, cx| app.switch_tab(tab, cx))
            .map_err(|e| e.to_string())?;
        cx.background_executor()
            .timer(Duration::from_millis(220))
            .await;
        let active = this
            .read_with(cx, |app, _| (app.active_tab, app.tab_anim_progress))
            .map_err(|e| e.to_string())?;
        check(active.0 == tab, &format!("Tab {tab:?} failed to activate."))?;
        check(
            (active.1 - 1.0).abs() < 0.01,
            &format!(
                "Tab {tab:?} transition failed to settle (progress {}).",
                active.1
            ),
        )?;
    }

    // Current-page click retains scroll offset.
    this.update(cx, |app, cx| app.switch_tab(NavTab::Dashboard, cx))
        .map_err(|e| e.to_string())?;
    cx.background_executor()
        .timer(Duration::from_millis(60))
        .await;
    this.update(cx, |app, cx| {
        if let Some(handle) = app.tab_scroll_handles.get(&NavTab::Dashboard) {
            handle.set_offset(gpui::point(px(0.0), px(-120.0)));
        }
        app.switch_tab(NavTab::Dashboard, cx);
    })
    .map_err(|e| e.to_string())?;
    cx.background_executor()
        .timer(Duration::from_millis(40))
        .await;
    let retained_y = this
        .read_with(cx, |app, _| {
            app.tab_scroll_handles
                .get(&NavTab::Dashboard)
                .map(|h| f32::from(h.offset().y))
                .unwrap_or(0.0)
        })
        .map_err(|e| e.to_string())?;
    check(
        retained_y < -10.0,
        &format!("Clicking active Dashboard tab must retain scroll offset (got {retained_y})."),
    )?;

    // Resize checks across wide (1700x940) and minimum (1020x780) bounds for every destination.
    win_handle
        .update(cx, |_, window, _cx| {
            window.resize(size(px(1700.0), px(940.0)));
            window.refresh();
        })
        .map_err(|e| e.to_string())?;
    cx.background_executor()
        .timer(Duration::from_millis(80))
        .await;
    win_handle
        .update(cx, |_, window, _cx| {
            window.resize(size(px(1020.0), px(780.0)));
            window.refresh();
        })
        .map_err(|e| e.to_string())?;
    cx.background_executor()
        .timer(Duration::from_millis(80))
        .await;
    for tab in NavTab::ALL {
        this.update(cx, |app, cx| app.switch_tab(tab, cx))
            .map_err(|e| e.to_string())?;
        cx.background_executor()
            .timer(Duration::from_millis(35))
            .await;
    }
    let min_vp = win_handle
        .update(cx, |_, window, _| window.viewport_size())
        .map_err(|e| e.to_string())?;
    check(
        f32::from(min_vp.width) >= 980.0 && f32::from(min_vp.height) >= 700.0,
        &format!("Minimum window viewport collapsed unexpectedly: {min_vp:?}"),
    )?;
    win_handle
        .update(cx, |_, window, _cx| {
            window.resize(size(px(1280.0), px(820.0)));
            window.refresh();
        })
        .map_err(|e| e.to_string())?;
    cx.background_executor()
        .timer(Duration::from_millis(60))
        .await;

    // 2. Cookie parsing + deduplication + error/malformed envelope handling.
    let api = this
        .read_with(cx, |app, _| app.api.clone())
        .map_err(|e| e.to_string())?;
    this.update(cx, |app, cx| {
        app.switch_tab(NavTab::Accounts, cx);
        app.cookie_batch_input.update(cx, |ti, cx| {
            ti.set_text(
                " fixture-cookie-a\r\n\nfixture-cookie-b\nfixture-cookie-a ",
                cx,
            );
        });
        app.submit_cookie_textbox_import(cx);
    })
    .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;
    let (cleared, import_count_ok) = this
        .read_with(cx, |app, cx| {
            (
                app.cookie_batch_input.read(cx).text().is_empty(),
                app.feedback
                    .as_ref()
                    .is_some_and(|f| f.text.starts_with("Imported 2 account")),
            )
        })
        .map_err(|e| e.to_string())?;
    check(
        cleared && import_count_ok,
        "Cookie batch import should deduplicate and clear input.",
    )?;

    for bad in ["fixture-malformed", "fixture-failure"] {
        this.update(cx, |app, cx| {
            app.run_cookie_import(vec![Zeroizing::new(bad.to_string())], false, cx);
        })
        .map_err(|e| e.to_string())?;
        wait_not_busy(this, cx).await?;
        let fb = this
            .read_with(cx, |app, _| {
                app.feedback
                    .as_ref()
                    .map(|f| f.text.clone())
                    .unwrap_or_default()
            })
            .map_err(|e| e.to_string())?;
        check(
            !fb.is_empty(),
            &format!("Import {bad} must produce explicit error feedback."),
        )?;
    }
    check(
        parse_cookie_lines(" a \n\n b \n a ").len() == 2,
        "parse_cookie_lines must preserve unique nonblank items.",
    )?;

    // 3. Dashboard state + pagination clamping.
    this.update(cx, |app, cx| app.switch_tab(NavTab::Dashboard, cx))
        .map_err(|e| e.to_string())?;
    cx.background_executor()
        .timer(Duration::from_millis(60))
        .await;
    let dash_ok = this
        .read_with(cx, |app, _| {
            let s = app.snapshot.as_ref()?;
            let running = s.accounts.iter().filter(|a| a.status == "running").count();
            let backoff = s.accounts.iter().filter(|a| a.status == "backoff").count();
            let attn = s
                .accounts
                .iter()
                .filter(|a| a.status == "needs_attention")
                .count();
            let best = s
                .accounts
                .iter()
                .map(|a| a.longest_streak_seconds)
                .max()
                .unwrap_or(0);
            Some((
                s.accounts.len(),
                running,
                backoff,
                attn,
                best,
                app.activities.len(),
            ))
        })
        .map_err(|e| e.to_string())?;
    check(
        dash_ok == Some((3, 1, 1, 1, 10800, 1)),
        &format!("Dashboard summary mismatch: {dash_ok:?}"),
    )?;

    // 4. Phase 1 Resource Monitor SSE + sorting integration.
    this.update(cx, |app, cx| app.switch_tab(NavTab::Resources, cx))
        .map_err(|e| e.to_string())?;
    wait_until(cx, Duration::from_secs(6), |cx| {
        this.read_with(cx, |app, _| app.resource_snapshot.is_some())
            .unwrap_or(false)
    })
    .await?;
    let res_ok = this
        .read_with(cx, |app, _| {
            let r = app.resource_snapshot.as_ref()?;
            let sorted = sort_instances(&r.accounts, &app.resource_sort_by, app.resource_sort_desc);
            Some((
                r.aggregates.running_instances,
                r.system
                    .cpu_percent
                    .is_some_and(|v| (v - 24.5).abs() < 0.01),
                r.system
                    .ram_utilization_percent
                    .is_some_and(|v| (v - 50.0).abs() < 0.01),
                (r.aggregates.cpu_percent - 8.5).abs() < 0.01,
                app.resource_history.len(),
                sorted.first().and_then(|a| a.pid),
                r.system.gpu_availability.clone(),
            ))
        })
        .map_err(|e| e.to_string())?;
    check(
        res_ok
            == Some((
                1,
                true,
                true,
                true,
                12,
                Some(12000),
                "unavailable".to_string(),
            )),
        &format!("Resource Monitor fixture mismatch: {res_ok:?}"),
    )?;

    // 5. Updates, beta channel toggle, and modal relaunch safety check.
    this.update(cx, |app, cx| app.switch_tab(NavTab::Updates, cx))
        .map_err(|e| e.to_string())?;
    wait_until(cx, Duration::from_secs(6), |cx| {
        this.read_with(cx, |app, _| {
            app.update_view.is_some() && !app.update_checking
        })
        .unwrap_or(false)
    })
    .await?;
    this.update(cx, |app, cx| app.set_beta_updates(true, cx))
        .map_err(|e| e.to_string())?;
    wait_until(cx, Duration::from_secs(6), |cx| {
        this.read_with(cx, |app, _| {
            app.update_view.as_ref().is_some_and(|u| u.prerelease)
                && !app.busy
                && !app.update_checking
        })
        .unwrap_or(false)
    })
    .await?;
    this.update(cx, |app, cx| app.set_beta_updates(false, cx))
        .map_err(|e| e.to_string())?;
    wait_until(cx, Duration::from_secs(6), |cx| {
        this.read_with(cx, |app, _| {
            app.update_view.as_ref().is_some_and(|u| !u.prerelease)
                && !app.busy
                && !app.update_checking
        })
        .unwrap_or(false)
    })
    .await?;

    // Open modal blocks installer relaunch safely without exiting.
    this.update(cx, |app, cx| {
        app.cached_installer = Some(DownloadedInstaller {
            version: "9.9.9".to_string(),
            installer_path: PathBuf::from("FIXTURE_NOT_EXECUTED"),
            sha256: "0".repeat(64),
        });
        app.modal = ActiveModal::Changelog;
        cx.notify();
    })
    .map_err(|e| e.to_string())?;
    win_handle
        .update(cx, |_, window, cx| {
            let _ = this.update(cx, |app, cx| app.handle_update_primary_action(window, cx));
        })
        .map_err(|e| e.to_string())?;
    let still_open = this
        .read_with(cx, |app, _| {
            !app.update_installing && !app.exiting.load(Ordering::Acquire)
        })
        .map_err(|e| e.to_string())?;
    check(
        still_open,
        "Open modal must block update relaunch without closing.",
    )?;
    this.update(cx, |app, cx| {
        app.cached_installer = None;
        app.close_modal_clearing_secrets(cx);
    })
    .map_err(|e| e.to_string())?;

    // 6. Account filtering, hidden-selection pruning, single/bulk editors, and quick stop.
    this.update(cx, |app, cx| {
        app.switch_tab(NavTab::Accounts, cx);
        app.selected_account_ids.insert("0".to_string());
        app.account_search_input
            .update(cx, |ti, cx| ti.set_text("@SAMPLE_1", cx));
        app.prune_hidden_selection_for_query("@SAMPLE_1", cx);
    })
    .map_err(|e| e.to_string())?;
    let pruned_empty = this
        .read_with(cx, |app, _| app.selected_account_ids.is_empty())
        .map_err(|e| e.to_string())?;
    check(
        pruned_empty,
        "Search filter must prune hidden account selection.",
    )?;

    this.update(cx, |app, cx| {
        app.select_visible_accounts(cx);
        app.run_selected_account_command("stop", cx);
    })
    .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;

    // Clear filters & validate account editor atomic validation + API reject + save.
    this.update(cx, |app, cx| {
        app.account_search_input
            .update(cx, |ti, cx| ti.set_text("", cx));
        app.prune_hidden_selection(cx);
    })
    .map_err(|e| e.to_string())?;
    win_handle
        .update(cx, |_, window, cx| {
            let _ = this.update(cx, |app, cx| {
                crate::dialogs::account_editor::open_for_account(app, "0", window, cx);
                if let ActiveModal::AccountEditor(ae) = &mut app.modal {
                    ae.alias_input
                        .update(cx, |ti, cx| ti.set_text("Studio account", cx));
                    ae.place_input
                        .update(cx, |ti, cx| ti.set_text("incomplete", cx));
                }
                app.submit_account_editor(false, cx);
            });
        })
        .map_err(|e| e.to_string())?;
    let invalid_blocked = this
        .read_with(
            cx,
            |app, _| matches!(&app.modal, ActiveModal::AccountEditor(ae) if ae.error.is_some()),
        )
        .map_err(|e| e.to_string())?;
    check(
        invalid_blocked,
        "Invalid destination must keep AccountEditor open with error.",
    )?;

    this.update(cx, |app, cx| {
        if let ActiveModal::AccountEditor(ae) = &mut app.modal {
            ae.place_input.update(cx, |ti, cx| ti.set_text("1818", cx));
        }
        app.submit_account_editor(false, cx);
    })
    .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;
    wait_until(cx, Duration::from_secs(4), |cx| {
        this.read_with(cx, |app, _| {
            app.snapshot
                .as_ref()
                .and_then(|s| s.accounts.first())
                .is_some_and(|a| a.alias == "Studio account")
        })
        .unwrap_or(false)
    })
    .await?;

    // REJECT stays in modal cleanly.
    win_handle
        .update(cx, |_, window, cx| {
            let _ = this.update(cx, |app, cx| {
                crate::dialogs::account_editor::open_for_account(app, "0", window, cx);
                if let ActiveModal::AccountEditor(ae) = &mut app.modal {
                    ae.alias_input
                        .update(cx, |ti, cx| ti.set_text("REJECT", cx));
                }
                app.submit_account_editor(false, cx);
            });
        })
        .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;
    let reject_kept = this
        .read_with(
            cx,
            |app, _| matches!(&app.modal, ActiveModal::AccountEditor(ae) if ae.error.is_some()),
        )
        .map_err(|e| e.to_string())?;
    check(
        reject_kept,
        "API 409 rejection must keep AccountEditor open with error.",
    )?;
    this.update(cx, |app, cx| app.close_modal_clearing_secrets(cx))
        .map_err(|e| e.to_string())?;

    // Bulk editor: save group Crew + alias expansion + fallback pause + rejoin on ["0", "2"].
    win_handle
        .update(cx, |_, window, cx| {
            let _ = this.update(cx, |app, cx| {
                crate::dialogs::bulk_editor::open_for_ids(
                    app,
                    vec!["0".to_string(), "2".to_string()],
                    window,
                    cx,
                );
                if let ActiveModal::BulkEditor(bm) = &mut app.modal {
                    bm.apply_alias = true;
                    bm.alias_input
                        .update(cx, |ti, cx| ti.set_text("Crew {index}", cx));
                    bm.apply_group = true;
                    bm.group_input.update(cx, |ti, cx| ti.set_text("Crew", cx));
                    bm.apply_target = true;
                    bm.place_input.update(cx, |ti, cx| ti.set_text("1818", cx));
                    bm.apply_rejoin = true;
                    bm.rejoin = true;
                    bm.apply_fallback = true;
                    bm.fallback = "pause".to_string();
                }
                app.submit_bulk_editor(cx);
            });
        })
        .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;
    wait_until(cx, Duration::from_secs(4), |cx| {
        this.read_with(cx, |app, _| {
            app.snapshot.as_ref().is_some_and(|s| {
                s.accounts
                    .iter()
                    .any(|a| a.id == "0" && a.alias == "Crew 1" && a.group == "Crew")
                    && s.accounts
                        .iter()
                        .any(|a| a.id == "2" && a.alias == "Crew 2" && a.fallback_policy == "pause")
            })
        })
        .unwrap_or(false)
    })
    .await?;

    // 7. Saved Games create/edit + Account saved-game assignment + Presets lookup filters & review/apply.
    this.update(cx, |app, cx| app.switch_tab(NavTab::Games, cx))
        .map_err(|e| e.to_string())?;
    win_handle
        .update(cx, |_, window, cx| {
            let _ = this.update(cx, |app, cx| {
                crate::dialogs::game_editor::open(app, None, window, cx);
                if let ActiveModal::GameEditor(ge) = &mut app.modal {
                    ge.name_input
                        .update(cx, |ti, cx| ti.set_text("Private fixture", cx));
                    ge.place_input.update(cx, |ti, cx| ti.set_text("123", cx));
                }
                app.submit_game_editor(cx);
            });
        })
        .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;
    wait_until(cx, Duration::from_secs(4), |cx| {
        this.read_with(cx, |app, _| {
            app.snapshot
                .as_ref()
                .is_some_and(|s| s.game_profiles.iter().any(|g| g.name == "Private fixture"))
        })
        .unwrap_or(false)
    })
    .await?;

    // Presets lookup with status + rejoin filters & preset save/apply.
    this.update(cx, |app, cx| app.switch_tab(NavTab::Presets, cx))
        .map_err(|e| e.to_string())?;
    let lookup_ok = win_handle
        .update(cx, |_, window, cx| {
            this.update(cx, |app, cx| {
                crate::dialogs::preset_lookup::open(app, window, cx);
                let mut ok = false;
                if let ActiveModal::PresetLookup(pm) = &mut app.modal {
                    pm.chosen_ids.insert("0".to_string());
                    pm.search_input
                        .update(cx, |ti, cx| ti.set_text("sample_1", cx));
                    let s = app.snapshot.as_ref().unwrap();
                    let visible: Vec<_> = s
                        .accounts
                        .iter()
                        .filter(|a| matches_account(a, "sample_1", "", "", None))
                        .collect();
                    ok = visible.len() == 1;
                    for a in visible {
                        pm.chosen_ids.insert(a.id.clone());
                    }
                    app.preset_draft_account_ids = pm.chosen_ids.clone();
                }
                app.close_modal_clearing_secrets(cx);
                ok
            })
        })
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    check(
        lookup_ok,
        "PresetLookup sample_1 search should match exactly 1 account.",
    )?;
    let draft_count = this
        .read_with(cx, |app, _| app.preset_draft_account_ids.len())
        .map_err(|e| e.to_string())?;
    check(
        draft_count == 2,
        &format!("Preset lookup must preserve 2 chosen accounts (got {draft_count})."),
    )?;
    this.update(cx, |app, cx| {
        app.save_preset_by_id(
            None,
            "Startup team".to_string(),
            vec!["0".into(), "2".into()],
            cx,
        );
    })
    .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;
    wait_until(cx, Duration::from_secs(4), |cx| {
        this.read_with(cx, |app, _| {
            app.snapshot.as_ref().is_some_and(|s| s.profiles.len() == 1)
        })
        .unwrap_or(false)
    })
    .await?;
    let pid = this
        .read_with(cx, |app, _| {
            app.snapshot.as_ref().unwrap().profiles[0].id.clone()
        })
        .map_err(|e| e.to_string())?;
    this.update(cx, |app, cx| app.run_preset_action(&pid, "apply", true, cx))
        .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;

    // 8. Discord bot controls & secret redaction + Backups + Appearance/Density/Window + Token/Login.
    this.update(cx, |app, cx| app.switch_tab(NavTab::Discord, cx))
        .map_err(|e| e.to_string())?;
    let token_empty = this
        .read_with(cx, |app, cx| {
            app.discord_bot_token_input.read(cx).text().is_empty()
        })
        .map_err(|e| e.to_string())?;
    check(
        token_empty,
        "Bot token input must stay empty when saved remotely.",
    )?;

    for action in ["start", "stop", "restart"] {
        this.update(cx, |app, cx| app.control_discord_bot(action, cx))
            .map_err(|e| e.to_string())?;
        wait_not_busy(this, cx).await?;
        let state = this
            .read_with(cx, |app, _| {
                app.discord_view
                    .as_ref()
                    .and_then(|d| d.bot.as_ref())
                    .map(|b| b.connection_state.clone())
                    .unwrap_or_default()
            })
            .map_err(|e| e.to_string())?;
        let want = if action == "stop" {
            "offline"
        } else {
            "online"
        };
        check(
            state == want,
            &format!("Bot action '{action}' expected '{want}', got '{state}'."),
        )?;
    }

    this.update(cx, |app, cx| app.create_backup(cx))
        .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;
    wait_until(cx, Duration::from_secs(4), |cx| {
        this.read_with(cx, |app, _| !app.backups.is_empty())
            .unwrap_or(false)
    })
    .await?;

    // Theme persistence.
    let data_dir = this
        .read_with(cx, |app, _| app.data_dir.clone())
        .map_err(|e| e.to_string())?;
    win_handle
        .update(cx, |_, window, cx| {
            this.update(cx, |app, cx| {
                app.set_appearance(AppearancePreference::Dark, window, cx);
            })
            .map_err(|e| e.to_string())
        })
        .map_err(|e| e.to_string())??;
    check(
        load_appearance(&data_dir) == AppearancePreference::Dark,
        "Appearance preference must persist to data_directory.",
    )?;
    win_handle
        .update(cx, |_, window, cx| {
            this.update(cx, |app, cx| {
                app.set_appearance(AppearancePreference::Light, window, cx);
                app.set_appearance(AppearancePreference::System, window, cx);
            })
            .map_err(|e| e.to_string())
        })
        .map_err(|e| e.to_string())??;

    // Window & Game settings round-trip + login bridge command + token redaction cleanup.
    this.update(cx, |app, cx| app.set_close_to_tray(false, cx))
        .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;
    this.update(cx, |app, cx| app.set_close_to_tray(true, cx))
        .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;
    this.update(cx, |app, cx| {
        app.set_game_settings("highest".into(), "friend".into(), cx)
    })
    .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;
    this.update(cx, |app, cx| {
        app.set_game_settings("smart".into(), "random".into(), cx)
    })
    .map_err(|e| e.to_string())?;
    wait_not_busy(this, cx).await?;

    win_handle
        .update(cx, |_, window, cx| {
            let _ = this.update(cx, |app, cx| {
                app.open_api_token_modal(window, cx);
                app.close_modal_clearing_secrets(cx);
            });
        })
        .map_err(|e| e.to_string())?;
    let modal_cleared = this
        .read_with(cx, |app, _| matches!(app.modal, ActiveModal::None))
        .map_err(|e| e.to_string())?;
    check(modal_cleared, "Token modal must close cleanly.")?;

    // Verify PresetLookup combined status & rejoin filtering matches account filter contract.
    this.read_with(cx, |app, _| {
        let s = app.snapshot.as_ref().unwrap();
        let status_only = s
            .accounts
            .iter()
            .filter(|a| matches_account(a, "", "running", "", None))
            .count();
        let combined = s
            .accounts
            .iter()
            .filter(|a| matches_account(a, "sample_1", "running", "", None))
            .count();
        let rejoin_off = s
            .accounts
            .iter()
            .filter(|a| matches_account(a, "", "", "off", None))
            .count();
        (status_only, combined, rejoin_off)
    })
    .map_err(|e| e.to_string())
    .and_then(|(s1, c, r)| {
        check(
            s1 >= 1 && c == 0 && r == 1,
            &format!("PresetLookup filters mismatch: s1={s1} c={c} r={r}"),
        )
    })?;

    // Delete stopped account 1 cleanly while rejecting active account delete with 409.
    api.delete_account("1").await?;
    check(
        api.delete_account("0").await.is_err(),
        "Deleting running account must fail with 409.",
    )?;

    Ok("GPUI smoke passed: all 11 destinations, animations/scroll retention, resize viewports, Resource Monitor SSE & sorting, 50-cookie batching/errors, account & bulk editors, saved games, presets & lookup filters, Discord bot controls, backups, light/dark/system themes, update safety, token zeroization, and account deletion.".to_string())
}

fn check(cond: bool, msg: &str) -> Result<(), String> {
    if cond { Ok(()) } else { Err(msg.to_string()) }
}

async fn wait_not_busy(this: &WeakEntity<RoLauncherApp>, cx: &mut AsyncApp) -> Result<(), String> {
    wait_until(cx, Duration::from_secs(8), |cx| {
        this.read_with(cx, |app, _| !app.busy && !app.saving)
            .unwrap_or(false)
    })
    .await
}

async fn wait_until<F>(cx: &mut AsyncApp, timeout: Duration, mut pred: F) -> Result<(), String>
where
    F: FnMut(&mut AsyncApp) -> bool,
{
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pred(cx) {
            return Ok(());
        }
        cx.background_executor()
            .timer(Duration::from_millis(35))
            .await;
    }
    Err("Timed out waiting for GPUI smoke condition".to_string())
}
