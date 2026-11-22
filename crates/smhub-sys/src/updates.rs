//! OS updates: the update server's manifests (cached for an hour) and
//! installation with the `rauc` command, reporting its progress.

use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;
use smhub_model::{Error, Result};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::{Sys, lock};

pub const UPDATE_BASE: &str = "https://updates.smlight.tech/firmware/smhub/os";
const CACHE: Duration = Duration::from_secs(3600);
/// Changelogs fetched per channel (the old UI showed the four newest releases).
const CHANGELOGS: usize = 4;

#[derive(Debug, Clone, Serialize)]
pub struct Release {
    pub version: String,
    pub channel: String,
    pub changelog: Option<String>,
    #[serde(skip)]
    pub bundle_url: String,
}

/// The cached result of asking the update server.
#[derive(Debug, Clone)]
pub struct Cached {
    at: Instant,
    pub channel: String,
    pub releases: Vec<Release>,
    pub checked_at: i64,
    pub error: Option<String>,
}

/// Compare OS versions such as `1.0.2`, `1.0.0.beta5`, `0.9.5.dev1`: numeric parts compare as
/// numbers; a part with letters is a pre-release, lower than the release without it.
pub fn os_version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    #[derive(Debug, PartialEq)]
    enum T {
        Num(u64),
        Pre(u8, String),
    }
    fn rank(tag: &str) -> u8 {
        match tag {
            "dev" => 0,
            "a" | "alpha" => 1,
            "b" | "beta" => 2,
            "rc" | "c" | "pre" => 3,
            _ => 4,
        }
    }
    fn tokens(v: &str) -> Vec<T> {
        let mut out = Vec::new();
        for part in v.trim().trim_start_matches('v').split(['.', '-', '_', '+']) {
            let mut num = String::new();
            let mut tag = String::new();
            for c in part.chars() {
                if c.is_ascii_digit() {
                    if !tag.is_empty() {
                        let t = std::mem::take(&mut tag).to_ascii_lowercase();
                        out.push(T::Pre(rank(&t), t));
                    }
                    num.push(c);
                } else {
                    if !num.is_empty() {
                        out.push(T::Num(std::mem::take(&mut num).parse().unwrap_or(0)));
                    }
                    tag.push(c);
                }
            }
            if !num.is_empty() {
                out.push(T::Num(num.parse().unwrap_or(0)));
            }
            if !tag.is_empty() {
                let t = tag.to_ascii_lowercase();
                out.push(T::Pre(rank(&t), t));
            }
        }
        out
    }
    let (ta, tb) = (tokens(a), tokens(b));
    for i in 0..ta.len().max(tb.len()) {
        let o = match (ta.get(i), tb.get(i)) {
            (Some(T::Num(x)), Some(T::Num(y))) => x.cmp(y),
            (Some(T::Pre(rx, sx)), Some(T::Pre(ry, sy))) => rx.cmp(ry).then_with(|| sx.cmp(sy)),
            (Some(T::Pre(..)), Some(T::Num(_)) | None) => Ordering::Less,
            (Some(T::Num(_)) | None, Some(T::Pre(..))) => Ordering::Greater,
            (Some(T::Num(x)), None) => if *x == 0 { Ordering::Equal } else { Ordering::Greater },
            (None, Some(T::Num(y))) => if *y == 0 { Ordering::Equal } else { Ordering::Less },
            (None, None) => Ordering::Equal,
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

impl Sys {
    /// Download a URL with the system `curl` (decision D19): small text only.
    pub async fn fetch_text(&self, url: &str, limit: Duration) -> Result<String> {
        if !url.starts_with("https://") {
            return Err(Error::bad("only https addresses are fetched"));
        }
        let secs = limit.as_secs().max(1).to_string();
        let out = self.exec("curl", &["-fsSL", "--max-filesize", "4194304", "-m", &secs, url], limit + Duration::from_secs(2)).await?;
        if out.ok() {
            Ok(out.stdout)
        } else {
            Err(Error::Unavailable(format!("cannot reach {url}: {}", out.error_text())))
        }
    }

    /// Download a URL to a file with `curl`.
    pub async fn fetch_file(&self, url: &str, dest: &std::path::Path, limit: Duration) -> Result<()> {
        if !url.starts_with("https://") {
            return Err(Error::bad("only https addresses are fetched"));
        }
        let secs = limit.as_secs().max(1).to_string();
        let d = dest.to_string_lossy().into_owned();
        let out = self.exec("curl", &["-fsSL", "-m", &secs, "-o", &d, url], limit + Duration::from_secs(2)).await?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("download failed: {}", out.error_text()))) }
    }

    async fn channel_releases(&self, ch: &str) -> Result<Vec<Release>> {
        let text = self.fetch_text(&format!("{UPDATE_BASE}/{ch}/manifest.json"), Duration::from_secs(20)).await?;
        let list: Vec<serde_json::Value> = serde_json::from_str(&text).map_err(|e| Error::Unavailable(format!("manifest: {e}")))?;
        let mut out = Vec::new();
        for u in list {
            let Some(version) = u.get("version").and_then(|v| v.as_str()) else { continue };
            let bundle = u.get("bundle").and_then(|v| v.as_str()).unwrap_or("");
            out.push(Release {
                version: version.to_owned(),
                channel: u.get("channel").and_then(|v| v.as_str()).unwrap_or(ch).to_owned(),
                changelog: u.get("changelog").and_then(|v| v.as_str()).map(|c| format!("{UPDATE_BASE}/{ch}/{c}")),
                bundle_url: format!("{UPDATE_BASE}/{ch}/{bundle}"),
            });
        }
        out.sort_by(|a, b| os_version_cmp(&b.version, &a.version));
        for r in out.iter_mut().take(CHANGELOGS) {
            if let Some(url) = r.changelog.take() {
                r.changelog = self.fetch_text(&url, Duration::from_secs(15)).await.ok();
            }
        }
        for r in out.iter_mut().skip(CHANGELOGS) {
            r.changelog = None;
        }
        Ok(out)
    }

    /// OS releases for a channel; `dev` includes stable releases as the old backend did.
    /// Cached for an hour unless `force`.
    pub async fn os_releases(&self, channel: &str, force: bool) -> Cached {
        if !force {
            if let Some(c) = lock(&self.updates_cache).as_ref() {
                if c.channel == channel && c.at.elapsed() < CACHE {
                    return c.clone();
                }
            }
        }
        let chans: &[&str] = if channel == "dev" { &["stable", "dev"] } else { &["stable"] };
        let mut releases = Vec::new();
        let mut error = None;
        for ch in chans {
            match self.channel_releases(ch).await {
                Ok(mut r) => releases.append(&mut r),
                Err(e) => error = Some(e.message().to_owned()),
            }
        }
        releases.sort_by(|a, b| os_version_cmp(&b.version, &a.version));
        if !releases.is_empty() {
            error = None;
        }
        let c = Cached { at: Instant::now(), channel: channel.to_owned(), releases, checked_at: smhub_model::time::now_unix(), error };
        *lock(&self.updates_cache) = Some(c.clone());
        c
    }

    /// The last cached answer, without asking the server.
    pub fn os_releases_cached(&self) -> Option<Cached> {
        lock(&self.updates_cache).clone()
    }

    /// Install an OS bundle with `rauc install`, calling `progress(percent, step)` for every
    /// progress line.
    pub async fn rauc_install<F: FnMut(u8, String)>(&self, bundle_url: &str, mut progress: F) -> Result<()> {
        let mut c = self.command("rauc");
        c.args(["install", bundle_url]).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(false);
        let mut child = c.spawn().map_err(|e| Error::Unavailable(format!("cannot run rauc: {e}")))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let err_task = tokio::spawn(async move {
            let mut text = String::new();
            if let Some(e) = stderr {
                let mut lines = BufReader::new(e).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    if text.len() < 4096 {
                        text.push_str(&l);
                        text.push('\n');
                    }
                }
            }
            text
        });
        if let Some(o) = stdout {
            let mut lines = BufReader::new(o).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let t = l.trim();
                if let Some((pct, rest)) = t.split_once('%') {
                    if let Ok(p) = pct.trim().parse::<u8>() {
                        progress(p.min(100), rest.trim().to_owned());
                    }
                }
            }
        }
        let status = tokio::time::timeout(Duration::from_secs(3600), child.wait())
            .await
            .map_err(|_| Error::Unavailable("the OS update did not finish within an hour".into()))?
            .map_err(|e| Error::internal(format!("rauc: {e}")))?;
        let err_text = err_task.await.unwrap_or_default();
        if status.success() {
            Ok(())
        } else {
            Err(Error::internal(format!("rauc install failed: {}", err_text.trim())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn os_versions() {
        assert_eq!(os_version_cmp("1.0.2", "1.0.0"), Ordering::Greater);
        assert_eq!(os_version_cmp("1.0.0.beta5", "1.0.0"), Ordering::Less);
        assert_eq!(os_version_cmp("1.0.0.beta6", "1.0.0.beta5"), Ordering::Greater);
        assert_eq!(os_version_cmp("0.9.5.dev1", "0.9.6"), Ordering::Less);
        assert_eq!(os_version_cmp("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(os_version_cmp("1.0.2", "1.0.2"), Ordering::Equal);
    }
}
