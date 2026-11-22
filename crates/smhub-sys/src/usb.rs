//! The USB-C port's role (`/etc/usb_mode`, applied at boot by the OS) and connected devices.

use serde::Serialize;
use smhub_model::{Error, Result};

use crate::cmd::QUICK;
use crate::{Sys, fsx};

pub const MODE_FILE: &str = "/etc/usb_mode";

#[derive(Debug, Clone, Serialize)]
pub struct UsbDevice {
    pub id: String,
    pub name: String,
    pub bus: Option<String>,
}

/// Parse plain `lsusb` lines: `Bus 001 Device 002: ID 1d6b:0002 Linux Foundation 2.0 root hub`.
pub fn parse_lsusb(text: &str) -> Vec<UsbDevice> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("Bus ")?;
            let (bus, rest) = rest.split_once(' ')?;
            let (_, rest) = rest.split_once("ID ")?;
            let (id, name) = rest.split_once(' ').unwrap_or((rest, ""));
            Some(UsbDevice { id: id.to_owned(), name: name.trim().to_owned(), bus: Some(bus.to_owned()) })
        })
        .collect()
}

impl Sys {
    /// The configured role, `device` or `host`.
    pub fn usb_mode(&self) -> String {
        match fsx::read_trim(&self.p(MODE_FILE)).as_deref() {
            Some("host") => "host".into(),
            _ => "device".into(),
        }
    }

    /// The role the port has now, from the role switch in sysfs, when the kernel exposes it.
    pub fn usb_role_now(&self) -> Option<String> {
        let rd = std::fs::read_dir(self.p("/sys/class/usb_role")).ok()?;
        for e in rd.flatten() {
            if let Some(r) = fsx::read_trim(&e.path().join("role")) {
                return Some(if r == "host" { "host".into() } else { "device".into() });
            }
        }
        None
    }

    /// Write `/etc/usb_mode` (the value only, no newline, as the old single-value provider left
    /// it after its first write... the old provider wrote a newline; the OS reads it trimmed).
    pub fn set_usb_mode(&self, mode: &str) -> Result<()> {
        if mode != "device" && mode != "host" {
            return Err(Error::invalid("mode", "use device or host"));
        }
        if self.usb_mode() == mode && fsx::read_trim(&self.p(MODE_FILE)).is_some() {
            return Ok(());
        }
        fsx::write_plain(&self.p(MODE_FILE), format!("{mode}\n").as_bytes())
    }

    /// Connected USB devices from `lsusb`.
    pub async fn usb_devices(&self) -> Vec<UsbDevice> {
        match self.exec("lsusb", &[], QUICK).await {
            Ok(o) => parse_lsusb(&o.stdout),
            Err(_) => Vec::new(),
        }
    }

    /// USB serial ports by id (`/dev/serial/by-id/*`) and `ttyUSB`/`ttyACM` devices.
    pub fn usb_serial_ports(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.p("/dev/serial/by-id")) {
            for e in rd.flatten() {
                out.push(format!("/dev/serial/by-id/{}", e.file_name().to_string_lossy()));
            }
        }
        if let Ok(rd) = std::fs::read_dir(self.p("/dev")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("ttyUSB") || n.starts_with("ttyACM") {
                    out.push(format!("/dev/{n}"));
                }
            }
        }
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsusb() {
        let d = parse_lsusb("Bus 001 Device 001: ID 1d6b:0002 Linux Foundation 2.0 root hub\nBus 001 Device 002: ID 1e0e:9011 Qualcomm\n");
        assert_eq!(d.len(), 2);
        assert_eq!(d[1].id, "1e0e:9011");
        assert_eq!(d[0].name, "Linux Foundation 2.0 root hub");
    }
}
