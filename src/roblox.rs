use crate::model::Target;
use reqwest::{
    Client, StatusCode,
    header::{COOKIE, HeaderValue},
};
use serde::Deserialize;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Network,
    RateLimit,
    Auth,
    Permission,
    TargetUnavailable,
    Unsupported,
    Other,
}
#[derive(Clone, Debug)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
}
impl Failure {
    pub fn new(kind: FailureKind, message: &str) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub fn network() -> Self {
        Self::new(
            FailureKind::Network,
            "Roblox could not be reached; recovery is suspended until connectivity returns",
        )
    }
}

#[derive(Deserialize)]
pub struct User {
    pub id: u64,
    pub name: String,
}

#[derive(Clone)]
pub struct Roblox {
    client: Client,
}
impl Roblox {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(20))
            .user_agent(concat!("RoLauncher/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| "Unable to initialize HTTPS client")?;
        Ok(Self { client })
    }
    fn cookie_header(cookie: &str) -> Result<HeaderValue, Failure> {
        let mut value = HeaderValue::from_str(&format!(".ROBLOSECURITY={cookie}"))
            .map_err(|_| Failure::new(FailureKind::Auth, "Invalid session cookie"))?;
        value.set_sensitive(true);
        Ok(value)
    }
    pub async fn user(&self, cookie: &str) -> Result<User, Failure> {
        let response = self
            .client
            .get("https://users.roblox.com/v1/users/authenticated")
            .header(COOKIE, Self::cookie_header(cookie)?)
            .send()
            .await
            .map_err(|_| Failure::network())?;
        Self::check(response.status(), "Account validation")?;
        response.json().await.map_err(|_| {
            Failure::new(
                FailureKind::Unsupported,
                "Roblox identity response was not recognized",
            )
        })
    }
    fn check(status: StatusCode, stage: &str) -> Result<(), Failure> {
        if status.is_success() {
            return Ok(());
        }
        let mut failure = match status {
            StatusCode::UNAUTHORIZED => {
                Failure::new(FailureKind::Auth, "Session expired; sign in again")
            }
            StatusCode::FORBIDDEN => Failure::new(
                FailureKind::Permission,
                "Roblox requires authentication or permission; sign in again",
            ),
            StatusCode::TOO_MANY_REQUESTS => Failure::new(
                FailureKind::RateLimit,
                "Roblox rate limit; retry will be delayed",
            ),
            _ if status.is_server_error() => Failure::network(),
            _ => Failure::new(
                FailureKind::Other,
                "Roblox request failed; check the account and destination",
            ),
        };
        failure.message = format!("{stage}: {} (HTTP {})", failure.message, status.as_u16());
        Err(failure)
    }
    pub async fn reachable(&self) -> bool {
        self.client
            .get("https://users.roblox.com/v1/users/1")
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }
    pub async fn game_details(
        &self,
        cookie: &str,
        place_id: u64,
    ) -> Result<(String, Option<String>), Failure> {
        let response = self
            .client
            .get("https://games.roblox.com/v1/games/multiget-place-details")
            .query(&[("placeIds", place_id.to_string())])
            .header(COOKIE, Self::cookie_header(cookie)?)
            .send()
            .await
            .map_err(|_| Failure::network())?;
        Self::check(response.status(), "Game lookup")?;
        let details: serde_json::Value = response
            .json()
            .await
            .map_err(|_| Failure::new(FailureKind::Other, "Game details unavailable"))?;
        let game = details
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| Failure::new(FailureKind::TargetUnavailable, "Game not found"))?;
        let name = game["name"]
            .as_str()
            .ok_or_else(|| Failure::new(FailureKind::Other, "Game name unavailable"))?
            .to_string();
        let universe = game["universeId"]
            .as_u64()
            .ok_or_else(|| Failure::new(FailureKind::Other, "Game universe unavailable"))?;
        // A delayed thumbnail must not prevent saving a destination.
        let thumbnail = async {
            let response = self
                .client
                .get("https://thumbnails.roblox.com/v1/games/icons")
                .query(&[
                    ("universeIds", universe.to_string()),
                    ("size", "150x150".into()),
                    ("format", "Png".into()),
                    ("isCircular", "false".into()),
                ])
                .header(COOKIE, Self::cookie_header(cookie).ok()?)
                .send()
                .await
                .ok()?;
            let value: serde_json::Value = response.json().await.ok()?;
            value["data"][0]["imageUrl"].as_str().map(str::to_string)
        }
        .await;
        Ok((name, thumbnail))
    }
    pub async fn launch_uri(
        &self,
        cookie: &str,
        target: &Target,
        tracker: &str,
    ) -> Result<Zeroizing<String>, Failure> {
        target
            .validate()
            .map_err(|_| Failure::new(FailureKind::Other, "Invalid launch destination"))?;
        let header = Self::cookie_header(cookie)?;
        let private = if let Some(link) = &target.private_server_link {
            Some(
                self.resolve_private(header.clone(), target.place_id, link)
                    .await?,
            )
        } else {
            None
        };
        let mut request = self.ticket_request(header.clone(), target.place_id);
        let first = request
            .try_clone()
            .ok_or_else(|| {
                Failure::new(
                    FailureKind::Other,
                    "Unable to prepare authentication request",
                )
            })?
            .send()
            .await
            .map_err(|_| Failure::network())?;
        let response = if first.status() == StatusCode::FORBIDDEN {
            let csrf = first
                .headers()
                .get("x-csrf-token")
                .cloned()
                .ok_or_else(|| {
                    Failure::new(
                        FailureKind::Auth,
                        "Session requires reauthentication; sign in again",
                    )
                })?;
            request = request.header("x-csrf-token", csrf);
            request.send().await.map_err(|_| Failure::network())?
        } else {
            first
        };
        Self::check(response.status(), "Authentication ticket")?;
        let ticket = Zeroizing::new(
            response
                .headers()
                .get("rbx-authentication-ticket")
                .and_then(|v| v.to_str().ok())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    Failure::new(
                        FailureKind::Unsupported,
                        "Roblox did not provide an authentication ticket",
                    )
                })?
                .to_owned(),
        );
        let mut join = url::Url::parse("https://assetgame.roblox.com/game/PlaceLauncher.ashx")
            .expect("constant URL");
        if let Some((access, code)) = private {
            join.query_pairs_mut()
                .append_pair("request", "RequestPrivateGame")
                .append_pair("accessCode", &access)
                .append_pair("linkCode", &code);
        } else if let Some(job) = target.job_id {
            join.query_pairs_mut()
                .append_pair("request", "RequestGameJob")
                .append_pair("gameId", &job.to_string());
        } else {
            join.query_pairs_mut().append_pair("request", "RequestGame");
        }
        join.query_pairs_mut()
            .append_pair("placeId", &target.place_id.to_string())
            .append_pair("browserTrackerId", tracker)
            .append_pair("isPlayTogetherGame", "false");
        let encoded: String =
            url::form_urlencoded::byte_serialize(join.as_str().as_bytes()).collect();
        Ok(Zeroizing::new(format!(
            "roblox-player:1+launchmode:play+gameinfo:{}+launchtime:{}+placelauncherurl:{}+browsertrackerid:{}+robloxLocale:en_us+gameLocale:en_us+channel:+LaunchExp:InApp",
            *ticket,
            chrono::Utc::now().timestamp_millis(),
            encoded,
            tracker
        )))
    }
    fn ticket_request(&self, cookie: HeaderValue, place_id: u64) -> reqwest::RequestBuilder {
        // Roblox requires JSON media type even though this endpoint has no parameters.
        // An absent Content-Type passes the CSRF challenge but then receives HTTP 415.
        self.client
            .post("https://auth.roblox.com/v1/authentication-ticket/")
            .header(COOKIE, cookie)
            .header(
                "Referer",
                format!("https://www.roblox.com/games/{place_id}"),
            )
            .json(&serde_json::json!({}))
    }
    async fn resolve_private(
        &self,
        header: HeaderValue,
        place: u64,
        link: &str,
    ) -> Result<(Zeroizing<String>, String), Failure> {
        let parsed = url::Url::parse(link)
            .map_err(|_| Failure::new(FailureKind::Other, "Invalid private link"))?;
        if let Some((_, code)) = parsed
            .query_pairs()
            .find(|(k, _)| k == "privateServerLinkCode")
        {
            let code = code.into_owned();
            let access = self.private_page(header, place, &code).await?;
            return Ok((access, code));
        }
        let code = parsed
            .query_pairs()
            .find(|(k, _)| k == "code")
            .map(|(_, v)| v.into_owned())
            .ok_or_else(|| {
                Failure::new(FailureKind::Other, "Private share link is missing its code")
            })?;
        let request = self
            .client
            .post("https://apis.roblox.com/sharelinks/v1/resolve-link")
            .header(COOKIE, header.clone())
            .header("Referer", "https://www.roblox.com/")
            .header("Origin", "https://www.roblox.com")
            .json(&serde_json::json!({"linkId":code,"linkType":"Server"}));
        let first = request
            .try_clone()
            .ok_or_else(|| {
                Failure::new(FailureKind::Other, "Unable to prepare share-link request")
            })?
            .send()
            .await
            .map_err(|_| Failure::network())?;
        let response = if first.status() == StatusCode::FORBIDDEN {
            let csrf = first
                .headers()
                .get("x-csrf-token")
                .cloned()
                .ok_or_else(|| {
                    Failure::new(
                        FailureKind::Permission,
                        "Private-server link requires authentication or permission",
                    )
                })?;
            request
                .header("x-csrf-token", csrf)
                .send()
                .await
                .map_err(|_| Failure::network())?
        } else {
            first
        };
        if response.status() == StatusCode::NOT_FOUND {
            return Err(Failure::new(
                FailureKind::TargetUnavailable,
                "Private-server share link no longer exists",
            ));
        }
        Self::check(response.status(), "Private share-link resolution")?;
        let value: serde_json::Value = response.json().await.map_err(|_| {
            Failure::new(
                FailureKind::Unsupported,
                "Share-link response format was not recognized",
            )
        })?;
        let (access, link_code) = share_invite(&value, place)?;
        let access = match access {
            Some(a) => Zeroizing::new(a),
            None => self.private_page(header, place, &link_code).await?,
        };
        Ok((access, link_code))
    }
    async fn private_page(
        &self,
        header: HeaderValue,
        place: u64,
        code: &str,
    ) -> Result<Zeroizing<String>, Failure> {
        let mut page = url::Url::parse(&format!("https://www.roblox.com/games/{place}"))
            .expect("constant URL");
        page.query_pairs_mut()
            .append_pair("privateServerLinkCode", code);
        for _ in 0..4 {
            let response = self
                .client
                .get(page.clone())
                .header(COOKIE, header.clone())
                .send()
                .await
                .map_err(|_| Failure::network())?;
            if response.status().is_redirection() {
                let next = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| page.join(v).ok())
                    .ok_or_else(|| {
                        Failure::new(
                            FailureKind::Unsupported,
                            "Private-server redirect was not recognized",
                        )
                    })?;
                if next.scheme() != "https"
                    || !matches!(next.host_str(), Some("www.roblox.com" | "web.roblox.com"))
                    || !next.username().is_empty()
                    || next.password().is_some()
                    || next.port().is_some()
                {
                    return Err(Failure::new(
                        FailureKind::Unsupported,
                        "Private-server redirect left the allowed Roblox hosts",
                    ));
                }
                page = next;
                continue;
            }
            Self::check(response.status(), "Private-server page")?;
            let html = Zeroizing::new(response.text().await.map_err(|_| Failure::network())?);
            return private_access_code(&html).map(|code|Zeroizing::new(code.to_owned())).ok_or_else(||Failure::new(FailureKind::Unsupported,"Roblox private-server page format was not recognized; refusing to guess the destination"));
        }
        Err(Failure::new(
            FailureKind::Unsupported,
            "Too many private-server redirects",
        ))
    }
}

