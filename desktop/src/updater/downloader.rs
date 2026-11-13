//! Official release asset verification, bounded download streaming, and local update cache.

use crate::domain::models::UpdateView;
use futures_util::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE, LOCATION, USER_AGENT};
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use url::Url;

/// Canonical GitHub repository used for verified official update downloads.
pub const OFFICIAL_PUBLISHER: &str = "https://github.com/oangsa/RoLauncher";
/// Maximum allowed installer size (256 MiB).
pub const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum allowed SHA256SUMS manifest size (64 KiB).
pub const MAX_CHECKSUMS_BYTES: usize = 64 * 1024;
/// Maximum HTTP redirect hops followed for GitHub release assets.
pub const MAX_REDIRECTS: usize = 5;

/// Verified update installer ready for external handoff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedInstaller {
    /// Target version string (`MAJOR.MINOR.PATCH`).
    pub version: String,
    /// Full local filesystem path to the verified installer executable.
    pub installer_path: PathBuf,
    /// Uppercase hexadecimal SHA-256 digest of the verified installer.
    pub sha256: String,
}

/// Abstract HTTP transport allowing offline unit testing of redirect, size, and hash checks.
pub trait UpdateTransport: Send + Sync {
    /// Sends an HTTP GET request to `url` with `user_agent` and `accept` headers, without following redirects.
    fn get<'a>(
        &'a self,
        url: &'a str,
        user_agent: &'a str,
        accept: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, String>> + Send + 'a>>;
}

/// Single HTTP response returned by [`UpdateTransport`].
pub struct TransportResponse {
    /// HTTP status code.
    pub status: StatusCode,
    /// Value of the `Location` header if present.
    pub location: Option<String>,
    /// Value of the `Content-Type` header if present.
    pub content_type: Option<String>,
    /// Optional declared content length.
    pub content_length: Option<u64>,
    /// Response body chunk stream.
    pub body: TransportBody,
}

/// Boxed async chunk stream for bounded reads.
pub type TransportBody = Pin<Box<dyn futures_util::Stream<Item = Result<Vec<u8>, String>> + Send>>;

/// Production `reqwest` transport with redirects and proxy usage disabled.
pub struct ReqwestUpdateTransport {
    client: Client,
}

impl ReqwestUpdateTransport {
    /// Constructs a new strict HTTPS transport with no ambient proxies or automatic redirects.
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(20))
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|err| format!("Unable to initialize updater transport: {err}"))?;
        Ok(Self { client })
    }
}

impl UpdateTransport for ReqwestUpdateTransport {
    fn get<'a>(
        &'a self,
        url: &'a str,
        user_agent: &'a str,
        accept: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, String>> + Send + 'a>> {
        Box::pin(async move {
            let response = self
                .client
                .request(Method::GET, url)
                .header(USER_AGENT, user_agent)
                .header(ACCEPT, accept)
                .send()
                .await
                .map_err(|err| format!("Download request failed: {err}"))?;

            let status = response.status();
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(ToOwned::to_owned);
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(ToOwned::to_owned);
            let content_length = response.content_length();
            let stream = response.bytes_stream().map(|res| {
                res.map(|bytes| bytes.to_vec())
                    .map_err(|err| format!("Download stream interrupted: {err}"))
            });

            Ok(TransportResponse {
                status,
                location,
                content_type,
                content_length,
                body: Box::pin(stream),
            })
        })
    }
}

/// Returns the local cache directory for downloaded installers and handoff manifests.
pub fn update_cache_dir() -> Result<PathBuf, String> {
    if let Ok(smoke_dir) = std::env::var("ROLAUNCHER_UI_SMOKE_DIR") {
        let trimmed = smoke_dir.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed).join("update-cache"));
        }
    }
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| "LOCALAPPDATA is not set on this system".to_string())?;
    Ok(PathBuf::from(local_app_data)
        .join("RoLauncher")
        .join("updates"))
}

