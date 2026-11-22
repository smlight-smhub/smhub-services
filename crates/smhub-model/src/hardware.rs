//! The hardware model table (decision D27): one row per model string from the EEPROM.
//! The radio page, app defaults and the `system` resource all come from here.
//!
//! Sources: the old backend's `DEVICE_MAPS` (`fastapi-backend/.../api/routes/radio.py`), the old
//! UI's `MODEL_MAP` and the board device trees.

use serde::Serialize;

/// Product family, which selects the device tree and the board's peripherals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Smhub,
    Nano,
    Nanomr,
}

/// Radio chip vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Ti,
    Silabs,
}

/// One radio module position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RadioSlot {
    /// 1-based slot number shown to the user; the port is `/dev/ttyS{index}`.
    pub index: u8,
    pub port: String,
    pub chip: &'static str,
    /// Firmware server key, e.g. `SMHUB-MG24`.
    pub key: &'static str,
    pub vendor: Vendor,
    pub protocols: Vec<&'static str>,
}

/// A radio mode the firmware server knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadioMode {
    ZigbeeCoordinator,
    ZigbeeRouter,
    Thread,
    ZwaveEu,
    ZwaveUs,
    ZwaveAnz,
}

impl RadioMode {
    pub const ALL: [Self; 6] =
        [Self::ZigbeeCoordinator, Self::ZigbeeRouter, Self::Thread, Self::ZwaveEu, Self::ZwaveUs, Self::ZwaveAnz];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ZigbeeCoordinator => "zigbee_coordinator",
            Self::ZigbeeRouter => "zigbee_router",
            Self::Thread => "thread",
            Self::ZwaveEu => "zwave_eu",
            Self::ZwaveUs => "zwave_us",
            Self::ZwaveAnz => "zwave_anz",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == s)
    }

    /// The numeric type the old UI stored in the `radios` page (`mode{N}`) and the firmware
    /// server's `type` parameter: 0 coordinator, 1 router, 2 Thread, 5/6/7 Z-Wave EU/US/ANZ.
    pub fn legacy_type(self) -> u8 {
        match self {
            Self::ZigbeeCoordinator => 0,
            Self::ZigbeeRouter => 1,
            Self::Thread => 2,
            Self::ZwaveEu => 5,
            Self::ZwaveUs => 6,
            Self::ZwaveAnz => 7,
        }
    }

    pub fn from_legacy_type(t: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.legacy_type().to_string() == t.trim())
    }

    pub fn is_zwave(self) -> bool {
        matches!(self, Self::ZwaveEu | Self::ZwaveUs | Self::ZwaveAnz)
    }

    /// The firmware server's `mode` family for this mode.
    pub fn server_mode(self) -> &'static str {
        match self {
            Self::ZigbeeCoordinator | Self::ZigbeeRouter => "zigbee",
            Self::Thread => "thread",
            Self::ZwaveEu | Self::ZwaveUs | Self::ZwaveAnz => "zwave",
        }
    }
}

/// Everything the daemon knows about a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// Lower-case model string, e.g. `essential`.
    pub model: String,
    pub product: String,
    pub family: Family,
    pub radios: Vec<RadioSlot>,
    /// The board has a slot for the 4G add-on.
    pub lte_slot: bool,
    /// Add-on modules (4G, Z-Wave, PoE, UPS) are detected by GPIO.
    pub addon_detection: bool,
    pub ambient_light: bool,
    pub buzzer: bool,
    pub ir: bool,
    /// The board has a microSD slot: SMHUB and Nano do, Nano MR does not, although
    /// every device tree enables `sdhci0`.
    pub sd_slot: bool,
}

impl ModelInfo {
    /// Modes this slot can be flashed to.
    pub fn modes_for(slot: &RadioSlot) -> Vec<RadioMode> {
        if slot.protocols.contains(&"zwave") {
            vec![RadioMode::ZwaveEu, RadioMode::ZwaveUs, RadioMode::ZwaveAnz]
        } else {
            vec![RadioMode::ZigbeeCoordinator, RadioMode::ZigbeeRouter, RadioMode::Thread]
        }
    }

    /// The Zigbee2MQTT adapter type for this model: `ember` for Silicon Labs-only models,
    /// `zstack` otherwise (old backend `apply_model_specific_defaults`).
    pub fn zigbee_adapter(&self) -> &'static str {
        if self.model.contains("nanomg") || self.model == "nanomr5" { "ember" } else { "zstack" }
    }
}

