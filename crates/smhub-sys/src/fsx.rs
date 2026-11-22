//! File helpers: atomic replacement, optional reads, ownership.

use std::io::Write;
use std::path::Path;

use smhub_model::{Error, Result};

/// Read a file to a string; `None` when it does not exist.
pub fn read_opt(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(String::from_utf8_lossy(&b).into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::internal(format!("read {}: {e}", path.display()))),
    }
}

/// Read a file to a string, trimming it; `None` when missing or empty.
pub fn read_trim(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_owned()) }
}

/// Replace a file atomically: write a temporary file next to it, flush it, rename it over the
/// old one. Keeps the old file's mode when there was one.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    write_atomic_inner(path, data, None)
}

/// Like `write_atomic`, for secrets: the file has `mode` from the moment it exists, so its
/// content is never readable by others, not even briefly.
pub fn write_atomic_private(path: &Path, data: &[u8], mode: u32) -> Result<()> {
    write_atomic_inner(path, data, Some(mode))
}

fn write_atomic_inner(path: &Path, data: &[u8], create_mode: Option<u32>) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = path.parent().ok_or_else(|| Error::internal(format!("no folder for {}", path.display())))?;
    std::fs::create_dir_all(dir).map_err(|e| Error::internal(format!("create {}: {e}", dir.display())))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.smhubd-{}", std::process::id()));
    let mode = create_mode.or_else(|| std::fs::metadata(path).ok().map(|m| std::os::unix::fs::PermissionsExt::mode(&m.permissions())));
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(create_mode.unwrap_or(0o666)).open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        if let Some(m) = mode {
            std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(m))?;
        }
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::internal(format!("write {}: {e}", path.display())));
    }
    Ok(())
}

/// Write a file plainly (truncate and write), as the old backend did for most files.
pub fn write_plain(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::internal(format!("create {}: {e}", dir.display())))?;
    }
    std::fs::write(path, data).map_err(|e| Error::internal(format!("write {}: {e}", path.display())))
}

/// User and group ids of an account from `/etc/passwd`.
pub fn user_ids(passwd: &Path, user: &str) -> Option<(u32, u32, String)> {
    let text = std::fs::read_to_string(passwd).ok()?;
    for line in text.lines() {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() >= 6 && f[0] == user {
            return Some((f[2].parse().ok()?, f[3].parse().ok()?, f[5].to_owned()));
        }
    }
    None
}

/// Change the owner of a path; errors are logged, not returned.
pub fn chown(path: &Path, uid: u32, gid: u32) {
    if let Err(e) = std::os::unix::fs::chown(path, Some(uid), Some(gid)) {
        tracing::warn!("chown {}: {e}", path.display());
    }
}
