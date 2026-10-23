use super::*;

struct Fixture {
    engine: Engine,
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("pr07-effects-{}", Uuid::new_v4()));
        let engine = Engine::open(Store::new(path.clone()).unwrap(), "synthetic".into()).unwrap();
        engine
            .transaction(|s| {
                for id in ["1", "2"] {
                    let mut a = Account::new(id.into(), format!("test{id}"));
                    a.target = Some(Target {
                        place_id: 1,
                        job_id: None,
                        private_server_link: None,
                    });
                    a.desired_running = true;
                    a.status = Status::Running;
                    a.operation_id = Some(Uuid::new_v4());
                    a.tracker = Some(format!("12345678901{id}"));
                    a.process = Some(ProcessIdentity {
                        pid: 4200 + id.parse::<u32>().unwrap(),
                        creation_time: format!("synthetic-{id}"),
                        tracker: a.tracker.clone().unwrap(),
                        generation: a.generation,
                    });
                    s.database.accounts.push(SavedAccount {
                        account: a,
                        encrypted_session: String::new(),
                    });
                }
                Ok(())
            })
            .unwrap();
        Self { engine, path }
    }
    fn account(&self) -> Account {
        self.engine.snapshot().accounts.remove(0)
    }
    fn players(&self) -> Vec<platform::Player> {
        self.engine
            .snapshot()
            .accounts
            .into_iter()
            .filter_map(|a| a.process)
            .map(|p| platform::Player {
                pid: p.pid,
                creation_time: p.creation_time,
                tracker: p.tracker,
            })
            .collect()
    }
    fn tick(&self, expected: &Account, now: Instant, attempts: &mut CloseAttempts) -> Vec<bool> {
        let mut effects = Vec::new();
        let players = self.players();
        let _ = self.engine.close_owned(
            expected,
            now,
            attempts,
            || Ok(players),
            |_, force| {
                effects.push(force);
                Ok(())
            },
        );
        effects
    }
    fn recovery(&self) {
        let a = self.account();
        self.engine.signal("1", a.generation, Signal::Disconnected);
        self.engine
            .update("1", a.generation, |a| {
                a.disconnected_since = Some(Utc::now() - ChronoDuration::seconds(31))
            })
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.path).unwrap();
    }
}

#[test]
fn passive_retired_records_do_not_repeat_process_discovery() {
    let f = Fixture::new();
    f.engine
        .transaction(|s| {
            for index in 0..256 {
                s.database.retired_launches.push(RetiredLaunch {
                    tracker: format!("passive-{index}"),
                    generation: Uuid::new_v4(),
                    account_id: "1".into(),
                    deadline: Utc::now() + ChronoDuration::seconds(90),
                    close_intent: None,
                    process: None,
                });
            }
            Ok(())
        })
        .unwrap();
    let retired = f
        .engine
        .0
        .state
        .lock()
        .unwrap()
        .database
        .retired_launches
        .clone();
    let mut discoveries = 0;
    let mut attempts = CloseAttempts::default();
    for old in &retired {
        f.engine
            .close_retired(
                old,
                &[],
                Instant::now(),
                &mut attempts,
                || {
                    discoveries += 1;
                    Ok(Vec::new())
                },
                |_, _| panic!("passive observation cannot close"),
            )
            .unwrap();
    }
    assert_eq!(discoveries, 0, "one shared monitor scan is sufficient");
}

#[test]
fn shared_retired_observation_allows_commands_and_only_actionable_records_rescan() {
    let f = Fixture::new();
    let (active, mut players) = pending_stop(&f);
    f.engine
        .transaction(|s| {
            for index in 0..256 {
                let generation = Uuid::new_v4();
                s.database.retired_launches.push(RetiredLaunch {
                    tracker: format!("passive-{index}"),
                    generation,
                    account_id: "1".into(),
                    deadline: Utc::now() + ChronoDuration::seconds(90),
                    close_intent: None,
                    process: (index < 32).then(|| ProcessIdentity {
                        pid: 10000 + index,
                        creation_time: format!("passive-{index}"),
                        tracker: format!("passive-{index}"),
                        generation,
                    }),
                });
            }
            Ok(())
        })
        .unwrap();
    players.extend(
        f.engine
            .0
            .state
            .lock()
            .unwrap()
            .database
            .retired_launches
            .iter()
            .filter_map(|r| r.process.as_ref())
            .map(|p| platform::Player {
                pid: p.pid,
                creation_time: p.creation_time.clone(),
                tracker: p.tracker.clone(),
            }),
    );
    let engine = f.engine.clone();
    let (scanning, shared_scan) = std::sync::mpsc::channel();
    let (supply, observation) = std::sync::mpsc::channel::<Vec<platform::Player>>();
    let (verifying, fresh_scan) = std::sync::mpsc::channel();
    let (resume, continue_scan) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        // Model the monitor's shared discovery outside the command mutex.
        scanning.send(()).unwrap();
        let players = observation.recv().unwrap();
        let retired = engine
            .0
            .state
            .lock()
            .unwrap()
            .database
            .retired_launches
            .clone();
        let mut discoveries = 0;
        let mut effects = Vec::new();
        let mut attempts = CloseAttempts::default();
        for old in retired.iter().rev() {
            engine
                .close_retired(
                    old,
                    &players,
                    Instant::now(),
                    &mut attempts,
                    || {
                        discoveries += 1;
                        assert!(engine.0.state.try_lock().is_err());
                        verifying.send(()).unwrap();
                        continue_scan.recv_timeout(Duration::from_secs(5)).unwrap();
                        Ok(players.clone())
                    },
                    |p, force| {
                        assert!(engine.0.state.try_lock().is_err());
                        effects.push((p.pid, force));
                        Ok(())
                    },
                )
                .unwrap();
        }
        (discoveries, effects)
    });
    shared_scan.recv_timeout(Duration::from_secs(5)).unwrap();
    // An unrelated command completes while the shared process scan is pending.
    f.engine.command("2", "stop").unwrap();
    supply.send(players.clone()).unwrap();
    fresh_scan.recv_timeout(Duration::from_secs(5)).unwrap();
    let engine = f.engine.clone();
    let (starting, command_started) = std::sync::mpsc::channel();
    let command = std::thread::spawn(move || {
        starting.send(()).unwrap();
        engine.command("1", "stop").unwrap()
    });
    command_started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    resume.send(()).unwrap();
    let (discoveries, effects) = worker.join().unwrap();
    command.join().unwrap();
    assert_eq!(discoveries, 1, "256 passive records add no fresh scans");
    assert_eq!(effects, vec![(4201, false)]);
    // The command and close serialized; stale pre-command observations cannot close again.
    f.engine.command("1", "stop").unwrap();
    assert!(
        f.engine
            .close_retired(
                &active,
                &players,
                Instant::now(),
                &mut CloseAttempts::default(),
                || panic!("superseded record cannot discover"),
                |_, _| panic!("superseded record cannot close"),
            )
            .is_err()
    );
}