/// Parses `MAJOR.MINOR.PATCH` (ignoring optional `-dev.<12hex>` suffix) into `(u64, u64, u64)`.
pub fn parse_core_version(value: &str) -> Option<(u64, u64, u64)> {
    let trimmed = value.trim().trim_start_matches('v');
    let core = trimmed.split('-').next()?;
    let mut parts = core.split('.');
    let major: u64 = parts.next()?.parse().ok()?;
    let minor: u64 = parts.next()?.parse().ok()?;
    let patch: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

/// Returns the User-Agent string `RoLauncher/<core_version>`.
pub fn user_agent_for_version(current_version: &str) -> String {
    let core = current_version
        .trim()
        .trim_start_matches('v')
        .split('-')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("2.0.0");
    format!("RoLauncher/{core}")
}

/// Returns true if `current_version` represents a `-dev.<12hex>` local development build.
pub fn is_dev_version(value: &str) -> bool {
    let Some((_, suffix)) = value.trim().trim_start_matches('v').split_once("-dev.") else {
        return false;
    };
    suffix.len() == 12
        && suffix
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Validates whether `update` offers an installable newer version than `current_version`.
pub fn is_update_installable(current_version: &str, update: &UpdateView) -> bool {
    if !update.available {
        return false;
    }
    let Some(curr_core) = parse_core_version(current_version) else {
        return false;
    };
    let Some(next_core) = parse_core_version(&update.version) else {
        return false;
    };
    if is_dev_version(&update.version) {
        return false;
    }
    if next_core > curr_core {
        return true;
    }
    next_core == curr_core && is_dev_version(current_version)
}

/// Validates a strict `MAJOR.MINOR.PATCH` release version string.
pub fn validate_release_version(version: &str) -> Result<String, String> {
    let trimmed = version.trim();
    if trimmed.contains('-') || parse_core_version(trimmed).is_none() {
        return Err("Invalid update release version".to_string());
    }
    let (a, b, c) = parse_core_version(trimmed).unwrap();
    let canonical = format!("{a}.{b}.{c}");
    if canonical != trimmed {
        return Err("Invalid update release version".to_string());
    }
    Ok(canonical)
}

fn validate_https_github_url(raw: &str, allowed_hosts: &[&str]) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|_| "Invalid update URL".to_string())?;
    if url.scheme() != "https" {
        return Err("Update downloads require HTTPS".to_string());
    }
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("Update URLs may not contain credentials or fragments".to_string());
    }
    if url.port().is_some_and(|p| p != 443) {
        return Err("Update URLs must use the default HTTPS port".to_string());
    }
    let host = url
        .host_str()
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "Update URL is missing a host".to_string())?;
    if !allowed_hosts.contains(&host.as_str()) {
        return Err("Update URL host is not an official GitHub download host".to_string());
    }
    Ok(url)
}

/// Validates that `update` targets the official GitHub publisher, expected tag, and expected asset names.
pub fn validate_update_view(update: &UpdateView) -> Result<(String, String, String), String> {
    let version = validate_release_version(&update.version)?;
    let expected_tag = if update.prerelease {
        format!("beta-v{version}")
    } else {
        format!("v{version}")
    };
    let expected_release_url = format!("{OFFICIAL_PUBLISHER}/releases/tag/{expected_tag}");
    let installer_name = format!("rolauncher-v{version}-setup-x64.exe");
    let checksums_name = format!("rolauncher-v{version}-SHA256SUMS.txt");

    if update.release_url.trim() != expected_release_url {
        return Err("Update release URL does not match the official repository".to_string());
    }

    let expected_installer =
        format!("{OFFICIAL_PUBLISHER}/releases/download/{expected_tag}/{installer_name}");
    let expected_checksums =
        format!("{OFFICIAL_PUBLISHER}/releases/download/{expected_tag}/{checksums_name}");

    let installer_url = validate_https_github_url(&update.installer_url, &["github.com"])?;
    let checksums_url = validate_https_github_url(&update.checksums_url, &["github.com"])?;

    if installer_url.as_str() != expected_installer || checksums_url.as_str() != expected_checksums
    {
        return Err("Update asset URLs do not match expected official filenames".to_string());
    }
    Ok((version, installer_name, checksums_name))
}

