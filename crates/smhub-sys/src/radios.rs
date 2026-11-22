//! Radio firmware: the list from the firmware server, download, and flashing through the Python
//! helper that wraps the proven vendor flashers (decision D6, reference §6). The service that
//! holds the radio's port is stopped while flashing and started again afterwards.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use smhub_model::hardware::{Family, RadioMode, RadioSlot, Vendor};
use smhub_model::{Error, Result};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::Sys;

const OTA: &str = "https://updates.smlight.tech/services/api/slzb-06x-ota.php?type=ZB&format=slzb&device=";
pub const HELPER: &str = "/opt/smhub-services/flasher/radio-flash.py";
pub const HELPER_LIB: &str = "/opt/smhub-services/flasher/lib";

#[derive(Debug, Clone, Serialize)]
pub struct Firmware {
    pub version: String,
    pub notes: Option<String>,
    pub dev: bool,
    #[serde(skip)]
    pub link: String,
}

fn device_id(key: &str) -> Option<u32> {
    Some(match key {
        "SMHUB-MG21" => 65,
        "SMHUB-CCP1" => 66,
        "SMHUB-MG24" => 67,
        "SMHUB-MG26" => 68,
        "SMHUB-CCP7" => 69,
        "SMHUB-CCP10" => 70,
        "SMHUB-ZG23" => 71,
        _ => return None,
    })
}

/// Parse the firmware server's answer, keeping items of the requested type, newest first.
pub fn parse_firmware(text: &str, want_type: u8) -> Vec<Firmware> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    let mut out = Vec::new();
    for it in items {
        let ver = ["ver", "rev", "version"].iter().find_map(|k| it.get(*k).and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_i64().map(|n| n.to_string()))));
        let Some(version) = ver else { continue };
        let t = it.get("type").and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
        if t.is_some_and(|t| t != i64::from(want_type)) {
            continue;
        }
        let prod = it.get("prod").and_then(serde_json::Value::as_bool).unwrap_or(true);
        let dev = it.get("dev").and_then(serde_json::Value::as_bool).unwrap_or(!prod);
        let link = ["link", "url"].iter().find_map(|k| it.get(*k).and_then(|v| v.as_str()).map(str::to_owned)).unwrap_or_default();
        let notes = it.get("notes").and_then(|v| v.as_str()).map(str::to_owned).filter(|n| !n.is_empty());
        out.push(Firmware { version, notes, dev, link });
    }
    out
}

/// Reset wiring name the helper understands, per model and slot (reference §6.4).
pub fn reset_target(family: Family, model: &str, hw_rev: Option<&str>, slot: &RadioSlot) -> &'static str {
    if slot.protocols.contains(&"zwave") {
        return "ADDON";
    }
    match family {
        Family::Smhub => "SMHUB",
        Family::Nano => "NANO",
        Family::Nanomr if model == "nanomr4" && hw_rev == Some("0.97") => "NANO",
        Family::Nanomr => "NANOMR",
    }
}

impl Sys {
    /// Firmware for a slot and mode from the server.
    pub async fn radio_firmware(&self, slot: &RadioSlot, mode: RadioMode) -> Result<Vec<Firmware>> {
        let id = device_id(slot.key).ok_or_else(|| Error::NotSupported(format!("no firmware for {}", slot.key)))?;
        let url = format!("{OTA}{id}");
        let text = self.fetch_text(&url, Duration::from_secs(15)).await?;
        Ok(parse_firmware(&text, mode.legacy_type()))
    }

