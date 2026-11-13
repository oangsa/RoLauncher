//! Validated asynchronous HTTPS thumbnail loader converting decoded images to GPUI RenderImages.

use crate::theme::AppTheme;
use gpui::{AnyElement, RenderImage, div, img, prelude::*, px};
use image::Frame;
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use url::Url;

const MAX_THUMBNAIL_BYTES: usize = 4 * 1024 * 1024;

/// Cache entry state for one Roblox experience thumbnail URL.
#[derive(Clone)]
pub enum ThumbnailState {
    Loading,
    Loaded(Arc<RenderImage>),
    Failed,
}

/// Manages validated HTTPS Roblox thumbnail downloads and decoded BGRA `RenderImage` caching.
pub struct ThumbnailCache {
    client: Client,
    entries: HashMap<String, ThumbnailState>,
    in_flight: HashSet<String>,
}

impl ThumbnailCache {
    /// Constructs a proxy-free, redirect-disabled HTTPS client for thumbnail downloads.
    pub fn new() -> Self {
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(6))
            .timeout(std::time::Duration::from_secs(12))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            entries: HashMap::new(),
            in_flight: HashSet::new(),
        }
    }

    /// Validates that a URL uses HTTPS against an official Roblox CDN thumbnail host.
    pub fn is_allowed_thumbnail_url(raw: &str) -> bool {
        let Ok(url) = Url::parse(raw.trim()) else {
            return false;
        };
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some_and(|p| p != 443)
        {
            return false;
        }
        let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
            return false;
        };
        host == "thumbnails.roblox.com" || host == "rbxcdn.com" || host.ends_with(".rbxcdn.com")
    }

    /// Returns cached thumbnail state if already fetched or pending.
    pub fn get(&self, url: &str) -> Option<&ThumbnailState> {
        self.entries.get(url)
    }

    /// Begins fetching `url` on `runtime` if not already loaded or in flight, calling `on_done` when ready.
    pub fn request_load<F>(&mut self, url: String, runtime: &tokio::runtime::Handle, on_done: F)
    where
        F: FnOnce(String, Option<Arc<RenderImage>>) + Send + 'static,
    {
        if url.trim().is_empty() || !Self::is_allowed_thumbnail_url(&url) {
            return;
        }
        if self.entries.contains_key(&url) || !self.in_flight.insert(url.clone()) {
            return;
        }
        self.entries.insert(url.clone(), ThumbnailState::Loading);

        let client = self.client.clone();
        runtime.spawn(async move {
            let loaded = fetch_and_decode(&client, &url).await;
            on_done(url, loaded);
        });
    }

    /// Stores completed download result back into cache.
    pub fn complete(&mut self, url: String, image: Option<Arc<RenderImage>>) {
        self.in_flight.remove(&url);
        match image {
            Some(render_img) => {
                self.entries.insert(url, ThumbnailState::Loaded(render_img));
            }
            None => {
                self.entries.insert(url, ThumbnailState::Failed);
            }
        }
    }
}

async fn fetch_and_decode(client: &Client, url: &str) -> Option<Arc<RenderImage>> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    if resp
        .content_length()
        .is_some_and(|len| len > MAX_THUMBNAIL_BYTES as u64)
    {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    if bytes.is_empty() || bytes.len() > MAX_THUMBNAIL_BYTES {
        return None;
    }
    let dyn_img = image::load_from_memory(&bytes).ok()?;
    let mut rgba = dyn_img.to_rgba8();
    // GPUI RenderImage stores pixels in BGRA byte order.
    for pixel in rgba.pixels_mut() {
        let [r, g, b, a] = pixel.0;
        pixel.0 = [b, g, r, a];
    }
    Some(Arc::new(RenderImage::new([Frame::new(rgba)])))
}

/// Renders either a cached game thumbnail or a clean place placeholder box.
pub fn render_thumbnail_box(
    cache: &ThumbnailCache,
    url: Option<&str>,
    size_px: f32,
    theme: &AppTheme,
) -> AnyElement {
    let loaded = url.and_then(|u| match cache.get(u) {
        Some(ThumbnailState::Loaded(img_ref)) => Some(img_ref.clone()),
        _ => None,
    });

    if let Some(render_img) = loaded {
        return div()
            .size(px(size_px))
            .rounded(px(8.0))
            .overflow_hidden()
            .border_1()
            .border_color(theme.border_subtle)
            .bg(theme.bg_input)
            .child(
                img(render_img)
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover),
            )
            .into_any_element();
    }

    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(size_px))
        .rounded(px(8.0))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_badge)
        .text_size(px(11.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(theme.text_muted)
        .child("RBX")
        .into_any_element()
}