#[test]
fn shared_observation_never_replaces_fresh_verification_for_graceful_or_force() {
    for force in [false, true] {
        for failure in ["ambiguous", "pid", "creation", "missing", "error"] {
            let f = Fixture::new();
            let (mut old, players) = pending_stop(&f);
            let now = Instant::now();
            let mut attempts = CloseAttempts::default();
            if force {
                assert!(
                    f.engine
                        .close_retired(
                            &old,
                            &players,
                            now,
                            &mut attempts,
                            || Ok(players.clone()),
                            |_, force| {
                                assert!(!force);
                                Ok(())
                            },
                        )
                        .unwrap()
                );
                old = f.engine.0.state.lock().unwrap().database.retired_launches[0].clone();
            }
            let mut discoveries = 0;
            f.engine
                .close_retired(
                    &old,
                    &players,
                    now + Duration::from_secs(6),
                    &mut attempts,
                    || {
                        discoveries += 1;
                        let mut fresh = players.clone();
                        match failure {
                            "ambiguous" => fresh.push(platform::Player {
                                pid: 9999,
                                creation_time: "other".into(),
                                tracker: old.tracker.clone(),
                            }),
                            "pid" => fresh[0].pid = 9999,
                            "creation" => fresh[0].creation_time = "reused".into(),
                            "missing" => fresh.clear(),
                            _ => return Err("synthetic discovery error".into()),
                        }
                        Ok(fresh)
                    },
                    |_, _| panic!("shared observation must not authorize a close"),
                )
                .unwrap();
            assert_eq!(discoveries, 1, "{failure}, force={force}");
            let record = f.engine.0.state.lock().unwrap().database.retired_launches[0].clone();
            assert_eq!(record.process.unwrap().creation_time, "synthetic-1");
            assert!(record.close_intent.unwrap().denied);
        }
    }
}

