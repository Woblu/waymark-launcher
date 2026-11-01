//! HTTP for everything: cached metadata and verified, parallel downloads.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use futures_util::{StreamExt, TryStreamExt, stream};
use md5::Md5;
use reqwest::Url;
use reqwest::redirect::Policy;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha1::{Digest, Sha1};
use sha2::Sha512;
use tokio::io::AsyncWriteExt;

use crate::brand::USER_AGENT;
use crate::error::{Error, Result};

const PARALLEL_DOWNLOADS: usize = 24;
const ATTEMPTS: u32 = 4;
const MAX_REDIRECTS: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expected {
    Nothing,
    /// Only for sources that publish nothing stronger.
    Md5(String),
    Sha1(String),
    Sha512(String),
}

#[derive(Clone, Debug)]
pub struct Fetch {
    pub url: String,
    pub dest: PathBuf,
    pub hash: Expected,
    pub size: Option<u64>,
}

enum Hashing {
    Nothing,
    Md5(Md5),
    Sha1(Sha1),
    Sha512(Box<Sha512>),
}

impl Hashing {
    fn new(expected: &Expected) -> Self {
        match expected {
            Expected::Nothing => Hashing::Nothing,
            Expected::Md5(_) => Hashing::Md5(Md5::new()),
            Expected::Sha1(_) => Hashing::Sha1(Sha1::new()),
            Expected::Sha512(_) => Hashing::Sha512(Box::new(Sha512::new())),
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        match self {
            Hashing::Nothing => {}
            Hashing::Md5(h) => h.update(bytes),
            Hashing::Sha1(h) => h.update(bytes),
            Hashing::Sha512(h) => h.update(bytes),
        }
    }

    fn matches(self, expected: &Expected) -> bool {
        match (self, expected) {
            (Hashing::Md5(h), Expected::Md5(want)) => {
                hex::encode(h.finalize()).eq_ignore_ascii_case(want)
            }
            (Hashing::Sha1(h), Expected::Sha1(want)) => {
                hex::encode(h.finalize()).eq_ignore_ascii_case(want)
            }
            (Hashing::Sha512(h), Expected::Sha512(want)) => {
                hex::encode(h.finalize()).eq_ignore_ascii_case(want)
            }
            _ => true,
        }
    }
}

#[derive(Clone)]
pub struct Net {
    client: reqwest::Client,
}

impl Net {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(45))
            .pool_max_idle_per_host(PARALLEL_DOWNLOADS)
            .redirect(Policy::custom(|attempt| {
                // A safe address could otherwise hand the download to an unsafe one.
                if attempt.url().scheme() != "https" || is_local(attempt.url()) {
                    attempt.error("refused a redirect to an unsafe address")
                } else if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        Ok(Net { client })
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    pub async fn json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let response = self.client.get(url).send().await?;
        check(url, &response)?;
        Ok(response.json().await?)
    }

    /// Reads through a file cache. A fresh copy is used without touching the
    /// network, and a stale one still works when the network doesn't, which is
    /// what lets the launcher open instantly and work offline.
    pub async fn cached_json<T>(&self, url: &str, cache: &Path, max_age: Duration) -> Result<T>
    where
        T: DeserializeOwned + Serialize,
    {
        let fresh = tokio::fs::metadata(cache)
            .await
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| SystemTime::now().duration_since(at).ok())
            .is_some_and(|age| age < max_age);
        if fresh && let Some(value) = read_json(cache).await {
            return Ok(value);
        }
        match self.json::<T>(url).await {
            Ok(value) => {
                write_atomic(cache, &serde_json::to_vec(&value)?).await?;
                Ok(value)
            }
            Err(error) => read_json(cache).await.ok_or(error),
        }
    }

    pub async fn fetch(&self, item: &Fetch) -> Result<()> {
        self.fetch_with(item, &|_| {}).await
    }

    async fn fetch_with(&self, item: &Fetch, on_bytes: &(dyn Fn(u64) + Sync)) -> Result<()> {
        if already_there(item).await {
            on_bytes(item.size.unwrap_or(0));
            return Ok(());
        }
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.download(item, on_bytes).await {
                Ok(()) => return Ok(()),
                Err(error) if attempt >= ATTEMPTS || !worth_retrying(&error) => return Err(error),
                Err(error) => {
                    tracing::warn!(url = %item.url, %error, attempt, "download failed, retrying");
                    tokio::time::sleep(Duration::from_millis(250 << attempt)).await;
                }
            }
        }
    }

