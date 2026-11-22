//! The hub's own hardware (docs/peripherals.md): one module per device behind one API. Each
//! device has an ESPHome backend (the second core owns it) and a Linux backend (the main
//! computer does). Every caller passes the same queues and rules; hub events can sound the
//! buzzer and show signals on the ring; night mode and the user button's gestures live here.

pub mod button;
pub mod buzzer;
pub mod esp;
pub mod ir;
pub mod leds;
pub mod light;
pub mod routes;
pub mod ups;

use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use smhub_model::time::{now_unix, rfc3339};
use smhub_model::{Error, Result};
use tokio::sync::{Notify, mpsc};

use crate::state::{AppState, lock};

/// Activity entries kept in memory for the page.
pub const ACTIVITY: usize = 100;

/// Who drives a device now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Owner {
    /// The ESPHome firmware on the second core.
    Esphome,
    /// The hub's own daemon on the main computer.
    Linux,
    /// One of the OS's C services runs and holds it; the hub leaves it alone.
    Other,
    /// Nothing can drive it now (missing hardware, or ESPHome not installed).
    #[default]
    None,
}

/// A window of the day, `HH:MM` to `HH:MM` in the hub's time zone; it may pass midnight.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Window {
    pub enabled: bool,
    pub from: String,
    pub to: String,
}

impl Window {
    fn new(from: &str, to: &str) -> Self {
        Self { enabled: false, from: from.into(), to: to.into() }
    }

    fn minutes(s: &str) -> Option<i64> {
        let (h, m) = s.split_once(':')?;
        let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
        ((0..24).contains(&h) && (0..60).contains(&m) && s.len() == 5).then_some(h * 60 + m)
    }

    fn valid(&self) -> bool {
        Self::minutes(&self.from).is_some() && Self::minutes(&self.to).is_some()
    }

    /// Whether a minute of the day is inside the window.
    pub fn contains(&self, minute: i64) -> bool {
        let (Some(a), Some(b)) = (Self::minutes(&self.from), Self::minutes(&self.to)) else { return false };
        if !self.enabled || a == b {
            return false;
        }
        if a < b { (a..b).contains(&minute) } else { minute >= a || minute < b }
    }

    /// Seconds until the window next starts or ends, from a second of the day.
    pub fn next_change(&self, second: i64) -> Option<i64> {
        let (a, b) = (Self::minutes(&self.from)? * 60, Self::minutes(&self.to)? * 60);
        let until = |t: i64| (t - second).rem_euclid(86_400).max(1);
        Some(until(a).min(until(b)))
    }
}

