//! The SIMCom A76xx 4G add-on (reference §12): presence by GPIO and USB, a few AT queries on
//! its command port, and power and reset by GPIO sequences the way the OS's `modem-power` does.
//! Queries run only when asked; there is no polling loop.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use serde::Serialize;
use smhub_model::{Error, Result};

use crate::{Sys, fsx};

pub const USB_ID: &str = "1e0e:9011";

#[derive(Debug, Clone, Serialize, Default)]
pub struct ModemStatus {
    pub registered: Option<u8>,
    pub operator: Option<String>,
    pub technology: Option<String>,
    pub signal_dbm: Option<i32>,
    pub signal_percent: Option<u8>,
    pub imei: Option<String>,
}

fn parse_status(lines: &[String]) -> ModemStatus {
    let mut s = ModemStatus::default();
    for l in lines {
        if let Some(r) = l.strip_prefix("+CSQ:") {
            let rssi: i32 = r.split(',').next().and_then(|v| v.trim().parse().ok()).unwrap_or(99);
            if rssi != 99 {
                s.signal_dbm = Some(-113 + 2 * rssi);
                s.signal_percent = u8::try_from((rssi.clamp(0, 31) * 100) / 31).ok();
            }
        } else if let Some(r) = l.strip_prefix("+CEREG:") {
            let v: Vec<&str> = r.split(',').map(str::trim).collect();
            s.registered = v.get(1).or(v.first()).and_then(|x| x.parse().ok());
        } else if let Some(r) = l.strip_prefix("+COPS:") {
            let v: Vec<&str> = r.split(',').map(str::trim).collect();
            s.operator = v.get(2).map(|o| o.trim_matches('"').to_owned()).filter(|o| !o.is_empty());
            s.technology = v.get(3).and_then(|t| t.parse::<u8>().ok()).map(|t| match t {
                7 => "LTE".to_owned(),
                2 | 4 | 5 | 6 => "3G".to_owned(),
                0 | 1 | 3 => "2G".to_owned(),
                n => format!("mode {n}"),
            });
        } else if l.len() == 15 && l.chars().all(|c| c.is_ascii_digit()) {
            s.imei = Some(l.clone());
        }
    }
    s
}

fn at_session(port: &str, commands: &[&str]) -> std::io::Result<Vec<String>> {
    use nix::sys::termios::{self, BaudRate, SetArg, SpecialCharacterIndices};
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(port)?;
    let mut t = termios::tcgetattr(&f).map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
    termios::cfmakeraw(&mut t);
    termios::cfsetspeed(&mut t, BaudRate::B115200).map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
    t.control_chars[SpecialCharacterIndices::VMIN as usize] = 0;
    t.control_chars[SpecialCharacterIndices::VTIME as usize] = 5;
    termios::tcsetattr(&f, SetArg::TCSANOW, &t).map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
    let mut out = Vec::new();
    for c in commands {
        f.write_all(format!("{c}\r\n").as_bytes())?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut buf = Vec::new();
        let mut chunk = [0u8; 256];
        while Instant::now() < deadline {
            let n = f.read(&mut chunk)?;
            buf.extend_from_slice(&chunk[..n]);
            let text = String::from_utf8_lossy(&buf);
            if text.contains("\nOK") || text.contains("ERROR") {
                break;
            }
        }
        out.extend(String::from_utf8_lossy(&buf).lines().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty() && *l != "OK" && l != c));
    }
    Ok(out)
}

impl Sys {
    /// Whether the 4G module is fitted (`4g_check` is active-low).
    pub async fn modem_fitted(&self) -> bool {
        if !self.model().lte_slot {
            return false;
        }
        match self.exec("gpioget", &["--numeric", "--active-low", "4g_check"], Duration::from_secs(5)).await {
            Ok(o) if o.ok() => o.stdout.trim() == "1",
            _ => false,
        }
    }

    /// The modem's AT command port: the highest `ttyUSB` of the SIMCom device.
    pub fn modem_port(&self) -> Option<String> {
        let mut ports = Vec::new();
        for e in std::fs::read_dir(self.p("/sys/bus/usb-serial/devices")).ok()?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(real) = std::fs::canonicalize(e.path()) else { continue };
            let mut dir = real.as_path();
            while let Some(parent) = dir.parent() {
                if dir.file_name().is_some_and(|n| n.to_string_lossy().contains(':')) {
                    let v = fsx::read_trim(&parent.join("idVendor"));
                    let p = fsx::read_trim(&parent.join("idProduct"));
                    if v.as_deref() == Some("1e0e") && p.as_deref() == Some("9011") && self.p(&format!("/dev/{name}")).exists() {
                        ports.push(format!("/dev/{name}"));
                    }
                    break;
                }
                dir = parent;
            }
        }
        ports.sort();
        ports.pop()
    }

    /// Ask the modem for its state.
    pub async fn modem_status(&self) -> Result<ModemStatus> {
        let port = self.modem_port().ok_or_else(|| Error::Unavailable("the modem is not connected over USB".into()))?;
        let lines = tokio::task::spawn_blocking(move || at_session(&port, &["ATE0", "AT+CSQ", "AT+CEREG?", "AT+COPS?", "AT+CGSN"]))
            .await
            .map_err(|e| Error::internal(e.to_string()))?
            .map_err(|e| Error::Unavailable(format!("modem: {e}")))?;
        Ok(parse_status(&lines))
    }

    /// Power the modem on or off, or reset it (the OS's `modem-power` timings).
    pub async fn modem_pulse(&self, what: &str) -> Result<()> {
        let (line, args) = match what {
            "on" => ("4g_power", ["-t", "100ms,100ms,0", "4g_power=1"]),
            "off" => ("4g_power", ["-t", "100ms,2500ms,0", "4g_power=0"]),
            "reset" => ("4g_rst", ["-t", "100ms,2500ms,0", "4g_rst=0"]),
            _ => return Err(Error::bad("unknown modem action")),
        };
        let out = self.exec("gpioset", &args, Duration::from_secs(10)).await?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("gpioset {line}: {}", out.error_text()))) }
    }

    /// The modem's LED: on or off (`4g_led`, active-high).
    pub async fn modem_led(&self, on: bool) -> Result<()> {
        let assign = format!("4g_led={}", u8::from(on));
        let out = self.exec("gpioset", &["-t", "0", &assign], Duration::from_secs(5)).await?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("gpioset 4g_led: {}", out.error_text()))) }
    }

    /// Whether the USB device of the modem is visible.
    pub async fn modem_on_usb(&self) -> bool {
        self.exec("lsusb", &[], Duration::from_secs(5)).await.is_ok_and(|o| o.stdout.contains(USB_ID))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_parsing() {
        let s = parse_status(&["+CSQ: 20,99".into(), "+CEREG: 0,1".into(), "+COPS: 0,0,\"Salt\",7".into(), "867584031234567".into()]);
        assert_eq!(s.signal_dbm, Some(-73));
        assert_eq!(s.registered, Some(1));
        assert_eq!(s.operator.as_deref(), Some("Salt"));
        assert_eq!(s.technology.as_deref(), Some("LTE"));
        assert!(s.imei.is_some());
    }
}
