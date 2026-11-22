//! Time zones: the list comes from the OS's `zoneinfo` and its index of current zone names
//! (`zone.tab`); the setting lives in `/etc/timezone` with `/etc/localtime` linked to the zone
//! file, as the old `timezone` provider did. No zone is named here: the OS decides.

use std::time::Duration;

use smhub_model::{Error, Result};

use crate::{Sys, fsx};

pub const ZONEINFO: &str = "/usr/share/zoneinfo";

fn valid_zone(z: &str) -> bool {
    !z.is_empty()
        && z.len() <= 64
        && !z.contains("..")
        && !z.starts_with('/')
        && z.chars().all(|c| c.is_ascii_alphanumeric() || "/_+-".contains(c))
}

impl Sys {
    /// The hub's time zone: `/etc/timezone`, else the target of `/etc/localtime`, else UTC.
    pub fn timezone(&self) -> String {
        if let Some(z) = fsx::read_trim(&self.p("/etc/timezone")) {
            return z;
        }
        if let Ok(target) = std::fs::read_link(self.p("/etc/localtime")) {
            let t = target.to_string_lossy();
            if let Some(i) = t.find("zoneinfo/") {
                return t[i + 9..].to_owned();
            }
        }
        "UTC".into()
    }

    /// The zones people choose from, sorted: the current names the OS's `zone.tab` lists, for
    /// which it has a zone file, plus `UTC`. Old names the OS keeps for compatibility are not in
    /// `zone.tab`, so each zone appears once. Without `zone.tab`, every `Area/City` file.
    pub fn timezones(&self) -> Vec<String> {
        let Ok(tab) = std::fs::read_to_string(self.p(&format!("{ZONEINFO}/zone.tab"))) else {
            return self.zone_files();
        };
        let mut out: Vec<String> = tab
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split('\t').nth(2))
            .chain(["UTC"])
            .filter(|z| self.settable(z))
            .map(str::to_owned)
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Every zone file of the form `Area/City`, plus `UTC`, sorted.
    fn zone_files(&self) -> Vec<String> {
        fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>, depth: u8) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !name.starts_with(|c: char| c.is_ascii_uppercase()) {
                    continue;
                }
                let full = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
                if e.path().is_dir() {
                    if depth < 3 {
                        walk(&e.path(), &full, out, depth + 1);
                    }
                } else {
                    out.push(full);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.p(ZONEINFO), "", &mut out, 0);
        out.retain(|z| {
            (z.contains('/') || z == "UTC")
                && !z.starts_with("SystemV")
                && (!z.starts_with("Etc/") || z == "Etc/UTC")
        });
        out.sort();
        out.dedup();
        out
    }

    /// Point `/etc/localtime` at a zone file.
    pub fn link_localtime(&self, zone: &str) -> Result<()> {
        let link = self.p("/etc/localtime");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(format!("{ZONEINFO}/{zone}"), &link)
            .map_err(|e| Error::internal(format!("link /etc/localtime: {e}")))
    }

    /// Whether a zone can be set: a zone file the OS has.
    pub fn settable(&self, zone: &str) -> bool {
        valid_zone(zone) && self.p(&format!("{ZONEINFO}/{zone}")).is_file()
    }

    /// The name a zone is saved under: the zone itself when the list has it; else the listed
    /// zone whose file has the same content, which is how the OS keeps an old name; else the
    /// zone as given.
    pub fn current_name(&self, zone: &str) -> String {
        let list = self.timezones();
        if list.iter().any(|z| z == zone) {
            return zone.to_owned();
        }
        let read = |z: &str| std::fs::read(self.p(&format!("{ZONEINFO}/{z}"))).ok();
        let Some(data) = read(zone) else { return zone.to_owned() };
        let same: Vec<&String> = list.iter().filter(|z| read(z).is_some_and(|d| d == data)).collect();
        let area = zone.split('/').next().unwrap_or_default();
        same.iter()
            .find(|z| z.split('/').next() == Some(area))
            .or(same.first())
            .map_or_else(|| zone.to_owned(), |z| (*z).clone())
    }

    /// Set the time zone: `/etc/timezone` (value and newline) and `/etc/localtime`, under the
    /// zone's current name ([`Sys::current_name`]). Returns the name saved. Setting the zone the
    /// hub already has changes nothing and is never refused.
    pub fn set_timezone(&self, zone: &str) -> Result<String> {
        if self.timezone() == zone {
            return Ok(zone.to_owned());
        }
        if !self.settable(zone) {
            return Err(Error::invalid("timezone", "unknown time zone"));
        }
        let zone = self.current_name(zone);
        if self.timezone() != zone {
            fsx::write_plain(&self.p("/etc/timezone"), format!("{zone}\n").as_bytes())?;
            self.link_localtime(&zone)?;
        }
        Ok(zone)
    }

    /// The hub's UTC offset now, in seconds east of UTC (`date +%z`); `None` when `date` fails
    /// or says something else.
    pub async fn utc_offset(&self) -> Option<i64> {
        let out = self.run("date", &["+%z"], Duration::from_secs(3)).await.ok()?;
        parse_offset(&out)
    }
}