fn share_invite(
    value: &serde_json::Value,
    place: u64,
) -> Result<(Option<String>, String), Failure> {
    let fail = || {
        Failure::new(
            FailureKind::Unsupported,
            "Roblox share-link invite format was not recognized; use a full private-server game URL",
        )
    };
    let invite = value.get("privateServerInviteData").ok_or_else(fail)?;
    if invite
        .get("placeId")
        .and_then(|v| v.as_u64())
        .is_some_and(|id| id != place)
    {
        return Err(Failure::new(
            FailureKind::Other,
            "Private-server link belongs to a different PlaceId",
        ));
    }
    let code = invite
        .get("linkCode")
        .and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_u64().map(|n| n.to_string()))
        })
        .ok_or_else(fail)?;
    if code.is_empty()
        || code.len() > 512
        || !code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(fail());
    }
    let access = invite
        .get("accessCode")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    if access
        .as_ref()
        .is_some_and(|code| uuid::Uuid::parse_str(code).is_err())
    {
        return Err(fail());
    }
    Ok((access, code))
}

pub fn normalize_cookie(value: &str) -> Result<Zeroizing<String>, String> {
    let cookie = value
        .trim()
        .strip_prefix(".ROBLOSECURITY=")
        .unwrap_or(value.trim());
    if cookie.is_empty()
        || cookie.len() > 16384
        || cookie.chars().any(|c| c.is_control() || c == ';')
    {
        return Err("Invalid session cookie; paste only the cookie value".into());
    }
    Ok(Zeroizing::new(cookie.to_owned()))
}