#[test]
fn restart_closes_current_generation_but_stale_restart_cannot_close_newer_client() {
    let f = Fixture::new();
    let old = f.account();
    f.engine.command("1", "restart").unwrap();
    let mut attempts = CloseAttempts::default();
    assert!(f.tick(&old, Instant::now(), &mut attempts).is_empty());
    assert_eq!(
        f.tick(&f.account(), Instant::now(), &mut attempts),
        vec![false]
    );
    let current = f.account();
    f.engine.command("1", "stop").unwrap();
    assert!(f.tick(&current, Instant::now(), &mut attempts).is_empty());
    assert_eq!(
        f.tick(&f.account(), Instant::now(), &mut attempts),
        vec![false]
    );
}
#[test]
fn recovery_termination_remains_available_under_the_same_valid_intent() {
    let f = Fixture::new();
    f.recovery();
    let now = Instant::now();
    let mut attempts = CloseAttempts::default();
    assert_eq!(f.tick(&f.account(), now, &mut attempts), vec![true]);
    assert_eq!(f.account().failures, 1);
    assert!(
        !f.account().disconnect_notified,
        "Backoff must retain the existing notification normalization"
    );
    assert_eq!(
        f.engine.activity(Some("1")).first().unwrap().status,
        Status::Backoff
    );
    assert_eq!(
        f.tick(&f.account(), now + Duration::from_secs(6), &mut attempts),
        vec![true]
    );
    assert_eq!(f.account().failures, 1);
}
#[test]
fn recovery_disabled_after_close_cannot_repeat_termination_or_consume_more_retries() {
    let f = Fixture::new();
    f.recovery();
    let now = Instant::now();
    let mut attempts = CloseAttempts::default();
    assert_eq!(f.tick(&f.account(), now, &mut attempts), vec![true]);
    f.engine
        .patch(
            "1",
            AccountPatch {
                auto_recovery: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
    for offset in [6, 12, 20] {
        assert!(
            f.tick(
                &f.account(),
                now + Duration::from_secs(offset),
                &mut attempts
            )
            .is_empty()
        );
    }
    assert_eq!(f.account().failures, 1);
    assert!(f.account().process.is_some());
    f.engine
        .patch(
            "1",
            AccountPatch {
                auto_recovery: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        f.tick(&f.account(), now + Duration::from_secs(30), &mut attempts)
            .is_empty()
    );
    f.engine.command("1", "stop").unwrap();
    f.engine.set_launch_allowed(false);
    assert_eq!(
        f.tick(&f.account(), now + Duration::from_secs(40), &mut attempts),
        vec![false]
    );
}
#[test]
fn revoked_replacement_before_first_effect_preserves_process_and_retry_budget() {
    let f = Fixture::new();
    f.recovery();
    let a = f.account();
    f.engine
        .patch(
            "1",
            AccountPatch {
                auto_recovery: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        f.tick(&a, Instant::now(), &mut CloseAttempts::default())
            .is_empty()
    );
    assert!(
        f.tick(&f.account(), Instant::now(), &mut CloseAttempts::default())
            .is_empty()
    );
    assert_eq!(f.account().failures, 0);
    assert!(f.account().process.is_some());
}
#[test]
fn ambiguous_tracker_pid_reuse_and_shared_account_ownership_all_deny_close() {
    for scenario in ["ambiguous", "pid-reuse", "creation-time", "shared-owner"] {
        let f = Fixture::new();
        f.engine.command("1", "stop").unwrap();
        let a = f.account();
        let mut players = f.players();
        match scenario {
            "ambiguous" => players.push(platform::Player {
                pid: 9999,
                creation_time: "other".into(),
                tracker: a.tracker.clone().unwrap(),
            }),
            "pid-reuse" => players[0].pid = 9999,
            "creation-time" => players[0].creation_time = "reused".into(),
            _ => {
                f.engine
                    .transaction(|s| {
                        let p = s.database.accounts[0].account.process.clone();
                        s.database.accounts[1].account.process = p;
                        Ok(())
                    })
                    .unwrap();
            }
        }
        let mut closed = false;
        let _ = f.engine.close_owned(
            &a,
            Instant::now(),
            &mut CloseAttempts::default(),
            || Ok(players),
            |_, _| {
                closed = true;
                Ok(())
            },
        );
        assert!(!closed, "{scenario}");
        assert!(f.account().process.is_some());
    }
}
#[test]
fn failed_graceful_close_never_becomes_a_force_attempt_on_later_ticks() {
    let f = Fixture::new();
    f.engine.command("1", "stop").unwrap();
    let now = Instant::now();
    let mut attempts = CloseAttempts::default();
    let players = f.players();
    let _ = f.engine.close_owned(
        &f.account(),
        now,
        &mut attempts,
        || Ok(players),
        |_, force| {
            assert!(!force);
            Err("simulated failure".into())
        },
    );
    for offset in [6, 20] {
        assert!(
            f.tick(
                &f.account(),
                now + Duration::from_secs(offset),
                &mut attempts
            )
            .is_empty()
        );
    }
    assert_eq!(f.account().failures, 0);
}
#[test]
fn connected_signal_revokes_further_recovery_termination() {
    let f = Fixture::new();
    f.recovery();
    let now = Instant::now();
    let mut attempts = CloseAttempts::default();
    assert_eq!(f.tick(&f.account(), now, &mut attempts), vec![true]);
    let a = f.account();
    f.engine.signal("1", a.generation, Signal::Connected);
    assert!(
        f.tick(&f.account(), now + Duration::from_secs(6), &mut attempts)
            .is_empty()
    );
    assert_eq!(f.account().status, Status::Running);
}
#[test]
fn target_replacement_requires_current_fallback_and_recovery_settings() {
    for enabled in [true, false] {
        let f = Fixture::new();
        let a = f.account();
        f.engine
            .patch(
                "1",
                AccountPatch {
                    auto_recovery: Some(enabled),
                    fallback_policy: Some(FallbackPolicy::AllowPublic),
                    ..Default::default()
                },
            )
            .unwrap();
        f.engine
            .signal("1", a.generation, Signal::TargetUnavailable);
        let effects = f.tick(&f.account(), Instant::now(), &mut CloseAttempts::default());
        assert_eq!(effects, if enabled { vec![false] } else { vec![] });
    }
}
#[tokio::test]
async fn stop_during_awaited_recovery_supersedes_the_old_effect() {
    let f = Fixture::new();
    f.recovery();
    let old = f.account();
    let (send, recv) = tokio::sync::oneshot::channel();
    let engine = f.engine.clone();
    let players = f.players();
    let pending = tokio::spawn(async move {
        recv.await.unwrap();
        let mut effects = Vec::new();
        let _ = engine.close_owned(
            &old,
            Instant::now(),
            &mut CloseAttempts::default(),
            || Ok(players),
            |_, force| {
                effects.push(force);
                Ok(())
            },
        );
        effects
    });
    f.engine.command("1", "stop").unwrap();
    send.send(()).unwrap();
    assert!(pending.await.unwrap().is_empty());
    f.engine.set_launch_allowed(false);
    assert_eq!(
        f.tick(&f.account(), Instant::now(), &mut CloseAttempts::default()),
        vec![false]
    );
}
#[test]
fn legacy_live_state_is_observation_only_until_explicit_stop() {
    let f = Fixture::new();
    let a = f.account();
    f.engine
        .update("1", a.generation, |a| {
            a.desired_running = false;
            a.status = Status::Stopped;
        })
        .unwrap();
    assert!(
        f.tick(&f.account(), Instant::now(), &mut CloseAttempts::default())
            .is_empty()
    );
    f.engine.command("1", "stop").unwrap();
    assert_eq!(
        f.tick(&f.account(), Instant::now(), &mut CloseAttempts::default()),
        vec![false]
    );
}
#[test]
fn coordination_loss_during_process_discovery_prevents_replacement_effect() {
    let f = Fixture::new();
    f.engine.command("1", "restart").unwrap();
    let players = f.players();
    let mut closed = false;
    let gate = f.engine.0.launch_allowed.clone();
    let _ = f.engine.close_owned(
        &f.account(),
        Instant::now(),
        &mut CloseAttempts::default(),
        || {
            gate.store(false, Ordering::Release);
            Ok(players)
        },
        |_, _| {
            closed = true;
            Ok(())
        },
    );
    assert!(!closed);
}
#[test]
fn denied_close_stays_paused_after_reopen_even_if_client_exits_naturally() {
    let f = Fixture::new();
    f.engine.command("1", "restart").unwrap();
    f.engine.set_launch_allowed(false);
    assert!(
        f.tick(&f.account(), Instant::now(), &mut CloseAttempts::default())
            .is_empty()
    );
    let a = f.account();
    f.engine
        .update("1", a.generation, |a| {
            a.process = None;
            a.tracker = None;
        })
        .unwrap();
    let reopened = Engine::open(Store::new(f.path.clone()).unwrap(), "test".into()).unwrap();
    assert_eq!(
        reopened.snapshot().accounts[0].status,
        Status::NeedsAttention
    );
}
#[test]
fn retry_now_preserves_an_eligible_target_replacement_intent() {
    let f = Fixture::new();
    let a = f.account();
    f.engine
        .patch(
            "1",
            AccountPatch {
                fallback_policy: Some(FallbackPolicy::AllowPublic),
                ..Default::default()
            },
        )
        .unwrap();
    f.engine
        .signal("1", a.generation, Signal::TargetUnavailable);
    f.engine.retry_now("1").unwrap();
    assert_eq!(
        f.tick(&f.account(), Instant::now(), &mut CloseAttempts::default()),
        vec![false]
    );
}

#[test]
fn close_failure_remains_latched_when_the_denial_cannot_be_persisted() {
    let f = Fixture::new();
    f.engine.command("1", "stop").unwrap();
    let players = f.players();
    let now = Instant::now();
    let mut attempts = CloseAttempts::default();
    let _ = f.engine.close_owned(
        &f.account(),
        now,
        &mut attempts,
        || Ok(players),
        |_, _| {
            std::fs::create_dir(f.path.join("accounts.json.tmp")).unwrap();
            Err("simulated close and persistence failure".into())
        },
    );
    assert!(
        f.tick(&f.account(), now + Duration::from_secs(6), &mut attempts)
            .is_empty()
    );
}
#[test]
fn another_retired_record_cannot_claim_the_same_pid_under_a_different_tracker() {
    let f = Fixture::new();
    f.engine.command("1", "stop").unwrap();
    let a = f.account();
    f.engine
        .transaction(|s| {
            s.database.retired_launches.push(RetiredLaunch {
                tracker: "987654321012".into(),
                generation: Uuid::new_v4(),
                account_id: "2".into(),
                deadline: Utc::now() + ChronoDuration::seconds(90),
                close_intent: None,
                process: a.process.clone(),
            });
            Ok(())
        })
        .unwrap();
    assert!(
        f.tick(&a, Instant::now(), &mut CloseAttempts::default())
            .is_empty()
    );
}
#[tokio::test]
async fn obsolete_pending_launch_blocks_replacement_before_credentials_or_dispatch() {
    let f = Fixture::new();
    let a = f.account();
    f.engine
        .update("1", a.generation, |a| {
            a.process = None;
            a.status = Status::NeedsAttention;
        })
        .unwrap();
    assert!(f.engine.command("1", "start").is_err());
    let queued = f.account();
    f.engine.launch_one(queued).await;
    assert_eq!(f.account().status, Status::NeedsAttention);
    assert!(f.account().last_error.is_none());
    assert_eq!(f.account().failures, 0);
}
#[test]
fn persistence_failure_prevents_the_destructive_effect() {
    let f = Fixture::new();
    f.recovery();
    std::fs::create_dir(f.path.join("accounts.json.tmp")).unwrap();
    assert!(
        f.tick(&f.account(), Instant::now(), &mut CloseAttempts::default())
            .is_empty()
    );
    assert_eq!(f.account().failures, 0);
}

fn pending_stop(f: &Fixture) -> (RetiredLaunch, Vec<platform::Player>) {
    let a = f.account();
    let identity = a.process.clone().unwrap();
    f.engine
        .update("1", a.generation, |a| a.process = None)
        .unwrap();
    f.engine.command("1", "stop").unwrap();
    let old = Store::new(f.path.clone())
        .unwrap()
        .load()
        .unwrap()
        .retired_launches
        .remove(0);
    (
        old,
        vec![platform::Player {
            pid: identity.pid,
            creation_time: identity.creation_time,
            tracker: identity.tracker,
        }],
    )
}

#[test]
fn repeated_owned_denial_has_no_storage_or_broadcast_work_and_accepts_new_commands() {
    for action in ["stop", "restart"] {
        let f = Fixture::new();
        f.recovery();
        let now = Instant::now();
        let mut attempts = CloseAttempts::default();
        assert_eq!(f.tick(&f.account(), now, &mut attempts), vec![true]);
        f.engine.set_launch_allowed(false);
        assert!(
            f.tick(&f.account(), now + Duration::from_secs(6), &mut attempts)
                .is_empty()
        );
        let denied = f.account().close_intent.unwrap();
        assert!(denied.denied);
        let saves = f.engine.0.store.save_calls.load(Ordering::Relaxed);
        let mut events = f.engine.subscribe();
        for offset in [12, 18, 24] {
            assert!(
                f.tick(
                    &f.account(),
                    now + Duration::from_secs(offset),
                    &mut attempts
                )
                .is_empty()
            );
        }
        assert_eq!(f.engine.0.store.save_calls.load(Ordering::Relaxed), saves);
        assert!(matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        f.engine.set_launch_allowed(true);
        f.engine.command("1", action).unwrap();
        assert_ne!(f.account().close_intent.as_ref().unwrap().id, denied.id);
        assert_eq!(
            f.tick(&f.account(), now + Duration::from_secs(30), &mut attempts),
            vec![false]
        );
    }
}

#[test]
fn repeated_retired_denial_has_no_storage_or_broadcast_work_and_accepts_fresh_stop() {
    let f = Fixture::new();
    let (old, players) = pending_stop(&f);
    let now = Instant::now();
    let mut attempts = CloseAttempts::default();
    f.engine
        .close_retired(
            &old,
            &players,
            now,
            &mut attempts,
            || Ok(players.clone()),
            |_, _| Err("synthetic close failure".into()),
        )
        .unwrap_err();
    let denied = f.engine.0.state.lock().unwrap().database.retired_launches[0]
        .close_intent
        .clone()
        .unwrap();
    assert!(denied.denied);
    let saves = f.engine.0.store.save_calls.load(Ordering::Relaxed);
    let mut events = f.engine.subscribe();
    for offset in [6, 12, 18] {
        let old = f.engine.0.state.lock().unwrap().database.retired_launches[0].clone();
        f.engine
            .close_retired(
                &old,
                &players,
                now + Duration::from_secs(offset),
                &mut attempts,
                || panic!("denied intent must not discover again"),
                |_, _| panic!("denied intent must not close again"),
            )
            .unwrap();
    }
    assert_eq!(f.engine.0.store.save_calls.load(Ordering::Relaxed), saves);
    assert!(matches!(
        events.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    f.engine.command("1", "stop").unwrap();
    let old = f.engine.0.state.lock().unwrap().database.retired_launches[0].clone();
    assert_ne!(old.close_intent.as_ref().unwrap().id, denied.id);
    assert!(
        f.engine
            .close_retired(
                &old,
                &players,
                now + Duration::from_secs(30),
                &mut attempts,
                || Ok(players.clone()),
                |_, force| {
                    assert!(!force);
                    Ok(())
                },
            )
            .unwrap()
    );
}

#[test]
fn owned_denial_retries_only_unsaved_state_and_never_restores_automatic_force() {
    let f = Fixture::new();
    f.recovery();
    let now = Instant::now();
    let mut attempts = CloseAttempts::default();
    assert_eq!(f.tick(&f.account(), now, &mut attempts), vec![true]);
    let blocked_path = f.path.join("accounts.json.tmp");
    std::fs::create_dir(&blocked_path).unwrap();
    f.engine.set_launch_allowed(false);
    let mut events = f.engine.subscribe();
    assert!(
        f.tick(&f.account(), now + Duration::from_secs(6), &mut attempts)
            .is_empty()
    );
    // Coordination returning does not restore the old episode's force authority.
    f.engine.set_launch_allowed(true);
    for offset in [12, 18] {
        assert!(
            f.tick(
                &f.account(),
                now + Duration::from_secs(offset),
                &mut attempts
            )
            .is_empty()
        );
    }
    assert!(matches!(
        events.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    std::fs::remove_dir(blocked_path).unwrap();
    assert!(
        f.tick(&f.account(), now + Duration::from_secs(24), &mut attempts)
            .is_empty()
    );
    let denied = f.account();
    assert!(denied.close_intent.as_ref().unwrap().denied);
    assert_eq!(denied.status, Status::NeedsAttention);
    f.engine.signal("1", denied.generation, Signal::Connected);
    assert_eq!(f.account().close_intent, denied.close_intent);
    let saves = f.engine.0.store.save_calls.load(Ordering::Relaxed);
    let mut events = f.engine.subscribe();
    for offset in [30, 36, 42] {
        assert!(
            f.tick(
                &f.account(),
                now + Duration::from_secs(offset),
                &mut attempts
            )
            .is_empty()
        );
    }
    assert_eq!(f.engine.0.store.save_calls.load(Ordering::Relaxed), saves);
    assert!(matches!(
        events.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    f.engine.command("1", "restart").unwrap();
    assert_eq!(
        f.tick(&f.account(), now + Duration::from_secs(48), &mut attempts),
        vec![false]
    );
}

#[test]
fn unsaved_first_recovery_denial_cannot_regain_authority_with_a_new_automatic_intent() {
    for new_episode in ["same", "reconnect", "target"] {
        let f = Fixture::new();
        f.engine
            .patch(
                "1",
                AccountPatch {
                    fallback_policy: Some(FallbackPolicy::AllowPublic),
                    ..Default::default()
                },
            )
            .unwrap();
        f.recovery();
        let now = Instant::now();
        let mut attempts = CloseAttempts::default();
        let blocked_path = f.path.join("accounts.json.tmp");
        std::fs::create_dir(&blocked_path).unwrap();
        let a = f.account();
        assert!(
            f.engine
                .close_owned(
                    &a,
                    now,
                    &mut attempts,
                    || Err("synthetic discovery failure".into()),
                    |_, _| panic!("unverified recovery cannot close"),
                )
                .is_err()
        );
        assert!(
            f.account().close_intent.is_none(),
            "failed save rolled back new classification"
        );
        std::fs::remove_dir(blocked_path).unwrap();
        if new_episode == "reconnect" {
            f.engine.signal("1", a.generation, Signal::Connected);
            f.recovery();
        } else if new_episode == "target" {
            f.engine
                .signal("1", a.generation, Signal::TargetUnavailable);
        }
        assert!(
            f.tick(&f.account(), now + Duration::from_secs(6), &mut attempts)
                .is_empty()
        );
        assert!(f.account().close_intent.as_ref().unwrap().denied);
        f.engine.command("1", "restart").unwrap();
        assert_eq!(
            f.tick(&f.account(), now + Duration::from_secs(12), &mut attempts),
            vec![false]
        );
    }
}

#[test]
fn retired_denial_retries_only_unsaved_state_and_keeps_the_original_identity() {
    let f = Fixture::new();
    let (old, players) = pending_stop(&f);
    let now = Instant::now();
    let mut attempts = CloseAttempts::default();
    assert!(
        f.engine
            .close_retired(
                &old,
                &players,
                now,
                &mut attempts,
                || Ok(players.clone()),
                |_, force| {
                    assert!(!force);
                    Ok(())
                },
            )
            .unwrap()
    );
    let blocked_path = f.path.join("accounts.json.tmp");
    std::fs::create_dir(&blocked_path).unwrap();
    let mut events = f.engine.subscribe();
    for offset in [6, 12, 18] {
        let old = f.engine.0.state.lock().unwrap().database.retired_launches[0].clone();
        assert!(
            f.engine
                .close_retired(
                    &old,
                    &players,
                    now + Duration::from_secs(offset),
                    &mut attempts,
                    || Err("synthetic discovery failure".into()),
                    |_, _| panic!("unsaved denial cannot force"),
                )
                .is_err()
        );
    }
    assert!(matches!(
        events.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    std::fs::remove_dir(blocked_path).unwrap();
    let old = f.engine.0.state.lock().unwrap().database.retired_launches[0].clone();
    assert!(
        !f.engine
            .close_retired(
                &old,
                &players,
                now + Duration::from_secs(24),
                &mut attempts,
                || panic!("in-memory denial prevents rediscovery"),
                |_, _| panic!("denial prevents force"),
            )
            .unwrap()
    );
    let saves = f.engine.0.store.save_calls.load(Ordering::Relaxed);
    let mut events = f.engine.subscribe();
    for offset in [30, 36, 42] {
        let old = f.engine.0.state.lock().unwrap().database.retired_launches[0].clone();
        assert_eq!(old.process.as_ref().unwrap().creation_time, "synthetic-1");
        assert!(old.close_intent.as_ref().unwrap().denied);
        assert!(
            !f.engine
                .close_retired(
                    &old,
                    &players,
                    now + Duration::from_secs(offset),
                    &mut attempts,
                    || panic!("persisted denial prevents rediscovery"),
                    |_, _| panic!("denial prevents force"),
                )
                .unwrap()
        );
    }
    assert_eq!(f.engine.0.store.save_calls.load(Ordering::Relaxed), saves);
    assert!(matches!(
        events.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    f.engine.command("1", "stop").unwrap();
    let old = f.engine.0.state.lock().unwrap().database.retired_launches[0].clone();
    assert!(
        f.engine
            .close_retired(
                &old,
                &players,
                now + Duration::from_secs(48),
                &mut attempts,
                || Ok(players.clone()),
                |_, force| {
                    assert!(!force);
                    Ok(())
                },
            )
            .unwrap()
    );
}
#[test]
fn late_explicit_stop_closes_and_force_requires_the_same_current_stop() {
    let f = Fixture::new();
    let (old, players) = pending_stop(&f);
    let now = Instant::now();
    let mut attempts = CloseAttempts::default();
    let mut effects = Vec::new();
    f.engine
        .close_retired(
            &old,
            &players,
            now,
            &mut attempts,
            || Ok(players.clone()),
            |_, force| {
                effects.push(force);
                Ok(())
            },
        )
        .unwrap();
    let current = Store::new(f.path.clone())
        .unwrap()
        .load()
        .unwrap()
        .retired_launches
        .remove(0);
    f.engine
        .close_retired(
            &current,
            &players,
            now + Duration::from_secs(6),
            &mut attempts,
            || Ok(players.clone()),
            |_, force| {
                effects.push(force);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(effects, vec![false, true]);
    let a = f.account();
    f.engine
        .update("1", a.generation, |a| a.desired_running = true)
        .unwrap();
    f.engine
        .close_retired(
            &current,
            &players,
            now + Duration::from_secs(12),
            &mut attempts,
            || Ok(players.clone()),
            |_, _| panic!("revoked Stop cannot force"),
        )
        .unwrap();
    assert_eq!(f.account().status, Status::NeedsAttention);
}
#[test]
fn retired_stop_accepts_late_discovery_but_rejects_ambiguous_obsolete_and_reused_identity() {
    for scenario in ["ambiguous", "obsolete", "expired", "pid-reuse"] {
        let f = Fixture::new();
        let (mut old, mut players) = pending_stop(&f);
        let mut attempts = CloseAttempts::default();
        if scenario == "pid-reuse" {
            f.engine
                .close_retired(
                    &old,
                    &players,
                    Instant::now(),
                    &mut attempts,
                    || Ok(players.clone()),
                    |_, _| Ok(()),
                )
                .unwrap();
            old = Store::new(f.path.clone())
                .unwrap()
                .load()
                .unwrap()
                .retired_launches
                .remove(0);
            players[0].creation_time = "reused".into();
        } else if scenario == "ambiguous" {
            players.push(platform::Player {
                pid: 9999,
                creation_time: "other".into(),
                tracker: old.tracker.clone(),
            });
        } else {
            f.engine
                .transaction(|s| {
                    let old = &mut s.database.retired_launches[0];
                    if scenario == "obsolete" {
                        old.close_intent.as_mut().unwrap().reason = CloseReason::ObsoleteLaunch;
                    } else {
                        old.deadline = Utc::now() - ChronoDuration::seconds(1);
                    }
                    Ok(())
                })
                .unwrap();
            old = Store::new(f.path.clone())
                .unwrap()
                .load()
                .unwrap()
                .retired_launches
                .remove(0);
        }
        let mut closed = false;
        f.engine
            .close_retired(
                &old,
                &players,
                Instant::now() + Duration::from_secs(10),
                &mut attempts,
                || Ok(players.clone()),
                |_, _| {
                    closed = true;
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(closed, scenario == "expired", "{scenario}");
        assert_eq!(
            f.account().status,
            if scenario == "expired" {
                Status::Stopped
            } else {
                Status::NeedsAttention
            }
        );
    }
}
#[test]
fn retired_binding_survives_reopen_and_blocks_replacement_after_discovery_window() {
    let f = Fixture::new();
    let (old, players) = pending_stop(&f);
    f.engine
        .close_retired(
            &old,
            &players,
            Instant::now(),
            &mut CloseAttempts::default(),
            || Ok(players.clone()),
            |_, _| Ok(()),
        )
        .unwrap();
    f.engine
        .transaction(|s| {
            s.database.retired_launches[0].deadline = Utc::now() - ChronoDuration::seconds(1);
            Ok(())
        })
        .unwrap();
    let reopened = Engine::open(Store::new(f.path.clone()).unwrap(), "test".into()).unwrap();
    assert!(reopened.command("1", "start").is_err());
    assert!(reopened.remove("1").is_err());
    reopened
        .transaction(|s| {
            let a = account_mut(s, "2")?;
            a.desired_running = false;
            a.process = None;
            a.tracker = None;
            for saved in &mut s.database.accounts {
                saved.encrypted_session = platform::protect("synthetic-credential")?;
            }
            Ok(())
        })
        .unwrap();
    let backup = reopened.backup().unwrap();
    let restored = Store::new(f.path.clone())
        .unwrap()
        .read_backup(&backup)
        .unwrap();
    assert!(
        restored
            .accounts
            .iter()
            .all(|a| a.account.close_intent.is_none())
    );
    assert!(
        reopened.restore(&backup).is_err(),
        "restore must retain expired but still-owned retired evidence"
    );
    let db = Store::new(f.path.clone()).unwrap().load().unwrap();
    assert_eq!(db.retired_launches[0].process.as_ref().unwrap().pid, 4201);
}
#[test]
fn concurrent_commands_and_close_ticks_never_target_another_accounts_client() {
    let f = Fixture::new();
    let other = f.engine.snapshot().accounts[1].process.clone();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut threads = Vec::new();
    for actions in [["restart", "stop", "start"], ["stop", "start", "restart"]] {
        let engine = f.engine.clone();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            for _ in 0..4 {
                for action in actions {
                    let _ = engine.command("1", action);
                }
            }
        }));
    }
    barrier.wait();
    let mut attempts = CloseAttempts::default();
    for _ in 0..12 {
        let a = f.account();
        let players = f.players();
        let _ = f.engine.close_owned(
            &a,
            Instant::now(),
            &mut attempts,
            || Ok(players),
            |p, _| {
                assert_eq!(p.pid, 4201);
                assert_eq!(p.creation_time, "synthetic-1");
                Ok(())
            },
        );
    }
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(f.engine.snapshot().accounts[1].process, other);
}
#[cfg(windows)]
#[test]
fn native_windows_close_rejects_a_reused_or_unrelated_process_identity() {
    let identity = ProcessIdentity {
        pid: std::process::id(),
        creation_time: "synthetic-wrong-creation-time".into(),
        tracker: "123456789012".into(),
        generation: Uuid::new_v4(),
    };
    assert!(platform::close(&identity, false).is_err());
    assert!(platform::close(&identity, true).is_err());
}

#[test]
fn legacy_retired_record_observes_without_closing() {
    let path = std::env::temp_dir().join(format!("pr07-legacy-{}", Uuid::new_v4()));
    let engine = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
    let old: RetiredLaunch = serde_json::from_value(serde_json::json!({
        "tracker":"123456789012", "generation":Uuid::new_v4(), "account_id":"1", "deadline":Utc::now()+ChronoDuration::seconds(90)
    })).unwrap();
    engine
        .transaction(|s| {
            s.database.accounts.push(SavedAccount {
                account: Account::new("1".into(), "test".into()),
                encrypted_session: String::new(),
            });
            s.database.retired_launches.push(old.clone());
            Ok(())
        })
        .unwrap();
    let players = vec![platform::Player {
        pid: 4242,
        creation_time: "synthetic-1".into(),
        tracker: old.tracker.clone(),
    }];
    let mut closed = false;
    engine
        .close_retired(
            &old,
            &players,
            Instant::now(),
            &mut CloseAttempts::default(),
            || Ok(players.clone()),
            |_, _| {
                closed = true;
                Ok(())
            },
        )
        .unwrap();
    assert!(!closed);
    assert_eq!(engine.snapshot().accounts[0].status, Status::NeedsAttention);
    let db = Store::new(path.clone()).unwrap().load().unwrap();
    assert_eq!(db.retired_launches[0].process.as_ref().unwrap().pid, 4242);
    assert!(db.retired_launches[0].close_intent.is_none());
    engine.command("1", "stop").unwrap();
    let db = Store::new(path.clone()).unwrap().load().unwrap();
    engine
        .close_retired(
            &db.retired_launches[0],
            &players,
            Instant::now(),
            &mut CloseAttempts::default(),
            || Ok(players.clone()),
            |_, _| {
                closed = true;
                Ok(())
            },
        )
        .unwrap();
    assert!(
        closed,
        "explicit Stop must recover a uniquely observed legacy client"
    );
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn explicit_stop_closes_only_the_verified_owned_process() {
    let path = std::env::temp_dir().join(format!("pr07-{}", Uuid::new_v4()));
    let engine = Engine::open(Store::new(path.clone()).unwrap(), "test".into()).unwrap();
    engine
        .transaction(|s| {
            let mut a = Account::new("1".into(), "test".into());
            a.desired_running = true;
            a.process = Some(ProcessIdentity {
                pid: 4242,
                creation_time: "synthetic-1".into(),
                tracker: "123456789012".into(),
                generation: a.generation,
            });
            a.tracker = Some("123456789012".into());
            s.database.accounts.push(SavedAccount {
                account: a,
                encrypted_session: String::new(),
            });
            Ok(())
        })
        .unwrap();
    engine.command("1", "stop").unwrap();
    let a = engine.snapshot().accounts.remove(0);
    let identity = a.process.clone().unwrap();
    let player = platform::Player {
        pid: 4242,
        creation_time: "synthetic-1".into(),
        tracker: "123456789012".into(),
    };
    let mut closed = Vec::new();
    engine
        .close_owned(
            &a,
            Instant::now(),
            &mut CloseAttempts::default(),
            || Ok(vec![player]),
            |p, force| {
                closed.push((p.clone(), force));
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(closed, vec![(identity, false)]);
    drop(engine);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn a_new_recovery_episode_cannot_reuse_the_previous_episodes_force_timer() {
    for target_replacement in [false, true] {
        let f = Fixture::new();
        f.engine
            .patch(
                "1",
                AccountPatch {
                    fallback_policy: Some(FallbackPolicy::AllowPublic),
                    ..Default::default()
                },
            )
            .unwrap();
        let now = Instant::now();
        let mut attempts = CloseAttempts::default();
        if target_replacement {
            let a = f.account();
            f.engine
                .signal("1", a.generation, Signal::TargetUnavailable);
        } else {
            f.recovery();
        }
        assert_eq!(
            f.tick(&f.account(), now, &mut attempts),
            vec![!target_replacement]
        );
        let a = f.account();
        f.engine.signal("1", a.generation, Signal::Connected);
        // Both events occur before another monitor tick can discard the old timer.
        if target_replacement {
            f.engine
                .signal("1", a.generation, Signal::TargetUnavailable);
        } else {
            f.recovery();
        }
        assert_eq!(
            f.tick(&f.account(), now + Duration::from_secs(40), &mut attempts),
            vec![!target_replacement]
        );
    }
}
#[test]
fn confirmed_live_exit_preserves_stop_authority_for_a_separate_retired_client() {
    let f = Fixture::new();
    let original = f.account();
    let retired_generation = Uuid::new_v4();
    f.engine
        .transaction(|s| {
            s.database.retired_launches.push(RetiredLaunch {
                tracker: "987654321012".into(),
                generation: retired_generation,
                account_id: "1".into(),
                deadline: Utc::now() + ChronoDuration::seconds(90),
                close_intent: None,
                process: Some(ProcessIdentity {
                    pid: 5300,
                    creation_time: "retired-synthetic".into(),
                    tracker: "987654321012".into(),
                    generation: retired_generation,
                }),
            });
            Ok(())
        })
        .unwrap();
    f.engine.command("1", "stop").unwrap();
    let stopped = f.account();
    f.engine
        .owned_exit_observed(&stopped, stopped.process.as_ref().unwrap(), false);
    assert!(f.account().process.is_none());
    let old = Store::new(f.path.clone())
        .unwrap()
        .load()
        .unwrap()
        .retired_launches
        .remove(0);
    let players = vec![platform::Player {
        pid: 5300,
        creation_time: "retired-synthetic".into(),
        tracker: old.tracker.clone(),
    }];
    let mut effects = Vec::new();
    f.engine
        .close_retired(
            &old,
            &players,
            Instant::now(),
            &mut CloseAttempts::default(),
            || Ok(players.clone()),
            |p, force| {
                effects.push((p.pid, force));
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(effects, vec![(5300, false)]);
    // A stale exit observation cannot change the current Stop's desired state.
    f.engine
        .owned_exit_observed(&original, original.process.as_ref().unwrap(), false);
    assert!(!f.account().desired_running);
}

#[test]
fn legacy_intent_without_episode_identity_requires_and_accepts_fresh_stop() {
    let f = Fixture::new();
    f.engine.command("1", "stop").unwrap();
    let store = Store::new(f.path.clone()).unwrap();
    let mut legacy = serde_json::to_value(store.load().unwrap()).unwrap();
    legacy["accounts"][0]["account"]["close_intent"]
        .as_object_mut()
        .unwrap()
        .remove("id");
    store
        .save(&serde_json::from_value(legacy).unwrap())
        .unwrap();
    let engine = Engine::open(store, "legacy".into()).unwrap();
    let a = engine.snapshot().accounts.remove(0);
    let players = f.players();
    let _ = engine.close_owned(
        &a,
        Instant::now(),
        &mut CloseAttempts::default(),
        || Ok(players),
        |_, _| panic!("legacy ID-less intent must not close"),
    );
    engine.command("1", "stop").unwrap();
    let a = engine.snapshot().accounts.remove(0);
    let players = f.players();
    let mut closed = false;
    engine
        .close_owned(
            &a,
            Instant::now(),
            &mut CloseAttempts::default(),
            || Ok(players),
            |_, force| {
                assert!(!force);
                closed = true;
                Ok(())
            },
        )
        .unwrap();
    assert!(closed);
}

#[test]
fn merged_recovery_reserves_retry_before_close_and_preserves_rate_limit() {
    let f = Fixture::new();
    f.recovery();
    let deadline = Utc::now() + ChronoDuration::seconds(120);
    let a = f.account();
    f.engine
        .update("1", a.generation, |a| {
            a.recovery_reason = "RateLimit".into();
            a.next_retry = Some(deadline);
        })
        .unwrap();
    let a = f.account();
    let mut attempts = CloseAttempts::default();
    let players = f.players();
    assert!(
        f.engine
            .close_owned(
                &a,
                Instant::now(),
                &mut attempts,
                || Ok(players),
                |_, _| {
                    let saved = f.engine.0.store.load().unwrap().accounts.remove(0).account;
                    assert_eq!(
                        saved.failures, 1,
                        "retry must be durable before the close effect"
                    );
                    assert!(saved.next_retry.is_some_and(|time| time >= deadline));
                    Ok(())
                }
            )
            .unwrap()
    );
    assert_eq!(
        f.account().recovery_evidence,
        RecoveryEvidence::RecoveryStarted
    );
}

#[test]
fn merged_recovery_at_retry_cap_preserves_the_live_client() {
    let f = Fixture::new();
    f.recovery();
    let a = f.account();
    f.engine
        .update("1", a.generation, |a| a.failures = 4)
        .unwrap();
    let mut attempts = CloseAttempts::default();
    assert!(
        f.tick(&f.account(), Instant::now(), &mut attempts)
            .is_empty()
    );
    assert_eq!(f.account().failures, 5);
    assert_eq!(f.account().status, Status::NeedsAttention);
    assert!(f.account().process.is_some());
}

#[test]
fn native_kick_log_recovers_instead_of_waiting_for_confirmation() {
    let f = Fixture::new();
    let a = f.account();
    let identity = a.process.clone().unwrap();
    let log = f.path.join("native-kick.log");
    // Native network messages from the reported client; no numeric 267 is logged.
    std::fs::write(&log, concat!(
        "2026-10-22T20:38:09.411Z,15627.411133,51ec,7 [FLog::Network] Players service requests player disconnect\n",
        "2026-10-22T20:38:09.469Z,15627.469727,5ccc,7,Warning [FLog::Network] Lost connection with reason : You have been kicked from the game\n",
        "2026-10-22T20:38:11.176Z,15629.176758,5ccc,6,Info [DFLog::RbxTransportDummyClient] Connection closed by server for reason: RCC: 288 (DisconnectServerShutdown)\n",
    )).unwrap();
    let mut tail = crate::logs::Tail::new(log, false).unwrap();
    for event in tail.read_events().unwrap() {
        f.engine
            .observe("1", a.generation, &identity, Observation::DiskLog(event));
    }
    assert_eq!(
        f.account().recovery_evidence,
        RecoveryEvidence::ConfirmedTerminalDisconnect
    );
    assert_eq!(f.account().status, Status::Reconnecting);
    assert!(f.account().last_error.unwrap().contains("267"));
    let mut attempts = CloseAttempts::default();
    assert_eq!(
        f.tick(&f.account(), Instant::now(), &mut attempts),
        vec![true]
    );
    assert!(!launch_ready(
        &f.account(),
        Utc::now() + ChronoDuration::hours(1)
    ));
    let closing = f.account();
    f.engine.owned_exit_observed(&closing, &identity, true);
    assert!(launch_ready(
        &f.account(),
        Utc::now() + ChronoDuration::hours(1)
    ));
    assert_eq!(f.account().failures, 1);
}

#[test]
fn terminal_disconnect_terminates_the_client_before_relaunch() {
    for code in [267, 273, 279] {
        let f = Fixture::new();
        let other = f.engine.snapshot().accounts[1].process.clone();
        let a = f.account();
        let identity = a.process.clone().unwrap();
        f.engine.observe(
            "1",
            a.generation,
            &identity,
            Observation::DiskLog(
                crate::logs::parse_event(&format!(
                    "[FLog::Network] Sending disconnect with reason: {code}"
                ))
                .unwrap(),
            ),
        );
        let mut attempts = CloseAttempts::default();
        assert_eq!(
            f.tick(&f.account(), Instant::now(), &mut attempts),
            vec![true],
            "disconnect {code} must terminate the process, including its tray lifetime"
        );
        assert!(!launch_ready(
            &f.account(),
            Utc::now() + ChronoDuration::hours(1)
        ));
        let closing = f.account();
        f.engine.owned_exit_observed(&closing, &identity, true);
        assert!(launch_ready(
            &f.account(),
            Utc::now() + ChronoDuration::hours(1)
        ));
        assert_eq!(f.engine.snapshot().accounts[1].process, other);
    }
}

#[test]
fn kicked_267_closes_then_rejoins_only_after_exit() {
    let f = Fixture::new();
    let a = f.account();
    let identity = a.process.clone().unwrap();
    f.engine.observe(
        "1",
        a.generation,
        &identity,
        Observation::DiskLog(
            crate::logs::parse_event("[FLog::Network] Sending disconnect with reason: 267")
                .unwrap(),
        ),
    );
    let mut attempts = CloseAttempts::default();
    let now = Instant::now();
    assert_eq!(f.tick(&f.account(), now, &mut attempts), vec![true]);
    assert!(!launch_ready(
        &f.account(),
        Utc::now() + ChronoDuration::hours(1)
    ));
    assert_eq!(
        f.tick(&f.account(), now + Duration::from_secs(6), &mut attempts),
        vec![true]
    );
    let closing = f.account();
    f.engine.owned_exit_observed(&closing, &identity, true);
    assert!(launch_ready(
        &f.account(),
        Utc::now() + ChronoDuration::hours(1)
    ));
    assert_eq!(f.account().failures, 1);
}

#[test]
fn start_cannot_replace_an_unresolved_expired_launch() {
    let f = Fixture::new();
    let a = f.account();
    f.engine
        .update("1", a.generation, |a| {
            a.process = None;
            a.status = Status::NeedsAttention;
            a.ownership_deadline = Some(Utc::now() - ChronoDuration::seconds(1));
        })
        .unwrap();
    assert!(f.engine.command("1", "start").is_err());
    assert_eq!(f.account().tracker, a.tracker);
    f.engine.command("1", "stop").unwrap();
    assert!(f.engine.command("1", "start").is_err());
}

#[test]
fn global_close_stops_recovery_and_includes_untracked_players() {
    let f = Fixture::new();
    let mut players = f.players();
    players.push(platform::Player {
        pid: 9999,
        creation_time: "external".into(),
        tracker: String::new(),
    });
    let mut closed = Vec::new();
    assert_eq!(
        f.engine
            .close_all_with(
                || Ok(players),
                |p| {
                    closed.push(p.pid);
                    Ok(())
                }
            )
            .unwrap(),
        3
    );
    assert_eq!(closed, vec![4201, 4202, 9999]);
    let saved = Store::new(f.path.clone()).unwrap().load().unwrap();
    assert!(
        saved
            .accounts
            .iter()
            .all(|a| !a.account.desired_running && a.account.next_retry.is_none())
    );
    assert!(
        f.engine
            .snapshot()
            .accounts
            .iter()
            .all(|a| !launch_ready(a, Utc::now()))
    );
}

#[test]
fn global_close_persistence_failure_has_no_effect_and_partial_failure_stops_rejoin() {
    let f = Fixture::new();
    std::fs::create_dir(f.path.join("accounts.json.tmp")).unwrap();
    assert!(
        f.engine
            .close_all_with(
                || panic!("must persist before discovery"),
                |_| panic!("must not close")
            )
            .is_err()
    );
    std::fs::remove_dir(f.path.join("accounts.json.tmp")).unwrap();
    let players = f.players();
    let mut calls = 0;
    let error = f
        .engine
        .close_all_with(
            || Ok(players),
            |_| {
                calls += 1;
                if calls == 1 {
                    Err("access denied".into())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert_eq!(calls, 2);
    assert!(error.contains("Closed 1") && error.contains("1 could not"));
    assert!(
        f.engine
            .snapshot()
            .accounts
            .iter()
            .all(|a| !a.desired_running)
    );
}

#[test]
fn blocked_start_cleanup_requires_consent_and_protects_saved_launches() {
    let f = Fixture::new();
    f.engine.0.launch_allowed.store(false, Ordering::Release);
    f.engine
        .transaction(|s| {
            f.engine
                .cleanup_launch_conflict(s, |_| panic!("cleanup requires consent"))?;
            s.database.external_background_cleanup_enabled = true;
            f.engine.cleanup_launch_conflict(s, |protected| {
                assert_eq!(protected.len(), 2);
                assert_eq!(
                    protected[0],
                    s.database.accounts[0].account.process.clone().unwrap()
                );
                f.engine.0.launch_allowed.store(true, Ordering::Release);
                Ok(())
            })?;
            f.engine
                .cleanup_launch_conflict(s, |_| panic!("ready launches need no cleanup"))?;
            f.engine.0.launch_allowed.store(false, Ordering::Release);
            s.database.accounts[0].account.process = None;
            assert!(
                f.engine
                    .cleanup_launch_conflict(s, |_| panic!("unresolved clients stay protected"))
                    .is_err()
            );
            Ok(())
        })
        .unwrap();
}
