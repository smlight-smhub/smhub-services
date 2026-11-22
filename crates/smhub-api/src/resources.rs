//! The state of every resource, as `GET` returns it and the event connection sends it.

use std::time::Duration;

use serde_json::{Value, json};
use smhub_model::hardware::ModelInfo;
use smhub_model::time::{now_unix, rfc3339};
use smhub_model::{Error, Result};
use smhub_store::parse_bool;
use smhub_sys::services::{State as Svc, service_for_package};

use crate::state::{AppState, lock};

/// Services the hub shows besides app services.
const SYSTEM_SERVICES: &[&str] =
    &["networkmanager", "mosquitto", "sshd", "stunnel", "nginx", "ntpd", "avahi-daemon", "bluetoothd", "smhub-broker", "smhub-services"];

pub async fn state(hub: &AppState, r: &str) -> Result<Value> {
    match r {
        "system" => system(hub).await,
        "metrics" => Ok(serde_json::to_value(hub.sys.metrics())?),
        "network" => network(hub).await,
        "wifi" => wifi(hub).await,
        "services" => services(hub).await,
        "apps" => crate::apps::list(hub).await,
        "updates" => updates(hub).await,
        "lte" => lte(hub).await,
        "radios" => radios(hub).await,
        "operations" => Ok(json!({ "operations": hub.operations() })),
        "vpn" => vpn(hub).await,
        "hardware" => Ok(crate::hw::state_json(hub)),
        "ir" => crate::hw::ir::state_json(hub).await,
        other => Err(Error::not_found(format!("resource {other}"))),
    }
}

/// Read an add-on detection line (active-low, true = fitted).
async fn addon(hub: &AppState, args: &[&str]) -> bool {
    match hub.sys.exec("gpioget", args, Duration::from_secs(5)).await {
        Ok(o) if o.ok() => o.stdout.trim() == "1",
        _ => false,
    }
}

/// Hardware features, with add-on detection by GPIO for the full-size hub (old UI read
/// `4g_check`, `zwave_check`, `poe_check`, `ups_check`; chip numbers per reference §13.1).
pub async fn features(hub: &AppState) -> Value {
    if let Some((at, v)) = lock(&hub.features).clone() {
        if now_unix() - at < 600 {
            return v;
        }
    }
    let m: &ModelInfo = hub.sys.model();
    let (lte, zwave, poe) = if m.addon_detection {
        (
            hub.sys.modem_fitted().await,
            addon(hub, &["--numeric", "--active-low", "-c", "gpiochip3", "8"]).await,
            addon(hub, &["--numeric", "--active-low", "-c", "gpiochip0", "0"]).await,
        )
    } else {
        (false, false, false)
    };
    // a UPS board is not detected by a line (J10 pin 5 reads "fitted" on a hub without one):
    // the owner marks it as connected (Peripherals → UPS)
    let ups = crate::hw::ups::installed(hub);
    let v = json!({
        "wifi": hub.sys.p("/sys/class/net/wlan0").exists(),
        "bluetooth": hub.sys.p("/sys/class/bluetooth").exists() || m.family == smhub_model::hardware::Family::Smhub,
        "lte": lte,
        "zwave": zwave,
        "poe": poe,
        "ups": ups,
        "ambient_light": m.ambient_light,
        "buzzer": m.buzzer,
        "ir": m.ir,
        "esphome": hub.sys.esphome_available(),
        "sdcard": hub.sys.is_mounted("/mnt/sdcard"),
        "sd_slot": m.sd_slot,
        "usb_role": true,
    });
    *lock(&hub.features) = Some((now_unix(), v.clone()));
    v
}