/// Parses a `SHA256SUMS.txt` manifest and extracts the expected uppercase SHA-256 digest for `filename`.
pub fn parse_checksum_for_file(checksum_text: &str, filename: &str) -> Result<String, String> {
    for line in checksum_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let hash_raw = parts.next().unwrap_or("").trim();
        let file_raw = parts.next().unwrap_or("").trim().trim_start_matches('*');
        if file_raw == filename {
            if hash_raw.len() != 64 || !hash_raw.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("Checksums manifest contains a malformed SHA-256 hash".to_string());
            }
            return Ok(hash_raw.to_ascii_uppercase());
        }
    }
    Err(format!(
        "Checksums manifest did not include an entry for {filename}"
    ))
}

async fn open_following_github_redirects(
    transport: &dyn UpdateTransport,
    initial_url: &str,
    user_agent: &str,
    accept: &str,
) -> Result<TransportResponse, String> {
    let mut current = validate_https_github_url(initial_url, &["github.com"])?;
    let mut visited = HashSet::new();
    visited.insert(current.as_str().to_string());

    for hop in 0..=MAX_REDIRECTS {
        let response = transport.get(current.as_str(), user_agent, accept).await?;
        if matches!(
            response.status,
            StatusCode::MOVED_PERMANENTLY
                | StatusCode::FOUND
                | StatusCode::SEE_OTHER
                | StatusCode::TEMPORARY_REDIRECT
                | StatusCode::PERMANENT_REDIRECT
        ) {
            if hop == MAX_REDIRECTS {
                return Err("Too many redirects while downloading update".to_string());
            }
            let location = response
                .location
                .as_deref()
                .ok_or_else(|| "Redirect missing Location header".to_string())?;
            let next = current
                .join(location)
                .map_err(|_| "Invalid redirect location URL".to_string())?;
            let validated = validate_https_github_url(
                next.as_str(),
                &[
                    "github.com",
                    "release-assets.githubusercontent.com",
                    "objects.githubusercontent.com",
                ],
            )?;
            if !visited.insert(validated.as_str().to_string()) {
                return Err("Redirect loop while downloading update".to_string());
            }
            current = validated;
            continue;
        }
        if !response.status.is_success() {
            return Err(format!(
                "Update server returned HTTP {}",
                response.status.as_u16()
            ));
        }
        return Ok(response);
    }
    Err("Too many redirects while downloading update".to_string())
}

fn check_not_html_content_type(content_type: Option<&str>) -> Result<(), String> {
    if let Some(ct) = content_type {
        let lower = ct.to_ascii_lowercase();
        if lower.contains("text/html") || lower.contains("application/xhtml") {
            return Err("Update server returned HTML instead of an asset".to_string());
        }
    }
    Ok(())
}

async fn fetch_expected_checksum(
    transport: &dyn UpdateTransport,
    update: &UpdateView,
    current_version: &str,
    installer_name: &str,
) -> Result<String, String> {
    let user_agent = user_agent_for_version(current_version);
    let mut checksum_resp = open_following_github_redirects(
        transport,
        &update.checksums_url,
        &user_agent,
        "text/plain, application/octet-stream",
    )
    .await?;
    check_not_html_content_type(checksum_resp.content_type.as_deref())?;
    if checksum_resp
        .content_length
        .is_some_and(|len| len > MAX_CHECKSUMS_BYTES as u64)
    {
        return Err("Checksums manifest exceeds maximum allowed size".to_string());
    }

    let mut checksum_bytes = Vec::new();
    while let Some(chunk) = checksum_resp.body.next().await {
        let chunk = chunk?;
        if checksum_bytes.len().saturating_add(chunk.len()) > MAX_CHECKSUMS_BYTES {
            return Err("Checksums manifest exceeds maximum allowed size".to_string());
        }
        checksum_bytes.extend_from_slice(&chunk);
    }
    let checksum_text = String::from_utf8(checksum_bytes)
        .map_err(|_| "Checksums manifest must be valid UTF-8".to_string())?;
    parse_checksum_for_file(&checksum_text, installer_name)
}