impl Default for Window {
    fn default() -> Self {
        Self::new("22:00", "07:00")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
#[derive(Default)]
pub struct EventSounds {
    pub update_done: bool,
    pub failed: bool,
    pub crashed: bool,
    pub backup_done: bool,
    pub started: bool,
    pub power: bool,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Melody {
    pub name: String,
    pub rtttl: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Sounds {
    pub mute: bool,
    pub quiet: Window,
    pub events: EventSounds,
    pub melodies: Vec<Melody>,
}

impl Default for Sounds {
    fn default() -> Self {
        Self { mute: false, quiet: Window::new("22:00", "07:00"), events: EventSounds::default(), melodies: Vec::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct LightSettings {
    pub signals: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LedSettings {
    pub all_off: bool,
    pub night: Window,
}

impl Default for LedSettings {
    fn default() -> Self {
        Self { all_off: false, night: Window::new("23:00", "07:00") }
    }
}

/// What a button gesture does.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Action {
    /// `none`, `stop_sound`, `toggle_light`, `toggle_leds`, `play_sound`, `send_ir`, `call_url`
    pub action: String,
    pub preset: Option<String>,
    pub code: Option<String>,
    pub url: Option<String>,
}

impl Default for Action {
    fn default() -> Self {
        Self { action: "none".into(), preset: None, code: None, url: None }
    }
}

pub const ACTIONS: [&str; 7] = ["none", "stop_sound", "toggle_light", "toggle_leds", "play_sound", "send_ir", "call_url"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct ButtonSettings {
    pub short: Action,
    pub double: Action,
    pub long: Action,
    /// A click sound and a flash when an action ran.
    pub feedback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct IrSettings {
    pub listen: bool,
}

/// Settings of the hub's hardware, kept in `smhubd.db` (`hub_settings`, key `hardware`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Settings {
    pub sounds: Sounds,
    pub light: LightSettings,
    pub leds: LedSettings,
    /// BTN1, the first button from the left (full-size SMHUB).
    pub button: ButtonSettings,
    /// BTN2: the second from the left, and the only one of Nano and Nano MR.
    pub button2: ButtonSettings,
    pub ir: IrSettings,
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if !self.sounds.quiet.valid() {
            return Err(Error::invalid("sounds.quiet", "use HH:MM"));
        }
        if !self.leds.night.valid() {
            return Err(Error::invalid("leds.night", "use HH:MM"));
        }
        if self.sounds.melodies.len() > 32 {
            return Err(Error::invalid("sounds.melodies", "at most 32 melodies"));
        }
        let mut names = BTreeSet::new();
        for m in &self.sounds.melodies {
            if m.name.trim().is_empty() || m.name.chars().count() > 40 || !names.insert(m.name.trim().to_lowercase()) {
                return Err(Error::invalid("sounds.melodies", "each melody needs its own name of 1 to 40 characters"));
            }
            smhub_model::rtttl::parse(&m.rtttl).map_err(|e| Error::invalid("sounds.melodies", e))?;
        }
        let b1 = [("button.short", &self.button.short), ("button.double", &self.button.double), ("button.long", &self.button.long)];
        let b2 = [("button2.short", &self.button2.short), ("button2.double", &self.button2.double), ("button2.long", &self.button2.long)];
        for (field, a) in b1.into_iter().chain(b2) {
            if !ACTIONS.contains(&a.action.as_str()) {
                return Err(Error::invalid(field, "unknown action"));
            }
            if a.action == "play_sound" && !a.preset.as_deref().is_some_and(|p| smhub_model::rtttl::preset(p).is_some() || self.sounds.melodies.iter().any(|m| m.name == p)) {
                return Err(Error::invalid(field, "choose a sound"));
            }
            if a.action == "send_ir" && a.code.as_deref().is_none_or(str::is_empty) {
                return Err(Error::invalid(field, "choose an infrared code"));
            }
            if a.action == "call_url" {
                let u = a.url.as_deref().unwrap_or("");
                if !(u.starts_with("http://") || u.starts_with("https://")) || u.len() > 512 || u.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    return Err(Error::invalid(field, "an http:// or https:// address"));
                }
            }
        }
        Ok(())
    }
}

/// Merge a partial JSON object into another (objects merge, everything else replaces).
pub fn merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(slot) if slot.is_object() && v.is_object() => merge(slot, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// One line of recent activity.
#[derive(Debug, Clone, Serialize)]
pub struct Activity {
    pub at: String,
    /// `sound`, `light`, `leds`, `ir_send`, `ir_received`, `button`, `find`, `signal`
    pub kind: &'static str,
    pub detail: String,
    pub by: String,
    pub result: String,
}

pub(crate) struct Inner {
    pub settings: Settings,
    pub esp: esp::EspInfo,
    pub buzzer: buzzer::Buzzer,
    pub light: light::Light,
    pub leds: leds::Leds,
    pub button: button::Button,
    pub button2: button::Button,
    pub ir: ir::Ir,
    pub find_until: Option<i64>,
    pub activity: VecDeque<Activity>,
    pub crashed: BTreeSet<String>,
    pub dark: bool,
}

/// The hardware part of the hub's shared state.
pub struct Hw {
    pub(crate) st: Mutex<Inner>,
    pub(crate) esp_tx: Mutex<Option<mpsc::Sender<smhub_sys::esphome::Command>>>,
    pub(crate) esp_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) buzzer_wake: Notify,
    pub(crate) light_tx: Mutex<Option<mpsc::Sender<light::Frame>>>,
    pub(crate) leds_child: Mutex<Option<tokio::process::Child>>,
    pub(crate) ir_tx: Mutex<Option<mpsc::Sender<ir::SendJob>>>,
    pub(crate) ir_listen: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) button_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) night_wake: Notify,
    /// The hub's UTC offset in seconds, and when it was read; `None` until the first read.
    pub(crate) offset: Mutex<Option<(i64, Instant)>>,
    /// The owner marked a UPS board as connected to J10.
    pub ups: std::sync::atomic::AtomicBool,
}

impl Default for Hw {
    fn default() -> Self {
        Self {
            st: Mutex::new(Inner {
                settings: Settings::default(),
                esp: esp::EspInfo::default(),
                buzzer: buzzer::Buzzer::default(),
                light: light::Light::default(),
                leds: leds::Leds::default(),
                button: button::Button::default(),
                button2: button::Button::default(),
                ir: ir::Ir::default(),
                find_until: None,
                activity: VecDeque::new(),
                crashed: BTreeSet::new(),
                dark: false,
            }),
            esp_tx: Mutex::new(None),
            esp_task: Mutex::new(None),
            buzzer_wake: Notify::new(),
            light_tx: Mutex::new(None),
            leds_child: Mutex::new(None),
            ir_tx: Mutex::new(None),
            ir_listen: Mutex::new(None),
            button_task: Mutex::new(None),
            night_wake: Notify::new(),
            offset: Mutex::new(None),
            ups: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

/// Record an activity line and tell the pages.
pub(crate) fn note(hub: &AppState, kind: &'static str, detail: impl Into<String>, by: &str, result: impl Into<String>) {
    {
        let mut st = lock(&hub.hw.st);
        st.activity.push_front(Activity { at: rfc3339(now_unix()), kind, detail: detail.into(), by: by.to_owned(), result: result.into() });
        st.activity.truncate(ACTIVITY);
    }
    hub.publish_later(&["hardware"]);
}

/// Owners from what the kernel offers now and what the second core runs.
async fn detect(hub: &AppState) {
    let sys = &hub.sys;
    let m = sys.model().clone();
    let firmware = sys.rtos_firmware();
    let svc = sys.services().await;
    let other = |name: &str| svc.get(name).is_some_and(|st| *st == smhub_sys::services::State::Started);
    let keys = sys.keys_device().map(|(_, k)| k).unwrap_or_default();
    let mut st = lock(&hub.hw.st);
    st.esp.installed = firmware;
    st.buzzer.owner = if !m.buzzer {
        Owner::None
    } else if sys.pwm_owned() {
        if other("smhub-buzzer-daemon") { Owner::Other } else { Owner::Linux }
    } else if firmware {
        Owner::Esphome
    } else {
        Owner::None
    };
    st.light.owner = if !m.ambient_light {
        Owner::None
    } else if sys.spi_owned() {
        if other("smhub-ambilight-daemon") { Owner::Other } else { Owner::Linux }
    } else if firmware {
        Owner::Esphome
    } else {
        Owner::None
    };
    st.leds.owner = if firmware { Owner::Esphome } else { Owner::Linux };
    // the buttons are the kernel's on every model, whoever else runs: the hub reads their key
    // events itself (ESPHome only passes the same presses on to Home Assistant)
    let key = |code: u16| if keys.contains(&code) { Owner::Linux } else { Owner::None };
    st.button.owner = key(smhub_sys::periph::KEY_BTN1);
    st.button2.owner = key(smhub_sys::periph::KEY_BTN2);
    st.ir.available = m.ir && sys.ir_present();
}

/// Load the settings and start whatever the hardware needs. Called once at start.
pub async fn start(hub: &AppState) {
    let saved = hub.store.run(|db| Ok((db.hub_setting("hardware")?, db.hub_setting("light_look")?, db.hub_setting("leds_look")?))).await;
    let (settings, light_look, leds_look) = saved.unwrap_or((None, None, None));
    {
        let mut st = lock(&hub.hw.st);
        st.settings = settings.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        if let Some(l) = light_look.and_then(|s| serde_json::from_str(&s).ok()) {
            st.light.look = l;
        }
        if let Some((p, b)) = leds_look.and_then(|s| serde_json::from_str::<(bool, bool)>(&s).ok()) {
            st.leds.power = Some(p);
            st.leds.blue = Some(b);
        }
    }
    detect(hub).await;
    esp::start(hub).await;
    buzzer::start(hub);
    light::start(hub);
    leds::start(hub);
    button::refresh(hub);
    ir::start(hub).await;
    start_night(hub);
    hub_started_sound(hub);
    ups::start(hub).await;
}

/// Owners may have changed (an app was installed or removed, ESPHome's key changed).
pub async fn refresh(hub: &AppState) {
    let before = {
        let st = lock(&hub.hw.st);
        (st.esp.installed, st.light.owner, st.leds.owner, st.buzzer.owner)
    };
    detect(hub).await;
    let after = {
        let st = lock(&hub.hw.st);
        (st.esp.installed, st.light.owner, st.leds.owner, st.buzzer.owner)
    };
    if before != after {
        esp::start(hub).await;
        light::start(hub);
        leds::start(hub);
        button::refresh(hub);
    }
    hub.publish_later(&["hardware"]);
}

/// The `hardware` resource.
pub fn state_json(hub: &AppState) -> Value {
    let st = lock(&hub.hw.st);
    let m = hub.sys.model();
    json!({
        "esphome": {
            "installed": st.esp.installed,
            "connected": st.esp.connected,
            "error": st.esp.error,
            "name": st.esp.name,
        },
        "buzzer": buzzer::json(&st, m.buzzer),
        "light": light::json(&st, m.ambient_light),
        "leds": leds::json(&st),
        "button": button::json(&st.button),
        "button2": button::json(&st.button2),
        "ir": { "available": st.ir.available },
        "find": st.find_until.is_some_and(|u| u > now_unix()),
        "dark": st.dark,
        "presets": smhub_model::rtttl::PRESETS.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        "activity": st.activity.iter().collect::<Vec<_>>(),
    })
}

pub fn settings_json(hub: &AppState) -> Value {
    serde_json::to_value(&lock(&hub.hw.st).settings).unwrap_or(Value::Null)
}

/// Apply a partial settings change, save it, and act on what changed.
pub async fn patch_settings(hub: &AppState, patch: &Value) -> Result<Value> {
    let current = settings_json(hub);
    let mut merged = current.clone();
    merge(&mut merged, patch);
    let next: Settings = serde_json::from_value(merged).map_err(|e| Error::bad(format!("settings: {e}")))?;
    next.validate()?;
    let text = serde_json::to_string(&next)?;
    hub.store.run(move |db| db.set_hub_setting("hardware", &text)).await?;
    let before = std::mem::replace(&mut lock(&hub.hw.st).settings, next.clone());
    if before.leds != next.leds {
        hub.hw.night_wake.notify_one();
    }
    if before.button != next.button {
        button::refresh(hub);
    }
    if before.ir != next.ir {
        ir::listen_refresh(hub);
    }
    hub.publish_later(&["hardware"]);
    Ok(serde_json::to_value(&next)?)
}

/// How long a read UTC offset is used before `date` is asked again.
const OFFSET_AGE: Duration = Duration::from_secs(600);

/// A read offset while it is younger than [`OFFSET_AGE`]. The age is measured on the monotonic
/// clock, so setting the wall clock (NTP after boot) does not keep an old value.
fn fresh_offset(last: Option<(i64, Instant)>) -> Option<i64> {
    last.filter(|(_, at)| at.elapsed() < OFFSET_AGE).map(|(off, _)| off)
}

/// The hub's UTC offset in seconds (`date +%z`), read at most every ten minutes. When `date`
/// fails, the last offset (or UTC) is used until the next read.
pub(crate) async fn utc_offset(hub: &AppState) -> i64 {
    let last = *lock(&hub.hw.offset);
    if let Some(off) = fresh_offset(last) {
        return off;
    }
    let off = hub.sys.utc_offset().await.or(last.map(|(off, _)| off)).unwrap_or(0);
    *lock(&hub.hw.offset) = Some((off, Instant::now()));
    off
}

/// The time zone changed: the offset is read again, and night mode plans with it.
pub(crate) fn zone_changed(hub: &AppState) {
    *lock(&hub.hw.offset) = None;
    hub.hw.night_wake.notify_one();
}

/// Seconds since local midnight.
pub(crate) async fn local_second(hub: &AppState) -> i64 {
    (now_unix() + utc_offset(hub).await).rem_euclid(86_400)
}

/// Night mode and "all LEDs off": the LEDs and the ring go dark inside the window and come back
/// afterwards. The task sleeps until the next change; settings changes wake it.
fn start_night(hub: &AppState) {
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        loop {
            let second = local_second(&h).await;
            let (dark, wait) = {
                let st = lock(&h.hw.st);
                let night = &st.settings.leds.night;
                let dark = st.settings.leds.all_off || night.contains(second / 60);
                let wait = if night.enabled { night.next_change(second).unwrap_or(3600) } else { 3600 };
                (dark, wait.min(3600))
            };
            let changed = {
                let mut st = lock(&h.hw.st);
                let c = st.dark != dark;
                st.dark = dark;
                c
            };
            if changed {
                leds::set_dark(&h, dark).await;
                light::set_dark(&h, dark).await;
                h.publish("hardware").await;
            }
            tokio::select! {
                () = tokio::time::sleep(Duration::from_secs(u64::try_from(wait).unwrap_or(3600) + 1)) => {}
                () = h.hw.night_wake.notified() => {}
            }
        }
    });
}

/// Whether hub event sounds are silenced now (mute, or quiet hours).
pub(crate) async fn events_quiet(hub: &AppState) -> bool {
    let second = local_second(hub).await;
    let st = lock(&hub.hw.st);
    st.settings.sounds.mute || st.settings.sounds.quiet.contains(second / 60)
}

/// A hub event: the sound for it (if switched on) and a ring signal (if signals are on).
pub(crate) fn event(hub: &AppState, what: &'static str) {
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        let (sound, preset) = {
            let st = lock(&h.hw.st);
            let e = &st.settings.sounds.events;
            match what {
                "update_done" => (e.update_done, "success"),
                "failed" => (e.failed, "error"),
                "crashed" => (e.crashed, "warning"),
                "backup_done" => (e.backup_done, "notification"),
                "started" => (e.started, "startup"),
                "power" => (e.power, "shutdown"),
                _ => (false, "beep"),
            }
        };
        if sound {
            if let Some(r) = smhub_model::rtttl::preset(preset) {
                let _ = buzzer::request(&h, preset, r, buzzer::Prio::Event, "hub").await;
            }
        }
    });
}

/// An operation started: the ring shows "working" while it runs.
pub fn op_started(hub: &AppState, kind: &str) {
    if matches!(kind, "os_update" | "radio_flash" | "backup_restore" | "factory_restore") || kind.starts_with("app") {
        light::signal(hub, light::SignalKind::Working);
    }
}

/// An operation ended: its sound and the ring's done/failed signal.
pub fn op_finished(hub: &AppState, kind: &str, ok: bool) {
    let tracked = matches!(kind, "os_update" | "radio_flash" | "backup_restore" | "factory_restore") || kind.starts_with("app");
    if !tracked {
        return;
    }
    light::signal(hub, if ok { light::SignalKind::Done } else { light::SignalKind::Failed });
    if !ok {
        event(hub, "failed");
    } else if kind == "os_update" || kind.starts_with("app") {
        event(hub, "update_done");
    }
}

/// Services changed: a service that newly crashed sounds the "crashed" event.
pub fn services_changed(hub: &AppState, crashed: BTreeSet<String>) {
    let new = {
        let mut st = lock(&hub.hw.st);
        let new = crashed.difference(&st.crashed).count() > 0;
        st.crashed = crashed;
        new
    };
    if new {
        event(hub, "crashed");
    }
}

/// The start sound, once per boot (not when only the daemon restarts).
fn hub_started_sound(hub: &AppState) {
    let marker = hub.sys.p("/run/smhubd-started");
    let uptime = std::fs::read_to_string(hub.sys.p("/proc/uptime")).ok().and_then(|s| s.split_whitespace().next().and_then(|u| u.parse::<f64>().ok())).unwrap_or(0.0);
    if marker.exists() || uptime > 900.0 {
        let _ = std::fs::write(&marker, "");
        return;
    }
    let _ = std::fs::write(&marker, "");
    event(hub, "started");
}

/// "Find my hub": a chime every 2 s for 30 s and an amber pulse on the ring, until stopped.
pub async fn find(hub: &AppState, on: bool, by: &str) -> Result<()> {
    if !on {
        lock(&hub.hw.st).find_until = None;
        buzzer::stop(hub, false).await;
        light::signal(hub, light::SignalKind::End);
        note(hub, "find", "stop", by, "stopped");
        return Ok(());
    }
    let can = {
        let st = lock(&hub.hw.st);
        st.buzzer.owner == Owner::Esphome || st.buzzer.owner == Owner::Linux || st.light.owner == Owner::Esphome || st.light.owner == Owner::Linux
    };
    if !can {
        return Err(Error::Unavailable("this hub has no buzzer or light to show itself with".into()));
    }
    let until = now_unix() + 30;
    lock(&hub.hw.st).find_until = Some(until);
    light::signal(hub, light::SignalKind::Attention);
    note(hub, "find", "start", by, "started");
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        let chime = smhub_model::rtttl::preset("chime").unwrap_or("Chime:d=8,o=5,b=120:e6");
        loop {
            let active = lock(&h.hw.st).find_until.is_some_and(|u| u > now_unix());
            if !active {
                break;
            }
            let _ = buzzer::request(&h, "chime", chime, buzzer::Prio::Alarm, "find").await;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let ended = {
            let mut st = lock(&h.hw.st);
            let e = st.find_until.is_some();
            st.find_until = None;
            e
        };
        if ended {
            light::signal(&h, light::SignalKind::End);
        }
        h.publish("hardware").await;
    });
    hub.publish_later(&["hardware"]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows() {
        let w = Window { enabled: true, from: "22:00".into(), to: "07:00".into() };
        assert!(w.contains(23 * 60));
        assert!(w.contains(60));
        assert!(!w.contains(12 * 60));
        assert_eq!(w.next_change(21 * 3600), Some(3600));
        let d = Window { enabled: true, from: "08:00".into(), to: "09:30".into() };
        assert!(d.contains(9 * 60));
        assert!(!d.contains(10 * 60));
        assert!(!Window { enabled: true, from: "25:00".into(), to: "07:00".into() }.valid());
    }

    /// Nothing read yet means a read (the old cache started at `i64::MIN` and overflowed); a read
    /// offset is kept for ten minutes.
    #[test]
    fn offset_cache() {
        assert_eq!(fresh_offset(None), None);
        let now = Instant::now();
        assert_eq!(fresh_offset(Some((7200, now))), Some(7200));
        if let Some(old) = now.checked_sub(OFFSET_AGE + Duration::from_secs(1)) {
            assert_eq!(fresh_offset(Some((7200, old))), None);
        }
        let hw = Hw::default();
        assert_eq!(fresh_offset(*lock(&hw.offset)), None);
    }

    #[test]
    fn settings_merge_and_check() {
        let mut base = serde_json::to_value(Settings::default()).unwrap_or(Value::Null);
        merge(&mut base, &json!({"sounds": {"mute": true, "quiet": {"enabled": true}}, "button": {"short": {"action": "toggle_light"}}}));
        let s: Settings = serde_json::from_value(base).unwrap_or_default();
        assert!(s.sounds.mute && s.sounds.quiet.enabled && s.sounds.quiet.from == "22:00");
        assert!(s.validate().is_ok());
        let mut bad = s.clone();
        bad.button.long = Action { action: "call_url".into(), url: Some("ftp://x".into()), ..Action::default() };
        assert!(bad.validate().is_err());
        bad.button.long = Action { action: "play_sound".into(), preset: Some("doorbell".into()), ..Action::default() };
        assert!(bad.validate().is_ok());
    }
}