pub async fn system(hub: &AppState) -> Result<Value> {
    let m = hub.sys.model();
    let e = hub.sys.eeprom();
    let feats = features(hub).await;
    let mut radios: Vec<Value> = m.radios.iter().map(|r| serde_json::to_value(r).unwrap_or(Value::Null)).collect();
    if !feats["zwave"].as_bool().unwrap_or(false) && m.addon_detection {
        radios.retain(|r| !r["protocols"].as_array().is_some_and(|p| p.iter().any(|x| x == "zwave")));
    }
    Ok(json!({
        "model": m.model,
        "product": m.product,
        "family": m.family,
        "hardware_revision": e.map(|e| e.hw_rev.clone()),
        "device_id": e.map(|e| e.uuid.clone()),
        "hostname": hub.sys.hostname(),
        "os_version": hub.sys.os_version(),
        "core_version": hub.cfg.version,
        // the installed web UI; a page built for another version reloads itself
        "ui_version": std::fs::read_to_string(hub.sys.p("/var/lib/opkg/info/smhub-ui.control")).ok().and_then(|t| t.lines().find_map(|l| l.strip_prefix("Version:").map(|v| v.trim().to_owned()))),
        "uptime_s": hub.sys.uptime(),
        "time": rfc3339(now_unix()),
        "timezone": hub.sys.timezone(),
        "auth": { "required": hub.security_required.load(std::sync::atomic::Ordering::Relaxed) },
        "setup_needed": hub.setup_needed.load(std::sync::atomic::Ordering::Relaxed),
        "features": feats,
        "radios": radios,
    }))
}

pub async fn internet(hub: &AppState) -> Option<bool> {
    if let Some((at, v)) = *lock(&hub.internet) {
        if now_unix() - at < 60 {
            return v;
        }
    }
    let v = hub.sys.internet().await;
    *lock(&hub.internet) = Some((now_unix(), v));
    v
}

pub async fn network(hub: &AppState) -> Result<Value> {
    let ifaces = hub.sys.interfaces().await;
    Ok(json!({ "hostname": hub.sys.hostname(), "internet": internet(hub).await, "interfaces": ifaces }))
}

pub async fn wifi(hub: &AppState) -> Result<Value> {
    let w = hub.sys.wifi().await;
    let mut v = serde_json::to_value(w)?;
    if let Some(o) = v.as_object_mut() {
        o.insert("scanned_at".into(), json!(lock(&hub.wifi_scanned).map(rfc3339)));
    }
    Ok(v)
}

pub async fn vpn(hub: &AppState) -> Result<Value> {
    let wg = hub.sys.wireguard();
    let (active, state) = if wg.is_some() { hub.sys.wireguard_active().await } else { (false, "unconfigured") };
    let settings = wg.clone().unwrap_or_default();
    Ok(json!({
        "configured": wg.is_some(),
        "active": active,
        "state": state,
        "settings": settings,
        "handshake_at": null,
        "rx_bytes": null,
        "tx_bytes": null,
    }))
}

pub async fn services(hub: &AppState) -> Result<Value> {
    let snap = hub.sys.services().await;
    let rows = hub.store.run(|db| db.apps()).await?;
    let mut out: Vec<Value> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut names: Vec<&str> = SYSTEM_SERVICES.to_vec();
    if hub.sys.ser2net_configured() || hub.sys.service_autostart("ser2net") {
        names.push("ser2net");
    }
    for name in names {
        if !hub.sys.service_exists(name) {
            continue;
        }
        seen.insert(name.to_owned());
        out.push(json!({
            "name": name,
            "app": null,
            "state": snap.get(name).copied().unwrap_or(Svc::Stopped),
            "autostart": hub.sys.service_autostart(name),
            "kind": "system",
        }));
    }
    for r in rows.iter().filter(|r| r.installed && r.is_service) {
        let schema = hub.sys.package_schema(&r.key);
        let svc = service_for_package(schema.as_ref(), &r.key);
        if !seen.insert(svc.clone()) || !hub.sys.service_exists(&svc) {
            continue;
        }
        out.push(json!({
            "name": svc,
            "app": r.key,
            "state": snap.get(&svc).copied().unwrap_or(Svc::Stopped),
            "autostart": hub.sys.service_autostart(&svc),
            "kind": "app",
        }));
    }
    Ok(json!({ "services": out }))
}

/// The update channel: `dev` when the dev feed is configured.
pub fn channel(hub: &AppState) -> &'static str {
    hub.sys.app_channel()
}

