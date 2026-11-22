//! Device identity and OS facts: the EEPROM, `/etc/os-release`, hostname, uptime, power.

use std::collections::BTreeMap;

use smhub_model::{Error, Result};

use crate::Sys;
use crate::cmd::QUICK;
use crate::fsx;

pub const EEPROM: &str = "/sys/bus/i2c/devices/1-0050/eeprom";

/// The parsed EEPROM header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eeprom {
    pub version: u8,
    pub model: String,
    pub hw_rev: String,
    /// 32 lower-case hex digits.
    pub uuid: String,
    pub timestamp: Option<u64>,
}

fn ascii(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0 || c == 0xff).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_owned()
}

/// Parse the EEPROM, V1 header (version byte 1) or the legacy layout, as the old
/// `os_info.parse_v1_header` / `parse_legacy` did.
pub fn parse_eeprom(data: &[u8]) -> Option<Eeprom> {
    if data.len() >= 46 && data[0] == 1 {
        let mut ts = [0u8; 8];
        ts.copy_from_slice(&data[38..46]);
        return Some(Eeprom {
            version: 1,
            model: ascii(&data[1..16]),
            uuid: smhub_auth::hex(&data[16..32]),
            hw_rev: ascii(&data[32..38]),
            timestamp: Some(u64::from_le_bytes(ts)),
        });
    }
    if data.len() >= 32 {
        let prod_end = data[..10].iter().position(|&c| c == 0).unwrap_or(10);
        let hw_end = data[10..16].iter().position(|&c| c == 0).map_or(16, |p| 10 + p);
        return Some(Eeprom {
            version: 0,
            model: String::from_utf8_lossy(&data[..prod_end]).trim().to_owned(),
            hw_rev: String::from_utf8_lossy(&data[10..hw_end]).trim().to_owned(),
            uuid: smhub_auth::hex(&data[16..32]),
            timestamp: None,
        });
    }
    None
}

pub(crate) fn read_eeprom(sys: &Sys) -> Option<Eeprom> {
    let data = std::fs::read(sys.p(EEPROM)).ok()?;
    let e = parse_eeprom(&data)?;
    if e.model.is_empty() { None } else { Some(e) }
}

impl Sys {
    /// `/etc/os-release` as lower-case keys.
    pub fn os_release(&self) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(self.p("/etc/os-release")) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    map.insert(k.trim().to_ascii_lowercase(), v.trim().trim_matches('"').to_owned());
                }
            }
        }
        map
    }

    /// The OS version (`VERSION_ID`).
    pub fn os_version(&self) -> String {
        self.os_release().get("version_id").cloned().unwrap_or_else(|| "0.0.0".into())
    }

    /// The hostname from `/etc/hostname`, else the kernel's.
    pub fn hostname(&self) -> String {
        fsx::read_trim(&self.p("/etc/hostname"))
            .or_else(|| fsx::read_trim(&self.p("/proc/sys/kernel/hostname")))
            .unwrap_or_else(|| "smhub".into())
    }

    /// Seconds since boot.
    pub fn uptime(&self) -> f64 {
        std::fs::read_to_string(self.p("/proc/uptime"))
            .ok()
            .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse().ok()))
            .unwrap_or(0.0)
    }

    /// Reboot, the way the old backend did (`reboot`).
    pub async fn reboot(&self) -> Result<()> {
        self.run("reboot", &[], QUICK).await.map(|_| ())
    }

    /// Power off (`poweroff`).
    pub async fn poweroff(&self) -> Result<()> {
        self.run("poweroff", &[], QUICK).await.map(|_| ())
    }

    /// Set the hostname: `/etc/hostname` (single-value file, as the old `hostname` provider)
    /// and the running kernel's name.
    pub async fn set_hostname(&self, name: &str) -> Result<()> {
        let valid = !name.is_empty()
            && name.len() <= 63
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !name.starts_with('-')
            && !name.ends_with('-');
        if !valid {
            return Err(Error::invalid("hostname", "use letters, digits and hyphens, up to 63 characters"));
        }
        if fsx::read_trim(&self.p("/etc/hostname")).as_deref() == Some(name) {
            return Ok(());
        }
        fsx::write_plain(&self.p("/etc/hostname"), format!("{name}\n").as_bytes())?;
        if self.is_live() {
            if let Err(e) = self.run("hostname", &[name], QUICK).await {
                tracing::warn!("hostname: {e}");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_header() {
        let mut d = vec![0u8; 64];
        d[0] = 1;
        d[1..10].copy_from_slice(b"essential");
        d[10..16].fill(0xff);
        d[16..32].copy_from_slice(&[0xab; 16]);
        d[32..36].copy_from_slice(b"1.01");
        d[36..38].fill(0xff);
        d[38..46].copy_from_slice(&1784143970u64.to_le_bytes());
        let e = parse_eeprom(&d);
        assert_eq!(e.as_ref().map(|e| e.model.as_str()), Some("essential"));
        assert_eq!(e.as_ref().map(|e| e.hw_rev.as_str()), Some("1.01"));
        assert_eq!(e.as_ref().and_then(|e| e.timestamp), Some(1784143970));
    }
}