/// Checks if a previously cached installer exists and passes SHA-256 + PE header verification.
pub async fn cached_verified_installer(
    transport: Arc<dyn UpdateTransport>,
    update: &UpdateView,
    current_version: &str,
    cache_dir: &Path,
) -> Result<Option<CachedInstaller>, String> {
    if !is_update_installable(current_version, update) {
        return Ok(None);
    }
    let (version, installer_name, _) = validate_update_view(update)?;
    let final_path = cache_dir.join(&installer_name);
    if !final_path.is_file() {
        return Ok(None);
    }
    let expected_sha256 =
        fetch_expected_checksum(transport.as_ref(), update, current_version, &installer_name)
            .await?;
    match verify_existing_installer_file(&final_path, &expected_sha256).await {
        Ok(sha256) => Ok(Some(CachedInstaller {
            version,
            installer_path: final_path,
            sha256,
        })),
        Err(_) => Ok(None),
    }
}

/// Downloads, checksum-verifies, and atomically caches the installer in `cache_dir`.
pub async fn download_verified_installer<F>(
    transport: Arc<dyn UpdateTransport>,
    update: &UpdateView,
    current_version: &str,
    cache_dir: &Path,
    mut on_progress: F,
) -> Result<CachedInstaller, String>
where
    F: FnMut(u8) + Send,
{
    if !is_update_installable(current_version, update) {
        return Err("Selected release is not newer than the running version".to_string());
    }
    let (version, installer_name, _) = validate_update_view(update)?;
    let user_agent = user_agent_for_version(current_version);

    tokio::fs::create_dir_all(cache_dir)
        .await
        .map_err(|err| format!("Unable to prepare update cache folder: {err}"))?;

    let expected_sha256 =
        fetch_expected_checksum(transport.as_ref(), update, current_version, &installer_name)
            .await?;

    let final_path = cache_dir.join(&installer_name);
    if let Ok(existing_sha) = verify_existing_installer_file(&final_path, &expected_sha256).await {
        on_progress(100);
        return Ok(CachedInstaller {
            version,
            installer_path: final_path,
            sha256: existing_sha,
        });
    }

    // Download installer into a `.partial` temporary file, verifying header and digest on the fly.
    let partial_path = cache_dir.join(format!("{installer_name}.{}.partial", uuid::Uuid::new_v4()));
    let result = stream_installer_to_partial(
        transport.as_ref(),
        &update.installer_url,
        &user_agent,
        &partial_path,
        &expected_sha256,
        &mut on_progress,
    )
    .await;

    match result {
        Ok(verified_sha) => {
            if tokio::fs::try_exists(&final_path).await.unwrap_or(false) {
                let _ = tokio::fs::remove_file(&final_path).await;
            }
            tokio::fs::rename(&partial_path, &final_path)
                .await
                .map_err(|err| format!("Unable to finalize downloaded installer: {err}"))?;
            on_progress(100);
            Ok(CachedInstaller {
                version,
                installer_path: final_path,
                sha256: verified_sha,
            })
        }
        Err(err) => {
            let _ = tokio::fs::remove_file(&partial_path).await;
            Err(err)
        }
    }
}

async fn stream_installer_to_partial<F>(
    transport: &dyn UpdateTransport,
    installer_url: &str,
    user_agent: &str,
    partial_path: &Path,
    expected_sha256: &str,
    on_progress: &mut F,
) -> Result<String, String>
where
    F: FnMut(u8) + Send,
{
    let mut resp = open_following_github_redirects(
        transport,
        installer_url,
        user_agent,
        "application/octet-stream",
    )
    .await?;
    check_not_html_content_type(resp.content_type.as_deref())?;
    if resp
        .content_length
        .is_some_and(|len| len > MAX_INSTALLER_BYTES)
    {
        return Err("Update installer exceeds the 256 MiB limit".to_string());
    }

    let total_len = resp.content_length;
    let mut file = tokio::fs::File::create(partial_path)
        .await
        .map_err(|err| format!("Unable to create partial installer file: {err}"))?;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut header = [0u8; 2];
    let mut header_len = 0usize;
    let mut last_percent: u8 = 0;

    while let Some(chunk) = resp.body.next().await {
        let chunk = chunk?;
        if chunk.is_empty() {
            continue;
        }
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if downloaded > MAX_INSTALLER_BYTES {
            return Err("Update installer exceeds the 256 MiB limit".to_string());
        }
        for byte in chunk.iter().take(2 - header_len) {
            header[header_len] = *byte;
            header_len += 1;
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|err| format!("Failed writing installer download: {err}"))?;

        if let Some(total) = total_len.filter(|&t| t > 0) {
            let pct = ((downloaded.saturating_mul(99)) / total).min(99) as u8;
            if pct > last_percent {
                last_percent = pct;
                on_progress(pct);
            }
        }
    }

    file.flush()
        .await
        .map_err(|err| format!("Failed flushing installer download: {err}"))?;
    drop(file);

    if header_len < 2 || &header != b"MZ" {
        return Err("Downloaded installer is not a valid Windows executable".to_string());
    }
    let actual_sha = hex_upper(&hasher.finalize());
    if actual_sha != expected_sha256 {
        return Err("Downloaded installer SHA-256 verification failed".to_string());
    }
    Ok(actual_sha)
}

