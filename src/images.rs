use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use reqwest::Client;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use crate::db::Db;
use crate::error::{Error, Result};
use crate::models::{ImageRef, MediaCacheEntry};

pub struct ImageDownloader {
    client: Client,
    media_dir: PathBuf,
    max_bytes: u64,
    allow_private: bool,
}

impl ImageDownloader {
    pub fn new(media_dir: impl AsRef<Path>, max_bytes: u64, allow_private: bool) -> Result<Self> {
        std::fs::create_dir_all(media_dir.as_ref())?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("ReadingSteiner/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            client,
            media_dir: media_dir.as_ref().to_path_buf(),
            max_bytes: if max_bytes == 0 {
                10 * 1024 * 1024
            } else {
                max_bytes
            },
            allow_private,
        })
    }

    pub async fn ensure(
        &self,
        db: &Mutex<Db>,
        image: &ImageRef,
    ) -> Result<Option<MediaCacheEntry>> {
        if let Some(cached) = db.lock().await.get_media_cache(&image.canonical_url)? {
            return Ok(Some(cached));
        }
        let url = url::Url::parse(&image.canonical_url)
            .map_err(|e| Error::other(format!("invalid image url {}: {e}", image.canonical_url)))?;
        if !self.allow_private {
            self.ensure_public(&url).await?;
        }
        if !matches!(url.scheme(), "http" | "https") {
            return Ok(None);
        }

        let resp = self.client.get(url.clone()).send().await?;
        if !resp.status().is_success() {
            warn!(url = %url, status = %resp.status(), "image download failed");
            return Ok(None);
        }
        let mime = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        if !mime.starts_with("image/") {
            warn!(url = %url, mime = %mime, "rejecting non-image content type");
            return Ok(None);
        }
        let bytes = resp.bytes().await?;
        if bytes.len() as u64 > self.max_bytes {
            warn!(url = %url, len = bytes.len(), "image exceeds size limit");
            return Ok(None);
        }
        let sha256 = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            hex(&hasher.finalize())
        };
        let ext = mime_guess::get_mime_extensions_str(&mime)
            .and_then(|e| e.first().copied())
            .unwrap_or("bin");
        let file_name = format!("{sha256}.{ext}");
        let file_path = self.media_dir.join(&file_name);
        if !file_path.exists() {
            tokio::fs::write(&file_path, &bytes).await?;
        }
        let phash = compute_phash(&bytes);
        let entry = MediaCacheEntry {
            canonical_url: image.canonical_url.clone(),
            sha256,
            mime,
            size: bytes.len() as i64,
            file_path: file_path.to_string_lossy().into_owned(),
            telegram_file_id: None,
            phash,
            fetched_at: Utc::now(),
        };
        db.lock().await.insert_media_cache(&entry)?;
        debug!(url = %image.canonical_url, sha = %entry.sha256, "image cached");
        Ok(Some(entry))
    }

    async fn ensure_public(&self, url: &url::Url) -> Result<()> {
        let host = url
            .host_str()
            .ok_or_else(|| Error::other("image url has no host"))?;
        // If host is already an IP literal, check directly.
        if let Ok(ip) = host.parse::<IpAddr>() {
            if is_private_ip(ip) {
                return Err(Error::other(format!(
                    "SSRF blocked private image host: {host}"
                )));
            }
            return Ok(());
        }
        let lookup =
            tokio::net::lookup_host((host, url.port_or_known_default().unwrap_or(80))).await?;
        for addr in lookup {
            let ip = addr.ip();
            if is_private_ip(ip) {
                return Err(Error::other(format!(
                    "SSRF blocked private image host: {host} -> {ip}"
                )));
            }
        }
        Ok(())
    }
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4 == Ipv4Addr::UNSPECIFIED
                || v4.is_broadcast()
                || is_reserved_v4(v4)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_multicast()
        }
    }
}

fn is_reserved_v4(v4: Ipv4Addr) -> bool {
    matches!(
        v4.octets(),
        [10, _, _, _]
            | [172, 16..=31, _, _]
            | [192, 168, _, _]
            | [169, 254, _, _]
            | [127, _, _, _]
            | [0, _, _, _]
            | [100, 64..=127, _, _]
            | [192, 0, 0, _]
            | [198, 18..=19, _, _]
    )
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn compute_phash(bytes: &[u8]) -> Option<String> {
    let img = image::load_from_memory(bytes).ok()?;
    let small = img
        .resize_exact(8, 8, image::imageops::FilterType::Triangle)
        .to_luma8();
    let mut sum = 0u64;
    for p in small.pixels() {
        sum += p.0[0] as u64;
    }
    let avg = (sum / 64) as u8;
    let mut bits = String::new();
    for (i, p) in small.pixels().enumerate() {
        bits.push(if p.0[0] >= avg { '1' } else { '0' });
        if i % 8 == 7 && i != 63 {
            bits.push('-');
        }
    }
    Some(bits)
}
