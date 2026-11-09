use super::*;

pub(super) fn same_preset(a: &Account, b: &Account) -> bool {
    a.id != b.id
        && a.game_profile_id.is_some()
        && a.game_profile_id == b.game_profile_id
        && a.target == b.target
        && a.effective_target() == b.effective_target()
}

fn peer_join_pending(a: &Account, now: chrono::DateTime<Utc>) -> bool {
    matches!(
        a.status,
        Status::Launching | Status::Unknown | Status::Running
    ) && a.ownership_deadline.is_some_and(|deadline| deadline > now)
}

impl Engine {
    pub(super) async fn select_launch_target(
        &self,
        account: &Account,
        mut target: Target,
        cookie: &str,
    ) -> Result<Option<Target>, Failure> {
        // Explicit destinations always take precedence over public-server preferences.
        if target.job_id.is_some() || target.private_server_link.is_some() {
            return Ok(Some(target));
        }
        let (selection, peers, seats, pending_peer) = {
            let state = self.0.state.lock().unwrap();
            let db = &state.database;
            let together = db.join_type == JoinType::Friend && account.game_profile_id.is_some();
            let members: Vec<_> = db
                .accounts
                .iter()
                .filter(|s| {
                    together && same_preset(account, &s.account) && s.account.desired_running
                })
                .collect();
            let peers: Vec<_> = members
                .iter()
                .filter(|s| s.account.process.is_some() || s.account.tracker.is_some())
                .filter_map(|s| {
                    s.account
                        .id
                        .parse::<u64>()
                        .ok()
                        .map(|id| (id, s.encrypted_session.clone()))
                })
                .collect();
            let pending = members
                .iter()
                .any(|s| peer_join_pending(&s.account, Utc::now()));
            (
                db.server_selection,
                peers,
                (members.len() + 1) as u32,
                pending,
            )
        };
        target.job_id = if peers.is_empty() {
            self.0
                .roblox
                .select_public_server(cookie, target.place_id, selection, seats)
                .await?
        } else {
            let users: Vec<_> = peers.iter().map(|(id, _)| *id).collect();
            let mut job = self
                .0
                .roblox
                .friend_server(cookie, &users, target.place_id)
                .await?;
            if job.is_none() {
                // Roblox can hide a friend's presence even when the accounts are friends.
                // Managed peers can report their own server through their saved session.
                for (user, session) in peers {
                    let Ok(peer_cookie) = platform::unprotect(&session) else {
                        continue;
                    };
                    match self
                        .0
                        .roblox
                        .friend_server(&peer_cookie, &[user], target.place_id)
                        .await
                    {
                        Ok(Some(id)) => {
                            job = Some(id);
                            break;
                        }
                        Err(e)
                            if matches!(e.kind, FailureKind::Network | FailureKind::RateLimit) =>
                        {
                            return Err(e);
                        }
                        _ => {} // A stale peer session must not invalidate the joining account.
                    }
                }
            }
            match job {
                Some(job) => Some(job),
                None if pending_peer => return Ok(None),
                None => {
                    return Err(Failure::new(
                        FailureKind::Other,
                        "Same-preset server could not be confirmed in the configured place. Keep a managed peer running there and check Roblox join permissions; no separate server will be selected",
                    ));
                }
            }
        };
        Ok(Some(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn friend_groups_require_explicit_same_preset_and_destination() {
        let mut a = Account::new("1".into(), "one".into());
        a.target = Some(Target {
            place_id: 1,
            job_id: None,
            private_server_link: None,
        });
        let mut b = a.clone();
        b.id = "2".into();
        assert!(!same_preset(&a, &b));
        let now = Utc::now();
        a.ownership_deadline = Some(now + ChronoDuration::seconds(90));
        for status in [Status::Launching, Status::Unknown, Status::Running] {
            a.status = status;
            assert!(peer_join_pending(&a, now));
            assert!(!peer_join_pending(&a, now + ChronoDuration::seconds(91)));
        }
        a.status = Status::NeedsAttention;
        assert!(!peer_join_pending(&a, now));
        a.game_profile_id = Some(Uuid::new_v4());
        b.game_profile_id = a.game_profile_id;
        assert!(same_preset(&a, &b));
        b.game_profile_id = Some(Uuid::new_v4());
        assert!(!same_preset(&a, &b));
        b.game_profile_id = a.game_profile_id;
        b.target.as_mut().unwrap().place_id = 2;
        assert!(!same_preset(&a, &b));
        b.target = a.target.clone();
        b.public_fallback_active = true;
        assert!(same_preset(&a, &b)); // A failed public join must not split the group via fallback.
        a.target.as_mut().unwrap().job_id = Some(Uuid::new_v4());
        b.target = a.target.clone();
        assert!(!same_preset(&a, &b));
    }
}
