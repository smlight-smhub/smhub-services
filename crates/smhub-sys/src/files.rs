//! The file manager's view of the hub: the device user's home, the SD card, and each installed
//! app's data folders. Writing is allowed only where the device user owns the file, as the old
//! file manager enforced (reference §9).

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use smhub_model::time::rfc3339_system;
use smhub_model::{Error, Result};

use crate::{DEVICE_USER, Sys, fsx};

#[derive(Debug, Clone, Serialize)]
pub struct Root {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub name: String,
    pub kind: &'static str,
    pub size: u64,
    pub modified: String,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub path: String,
    pub writable: bool,
    pub roots: Vec<Root>,
    pub entries: Vec<Entry>,
}

/// Most entries listed for one folder.
const MAX_ENTRIES: usize = 5000;

impl Sys {
    fn device_uid(&self) -> Option<(u32, u32, String)> {
        fsx::user_ids(&self.p("/etc/passwd"), DEVICE_USER)
    }

    /// Roots the file manager may use.
    pub fn file_roots(&self) -> Vec<Root> {
        let home = self.device_uid().map_or_else(|| format!("/home/{DEVICE_USER}"), |(_, _, h)| h);
        let mut out = vec![Root { path: home, name: "home".into() }];
        if self.p("/mnt/sdcard").is_dir() && (!self.is_live() || self.is_mounted("/mnt/sdcard")) {
            out.push(Root { path: "/mnt/sdcard".into(), name: "sdcard".into() });
        }
        for (pkg, _) in self.installed_packages() {
            if let Some(s) = self.package_schema(&pkg) {
                let dirs = crate::backups::data_dirs(&s);
                let several = dirs.len() > 1;
                for d in dirs {
                    if d.starts_with('/') && self.p(&d).is_dir() && !out.iter().any(|r| r.path == d) {
                        // an app with several folders: name each after its last part ("zeesm data")
                        let last = d.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
                        let name = if several { format!("{pkg} {last}") } else { pkg.clone() };
                        out.push(Root { path: d, name });
                    }
                }
            }
        }
        out
    }

    /// Resolve a path the client gave to a real path inside one of the roots.
    /// Returns (OS path as the client sees it, real path).
    pub fn resolve_file(&self, path: &str) -> Result<(String, PathBuf)> {
        if path.contains('\0') || !path.starts_with('/') {
            return Err(Error::invalid("path", "use an absolute path"));
        }
        let roots = self.file_roots();
        let clean: Vec<&str> = path.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
        if clean.contains(&"..") {
            return Err(Error::invalid("path", "'..' is not allowed"));
        }
        let shown = format!("/{}", clean.join("/"));
        let root = roots
            .iter()
            .filter(|r| shown == r.path || shown.starts_with(&format!("{}/", r.path)))
            .max_by_key(|r| r.path.len())
            .ok_or_else(|| Error::Forbidden("this place is outside the file manager's folders".into()))?;
        let real = self.p(&shown);
        let real_root = self.p(&root.path).canonicalize().map_err(|_| Error::not_found("folder"))?;
        let check = if real.exists() { real.canonicalize()? } else {
            real.parent().map(Path::to_path_buf).unwrap_or_default().canonicalize().map_err(|_| Error::not_found("folder"))?
        };
        if !check.starts_with(&real_root) {
            return Err(Error::Forbidden("this link leads outside the file manager's folders".into()));
        }
        Ok((shown, real))
    }

    fn writable(&self, m: &std::fs::Metadata) -> bool {
        self.device_uid().is_some_and(|(u, _, _)| m.uid() == u)
    }

    /// Whether the device user owns the path (the old rule for every change).
    pub fn file_writable(&self, real: &Path) -> bool {
        std::fs::symlink_metadata(real).is_ok_and(|m| self.writable(&m))
    }

    pub fn list_folder(&self, path: Option<&str>) -> Result<Folder> {
        let roots = self.file_roots();
        let path = path.filter(|p| !p.is_empty()).map_or_else(|| roots[0].path.clone(), str::to_owned);
        let (shown, real) = self.resolve_file(&path)?;
        let meta = std::fs::metadata(&real).map_err(|_| Error::not_found("folder"))?;
        if !meta.is_dir() {
            return Err(Error::invalid("path", "not a folder"));
        }
        let mut entries = Vec::new();
        for e in std::fs::read_dir(&real)?.flatten().take(MAX_ENTRIES) {
            let Ok(lm) = e.metadata() else { continue };
            let ft = e.file_type().ok();
            let kind = if ft.is_some_and(|t| t.is_symlink()) {
                "link"
            } else if lm.is_dir() {
                "dir"
            } else {
                "file"
            };
            let m = std::fs::metadata(e.path()).unwrap_or(lm);
            entries.push(Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                kind: if kind == "link" && m.is_dir() { "dir" } else { kind },
                size: if m.is_dir() { 0 } else { m.len() },
                modified: m.modified().map(rfc3339_system).unwrap_or_default(),
                writable: self.writable(&m),
            });
        }
        entries.sort_by(|a, b| (a.kind != "dir").cmp(&(b.kind != "dir")).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(Folder { path: shown, writable: self.writable(&meta), roots, entries })
    }