pub async fn updates(hub: &AppState) -> Result<Value> {
    let ch = channel(hub);
    let current = hub.sys.os_version();
    let cached = hub.sys.os_releases_cached().filter(|c| c.channel == ch);
    let releases: Vec<Value> = cached
        .as_ref()
        .map(|c| {
            c.releases
                .iter()
                .map(|r| {
                    json!({
                        "version": r.version,
                        "channel": if r.channel == "dev" { "dev" } else { "stable" },
                        "changelog": r.changelog,
                        "newer": smhub_sys::updates::os_version_cmp(&r.version, &current) == std::cmp::Ordering::Greater,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let update_available = releases.iter().any(|r| r["newer"].as_bool() == Some(true));
    let apps = crate::apps::list(hub).await?;
    let auto = hub.store.run(|db| db.page_setting("updates", "auto_update")).await?.is_some_and(|v| parse_bool(&v));
    Ok(json!({
        "os": {
            "current": current,
            "channel": ch,
            "releases": releases,
            "update_available": update_available,
            "checked_at": cached.as_ref().map(|c| rfc3339(c.checked_at)),
            "error": cached.as_ref().and_then(|c| c.error.clone()),
        },
        "apps": { "updates": apps["updates"] },
        "auto_update": auto,
    }))
}

pub async fn lte(hub: &AppState) -> Result<Value> {
    let page = hub.store.run(|db| db.page_settings("lte")).await?;
    let settings = json!({
        "apn": page.get("apn").cloned().unwrap_or_default(),
        "autostart": hub.sys.package_flag("lte", "autostart"),
        "led": !page.get("led_control").is_some_and(|v| parse_bool(v)),
    });
    let present = features(hub).await["lte"].as_bool().unwrap_or(false);
    if !present {
        return Ok(json!({ "present": false, "powered": false, "state": "absent", "usb_host_needed": false, "settings": settings }));
    }
    let usb_host = hub.sys.usb_role_now().unwrap_or_else(|| hub.sys.usb_mode()) == "host";
    let on_usb = hub.sys.modem_on_usb().await;
    if !on_usb {
        return Ok(json!({ "present": true, "powered": false, "state": "off", "usb_host_needed": !usb_host, "settings": settings }));
    }
    if let Some((at, v)) = lock(&hub.modem).clone() {
        if now_unix() - at < 20 {
            return Ok(v);
        }
    }
    let v = match hub.sys.modem_status().await {
        Ok(s) => {
            let state = match s.registered {
                Some(1) => "registered",
                Some(5) => "roaming",
                Some(3) => "denied",
                Some(2) | Some(0) => "searching",
                _ => "searching",
            };
            json!({
                "present": true, "powered": true, "state": state,
                "operator": s.operator, "technology": s.technology,
                "signal_dbm": s.signal_dbm, "signal_percent": s.signal_percent, "imei": s.imei,
                "usb_host_needed": false, "settings": settings,
            })
        }
        Err(e) => json!({ "present": true, "powered": true, "state": "error", "error": e.message(), "usb_host_needed": false, "settings": settings }),
    };
    *lock(&hub.modem) = Some((now_unix(), v.clone()));
    Ok(v)
}

pub async fn radios(hub: &AppState) -> Result<Value> {
    let sys_v = system(hub).await?;
    let page = hub.store.run(|db| db.page_settings("radios")).await?;
    let model = hub.sys.model();
    let mut out = Vec::new();
    for slot in model.radios.iter() {
        if !sys_v["radios"].as_array().is_some_and(|a| a.iter().any(|r| r["index"] == slot.index)) {
            continue;
        }
        let i = slot.index - 1;
        let mode = page
            .get(&format!("mode{i}"))
            .and_then(|m| smhub_model::hardware::RadioMode::from_legacy_type(m))
            .map(|m| m.as_str());
        let version = page.get(&format!("version{i}")).filter(|v| !v.is_empty()).cloned();
        let ieee = page.get(&format!("ieee{i}")).filter(|v| !v.is_empty()).cloned();
        let firmware = match (&version, mode) {
            (Some(v), Some(m)) => json!({ "mode": m, "version": v, "ieee": ieee }),
            _ => Value::Null,
        };
        let mut v = serde_json::to_value(slot)?;
        if let Some(o) = v.as_object_mut() {
            o.insert("modes".into(), json!(ModelInfo::modes_for(slot).iter().map(|m| m.as_str()).collect::<Vec<_>>()));
            o.insert("firmware".into(), firmware);
            o.insert("in_use_by".into(), json!(hub.sys.service_holding(&slot.port)));
        }
        out.push(v);
    }
    Ok(json!({ "radios": out }))
}
