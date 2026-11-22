//! The second core's ESPHome firmware: device-tree overlays that hand peripherals to it, the API
//! encryption key patched into its ELF, and the Bluetooth proxy mode (reference §5).

use smhub_model::{Error, Result};

use crate::{Sys, fsx};

pub const PATCHED_ELF: &str = "/opt/firmware/smhub-rtos.elf";
pub const OVERLAY_SRC: &str = "/lib/firmware/overlays";
pub const OVERLAY_DST: &str = "/mnt/misc/overlays";
pub const BT_MODE: &str = "/opt/firmware/bluetooth_proxy_mode";
/// Overlays the daemon's own package ships (packaging/smhub-services/overlays).
pub const OWN_OVERLAYS: &str = "/opt/smhub-services/overlays";
/// I2C2 on J10 for a UPS board, named `i2c2`.
pub const UPS_OVERLAY: &str = "ups-i2c2.dtbo";

/// The factory-default ESPHome API key shipped in every RTOS build. It is public (it is the
/// default in the old UI's schema); the patch looks for these bytes in the original ELF.
pub const FACTORY_KEY_B64: &str = "VGVzdGluZ1RSTkdFbmNyeXB0aW9uS2V5MTIzNDU2Nzg=";

/// Setting name to overlay file.
pub const OVERLAYS: [(&str, &str); 5] = [
    ("ambient_light", "rtos-spi0.dtbo"),
    ("buzzer", "rtos-pwm3.dtbo"),
    ("user_i2c", "rtos-i2c4.dtbo"),
    ("adc1", "rtos-adc1.dtbo"),
    ("adc2", "rtos-adc2.dtbo"),
];

fn b64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s.trim()).ok()
}

impl Sys {
    fn backup_elf(&self) -> std::path::PathBuf {
        let nano = self.model().model.contains("nano");
        self.p(if nano { "/var/lib/opkg/info/esphome-bin.nano.elf" } else { "/var/lib/opkg/info/esphome-bin.smhub.elf" })
    }

    /// Whether the ESPHome firmware package is installed.
    pub fn esphome_available(&self) -> bool {
        self.backup_elf().exists() || self.p(PATCHED_ELF).exists()
    }

    /// Whether an overlay is active (copied to the boot partition).
    pub fn overlay_enabled(&self, file: &str) -> bool {
        self.p(&format!("{OVERLAY_DST}/{file}")).exists()
    }

    /// Copy or remove overlays for the settings given. Returns whether anything changed.
    pub fn set_overlays(&self, settings: &[(&str, bool)]) -> Result<bool> {
        let mut changed = false;
        for (name, on) in settings {
            let Some((_, file)) = OVERLAYS.iter().find(|(n, _)| n == name) else { continue };
            let dst = self.p(&format!("{OVERLAY_DST}/{file}"));
            if *on {
                let src = self.p(&format!("{OVERLAY_SRC}/{file}"));
                if !dst.exists() && src.exists() {
                    std::fs::create_dir_all(self.p(OVERLAY_DST))?;
                    std::fs::copy(&src, &dst)?;
                    changed = true;
                }
            } else if dst.exists() {
                std::fs::remove_file(&dst)?;
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Whether the UPS board's overlay is among the boot overlays.
    pub fn ups_overlay(&self) -> bool {
        self.p(&format!("{OVERLAY_DST}/{UPS_OVERLAY}")).exists()
    }

    /// Put the UPS board's overlay among the boot overlays, or take it out. The OS's
    /// `dtbo-watcher` lists the folder in uEnv.txt at once; U-Boot applies it from the next start.
    /// Returns whether anything changed.
    pub fn set_ups_overlay(&self, on: bool) -> Result<bool> {
        let dst = self.p(&format!("{OVERLAY_DST}/{UPS_OVERLAY}"));
        if !on {
            if dst.exists() {
                std::fs::remove_file(&dst)?;
                return Ok(true);
            }
            return Ok(false);
        }
        if dst.exists() {
            return Ok(false);
        }
        let src = self.p(&format!("{OWN_OVERLAYS}/{UPS_OVERLAY}"));
        if !src.exists() {
            return Err(Error::Unavailable(format!("{UPS_OVERLAY} is missing from the hub's package")));
        }
        std::fs::create_dir_all(self.p(OVERLAY_DST))?;
        std::fs::copy(&src, &dst)?;
        Ok(true)
    }

    /// The Bluetooth proxy mode file's value.
    pub fn bt_mode(&self) -> Option<String> {
        fsx::read_trim(&self.p(BT_MODE))
    }

    /// Write the Bluetooth proxy mode (no newline). Returns whether it changed.
    pub fn set_bt_mode(&self, mode: &str) -> Result<bool> {
        if !matches!(mode, "off" | "hci" | "bluez" | "proxy") {
            return Err(Error::invalid("bluetooth_proxy_mode", "use off, hci or proxy"));
        }
        if self.bt_mode().as_deref() == Some(mode) {
            return Ok(false);
        }
        fsx::write_plain(&self.p(BT_MODE), mode.as_bytes())?;
        Ok(true)
    }

    /// Put an API encryption key into the RTOS ELF, as the old `patch_encryption_key`.
    /// Returns whether the ELF was rewritten.
    pub fn patch_esphome_key(&self, key_b64: &str) -> Result<bool> {
        let key = b64(key_b64).filter(|k| k.len() == 32).ok_or_else(|| Error::invalid("encryption_key", "the key must be 32 bytes in base64"))?;
        let factory = b64(FACTORY_KEY_B64).unwrap_or_default();
        let patched = self.p(PATCHED_ELF);
        let backup = self.backup_elf();
        if !patched.exists() {
            if !backup.exists() {
                return Ok(false);
            }
            if let Some(d) = patched.parent() {
                std::fs::create_dir_all(d)?;
            }
            std::fs::copy(&backup, &patched)?;
        }
        let current = std::fs::read(&patched)?;
        if current.windows(32).any(|w| w == key.as_slice()) {
            return Ok(false);
        }
        if !backup.exists() {
            return Ok(false);
        }
        let mut data = std::fs::read(&backup)?;
        if key != factory {
            let pos = data
                .windows(32)
                .position(|w| w == factory.as_slice())
                .ok_or_else(|| Error::internal("the factory key was not found in the ESPHome firmware"))?;
            data[pos..pos + 32].copy_from_slice(&key);
        }
        fsx::write_plain(&patched, &data)?;
        Ok(true)
    }

    /// The key currently in the RTOS ELF, if it is the factory key or a 32-byte value
    /// recognisable as the stored setting.
    pub fn esphome_key_matches(&self, key_b64: &str) -> bool {
        match (b64(key_b64), std::fs::read(self.p(PATCHED_ELF))) {
            (Some(k), Ok(elf)) if k.len() == 32 => elf.windows(32).any(|w| w == k.as_slice()),
            _ => false,
        }
    }
}