/// `date +%z`'s output (`+0200`, `-0530`) in seconds east of UTC.
fn parse_offset(s: &str) -> Option<i64> {
    let s = s.trim();
    let (sign, digits) = match (s.strip_prefix('+'), s.strip_prefix('-')) {
        (Some(d), _) => (1, d),
        (_, Some(d)) => (-1, d),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let h: i64 = digits.get(..2)?.parse().ok()?;
    let m: i64 = digits.get(2..)?.parse().ok()?;
    (h <= 14 && m < 60).then_some(sign * (h * 3600 + m * 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_as_date_prints_them() {
        assert_eq!(parse_offset("+0000\n"), Some(0));
        assert_eq!(parse_offset("+0200"), Some(7200));
        assert_eq!(parse_offset("-0530"), Some(-19_800));
        assert_eq!(parse_offset("+1345"), Some(49_500));
        assert_eq!(parse_offset("-1200"), Some(-43_200));
        for bad in ["", "0200", "+02", "+02:00", "+2a00", "+1500", "+0260", "UTC", "++0200"] {
            assert_eq!(parse_offset(bad), None, "{bad:?}");
        }
    }

    /// A zoneinfo tree with made-up zones: `(name, content)` files and an optional `zone.tab`.
    #[cfg(unix)]
    fn zoneinfo(tag: &str, files: &[(&str, &str)], tab: Option<&str>) -> (Sys, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("smhub-tz-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let zi = root.join("usr/share/zoneinfo");
        for (name, content) in files {
            let p = zi.join(name);
            std::fs::create_dir_all(p.parent().unwrap_or(&zi)).unwrap_or_default();
            std::fs::write(&p, content).unwrap_or_default();
        }
        if let Some(tab) = tab {
            std::fs::write(zi.join("zone.tab"), tab).unwrap_or_default();
        }
        std::fs::create_dir_all(root.join("etc")).unwrap_or_default();
        (Sys::new(&root, None), root)
    }

    #[cfg(unix)]
    #[test]
    fn list_comes_from_the_index_and_old_names_are_saved_as_current() {
        let files = [("Test/Current", "A"), ("Test/Old", "A"), ("Test/Other", "B"), ("Other/Old", "B"), ("UTC", "U")];
        let tab = "# comment\nXX\t+0000+00000\tTest/Current\nXX\t+0000+00000\tTest/Other\tsecond\nXX\t+0000+00000\tTest/Missing\n";
        let (sys, root) = zoneinfo("index", &files, Some(tab));
        // Listed names with a file, plus UTC; old names and listed names without a file are not.
        assert_eq!(sys.timezones(), ["Test/Current", "Test/Other", "UTC"]);
        // An old name is saved under the current name of the zone with the same file.
        assert_eq!(sys.set_timezone("Test/Old").ok().as_deref(), Some("Test/Current"));
        assert_eq!(sys.timezone(), "Test/Current");
        assert_eq!(sys.current_name("Other/Old"), "Test/Other");
        assert_eq!(sys.set_timezone("Test/Other").ok().as_deref(), Some("Test/Other"));
        // A zone without a file, or a path, is refused.
        assert!(sys.set_timezone("Test/Nowhere").is_err());
        assert!(sys.set_timezone("../../etc/passwd").is_err());
        // Saving the zone the hub already has changes nothing, even when its file is gone.
        std::fs::write(root.join("etc/timezone"), "Test/Gone\n").unwrap_or_default();
        assert_eq!(sys.set_timezone("Test/Gone").ok().as_deref(), Some("Test/Gone"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn without_an_index_every_area_city_file_is_listed() {
        let files = [("Test/A", "A"), ("Test/B", "B"), ("UTC", "U"), ("Plain", "P"), ("Etc/UTC", "U"), ("Etc/Other", "O"), ("SystemV/X", "X")];
        let (sys, root) = zoneinfo("files", &files, None);
        assert_eq!(sys.timezones(), ["Etc/UTC", "Test/A", "Test/B", "UTC"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn offset_from_date() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("smhub-tz-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap_or_default();
        let date = bin.join("date");
        let set = |body: &str| {
            std::fs::write(&date, format!("#!/bin/sh\n{body}\n")).unwrap_or_default();
            std::fs::set_permissions(&date, std::fs::Permissions::from_mode(0o755)).unwrap_or_default();
        };
        let sys = Sys::new(&root, Some(&bin));
        set(r#"[ "$1" = "+%z" ] && echo +0530"#);
        assert_eq!(sys.utc_offset().await, Some(19_800));
        set("echo garbage");
        assert_eq!(sys.utc_offset().await, None);
        set("exit 1");
        assert_eq!(sys.utc_offset().await, None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
