//! The service LEDs (power, blue): switched through ESPHome while it runs, else held by the
//! daemon through `gpioset`. Night mode and "all off" put them out and back.

use serde_json::{Value, json};
use smhub_model::{Error, Result};
use smhub_sys::esphome::Command;

use super::{Inner, Owner, esp, note};
use crate::state::{AppState, lock};

#[derive(Debug, Clone, Default)]
pub struct Leds {
    pub owner: Owner,
    pub known: bool,
    pub power: Option<bool>,
    pub blue: Option<bool>,
    /// The LEDs before night mode or "all off" put them out.
    saved: Option<(bool, bool)>,
}

pub(crate) fn json(st: &Inner) -> Value {
    let l = &st.leds;
    json!({
        "available": matches!(l.owner, Owner::Esphome | Owner::Linux),
        "owner": l.owner,
        "known": l.known,
        "power": l.power,
        "blue": l.blue,
    })
}

/// A switch state from ESPHome.
pub fn esp_state(hub: &AppState, power: bool, on: bool) {
    let mut st = lock(&hub.hw.st);
    let l = &mut st.leds;
    if power {
        l.power = Some(on);
    } else {
        l.blue = Some(on);
    }
    l.known = l.power.is_some() && l.blue.is_some();
    if on && st.dark {
        // somebody switched an LED on during the night: it stays as they set it
        if let Some((p, b)) = st.leds.saved.as_mut() {
            if power {
                *p = true;
            } else {
                *b = true;
            }
        }
    }
}

async fn apply(hub: &AppState, power: Option<bool>, blue: Option<bool>) -> Result<()> {
    let (owner, map, cur) = {
        let st = lock(&hub.hw.st);
        (st.leds.owner, st.esp.map.clone(), (st.leds.power.unwrap_or(true), st.leds.blue.unwrap_or(false)))
    };
    match owner {
        Owner::Esphome => {
            for (v, key) in [(power, map.power_led), (blue, map.blue_led)] {
                if let Some(v) = v {
                    let key = key.ok_or_else(|| Error::Unavailable("ESPHome has no such LED".into()))?;
                    esp::send(hub, Command::Switch { key, on: v })?;
                }
            }
            // shown at once; ESPHome's report follows
            let mut st = lock(&hub.hw.st);
            if let Some(p) = power {
                st.leds.power = Some(p);
            }
            if let Some(b) = blue {
                st.leds.blue = Some(b);
            }
        }
        Owner::Linux => {
            let (p, b) = (power.unwrap_or(cur.0), blue.unwrap_or(cur.1));
            {
                let mut child = lock(&hub.hw.leds_child);
                // the old holder lets go first, then the new one takes the lines
                if let Some(mut c) = child.take() {
                    let _ = c.start_kill();
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
            let c = hub.sys.leds_hold(p, b)?;
            *lock(&hub.hw.leds_child) = Some(c);
            {
                let mut st = lock(&hub.hw.st);
                st.leds.power = Some(p);
                st.leds.blue = Some(b);
                st.leds.known = true;
            }
        }
        _ => return Err(Error::Unavailable("the LEDs are not available now".into())),
    }
    Ok(())
}

/// A person (or a button) switches the LEDs.
pub async fn set(hub: &AppState, power: Option<bool>, blue: Option<bool>, by: &str) -> Result<()> {
    apply(hub, power, blue).await?;
    let linux = lock(&hub.hw.st).leds.owner == Owner::Linux;
    if linux {
        let look = {
            let st = lock(&hub.hw.st);
            (st.leds.power.unwrap_or(true), st.leds.blue.unwrap_or(false))
        };
        if let Ok(text) = serde_json::to_string(&look) {
            let _ = hub.store.run(move |db| db.set_hub_setting("leds_look", &text)).await;
        }
    }
    let what = [power.map(|p| format!("power {}", if p { "on" } else { "off" })), blue.map(|b| format!("blue {}", if b { "on" } else { "off" }))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    note(hub, "leds", what, by, "done");
    Ok(())
}

/// Night mode and "all off": out, then back as they were.
pub async fn set_dark(hub: &AppState, dark: bool) {
    let (cur, known) = {
        let st = lock(&hub.hw.st);
        ((st.leds.power.unwrap_or(true), st.leds.blue.unwrap_or(false)), st.leds.known || st.leds.owner == Owner::Linux)
    };
    if dark {
        if known {
            lock(&hub.hw.st).leds.saved = Some(cur);
        }
        let _ = apply(hub, Some(false), Some(false)).await;
    } else {
        let saved = lock(&hub.hw.st).leds.saved.take();
        let (p, b) = saved.unwrap_or((true, false));
        let _ = apply(hub, Some(p), Some(b)).await;
    }
}

/// At start, on the Linux side: show the saved state (power on, blue off by default).
pub fn start(hub: &AppState) {
    let (owner, dark) = {
        let st = lock(&hub.hw.st);
        (st.leds.owner, st.dark)
    };
    if owner != Owner::Linux {
        if let Some(mut c) = lock(&hub.hw.leds_child).take() {
            let _ = c.start_kill();
        }
        return;
    }
    let h = std::sync::Arc::clone(hub);
    tokio::spawn(async move {
        if dark {
            set_dark(&h, true).await;
        } else if let Err(e) = apply(&h, None, None).await {
            tracing::warn!("LEDs: {e}");
        }
    });
}