fn private_access_code(html: &str) -> Option<&str> {
    let tail = html.split("Roblox.GameLauncher.joinPrivateGame(").nth(1)?;
    let args = tail.split(')').next()?;
    let code = args.split(',').nth(1)?.trim().trim_matches(['\'', '"']);
    uuid::Uuid::parse_str(code).ok()?;
    Some(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ticket_requests_preserve_json_body_and_sensitive_cookie_on_csrf_retry() {
        let roblox = Roblox::new().unwrap();
        let request = roblox.ticket_request(Roblox::cookie_header("test-secret").unwrap(), 123);
        let first = request.try_clone().unwrap().build().unwrap();
        let retry = request.header("x-csrf-token", "test-csrf").build().unwrap();
        for request in [&first, &retry] {
            assert_eq!(request.method(), reqwest::Method::POST);
            assert_eq!(request.headers()["content-type"], "application/json");
            assert_eq!(request.body().unwrap().as_bytes().unwrap(), b"{}");
            assert!(request.headers()[COOKIE].is_sensitive());
            assert_eq!(
                request.headers()["referer"],
                "https://www.roblox.com/games/123"
            );
        }
        assert_eq!(retry.headers()["x-csrf-token"], "test-csrf");
    }
    #[test]
    fn failures_identify_stage_and_status_without_response_data() {
        let failure =
            Roblox::check(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Authentication ticket").unwrap_err();
        assert_eq!(failure.kind, FailureKind::Other);
        assert!(failure.message.contains("Authentication ticket:"));
        assert!(failure.message.contains("HTTP 415"));
        assert_eq!(
            Roblox::check(StatusCode::TOO_MANY_REQUESTS, "Authentication ticket")
                .unwrap_err()
                .kind,
            FailureKind::RateLimit
        );
        assert_eq!(
            Roblox::check(StatusCode::UNAUTHORIZED, "Account validation")
                .unwrap_err()
                .kind,
            FailureKind::Auth
        );
    }
    #[test]
    fn cookie_validation_and_private_parser() {
        assert!(normalize_cookie("cookie\r\nInjected: value").is_err());
        assert_eq!(
            &*normalize_cookie(".ROBLOSECURITY=example").unwrap(),
            "example"
        );
        assert_eq!(
            private_access_code(
                "Roblox.GameLauncher.joinPrivateGame(123, '12345678-1234-1234-1234-123456789012')"
            ),
            Some("12345678-1234-1234-1234-123456789012")
        );
        assert!(private_access_code("unexpected page").is_none());
    }
    #[test]
    fn share_invites_reject_unknown_formats_and_wrong_places() {
        let good = serde_json::json!({"privateServerInviteData":{"placeId":1,"linkCode":"12345678901234567890","accessCode":"12345678-1234-1234-1234-123456789012"}});
        assert!(share_invite(&good, 1).is_ok());
        assert!(share_invite(&good, 2).is_err());
        assert!(share_invite(&serde_json::json!({"error":"unknown"}), 1).is_err());
    }
}
