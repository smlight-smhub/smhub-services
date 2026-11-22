//! opkg packages: feed lists, installed status, per-package files, and the commands that change
//! them. Matches the old `system_config.packages.Packages` behaviour.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use smhub_model::{Error, Result};

use crate::Sys;

pub const LISTS: &str = "/var/lib/opkg/lists";
pub const STATUS: &str = "/var/lib/opkg/status";
pub const INFO: &str = "/var/lib/opkg/info";
pub const FLAGS: &str = "/opt/share/services";
pub const OPKG_LOG: &str = "/var/log/opkg.log";

/// One package from the feeds, with its installed version.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Package {
    pub package: String,
    pub latest_version: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub depends: Option<String>,
    pub size: Option<u64>,
    pub required_os: Option<String>,
    pub section: Option<String>,
    pub homepage: Option<String>,
    /// Name of the feed list it came from (`smhub_core`, `community`, ...).
    pub feed: String,
}

/// Compare two versions the way the old backend did: `-` is a separator, `~` sorts before
/// anything, numeric parts compare as numbers, others as strings. Returns `-1`, `0` or `1`.
pub fn compare_versions(a: &str, b: &str) -> i32 {
    #[derive(PartialEq)]
    enum T {
        Tilde,
        Part(String),
    }
    fn tokens(v: &str) -> Vec<T> {
        let mut out = Vec::new();
        let mut cur = String::new();
        for c in v.replace('-', ".").chars() {
            match c {
                '~' => {
                    if !cur.is_empty() {
                        out.push(T::Part(std::mem::take(&mut cur)));
                    }
                    out.push(T::Tilde);
                }
                '.' => {
                    if !cur.is_empty() {
                        out.push(T::Part(std::mem::take(&mut cur)));
                    }
                }
                _ => cur.push(c),
            }
        }
        if !cur.is_empty() {
            out.push(T::Part(cur));
        }
        out
    }
    let (ta, tb) = (tokens(a), tokens(b));
    let zero = T::Part("0".into());
    for i in 0..ta.len().max(tb.len()) {
        let x = ta.get(i).unwrap_or(&zero);
        let y = tb.get(i).unwrap_or(&zero);
        match (x, y) {
            (T::Tilde, T::Part(_)) => return -1,
            (T::Part(_), T::Tilde) => return 1,
            (T::Tilde, T::Tilde) => {}
            (T::Part(p), T::Part(q)) => match (p.parse::<u64>(), q.parse::<u64>()) {
                (Ok(m), Ok(n)) if m != n => return if m > n { 1 } else { -1 },
                (Ok(_), Ok(_)) => {}
                _ if p != q => return if p > q { 1 } else { -1 },
                _ => {}
            },
        }
    }
    0
}