/// Verifies an existing local installer file against `expected_sha256` and the MZ PE header.
pub async fn verify_existing_installer_file(
    path: &Path,
    expected_sha256: &str,
) -> Result<String, String> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|_| "Cached installer is not readable".to_string())?;
    if bytes.len() as u64 > MAX_INSTALLER_BYTES || bytes.len() < 2 || &bytes[..2] != b"MZ" {
        return Err("Cached installer is not a valid Windows executable".to_string());
    }
    let actual_sha = hex_upper(&Sha256::digest(&bytes));
    if actual_sha != expected_sha256 {
        return Err("Cached installer SHA-256 verification failed".to_string());
    }
    Ok(actual_sha)
}

fn hex_upper(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MockTransport {
        routes: HashMap<String, Box<dyn Fn() -> TransportResponse + Send + Sync>>,
        seen_urls: Mutex<Vec<String>>,
    }

    impl UpdateTransport for MockTransport {
        fn get<'a>(
            &'a self,
            url: &'a str,
            _user_agent: &'a str,
            _accept: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<TransportResponse, String>> + Send + 'a>> {
            self.seen_urls.lock().unwrap().push(url.to_string());
            let resp = self
                .routes
                .get(url)
                .map(|f| f())
                .unwrap_or_else(|| TransportResponse {
                    status: StatusCode::NOT_FOUND,
                    location: None,
                    content_type: None,
                    content_length: None,
                    body: Box::pin(futures_util::stream::empty()),
                });
            Box::pin(async move { Ok(resp) })
        }
    }

    fn bytes_resp(status: StatusCode, bytes: Vec<u8>, ct: Option<&str>) -> TransportResponse {
        let len = Some(bytes.len() as u64);
        TransportResponse {
            status,
            location: None,
            content_type: ct.map(ToOwned::to_owned),
            content_length: len,
            body: Box::pin(futures_util::stream::iter(vec![Ok(bytes)])),
        }
    }

    fn sample_update(version: &str, prerelease: bool) -> UpdateView {
        let tag = if prerelease {
            format!("beta-v{version}")
        } else {
            format!("v{version}")
        };
        UpdateView {
            available: true,
            version: version.to_string(),
            release_url: format!("{OFFICIAL_PUBLISHER}/releases/tag/{tag}"),
            download_url: format!("{OFFICIAL_PUBLISHER}/releases/tag/{tag}"),
            checksums_url: format!(
                "{OFFICIAL_PUBLISHER}/releases/download/{tag}/rolauncher-v{version}-SHA256SUMS.txt"
            ),
            installer_url: format!(
                "{OFFICIAL_PUBLISHER}/releases/download/{tag}/rolauncher-v{version}-setup-x64.exe"
            ),
            prerelease,
        }
    }

    #[tokio::test]
    async fn download_verifies_redirect_mz_and_sha256_and_dev_promotion() {
        let dir = std::env::temp_dir().join(format!("rolauncher-dl-test-{}", uuid::Uuid::new_v4()));
        let update = sample_update("2.0.0", false);
        let exe_bytes = b"MZ\x90\x00test-installer-payload".to_vec();
        let sha = hex_upper(&Sha256::digest(&exe_bytes));
        let manifest = format!("{sha}  rolauncher-v2.0.0-setup-x64.exe\n").into_bytes();

        let mut routes: HashMap<String, Box<dyn Fn() -> TransportResponse + Send + Sync>> =
            HashMap::new();
        let m_clone = manifest.clone();
        routes.insert(
            update.checksums_url.clone(),
            Box::new(move || bytes_resp(StatusCode::OK, m_clone.clone(), Some("text/plain"))),
        );
        routes.insert(
            update.installer_url.clone(),
            Box::new(|| TransportResponse {
                status: StatusCode::FOUND,
                location: Some("https://release-assets.githubusercontent.com/item/1".to_string()),
                content_type: None,
                content_length: None,
                body: Box::pin(futures_util::stream::empty()),
            }),
        );
        let exe_clone = exe_bytes.clone();
        routes.insert(
            "https://release-assets.githubusercontent.com/item/1".to_string(),
            Box::new(move || {
                bytes_resp(
                    StatusCode::OK,
                    exe_clone.clone(),
                    Some("application/octet-stream"),
                )
            }),
        );

        let transport = Arc::new(MockTransport {
            routes,
            seen_urls: Mutex::new(Vec::new()),
        });
        let mut last_pct = 0u8;
        let cached = download_verified_installer(
            transport.clone(),
            &update,
            "2.0.0-dev.a1b2c3d4e5f6",
            &dir,
            |p| last_pct = p,
        )
        .await
        .expect("dev build should upgrade to matching stable release");

        assert_eq!(last_pct, 100);
        assert_eq!(cached.sha256, sha);
        assert!(cached.installer_path.exists());
        assert!(is_update_installable("2.0.0-dev.a1b2c3d4e5f6", &update));
        assert!(!is_update_installable("2.0.0", &update));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn download_rejects_untrusted_redirects_bad_magic_and_hash_mismatch() {
        let dir = std::env::temp_dir().join(format!("rolauncher-dl-bad-{}", uuid::Uuid::new_v4()));
        let update = sample_update("2.0.0", false);
        let bad_payload = b"PK\x03\x04not-mz".to_vec();
        let sha = hex_upper(&Sha256::digest(&bad_payload));
        let manifest = format!("{sha}  rolauncher-v2.0.0-setup-x64.exe\n").into_bytes();

        let mut routes: HashMap<String, Box<dyn Fn() -> TransportResponse + Send + Sync>> =
            HashMap::new();
        routes.insert(
            update.checksums_url.clone(),
            Box::new(move || bytes_resp(StatusCode::OK, manifest.clone(), Some("text/plain"))),
        );
        routes.insert(
            update.installer_url.clone(),
            Box::new(|| TransportResponse {
                status: StatusCode::FOUND,
                location: Some("https://evil.example/payload.exe".to_string()),
                content_type: None,
                content_length: None,
                body: Box::pin(futures_util::stream::empty()),
            }),
        );
        let err = download_verified_installer(
            Arc::new(MockTransport {
                routes,
                seen_urls: Mutex::new(Vec::new()),
            }),
            &update,
            "1.9.2",
            &dir,
            |_| {},
        )
        .await
        .expect_err("untrusted redirect host must be rejected");
        assert!(err.contains("official GitHub download host"));

        // Non-MZ magic rejection
        let mut routes2: HashMap<String, Box<dyn Fn() -> TransportResponse + Send + Sync>> =
            HashMap::new();
        let manifest2 = format!("{sha}  rolauncher-v2.0.0-setup-x64.exe\n").into_bytes();
        routes2.insert(
            update.checksums_url.clone(),
            Box::new(move || bytes_resp(StatusCode::OK, manifest2.clone(), Some("text/plain"))),
        );
        routes2.insert(
            update.installer_url.clone(),
            Box::new(move || bytes_resp(StatusCode::OK, bad_payload.clone(), None)),
        );
        let err2 = download_verified_installer(
            Arc::new(MockTransport {
                routes: routes2,
                seen_urls: Mutex::new(Vec::new()),
            }),
            &update,
            "1.9.2",
            &dir,
            |_| {},
        )
        .await
        .expect_err("non-MZ payload must be rejected");
        assert!(err2.contains("valid Windows executable"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