    /// The OpenRC service whose process holds a device open (old `get_service_for_tty`).
    pub fn service_holding(&self, dev: &str) -> Option<String> {
        let mut holder = None;
        for e in std::fs::read_dir(self.p("/proc")).ok()?.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let Ok(fds) = std::fs::read_dir(e.path().join("fd")) else { continue };
            if fds.flatten().any(|fd| std::fs::read_link(fd.path()).is_ok_and(|t| t == Path::new(dev))) {
                holder = Some(name);
                break;
            }
        }
        let mut pids = vec![holder?];
        for _ in 0..10 {
            let Some(last) = pids.last() else { break };
            let stat = std::fs::read_to_string(self.p(&format!("/proc/{last}/stat"))).unwrap_or_default();
            let ppid = stat.rsplit(')').next().and_then(|r| r.split_whitespace().nth(1)).unwrap_or("0").to_owned();
            if ppid == "0" || ppid == "1" {
                break;
            }
            pids.push(ppid);
        }
        if let Ok(rd) = std::fs::read_dir(self.p("/run/openrc/daemons")) {
            for svc in rd.flatten() {
                for f in std::fs::read_dir(svc.path()).into_iter().flatten().flatten() {
                    let text = std::fs::read_to_string(f.path()).unwrap_or_default();
                    for l in text.lines() {
                        if let Some(pf) = l.strip_prefix("pidfile=") {
                            if let Ok(pid) = std::fs::read_to_string(self.p(pf.trim())) {
                                if pids.iter().any(|p| p == pid.trim()) {
                                    return Some(svc.file_name().to_string_lossy().into_owned());
                                }
                            }
                        }
                    }
                }
            }
        }
        for f in std::fs::read_dir(self.p("/run")).ok()?.flatten() {
            let n = f.file_name().to_string_lossy().into_owned();
            if let Some(stem) = n.strip_suffix(".pid") {
                if std::fs::read_to_string(f.path()).is_ok_and(|p| pids.iter().any(|x| x == p.trim())) {
                    return Some(stem.to_owned());
                }
            }
        }
        None
    }

    /// Download firmware to a file.
    pub async fn radio_download(&self, fw: &Firmware, dest: &Path) -> Result<()> {
        if fw.link.is_empty() {
            return Err(Error::Unavailable("the firmware has no download address".into()));
        }
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d)?;
        }
        self.fetch_file(&fw.link, dest, Duration::from_secs(300)).await
    }

    /// Whether the flash helper is installed.
    pub fn radio_helper_ready(&self) -> bool {
        self.p(HELPER).is_file()
    }

    /// Flash a firmware file with the helper, reporting `(percent, step)`.
    pub async fn radio_flash<F: FnMut(Option<f64>, Option<String>)>(
        &self,
        slot: &RadioSlot,
        file: &Path,
        ieee: Option<&str>,
        mut progress: F,
    ) -> Result<()> {
        if !self.radio_helper_ready() {
            return Err(Error::Unavailable("the radio flashing helper is not installed".into()));
        }
        let model = self.model();
        let target = reset_target(model.family, &model.model, self.eeprom().map(|e| e.hw_rev.as_str()), slot);
        let chip = match slot.vendor {
            Vendor::Ti => "ti",
            Vendor::Silabs if slot.protocols.contains(&"zwave") => "zwave",
            Vendor::Silabs => "silabs",
        };
        let mut c = self.command("python3");
        c.env("PYTHONPATH", self.p(HELPER_LIB))
            .arg(self.p(HELPER))
            .args(["--chip", chip, "--port", &slot.port, "--target", target, "--file"])
            .arg(file)
            .arg("--key")
            .arg(slot.key);
        if let Some(i) = ieee.filter(|i| !i.is_empty()) {
            c.args(["--ieee", i]);
        }
        c.stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(false);
        let mut child = c.spawn().map_err(|e| Error::Unavailable(format!("cannot start the flash helper: {e}")))?;
        let stderr = child.stderr.take();
        let err_task = tokio::spawn(async move {
            let mut tail: Vec<String> = Vec::new();
            if let Some(e) = stderr {
                let mut lines = BufReader::new(e).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    tail.push(l);
                    if tail.len() > 20 {
                        tail.remove(0);
                    }
                }
            }
            tail.join("\n")
        });
        let mut error: Option<String> = None;
        if let Some(o) = child.stdout.take() {
            let mut lines = BufReader::new(o).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&l) else { continue };
                if let Some(p) = v.get("progress").and_then(serde_json::Value::as_f64) {
                    progress(Some(p), None);
                }
                if let Some(s) = v.get("step").and_then(|s| s.as_str()) {
                    // the helper's stages, in the operation step names of the API
                    let step = match s {
                        "entering_bootloader" => Some("preparing"),
                        "flashing" => Some("flashing"),
                        "writing_ieee" => Some("writing"),
                        "ieee_failed" => {
                            tracing::warn!("the radio was flashed, but its IEEE address could not be written back");
                            None
                        }
                        _ => None,
                    };
                    if let Some(step) = step {
                        progress(None, Some(step.to_owned()));
                    }
                }
                if let Some(e) = v.get("error").and_then(|s| s.as_str()) {
                    error = Some(e.to_owned());
                }
            }
        }
        let status = tokio::time::timeout(Duration::from_secs(900), child.wait())
            .await
            .map_err(|_| Error::Unavailable("flashing took longer than 15 minutes".into()))?
            .map_err(|e| Error::internal(format!("flash helper: {e}")))?;
        let tail = err_task.await.unwrap_or_default();
        match (status.success(), error) {
            (true, None) => Ok(()),
            (_, Some(e)) => Err(Error::internal(e)),
            (false, None) => Err(Error::internal(format!("the flash helper failed: {}", tail.lines().last().unwrap_or("no output")))),
        }
    }

    /// Where downloaded firmware goes (on the user partition, not in RAM).
    pub fn firmware_cache(&self, index: u8) -> PathBuf {
        self.p(&format!("/mnt/user/.smhubd/radio-{index}.fw"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_list() {
        let t = r#"[{"ver":"20250101","type":0,"link":"https://x/a.bin","notes":"n"},{"ver":"2","type":1},{"rev":"7.4","type":"0","prod":false}]"#;
        let l = parse_firmware(t, 0);
        assert_eq!(l.len(), 2);
        assert_eq!(l[1].version, "7.4");
        assert!(l[1].dev);
    }
}