/// Parse an opkg `Packages` index or the status file into blocks of lower-case fields.
fn blocks(text: &str) -> Vec<BTreeMap<String, String>> {
    let mut out = Vec::new();
    let mut cur: BTreeMap<String, String> = BTreeMap::new();
    let mut last_key: Option<String> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            last_key = None;
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(k) = &last_key {
                if let Some(v) = cur.get_mut(k) {
                    v.push('\n');
                    v.push_str(line.trim());
                }
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            cur.insert(k.clone(), v.trim().to_owned());
            last_key = Some(k);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Installed packages from opkg's status file: name to version.
pub fn parse_status(text: &str) -> BTreeMap<String, String> {
    blocks(text)
        .into_iter()
        .filter(|b| b.get("status").is_some_and(|s| s.contains("installed") && !s.contains("not-installed")))
        .filter_map(|b| Some((b.get("package")?.clone(), b.get("version")?.clone())))
        .collect()
}

fn valid_package(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+.-_".contains(c))
        && !name.starts_with(['-', '.'])
}

/// Reject names that are not package names before they reach a path or a command.
pub fn check_package(name: &str) -> Result<()> {
    if valid_package(name) { Ok(()) } else { Err(Error::invalid("name", "not a package name")) }
}

impl Sys {
    /// Installed packages and versions.
    pub fn installed_packages(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(self.p(STATUS)).map(|t| parse_status(&t)).unwrap_or_default()
    }

    /// Every package of every feed list, newest version per name, with the installed version.
    /// Installed packages that no feed offers are included with their installed version.
    pub fn packages(&self) -> Vec<Package> {
        let installed = self.installed_packages();
        let mut by_name: BTreeMap<String, Package> = BTreeMap::new();
        let mut files: Vec<PathBuf> = std::fs::read_dir(self.p(LISTS))
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect())
            .unwrap_or_default();
        files.sort();
        for f in files {
            let feed = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if feed.ends_with(".sig") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            for b in blocks(&text) {
                let (Some(name), Some(version)) = (b.get("package"), b.get("version")) else { continue };
                if name == "smhub-web" {
                    continue;
                }
                let pkg = Package {
                    package: name.clone(),
                    latest_version: version.clone(),
                    version: installed.get(name).cloned(),
                    description: b.get("description").map(|d| d.lines().next().unwrap_or("").to_owned()),
                    depends: b.get("depends").cloned(),
                    size: b.get("size").and_then(|s| s.parse().ok()),
                    required_os: b.get("required-os-version").cloned(),
                    section: b.get("section").cloned(),
                    homepage: b.get("homepage").cloned(),
                    feed: feed.clone(),
                };
                match by_name.get(name) {
                    Some(existing) if compare_versions(version, &existing.latest_version) <= 0 => {}
                    _ => {
                        by_name.insert(name.clone(), pkg);
                    }
                }
            }
        }
        by_name.into_values().collect()
    }

    /// One package, from the feeds or installed.
    pub fn package(&self, name: &str) -> Option<Package> {
        self.packages().into_iter().find(|p| p.package == name)
    }

    /// A package's `schema.json`, if it ships one.
    pub fn package_schema(&self, name: &str) -> Option<serde_json::Value> {
        if !valid_package(name) {
            return None;
        }
        let text = std::fs::read_to_string(self.p(&format!("{INFO}/{name}.schema.json"))).ok()?;
        match serde_json::from_str(&text) {
            Ok(v) => Some(v),
            Err(e) => {
                tracing::warn!("schema of {name}: {e}");
                None
            }
        }
    }

    /// A package's changelog.
    pub fn package_changelog(&self, name: &str) -> Option<String> {
        if !valid_package(name) {
            return None;
        }
        std::fs::read_to_string(self.p(&format!("{INFO}/{name}.CHANGELOG.md"))).ok()
    }

    /// A file of the package's control data, e.g. `postinst`.
    pub fn package_info_file(&self, name: &str, file: &str) -> PathBuf {
        self.p(&format!("{INFO}/{name}.{file}"))
    }

    /// The package's icon file named in its schema (`system.icons.<kind>`), inside `/opt/<name>`.
    pub fn package_icon(&self, name: &str, kind: &str) -> Option<PathBuf> {
        let schema = self.package_schema(name)?;
        let icons = schema.pointer("/system/icons")?;
        let v = icons.get(kind)?;
        let rel = v.as_str().or_else(|| v.get("default").and_then(serde_json::Value::as_str))?;
        let base = self.p(&format!("/opt/{name}"));
        let file = base.join(rel).canonicalize().ok()?;
        let base = base.canonicalize().ok()?;
        if file.starts_with(&base) && file.is_file() { Some(file) } else { None }
    }

    fn flag_path(&self, package: &str, flag: &str) -> PathBuf {
        self.p(&format!("{FLAGS}/{package}.{flag}"))
    }

    /// Create or remove a flag file in `/opt/share/services`.
    pub fn set_package_flag(&self, package: &str, flag: &str, on: bool) -> Result<()> {
        check_package(package)?;
        let p = self.flag_path(package, flag);
        if on {
            if !p.exists() {
                crate::fsx::write_plain(&p, b"")?;
            }
        } else {
            let _ = std::fs::remove_file(&p);
        }
        Ok(())
    }

    pub fn package_flag(&self, package: &str, flag: &str) -> bool {
        valid_package(package) && self.flag_path(package, flag).exists()
    }

    /// Whether a package starts at boot: its service is in the default runlevel, or, for packages
    /// without an init script, the `.autostart` flag exists (old `check_autostart`).
    pub fn package_autostart(&self, package: &str) -> bool {
        let schema = self.package_schema(package);
        let svc = crate::services::service_for_package(schema.as_ref(), package);
        if self.service_exists(&svc) {
            self.p(&format!("/etc/runlevels/default/{svc}")).exists()
        } else {
            self.package_flag(package, "autostart")
        }
    }

    /// Set the `.autostart` flag and add or remove the service from the default runlevel
    /// (old `set_autostart`).
    pub async fn set_package_autostart(&self, package: &str, on: bool) -> Result<()> {
        self.set_package_flag(package, "autostart", on)?;
        let schema = self.package_schema(package);
        let svc = crate::services::service_for_package(schema.as_ref(), package);
        if self.service_exists(&svc) {
            self.set_service_autostart(&svc, on).await?;
        }
        Ok(())
    }

    /// Run `opkg <args>` with output appended to `/var/log/opkg.log`. The process gets its own
    /// process group so it finishes even if the daemon is restarted by the package it installs.
    pub async fn opkg(&self, args: &[&str], limit: Duration) -> Result<()> {
        use std::io::Write;
        let log_path = self.p(OPKG_LOG);
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path);
        let (out, err) = match log {
            Ok(mut f) => {
                let _ = writeln!(f, "smhubd: opkg {}", args.join(" "));
                let e = f.try_clone().map(Stdio::from).unwrap_or_else(|_| Stdio::null());
                (Stdio::from(f), e)
            }
            Err(_) => (Stdio::null(), Stdio::null()),
        };
        let mut c = self.command("opkg");
        c.args(args).stdout(out).stderr(err).kill_on_drop(false).process_group(0);
        let mut child = c.spawn().map_err(|e| Error::Unavailable(format!("cannot run opkg: {e}")))?;
        let status = tokio::time::timeout(limit, child.wait())
            .await
            .map_err(|_| Error::Unavailable(format!("opkg did not finish in {} s", limit.as_secs())))?
            .map_err(|e| Error::internal(format!("opkg: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::internal(format!(
                "opkg {} failed with exit code {}; see {OPKG_LOG}",
                args.join(" "),
                status.code().unwrap_or(-1)
            )))
        }
    }

    /// The opkg log's last lines, for error messages.
    pub fn opkg_log_tail(&self, lines: usize) -> String {
        let text = std::fs::read_to_string(self.p(OPKG_LOG)).unwrap_or_default();
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }

    /// Modification time of the newest feed list: when the lists were last downloaded.
    pub fn lists_refreshed(&self) -> Option<std::time::SystemTime> {
        std::fs::read_dir(self.p(LISTS))
            .ok()?
            .flatten()
            .filter_map(|e| e.metadata().ok()?.modified().ok())
            .max()
    }

    /// The app channel: `dev` when the dev feed line is in `/etc/opkg/smlight.conf`.
    pub fn app_channel(&self) -> &'static str {
        let text = std::fs::read_to_string(self.p(CHANNEL_CONF)).unwrap_or_default();
        if text.lines().any(|l| l.trim().starts_with("src/gz smhub_dev")) { "dev" } else { "stable" }
    }

    /// Add or remove the dev feed line, keeping every other line (old `Packages.set_channel`:
    /// blank lines are dropped, the file ends with a newline).
    pub fn set_app_channel(&self, channel: &str) -> Result<()> {
        let path = self.p(CHANNEL_CONF);
        let text = crate::fsx::read_opt(&path)?.ok_or_else(|| Error::Unavailable("opkg smlight.conf not found".into()))?;
        let mut lines: Vec<String> = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim().starts_with("src/gz smhub_dev"))
            .map(str::to_owned)
            .collect();
        if channel == "dev" {
            lines.push(DEV_LINE.to_owned());
        }
        let mut out = lines.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        crate::fsx::write_plain(&path, out.as_bytes())
    }
}

pub const CHANNEL_CONF: &str = "/etc/opkg/smlight.conf";
const DEV_LINE: &str = "src/gz smhub_dev https://pkg.smlight.tech/dev";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(compare_versions("1.0.0~rc1", "1.0.0"), -1);
        assert_eq!(compare_versions("1.1.18-1~dev1", "1.1.18-1"), -1);
        assert_eq!(compare_versions("2.13.0-1", "2.3.0-1"), 1);
        assert_eq!(compare_versions("1.0", "1.0.0"), 0);
        assert_eq!(compare_versions("1.0.5-1~dev3", "1.0.4-1"), 1);
    }

    #[test]
    fn status() {
        let s = parse_status("Package: a\nVersion: 1\nStatus: install ok installed\n\nPackage: b\nVersion: 2\nStatus: deinstall ok not-installed\n");
        assert_eq!(s.get("a").map(String::as_str), Some("1"));
        assert!(!s.contains_key("b"));
    }

    #[test]
    fn names() {
        assert!(valid_package("zigbee2mqtt"));
        assert!(valid_package("matterbridge-z2m"));
        assert!(!valid_package("../etc"));
        assert!(!valid_package("a b"));
    }
}
