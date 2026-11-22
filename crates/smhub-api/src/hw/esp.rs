//! The hub's link to ESPHome on the second core: one API connection while the firmware is
//! installed, the entities the hub uses found by name, and their states passed to the devices.
//! The buttons are not among them: the hub reads them from the kernel (`button.rs`).

use std::sync::Arc;

use smhub_model::{Error, Result};
use smhub_sys::esphome::{self, Command, Entity, Event, Kind, State};
use tokio::sync::mpsc;

use super::{leds, light};
use crate::state::{AppState, lock};

/// The entities of the firmware the hub uses.
#[derive(Debug, Clone, Default)]
pub struct Map {
    pub light: Option<u32>,
    pub power_led: Option<u32>,
    pub blue_led: Option<u32>,
    /// The user service that plays an RTTTL melody.
    pub rtttl: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct EspInfo {
    pub installed: bool,
    pub connected: bool,
    pub error: Option<String>,
    pub name: String,
    pub map: Map,
}

fn norm(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase()
}

/// Find the hub's devices among the entities, by object id or name.
pub fn map_of(list: &[Entity]) -> Map {
    let find = |kind: Kind, names: &[&str]| {
        list.iter().find(|e| e.kind == kind && names.iter().any(|n| norm(&e.object_id) == *n || norm(&e.name) == *n)).map(|e| e.key)
    };
    let light = find(Kind::Light, &["ambilight", "ambientlight"]).or_else(|| list.iter().find(|e| e.kind == Kind::Light).map(|e| e.key));
    let rtttl = list
        .iter()
        .filter(|e| e.kind == Kind::Service && e.args.len() == 1 && e.args.first().is_some_and(|(_, t)| *t == 3))
        .find(|e| norm(&e.name).contains("rtttl") || norm(&e.name).contains("play") || norm(&e.name).contains("buzzer"))
        .map(|e| e.key);
    Map {
        light,
        power_led: find(Kind::Switch, &["powerled", "ledpwr"]),
        blue_led: find(Kind::Switch, &["blueled", "ledcus", "customled"]),
        rtttl,
    }
}

async fn key(hub: &AppState) -> [u8; 32] {
    let page = hub.store.run(|db| db.page_settings("esphome")).await.unwrap_or_default();
    page.get("encryption_key")
        .and_then(|k| esphome::key_from_b64(k))
        .or_else(|| esphome::key_from_b64(smhub_sys::rtos::FACTORY_KEY_B64))
        .unwrap_or([0u8; 32])
}

/// Start (or restart) the link. Without the firmware nothing runs.
pub async fn start(hub: &AppState) {
    if let Some(t) = lock(&hub.hw.esp_task).take() {
        t.abort();
    }
    *lock(&hub.hw.esp_tx) = None;
    {
        let mut st = lock(&hub.hw.st);
        st.esp.connected = false;
        st.esp.map = Map::default();
        if !st.esp.installed {
            st.esp.error = None;
            return;
        }
    }
    let key = key(hub).await;
    let (ctx, crx) = mpsc::channel(esphome::COMMANDS);
    let (etx, mut erx) = mpsc::channel(esphome::EVENTS);
    *lock(&hub.hw.esp_tx) = Some(ctx);
    let h = Arc::clone(hub);
    let task = tokio::spawn(async move {
        let link = tokio::spawn(esphome::run(esphome::ADDR.to_owned(), key, crx, etx));
        while let Some(ev) = erx.recv().await {
            handle(&h, ev).await;
        }
        link.abort();
    });
    *lock(&hub.hw.esp_task) = Some(task);
}

async fn handle(hub: &AppState, ev: Event) {
    match ev {
        Event::Connected { name, server } => {
            let mut st = lock(&hub.hw.st);
            st.esp.connected = true;
            st.esp.error = None;
            st.esp.name = if name.is_empty() { server } else { name };
        }
        Event::Entities(list) => {
            let map = map_of(&list);
            let fw: Vec<String> = map
                .light
                .and_then(|k| list.iter().find(|e| e.key == k))
                .map(|e| e.effects.iter().filter(|x| !x.eq_ignore_ascii_case("none")).cloned().collect())
                .unwrap_or_default();
            let dark = {
                let mut st = lock(&hub.hw.st);
                st.esp.map = map;
                st.light.effects = fw.iter().map(|e| light::hub_effect(e)).collect();
                st.light.fw_effects = fw;
                st.dark
            };
            if dark {
                // night mode or "all off" began while ESPHome was away: apply it now, once the
                // states have arrived
                let h = Arc::clone(hub);
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    if lock(&h.hw.st).dark {
                        leds::set_dark(&h, true).await;
                        light::set_dark(&h, true).await;
                    }
                });
            }
        }
        Event::Disconnected(e) => {
            {
                let mut st = lock(&hub.hw.st);
                st.esp.connected = false;
                st.esp.error = Some(e);
                st.light.known = false;
                st.leds.known = false;
            }
        }
        Event::State(s) => {
            let map = lock(&hub.hw.st).esp.map.clone();
            match s {
                State::Light { key, on, brightness, rgb, effect } if Some(key) == map.light => light::esp_state(hub, on, brightness, rgb, &effect),
                State::Switch { key, on } if Some(key) == map.power_led => leds::esp_state(hub, true, on),
                State::Switch { key, on } if Some(key) == map.blue_led => leds::esp_state(hub, false, on),
                _ => return,
            }
        }
    }
    hub.publish_later(&["hardware"]);
}

/// Send a command to ESPHome. Refused while it is not connected or its queue is full.
pub fn send(hub: &AppState, cmd: Command) -> Result<()> {
    let tx = lock(&hub.hw.esp_tx).clone();
    let connected = lock(&hub.hw.st).esp.connected;
    match tx {
        Some(tx) if connected => tx.try_send(cmd).map_err(|_| Error::Busy("too many commands for ESPHome; try again".into())),
        _ => Err(Error::Unavailable("ESPHome is not answering".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(kind: Kind, key: u32, object_id: &str, name: &str) -> Entity {
        Entity { kind, key, object_id: object_id.into(), name: name.into(), effects: vec![], args: vec![] }
    }

    #[test]
    fn finds_devices() {
        let mut svc = e(Kind::Service, 9, "", "play_rtttl");
        svc.args = vec![("song".into(), 3)];
        let list = vec![
            e(Kind::Light, 1, "ambilight", "AmbiLight"),
            e(Kind::Switch, 2, "power_led", "Power LED"),
            e(Kind::Switch, 3, "blue_led", "Blue LED"),
            e(Kind::BinarySensor, 4, "user_button", "User Button"),
            e(Kind::BinarySensor, 5, "factory_reset_button", "Factory Reset Button"),
            svc,
        ];
        let m = map_of(&list);
        assert_eq!((m.light, m.power_led, m.blue_led, m.rtttl), (Some(1), Some(2), Some(3), Some(9)));
        // API 1.14 servers send no object id: the names alone find them (as on the hub)
        let bare: Vec<Entity> = list.into_iter().map(|e| Entity { object_id: String::new(), ..e }).collect();
        let m = map_of(&bare);
        assert_eq!((m.light, m.power_led, m.blue_led, m.rtttl), (Some(1), Some(2), Some(3), Some(9)));
    }
}
