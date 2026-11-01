//! HTTP client: retries, resumable downloads, JSON/text cache with stale-on-offline.
use crate::types::{CancelToken, Options};
use crate::{Error, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
    retries: u32,
    request_timeout: Duration,
    stall_timeout: Duration,
}

/// Cached text body with freshness info.
#[derive(Debug, Clone)]
pub struct Cached {
    pub body: String,
    /// Served from an expired cache because the network failed.
    pub stale: bool,
    pub offline: bool,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    #[serde(rename = "fetchedAt")]
    fetched_at: u64,
    url: String,
    body: String,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Http {
    pub fn new(o: &Options) -> Result<Self> {
        // reqwest honours HTTPS_PROXY / HTTP_PROXY / ALL_PROXY / NO_PROXY from the environment by default.
        let mut b = reqwest::Client::builder()
            .user_agent(o.user_agent.clone())
            .connect_timeout(Duration::from_secs(o.connect_timeout_secs));
        if let Some(p) = &o.proxy {
            b = b.proxy(reqwest::Proxy::all(p).map_err(|e| Error::Network(format!("bad proxy: {e}")))?);
        }
        Ok(Http {
            client: b.build()?,
            retries: o.retries.max(1),
            request_timeout: Duration::from_secs(o.request_timeout_secs),
            stall_timeout: Duration::from_secs(o.stall_timeout_secs),
        })
    }

    fn backoff(attempt: u32) -> Duration {
        Duration::from_millis(500 * 2u64.pow(attempt.min(5)))
    }

    /// GET text with retries (5xx / network errors are retried, 4xx are not).
    pub async fn get_text(&self, url: &str) -> Result<String> {
        let mut last = Error::Network("no attempt".into());
        for attempt in 0..self.retries {
            if attempt > 0 {
                tokio::time::sleep(Self::backoff(attempt - 1)).await;
            }
            let r = self.client.get(url).timeout(self.request_timeout).send().await;
            match r {
                Ok(resp) => {
                    let st = resp.status();
                    if st.is_success() {
                        match resp.text().await {
                            Ok(t) => return Ok(t),
                            Err(e) => last = e.into(),
                        }
                    } else if st.is_client_error() {
                        return Err(Error::Network(format!("HTTP {} for {url}", st.as_u16())));
                    } else {
                        last = Error::Network(format!("HTTP {} for {url}", st.as_u16()));
                    }
                }
                Err(e) => last = Error::Network(format!("{url}: {e}")),
            }
        }
        Err(last)
    }

    /// Cached GET. Fresh cache (< ttl) is used unless `force`; on network failure an expired cache is served (stale+offline).
    pub async fn get_cached(&self, cache_dir: &Path, name: &str, url: &str, ttl_secs: u64, force: bool) -> Result<Cached> {
        let file = cache_dir.join(format!("{name}.json"));
        let cached: Option<CacheFile> = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str(&s).ok());
        if let Some(c) = &cached {
            if c.url == url && !force && now_secs().saturating_sub(c.fetched_at) < ttl_secs {
                return Ok(Cached { body: c.body.clone(), stale: false, offline: false });
            }
        }
        match self.get_text(url).await {
            Ok(body) => {
                let _ = std::fs::create_dir_all(cache_dir);
                let cf = CacheFile { fetched_at: now_secs(), url: url.to_string(), body: body.clone() };
                if let Ok(json) = serde_json::to_string(&cf) {
                    let tmp = file.with_extension("json.tmp");
                    if std::fs::write(&tmp, json).is_ok() {
                        let _ = std::fs::rename(&tmp, &file);
                    }
                }
                Ok(Cached { body, stale: false, offline: false })
            }
            Err(e) => match cached {
                Some(c) if c.url == url => Ok(Cached { body: c.body, stale: true, offline: true }),
                _ => Err(e),
            },
        }
    }

    /// Download `url` into `part` (resuming from its current length), with retries.
    /// `on_progress(bytes_so_far, total)`. Leaves `part` in place on failure/cancel for later resume.
    pub async fn download(
        &self,
        url: &str,
        part: &Path,
        cancel: &CancelToken,
        on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
    ) -> Result<u64> {
        if let Some(p) = part.parent() {
            tokio::fs::create_dir_all(p).await?;
        }
        let mut last = Error::Network("no attempt".into());
        for attempt in 0..self.retries {
            cancel.check()?;
            if attempt > 0 {
                tokio::time::sleep(Self::backoff(attempt - 1)).await;
            }
            match self.download_once(url, part, cancel, on_progress).await {
                Ok(n) => return Ok(n),
                Err(Error::Network(m)) => last = Error::Network(m),
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    async fn download_once(
        &self,
        url: &str,
        part: &Path,
        cancel: &CancelToken,
        on_progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
    ) -> Result<u64> {
        let existing = tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
        let mut req = self.client.get(url);
        if existing > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={existing}-"));
        }
        let resp = tokio::time::timeout(self.request_timeout, req.send())
            .await
            .map_err(|_| Error::Network(format!("timeout connecting to {url}")))??;
        let st = resp.status();
        let (mut file, mut done, total) = if st.as_u16() == 206 {
            // Content-Range: bytes START-END/TOTAL
            let total = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.rsplit('/').next())
                .and_then(|t| t.parse::<u64>().ok())
                .or_else(|| resp.content_length().map(|l| l + existing));
            let f = tokio::fs::OpenOptions::new().append(true).open(part).await?;
            (f, existing, total)
        } else if st.is_success() {
            // Server ignored Range (200): restart from scratch.
            let f = tokio::fs::File::create(part).await?;
            (f, 0, resp.content_length())
        } else if st.as_u16() == 416 {
            // Our partial file is stale/oversized: drop it and retry from zero.
            let _ = tokio::fs::remove_file(part).await;
            return Err(Error::Network("HTTP 416 (range not satisfiable); restarting".into()));
        } else if st.is_client_error() {
            return Err(Error::NotFound(format!("HTTP {} for {url}", st.as_u16())));
        } else {
            return Err(Error::Network(format!("HTTP {} for {url}", st.as_u16())));
        };
        on_progress(done, total);
        let mut stream = resp.bytes_stream();
        let mut last_emit = std::time::Instant::now();
        loop {
            cancel.check()?;
            let next = tokio::time::timeout(self.stall_timeout, stream.next())
                .await
                .map_err(|_| Error::Network("download stalled".into()))?;
            match next {
                None => break,
                Some(Err(e)) => {
                    file.flush().await.ok();
                    return Err(Error::Network(format!("stream error: {e}")));
                }
                Some(Ok(chunk)) => {
                    file.write_all(&chunk).await?;
                    done += chunk.len() as u64;
                    if last_emit.elapsed() >= Duration::from_millis(80) {
                        on_progress(done, total);
                        last_emit = std::time::Instant::now();
                    }
                }
            }
        }
        file.flush().await?;
        if let Some(t) = total {
            if done != t {
                return Err(Error::Network(format!("incomplete download: {done} of {t} bytes")));
            }
        }
        on_progress(done, total);
        Ok(done)
    }
}

pub fn file_name_of_url(url: &str) -> String {
    url.rsplit('/').next().unwrap_or("download.zip").split('?').next().unwrap_or("download.zip").to_string()
}

pub fn join(root: &Path, parts: &[&str]) -> PathBuf {
    let mut p = root.to_path_buf();
    for x in parts {
        p.push(x);
    }
    p
}