fn chip(key: &str) -> Option<(&'static str, &'static str, Vendor, Vec<&'static str>)> {
    let zt = vec!["zigbee", "thread"];
    Some(match key {
        "CCP1" => ("SMHUB-CCP1", "CC2652P", Vendor::Ti, zt),
        "CCP7" => ("SMHUB-CCP7", "CC2652P7", Vendor::Ti, zt),
        "CCP10" => ("SMHUB-CCP10", "CC2674P10", Vendor::Ti, zt),
        "MG21" => ("SMHUB-MG21", "EFR32MG21", Vendor::Silabs, zt),
        "MG24" => ("SMHUB-MG24", "EFR32MG24", Vendor::Silabs, zt),
        "MG26" => ("SMHUB-MG26", "EFR32MG26", Vendor::Silabs, zt),
        "ZG23" => ("SMHUB-ZG23", "ZG23", Vendor::Silabs, vec!["zwave"]),
        _ => return None,
    })
}

fn slots(keys: &[&str]) -> Vec<RadioSlot> {
    keys.iter()
        .enumerate()
        .filter_map(|(i, k)| {
            let (key, chip_name, vendor, protocols) = chip(k)?;
            let index = u8::try_from(i + 1).ok()?;
            Some(RadioSlot { index, port: format!("/dev/ttyS{index}"), chip: chip_name, key, vendor, protocols })
        })
        .collect()
}

/// The row for a model string. Unknown or missing models fall back to `essential`, as the old
/// backend did.
pub fn lookup(model: Option<&str>) -> ModelInfo {
    let m = model.map(|s| s.trim().to_ascii_lowercase()).unwrap_or_default();
    let (product, family, keys): (&str, Family, &[&str]) = match m.as_str() {
        "premium" => ("SMHUB Premium", Family::Smhub, &["CCP10", "MG24", "ZG23"]),
        "nanop1" => ("SMHUB Nano P1", Family::Nano, &["CCP1"]),
        "nanop7" => ("SMHUB Nano P7", Family::Nano, &["CCP7"]),
        "nanop10" => ("SMHUB Nano P10", Family::Nano, &["CCP10"]),
        "nanomg21" => ("SMHUB Nano MG21", Family::Nano, &["MG21"]),
        "nanomg24" => ("SMHUB Nano MG24", Family::Nano, &["MG24"]),
        "nanomg26" => ("SMHUB Nano MG26", Family::Nano, &["MG26"]),
        "nanomr1" => ("SMHUB Nano MR1", Family::Nanomr, &["CCP7", "MG21"]),
        "nanomr2" => ("SMHUB Nano MR2", Family::Nanomr, &["CCP1", "MG21"]),
        "nanomr3" => ("SMHUB Nano MR3", Family::Nanomr, &["CCP10", "MG24"]),
        "nanomr4" => ("SMHUB Nano MR4", Family::Nanomr, &["CCP10", "MG26"]),
        "nanomr5" => ("SMHUB Nano MR5", Family::Nanomr, &["MG24", "MG24"]),
        "nanomr6" => ("SMHUB Nano MR6", Family::Nanomr, &["CCP7", "MG24"]),
        "nanomr7" => ("SMHUB Nano MR7", Family::Nanomr, &["CCP7", "MG26"]),
        "nanomrw" => ("SMHUB Nano MRW", Family::Nanomr, &["CCP7", "MG21"]),
        _ => ("SMHUB Essential", Family::Smhub, &["CCP1", "MG21", "ZG23"]),
    };
    let model = if product == "SMHUB Essential" && m != "essential" { "essential".to_owned() } else { m };
    let full = family == Family::Smhub;
    ModelInfo {
        model,
        product: product.to_owned(),
        family,
        radios: slots(keys),
        lte_slot: full,
        addon_detection: full,
        ambient_light: full,
        buzzer: full,
        ir: full,
        sd_slot: family != Family::Nanomr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_and_fallback() {
        let e = lookup(Some("Essential"));
        assert_eq!(e.model, "essential");
        assert_eq!(e.radios.len(), 3);
        assert_eq!(e.radios[2].port, "/dev/ttyS3");
        assert_eq!(e.radios[2].protocols, vec!["zwave"]);
        let n = lookup(Some("nanomg24"));
        assert_eq!(n.family, Family::Nano);
        assert_eq!(n.zigbee_adapter(), "ember");
        let u = lookup(None);
        assert_eq!(u.model, "essential");
        assert_eq!(lookup(Some("nanomr5")).zigbee_adapter(), "ember");
        assert_eq!(lookup(Some("nanomr3")).zigbee_adapter(), "zstack");
        assert!(e.sd_slot && n.sd_slot && !lookup(Some("nanomr1")).sd_slot);
    }

    #[test]
    fn legacy_modes() {
        assert_eq!(RadioMode::from_legacy_type("2"), Some(RadioMode::Thread));
        assert_eq!(RadioMode::ZwaveUs.legacy_type(), 6);
    }
}
