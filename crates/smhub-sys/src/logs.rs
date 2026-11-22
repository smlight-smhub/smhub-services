//! Logs under `/var/log`: list, tail, follow (with inotify, no polling), download.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use serde::Serialize;
use smhub_model::{Error, Result};
use tokio::sync::mpsc;

use crate::Sys;

pub const LOG_DIR: &str = "/var/log";

/// Log names that differ from the package name (old `LOG_FILE_MAPPING`). A name is what the API
/// uses, and it never contains `/`: the hub's nginx (`proxy_pass .../api/;`, part of the OS
/// image) decodes `%2F` before passing a request on, so `/logs/nginx%2Ferror` would arrive as
/// `/logs/nginx/error` and match no route.
pub fn log_for_package(package: &str) -> String {
    match package {
        "zwavejsui" => "zwave-js-ui",
        "smhub-ui" => "nginx-error",
        "openthread" => "otbr-agent",
        "nodered" => "node-red",
        "picoclaw-launcher" => "picoclaw",
        "tailscale" => "tailscaled",
        other => other,
    }
    .to_owned()
}

#[derive(Debug, Clone, Serialize)]
pub struct LogInfo {
    pub name: String,
    pub app: Option<String>,
    pub size: u64,
}

/// Logs kept in a folder under `/var/log`: name, file.
const IN_FOLDERS: &[(&str, &str)] = &[("nginx-error", "nginx/error.log")];

fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && !n.contains("..") && n.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

impl Sys {
    /// The file of a log: `messages` is `/var/log/messages`, `nginx-error` is
    /// `/var/log/nginx/error.log`, others `/var/log/<name>.log`.
    pub fn log_path(&self, name: &str) -> Result<PathBuf> {
        if !valid_name(name) {
            return Err(Error::invalid("name", "not a log name"));
        }
        let rel = match IN_FOLDERS.iter().find(|(n, _)| *n == name) {
            Some((_, file)) => (*file).to_owned(),
            None if name == "messages" => "messages".to_owned(),
            None => format!("{name}.log"),
        };
        Ok(self.p(&format!("{LOG_DIR}/{rel}")))
    }

    /// Whether a log belongs to the hub or to an installed package, so it is expected even before
    /// it exists: `/var/log` is in RAM, and an app that has not run since the hub started has
    /// written nothing yet.
    pub fn log_expected(&self, name: &str, installed: &[String]) -> bool {
        name == "messages" || name == "smhub-services" || installed.iter().any(|p| log_for_package(p) == name)
    }

