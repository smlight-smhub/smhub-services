//! Backups in the old format (reference §8): a `tar.xz` (`xz -1`) whose first member is
//! `metadata.json`, followed by the data folders without their leading `/`. Old and new backups
//! are interchangeable.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use smhub_model::{Error, Result};

use crate::{DEVICE_USER, Sys};

pub const FACTORY_DIR: &str = "/mnt/user/.factory-backup";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub device_id: Option<String>,
    pub timestamp: Option<String>,
    pub data_dirs: Vec<String>,
    #[serde(default)]
    pub apps: Vec<String>,
}

/// Data folders of a package's schema (`system.data_dir.default`, string or list).
pub fn data_dirs(schema: &serde_json::Value) -> Vec<String> {
    match schema.pointer("/system/data_dir/default") {
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
        _ => Vec::new(),
    }
}

/// The local time as `YYYYMMDD_HHMMSS` via `date`, for file names.
async fn stamp(sys: &Sys) -> String {
    sys.run("date", &["+%Y%m%d_%H%M%S"], Duration::from_secs(5)).await.map(|s| s.trim().to_owned()).unwrap_or_else(|_| "backup".into())
}

impl Sys {
    /// Folders a backup includes, in the old order: the home folder and every installed package's
    /// data folders, sorted, existing ones only; then the extra folders that exist.
    pub fn backup_dirs(&self, include_home: bool, extra: &[String]) -> Vec<String> {
        let mut set = std::collections::BTreeSet::new();
        set.insert(format!("/home/{DEVICE_USER}"));
        for (pkg, _) in self.installed_packages() {
            if let Some(s) = self.package_schema(&pkg) {
                set.extend(data_dirs(&s));
            }
        }
        let mut dirs: Vec<String> = set.into_iter().filter(|d| self.p(d).is_dir()).collect();
        if !include_home {
            dirs.retain(|d| d != &format!("/home/{DEVICE_USER}"));
        }
        for e in extra {
            let e = e.trim();
            if e.starts_with('/') && !e.contains("..") && self.p(e).is_dir() {
                dirs.push(e.to_owned());
            }
        }
        dirs
    }

    /// Start `tar` writing a backup to its stdout. The caller streams it and must wait for the
    /// child. `meta_file` is written first and removed by the caller afterwards.
    pub async fn backup_start(&self, dirs: &[String], work: &Path) -> Result<(tokio::process::Child, PathBuf, String)> {
        if dirs.is_empty() {
            return Err(Error::not_found("nothing to back up"));
        }
        let ts = self.run("date", &["+%Y-%m-%dT%H:%M:%S.000000"], Duration::from_secs(5)).await.map(|s| s.trim().to_owned()).ok();
        let meta = Metadata {
            device_id: self.eeprom().map(|e| e.uuid.clone()),
            timestamp: ts,
            data_dirs: dirs.to_vec(),
            apps: self.installed_packages().into_keys().collect(),
        };
        std::fs::create_dir_all(work)?;
        let meta_path = work.join(format!("backup-meta-{}.json", std::process::id()));
        std::fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)?;
        let rel_meta = meta_path.to_string_lossy().trim_start_matches('/').to_owned();
        let transform = format!("s|{rel_meta}|metadata.json|");
        let mut c = self.command("tar");
        c.arg("-I").arg("xz -1").arg("-cf").arg("-").arg("--transform").arg(&transform).arg(&meta_path);
        if !self.is_live() {
            c.arg("-C").arg(self.root());
        }
        for d in dirs {
            if self.is_live() { c.arg(d) } else { c.arg(d.trim_start_matches('/')) };
        }
        c.stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        let child = c.spawn().map_err(|e| Error::Unavailable(format!("cannot run tar: {e}")))?;
        let name = format!("user_backup_{}.tar.xz", stamp(self).await);
        Ok((child, meta_path, name))
    }

    /// Read `metadata.json` from a backup file.
    pub async fn backup_metadata(&self, file: &Path) -> Result<Metadata> {
        let f = file.to_string_lossy().into_owned();
        let out = self.exec("tar", &["-xJf", &f, "-O", "metadata.json"], Duration::from_secs(300)).await?;
        if !out.ok() || out.stdout.trim().is_empty() {
            return Err(Error::bad("this is not an SMHUB backup (no metadata.json)"));
        }
        serde_json::from_str(&out.stdout).map_err(|_| Error::bad("the backup's metadata.json is not valid"))
    }

    /// Restore a backup: empty each data folder it names (except the home folder, which is
    /// merged), then extract everything except `metadata.json` to `/`. Services must be stopped
    /// by the caller.
    pub async fn backup_restore(&self, file: &Path, meta: &Metadata) -> Result<()> {
        let home = format!("/home/{DEVICE_USER}");
        for d in &meta.data_dirs {
            if d == &home || !d.starts_with('/') || d.contains("..") || d == "/" {
                continue;
            }
            let p = self.p(d);
            if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    let path = e.path();
                    let r = if e.file_type().is_ok_and(|t| t.is_dir()) { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
                    if let Err(err) = r {
                        tracing::warn!("restore: clear {}: {err}", path.display());
                    }
                }
            }
        }
        let f = file.to_string_lossy().into_owned();
        let root = self.root().to_string_lossy().into_owned();
        let out = self.exec("tar", &["-xJpf", &f, "-C", &root, "--exclude=metadata.json"], Duration::from_secs(3600)).await?;
        if out.ok() { Ok(()) } else { Err(Error::internal(format!("tar: {}", out.error_text()))) }
    }

    /// The current factory backup: the last `backup-*.tar.xz` by name.
    pub fn factory_backup(&self) -> Option<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(self.p(FACTORY_DIR))
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with("backup-") && n.ends_with(".tar.xz")
            }))
            .collect();
        v.sort();
        v.pop()
    }

    pub fn delete_factory_backups(&self) -> Result<()> {
        if let Ok(rd) = std::fs::read_dir(self.p(FACTORY_DIR)) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("backup-") && n.ends_with(".tar.xz") {
                    std::fs::remove_file(e.path())?;
                }
            }
        }
        Ok(())
    }
}