    async fn download(&self, item: &Fetch, on_bytes: &(dyn Fn(u64) + Sync)) -> Result<()> {
        if let Some(parent) = item.dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let part = temp_beside(&item.dest);
        match self.stream_to(item, &part, on_bytes).await {
            Ok(()) => Ok(tokio::fs::rename(&part, &item.dest).await?),
            Err(error) => {
                let _ = tokio::fs::remove_file(&part).await;
                Err(error)
            }
        }
    }

    async fn stream_to(
        &self,
        item: &Fetch,
        part: &Path,
        on_bytes: &(dyn Fn(u64) + Sync),
    ) -> Result<()> {
        let response = self.client.get(&item.url).send().await?;
        check(&item.url, &response)?;
        let mut file = tokio::fs::File::create(part).await?;
        let mut hashing = Hashing::new(&item.hash);
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.try_next().await? {
            hashing.update(&chunk);
            file.write_all(&chunk).await?;
            on_bytes(chunk.len() as u64);
        }
        file.flush().await?;
        drop(file);
        if !hashing.matches(&item.hash) {
            return Err(Error::HashMismatch {
                url: item.url.clone(),
            });
        }
        Ok(())
    }

    /// Fetches a batch at once and reports `(bytes done, bytes total)`.
    pub async fn fetch_all(
        &self,
        items: Vec<Fetch>,
        progress: impl Fn(u64, u64) + Sync,
    ) -> Result<()> {
        let mut seen = HashSet::new();
        let items: Vec<Fetch> = items
            .into_iter()
            .filter(|i| seen.insert(i.dest.clone()))
            .collect();
        let total: u64 = items.iter().filter_map(|i| i.size).sum();
        let done = AtomicU64::new(0);
        let on_bytes = |n: u64| {
            let now = done.fetch_add(n, Ordering::Relaxed) + n;
            progress(now.min(total), total);
        };
        let on_bytes = &on_bytes;
        stream::iter(items)
            .map(|item| async move { self.fetch_with(&item, on_bytes).await })
            .buffer_unordered(PARALLEL_DOWNLOADS)
            .try_collect::<()>()
            .await
    }
}

/// Whether an address points at this computer or its home network. A stranger's
/// pack has no business reaching a router or a printer through the launcher.
pub fn is_local(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };
    match host.trim_matches(['[', ']']).parse() {
        Ok(IpAddr::V4(address)) => is_local_v4(address),
        Ok(IpAddr::V6(address)) => {
            let first = address.segments()[0];
            address.is_loopback()
                || address.is_unspecified()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
                || address.to_ipv4_mapped().is_some_and(is_local_v4)
        }
        Err(_) => {
            let name = host.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local")
        }
    }
}

fn is_local_v4(address: Ipv4Addr) -> bool {
    address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_broadcast()
}

fn check(url: &str, response: &reqwest::Response) -> Result<()> {
    if response.status().is_success() {
        Ok(())
    } else {
        Err(Error::Status {
            url: url.to_string(),
            status: response.status().as_u16(),
        })
    }
}

fn worth_retrying(error: &Error) -> bool {
    match error {
        Error::Http(_) | Error::HashMismatch { .. } => true,
        Error::Status { status, .. } => *status >= 500 || *status == 429,
        _ => false,
    }
}

/// A size match is enough on the fast path: re-hashing thousands of game
/// assets on every launch would cost seconds for no real protection.
async fn already_there(item: &Fetch) -> bool {
    match tokio::fs::metadata(&item.dest).await {
        Ok(meta) if meta.is_file() => item.size.is_none_or(|size| size == meta.len()),
        _ => false,
    }
}

fn temp_beside(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download");
    dest.with_file_name(format!("{name}.{}.part", uuid::Uuid::new_v4().simple()))
}

async fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = tokio::fs::read(path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = temp_beside(path);
    tokio::fs::write(&part, bytes).await?;
    tokio::fs::rename(&part, path).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_md5_is_really_compared() {
        let hashed = |text: &str| {
            let mut hashing = Hashing::new(&Expected::Md5(String::new()));
            hashing.update(text.as_bytes());
            hashing
        };
        let hello = Expected::Md5("5d41402abc4b2a76b9719d911017c592".to_string());
        assert!(hashed("hello").matches(&hello));
        assert!(!hashed("hullo").matches(&hello));
    }

    #[test]
    fn local_addresses_are_recognised() {
        let local = |url: &str| is_local(&Url::parse(url).unwrap());
        assert!(local("https://localhost/a") && local("https://127.0.0.1/a"));
        assert!(local("https://192.168.0.2/a") && local("https://[::1]/a"));
        assert!(!local("https://example.com/a") && !local("https://8.8.8.8/a"));
    }
}
