use super::*;
use crate::model::ServerSelection;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerPage {
    data: Vec<PublicServer>,
    next_page_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublicServer {
    id: String,
    playing: u32,
    max_players: u32,
    #[serde(default)]
    ping: Option<f64>,
}

impl PublicServer {
    fn ping(&self) -> Option<f64> {
        self.ping.filter(|p| p.is_finite() && *p > 0.0)
    }
}

#[cfg(test)]
fn choose_server(servers: &[PublicServer], mode: ServerSelection, seats: u32) -> Option<Uuid> {
    let candidates: Vec<_> = servers
        .iter()
        .filter_map(|s| {
            let id = Uuid::parse_str(&s.id).ok().filter(|id| !id.is_nil())?;
            (s.max_players.saturating_sub(s.playing) >= seats.max(1)).then_some((s, id))
        })
        .collect();
    // Reported ping is a server statistic, not a local latency or region measurement.
    // Prefer the least populated server within 25ms of the best reported ping.
    let best_ping = candidates
        .iter()
        .filter_map(|(s, _)| s.ping())
        .reduce(f64::min);
    candidates
        .into_iter()
        .filter(|(s, _)| {
            mode != ServerSelection::Smart
                || best_ping.is_none_or(|best| s.ping().is_some_and(|p| p <= best + 25.0))
        })
        .min_by(|(a, aid), (b, bid)| {
            let population = if mode == ServerSelection::Highest {
                b.playing.cmp(&a.playing)
            } else {
                a.playing.cmp(&b.playing)
            };
            population
                .then_with(|| {
                    a.ping()
                        .unwrap_or(f64::MAX)
                        .total_cmp(&b.ping().unwrap_or(f64::MAX))
                })
                .then_with(|| aid.cmp(bid))
        })
        .map(|(_, id)| id)
}

// One best ping per population preserves the original ranking without retaining every server.
#[derive(Default)]
struct ServerRanking {
    by_population: std::collections::BTreeMap<u32, (Option<f64>, Uuid)>,
    best_ping: Option<f64>,
    listed: bool,
}
impl ServerRanking {
    fn add(&mut self, servers: &[PublicServer], seats: u32) {
        self.listed |= !servers.is_empty();
        for server in servers {
            let Ok(id) = Uuid::parse_str(&server.id) else {
                continue;
            };
            if id.is_nil() || server.max_players.saturating_sub(server.playing) < seats.max(1) {
                continue;
            }
            let ping = server.ping();
            if let Some(ping) = ping {
                self.best_ping = Some(self.best_ping.map_or(ping, |best| best.min(ping)));
            }
            let entry = self
                .by_population
                .entry(server.playing)
                .or_insert((ping, id));
            if (ping.unwrap_or(f64::MAX), id) < (entry.0.unwrap_or(f64::MAX), entry.1) {
                *entry = (ping, id);
            }
        }
    }
    fn choice(&self, mode: ServerSelection) -> Option<Uuid> {
        let eligible = |(_, (ping, _)): &(&u32, &(Option<f64>, Uuid))| {
            mode != ServerSelection::Smart
                || self
                    .best_ping
                    .is_none_or(|best| ping.is_some_and(|p| p <= best + 25.0))
        };
        if mode == ServerSelection::Highest {
            self.by_population.iter().rev().find(eligible)
        } else {
            self.by_population.iter().find(eligible)
        }
        .map(|(_, (_, id))| *id)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresencePage {
    user_presences: Vec<Presence>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Presence {
    user_id: u64,
    user_presence_type: u8,
    place_id: Option<u64>,
    game_id: Option<String>,
}
fn friend_job(presences: &[Presence], users: &[u64], place: u64) -> Option<Uuid> {
    users.iter().find_map(|user| {
        presences
            .iter()
            .find(|p| p.user_id == *user)
            .filter(|p| p.user_presence_type == 2 && p.place_id == Some(place))
            .and_then(|p| p.game_id.as_deref())
            .and_then(|id| Uuid::parse_str(id).ok())
            .filter(|id| !id.is_nil())
    })
}

impl Roblox {
    pub async fn select_public_server(
        &self,
        cookie: &str,
        place: u64,
        mode: ServerSelection,
        seats: u32,
    ) -> Result<Option<Uuid>, Failure> {
        let header = Self::cookie_header(cookie)?;
        let mut cursor = None;
        let mut seen = std::collections::HashSet::new();
        let mut ranking = ServerRanking::default();
        let default_url = format!("https://games.roblox.com/v1/games/{place}/servers/Public");
        #[cfg(not(test))]
        let url = default_url.as_str();
        #[cfg(test)]
        let url = self.servers_url.as_deref().unwrap_or(&default_url);
        loop {
            let mut request = self.client.get(url).header(COOKIE, header.clone()).query(&[
                (
                    "sortOrder",
                    if mode == ServerSelection::Highest {
                        "Desc"
                    } else {
                        "Asc"
                    },
                ),
                ("limit", "100"),
                ("excludeFullGames", "true"),
            ]);
            if let Some(value) = &cursor {
                request = request.query(&[("cursor", value)]);
            }
            let response = request.send().await.map_err(|_| Failure::network())?;
            Self::check(response.status(), "Public server selection")?;
            let page: ServerPage = response.json().await.map_err(|_| {
                Failure::new(
                    FailureKind::Unsupported,
                    "Roblox public server response was not recognized",
                )
            })?;
            ranking.add(&page.data, seats);
            if mode != ServerSelection::Smart
                && let Some(id) = ranking.choice(mode)
            {
                return Ok(Some(id));
            }
            cursor = page.next_page_cursor.filter(|c| !c.is_empty());
            if cursor.is_none() {
                break;
            }
            if !seen.insert(cursor.clone().unwrap()) {
                return Err(Failure::new(
                    FailureKind::Unsupported,
                    "Roblox repeated a server-page cursor; search stopped to avoid a loop",
                ));
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        if !ranking.listed {
            return Ok(None);
        } // Let Roblox start a server for an empty game.
        ranking.choice(mode).map(Some).ok_or_else(|| Failure::new(FailureKind::Other, "No listed public server has enough open slots; retry later or choose another destination"))
    }

    pub async fn friend_server(
        &self,
        cookie: &str,
        users: &[u64],
        place: u64,
    ) -> Result<Option<Uuid>, Failure> {
        let url = "https://presence.roblox.com/v1/presence/users";
        #[cfg(test)]
        let url = self.presence_url.as_deref().unwrap_or(url);
        let request = self
            .client
            .post(url)
            .header(COOKIE, Self::cookie_header(cookie)?)
            .json(&serde_json::json!({"userIds": users}));
        let first = request
            .try_clone()
            .ok_or_else(|| Failure::new(FailureKind::Other, "Unable to prepare friend lookup"))?
            .send()
            .await
            .map_err(|_| Failure::network())?;
        let response = if first.status() == StatusCode::FORBIDDEN
            && let Some(csrf) = first.headers().get("x-csrf-token").cloned()
        {
            request
                .header("x-csrf-token", csrf)
                .send()
                .await
                .map_err(|_| Failure::network())?
        } else {
            first
        };
        Self::check(response.status(), "Same-preset friend lookup")?;
        let page: PresencePage = response.json().await.map_err(|_| {
            Failure::new(
                FailureKind::Unsupported,
                "Roblox presence response was not recognized",
            )
        })?;
        Ok(friend_job(&page.user_presences, users, place))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample_servers(count: usize) -> Vec<PublicServer> {
        (0..count)
            .map(|i| PublicServer {
                id: Uuid::from_u128(i as u128 + 1).to_string(),
                playing: (i % 50) as u32,
                max_players: 50,
                ping: (i % 7 != 0).then_some((20 + i % 250) as f64),
            })
            .collect()
    }
    #[test]
    fn streamed_ranking_matches_original_for_all_modes_and_seat_counts() {
        let servers = sample_servers(10_000);
        for mode in [
            ServerSelection::Smart,
            ServerSelection::Lowest,
            ServerSelection::Highest,
        ] {
            for seats in [0, 1, 10, 49, 50, 51] {
                let mut ranking = ServerRanking::default();
                for page in servers.chunks(100) {
                    ranking.add(page, seats);
                }
                assert_eq!(ranking.choice(mode), choose_server(&servers, mode, seats));
                assert!(ranking.by_population.len() <= 50);
            }
        }
    }
    #[test]
    #[ignore = "run release build explicitly for timing"]
    fn hotfix_benchmark_server_ranking() {
        for count in [500, 10_000] {
            let servers = sample_servers(count);
            let mut reports = Vec::new();
            for old in [true, false] {
                let start = std::time::Instant::now();
                let mut rounds = 0u64;
                while start.elapsed().as_secs_f64() < 2.0 {
                    let choice = if old {
                        choose_server(&servers, ServerSelection::Smart, 3)
                    } else {
                        let mut ranking = ServerRanking::default();
                        for page in servers.chunks(100) {
                            ranking.add(page, 3);
                        }
                        ranking.choice(ServerSelection::Smart)
                    };
                    std::hint::black_box(choice);
                    rounds += 1;
                }
                reports.push(serde_json::json!({"mode":if old {"baseline"} else {"new"}, "mean_ns":start.elapsed().as_nanos() as f64 / rounds as f64,"rounds":rounds}));
            }
            println!(
                "HOTFIX_BENCH {}",
                serde_json::json!({"workload":"ranking only; excludes HTTP, pagination and JSON decoding","servers":count,"reports":reports})
            );
        }
    }
    #[tokio::test]
    async fn smart_search_reaches_capacity_match_after_page_five() {
        use axum::{Json, Router, extract::Query, routing::get};
        let app = Router::new().route("/servers", get(|Query(query): Query<std::collections::HashMap<String, String>>| async move {
            let page = query.get("cursor").and_then(|c| c.parse::<u32>().ok()).unwrap_or(0);
            Json(serde_json::json!({
                "data": [{"id": Uuid::from_u128(page as u128 + 1).to_string(), "playing": if page < 5 {9} else {1}, "maxPlayers":10,"ping":if page < 5 {250} else {30}}],
                "nextPageCursor": if page < 6 {Some((page+1).to_string())} else {None}
            }))
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let worker = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut roblox = Roblox::new().unwrap();
        roblox.servers_url = Some(format!("http://{address}/servers"));
        let result = roblox
            .select_public_server("TEST", 1, ServerSelection::Smart, 3)
            .await;
        worker.abort();
        assert_eq!(result.unwrap().unwrap().as_u128(), 6);
    }
    #[test]
    fn ranking_and_friend_presence_respect_capacity_missing_ping_and_place() {
        let data = serde_json::json!({"data": [
            {"id":"00000000-0000-0000-0000-000000000001","playing":1,"maxPlayers":10,"ping":250},
            {"id":"00000000-0000-0000-0000-000000000002","playing":3,"maxPlayers":10,"ping":60},
            {"id":"00000000-0000-0000-0000-000000000003","playing":7,"maxPlayers":10,"ping":40},
            {"id":"00000000-0000-0000-0000-000000000004","playing":0,"maxPlayers":10,"ping":0},
            {"id":"00000000-0000-0000-0000-000000000005","playing":10,"maxPlayers":10,"ping":10},
            {"id":"invalid","playing":0,"maxPlayers":10,"ping":1}
        ], "nextPageCursor":null});
        let page: ServerPage = serde_json::from_value(data).unwrap();
        assert_eq!(
            choose_server(&page.data, ServerSelection::Smart, 1)
                .unwrap()
                .as_u128(),
            2
        );
        assert_eq!(
            choose_server(&page.data, ServerSelection::Lowest, 1)
                .unwrap()
                .as_u128(),
            4
        );
        assert_eq!(
            choose_server(&page.data, ServerSelection::Highest, 1)
                .unwrap()
                .as_u128(),
            3
        );
        assert_eq!(
            choose_server(&page.data, ServerSelection::Highest, 5)
                .unwrap()
                .as_u128(),
            2
        );
        assert!(choose_server(&page.data, ServerSelection::Smart, 11).is_none());
        let unknown: ServerPage = serde_json::from_value(serde_json::json!({"data":[{"id":"00000000-0000-0000-0000-000000000001","playing":0,"maxPlayers":2}],"nextPageCursor":null})).unwrap();
        assert!(choose_server(&unknown.data, ServerSelection::Smart, 1).is_some());
        let presence: PresencePage = serde_json::from_value(serde_json::json!({"userPresences":[
            {"userId":1,"userPresenceType":2,"placeId":99,"gameId":"00000000-0000-0000-0000-000000000003"},
            {"userId":2,"userPresenceType":2,"placeId":10,"gameId":"00000000-0000-0000-0000-000000000002"},
            {"userId":3,"userPresenceType":2,"placeId":null,"gameId":null}
        ]})).unwrap();
        assert_eq!(
            friend_job(&presence.user_presences, &[1, 2, 3], 10)
                .unwrap()
                .as_u128(),
            2
        );
        assert!(friend_job(&presence.user_presences, &[1, 3], 10).is_none());
    }
}