    fn require_writable(&self, real: &Path) -> Result<()> {
        if self.file_writable(real) { Ok(()) } else { Err(Error::Forbidden("this item belongs to the system and cannot be changed here".into())) }
    }

    fn own(&self, real: &Path) {
        if let Some((u, g, _)) = self.device_uid() {
            fsx::chown(real, u, g);
        }
    }

    fn valid_name(name: &str) -> Result<()> {
        if name.is_empty() || name.len() > 255 || name.contains('/') || name == "." || name == ".." {
            Err(Error::invalid("name", "not a valid name"))
        } else {
            Ok(())
        }
    }

    /// Create a folder (owned by the device user).
    pub fn make_folder(&self, path: &str) -> Result<()> {
        let (_, real) = self.resolve_file(path)?;
        let parent = real.parent().ok_or_else(|| Error::invalid("path", "no parent"))?;
        Self::valid_name(&real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())?;
        self.require_writable(parent)?;
        if real.exists() {
            return Err(Error::Conflict("a file or folder with this name exists".into()));
        }
        std::fs::create_dir(&real)?;
        self.own(&real);
        Ok(())
    }

    /// Save bytes to a file: an existing file must be writable; a new one is created in a
    /// writable folder and given to the device user.
    pub fn save_file(&self, path: &str, data: &[u8]) -> Result<()> {
        let (_, real) = self.resolve_file(path)?;
        if real.is_dir() {
            return Err(Error::invalid("path", "this is a folder"));
        }
        if real.exists() {
            self.require_writable(&real)?;
            fsx::write_plain(&real, data)
        } else {
            let parent = real.parent().ok_or_else(|| Error::invalid("path", "no parent"))?;
            self.require_writable(parent)?;
            fsx::write_plain(&real, data)?;
            self.own(&real);
            Ok(())
        }
    }

    /// Move a finished upload into place.
    pub fn place_file(&self, from: &Path, path: &str) -> Result<()> {
        let (_, real) = self.resolve_file(path)?;
        let parent = real.parent().ok_or_else(|| Error::invalid("path", "no parent"))?;
        Self::valid_name(&real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())?;
        self.require_writable(parent)?;
        if real.exists() {
            return Err(Error::Conflict("a file with this name exists".into()));
        }
        if std::fs::rename(from, &real).is_err() {
            std::fs::copy(from, &real)?;
            let _ = std::fs::remove_file(from);
        }
        self.own(&real);
        Ok(())
    }

    /// Move or copy.
    pub fn move_file(&self, from: &str, to: &str, copy: bool) -> Result<()> {
        let (_, src) = self.resolve_file(from)?;
        let (_, dst) = self.resolve_file(to)?;
        if !src.exists() {
            return Err(Error::not_found("item"));
        }
        if dst.exists() {
            return Err(Error::Conflict("an item with this name exists there".into()));
        }
        Self::valid_name(&dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())?;
        let dparent = dst.parent().ok_or_else(|| Error::invalid("to", "no parent"))?;
        self.require_writable(dparent)?;
        if !copy {
            self.require_writable(&src)?;
            return std::fs::rename(&src, &dst).map_err(Error::from);
        }
        fn copy_rec(a: &Path, b: &Path, depth: u32) -> std::io::Result<()> {
            if depth > 32 {
                return Ok(());
            }
            let m = std::fs::symlink_metadata(a)?;
            if m.is_dir() {
                std::fs::create_dir(b)?;
                for e in std::fs::read_dir(a)?.flatten() {
                    copy_rec(&e.path(), &b.join(e.file_name()), depth + 1)?;
                }
                Ok(())
            } else if m.file_type().is_symlink() {
                Ok(())
            } else {
                std::fs::copy(a, b).map(|_| ())
            }
        }
        copy_rec(&src, &dst, 0)?;
        self.chown_tree(&dst, 0);
        Ok(())
    }

    fn chown_tree(&self, p: &Path, depth: u32) {
        self.own(p);
        if depth < 32 && p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    self.chown_tree(&e.path(), depth + 1);
                }
            }
        }
    }

    pub fn delete_file(&self, path: &str) -> Result<()> {
        let (shown, real) = self.resolve_file(path)?;
        if self.file_roots().iter().any(|r| r.path == shown) {
            return Err(Error::Forbidden("a top folder cannot be deleted".into()));
        }
        self.require_writable(&real)?;
        if std::fs::symlink_metadata(&real)?.is_dir() { std::fs::remove_dir_all(&real) } else { std::fs::remove_file(&real) }.map_err(Error::from)
    }
}