    /// Logs that exist now.
    pub fn logs(&self, installed: &[String]) -> Vec<LogInfo> {
        let mut out = Vec::new();
        let mut push = |name: String, app: Option<String>| {
            if out.iter().any(|l: &LogInfo| l.name == name) {
                return;
            }
            if let Ok(p) = self.log_path(&name) {
                if let Ok(m) = std::fs::metadata(&p) {
                    out.push(LogInfo { name, app, size: m.len() });
                }
            }
        };
        push("messages".into(), None);
        push("smhub-services".into(), Some("smhub-services".into()));
        for pkg in installed {
            push(log_for_package(pkg), Some(pkg.clone()));
        }
        if let Ok(rd) = std::fs::read_dir(self.p(LOG_DIR)) {
            let mut names: Vec<String> = rd
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| e.file_name().to_str()?.strip_suffix(".log").map(str::to_owned))
                .collect();
            names.sort();
            for n in names {
                push(n, None);
            }
        }
        out
    }

    /// The last `lines` lines of a log, oldest first, optionally only those containing `filter`
    /// (case-insensitive). Reads backwards from the end in blocks, at most 8 MiB.
    pub fn log_tail(&self, name: &str, lines: usize, filter: Option<&str>) -> Result<Vec<String>> {
        let path = self.log_path(name)?;
        let mut f = std::fs::File::open(&path).map_err(|_| Error::not_found(format!("log {name}")))?;
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let needle = filter.map(str::to_lowercase).filter(|s| !s.is_empty());
        let mut out: Vec<String> = Vec::new();
        let mut pos = len;
        let mut carry: Vec<u8> = Vec::new();
        let limit = len.saturating_sub(8 * 1024 * 1024);
        while pos > limit && out.len() < lines {
            let start = pos.saturating_sub(64 * 1024).max(limit);
            let mut buf = vec![0u8; usize::try_from(pos - start).unwrap_or(0)];
            f.seek(SeekFrom::Start(start))?;
            f.read_exact(&mut buf)?;
            buf.extend_from_slice(&carry);
            let mut parts: Vec<&[u8]> = buf.split(|&b| b == b'\n').collect();
            carry = if start > limit { parts.remove(0).to_vec() } else { Vec::new() };
            for p in parts.into_iter().rev() {
                if out.len() >= lines {
                    break;
                }
                let s = String::from_utf8_lossy(p).trim_end().to_owned();
                if s.is_empty() {
                    continue;
                }
                if needle.as_ref().is_some_and(|n| !s.to_lowercase().contains(n)) {
                    continue;
                }
                out.push(s);
            }
            pos = start;
        }
        if !carry.is_empty() && out.len() < lines {
            let s = String::from_utf8_lossy(&carry).trim_end().to_owned();
            if !s.is_empty() && needle.as_ref().is_none_or(|n| s.to_lowercase().contains(n)) {
                out.push(s);
            }
        }
        out.reverse();
        Ok(out)
    }

    /// Follow a log from its current end: new lines arrive on the channel. Handles truncation
    /// (logrotate's copytruncate) and replacement. Ends when the receiver is dropped.
    pub fn log_follow(&self, name: &str, filter: Option<String>) -> Result<mpsc::Receiver<String>> {
        let path = self.log_path(name)?;
        let dir = path.parent().map(PathBuf::from).unwrap_or_else(|| self.p(LOG_DIR));
        let (tx, rx) = mpsc::channel::<String>(256);
        let needle = filter.map(|s| s.to_lowercase()).filter(|s| !s.is_empty());
        let mut events = crate::watch::watch(vec![dir], crate::watch::changes());
        tokio::spawn(async move {
            let mut offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let mut partial = String::new();
            loop {
                let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                if len < offset {
                    offset = 0;
                    partial.clear();
                }
                if len > offset {
                    let read = (|| -> std::io::Result<Vec<u8>> {
                        let mut f = std::fs::File::open(&path)?;
                        f.seek(SeekFrom::Start(offset))?;
                        let mut buf = Vec::new();
                        f.take(1024 * 1024).read_to_end(&mut buf)?;
                        Ok(buf)
                    })();
                    if let Ok(buf) = read {
                        offset += buf.len() as u64;
                        partial.push_str(&String::from_utf8_lossy(&buf));
                        while let Some(i) = partial.find('\n') {
                            let line: String = partial.drain(..=i).collect();
                            let line = line.trim_end().to_owned();
                            if line.is_empty() || needle.as_ref().is_some_and(|n| !line.to_lowercase().contains(n)) {
                                continue;
                            }
                            if tx.send(line).await.is_err() {
                                return;
                            }
                        }
                    }
                }
                tokio::select! {
                    ev = events.recv() => if ev.is_none() { return },
                    () = tx.closed() => return,
                }
            }
        });
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_never_contain_a_slash() {
        let root = std::env::temp_dir().join(format!("smhub-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("var/log/nginx")).unwrap_or_default();
        std::fs::write(root.join("var/log/nginx/error.log"), "a\nb\n").unwrap_or_default();
        std::fs::write(root.join("var/log/messages"), "m\n").unwrap_or_default();
        std::fs::write(root.join("var/log/rc.log"), "r\n").unwrap_or_default();
        let sys = Sys::new(&root, None);

        let installed = vec!["smhub-ui".to_owned()];
        let logs = sys.logs(&installed);
        let names: Vec<&str> = logs.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["messages", "nginx-error", "rc"]);
        assert!(names.iter().all(|n| !n.contains('/')));
        assert_eq!(logs[1].app.as_deref(), Some("smhub-ui"));

        assert_eq!(sys.log_tail("nginx-error", 10, None).unwrap_or_default(), ["a", "b"]);
        assert!(sys.log_expected("nginx-error", &installed));
        assert!(sys.log_path("nginx/error").is_err(), "the old name, with a slash, is refused");
        assert!(sys.log_path("../shadow").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
