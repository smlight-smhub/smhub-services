//! The ambient ring: the user's look underneath, short signals on top (docs/peripherals.md).
//! ESPHome draws its own effects and restores a flash by itself; on the Linux side the daemon
//! draws the same effects, LED by LED, and also still pictures.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use smhub_model::{Error, Result};
use smhub_sys::esphome::{Command, LightCommand};
use smhub_sys::periph::RING_LEDS;
use tokio::sync::mpsc;

use super::{Inner, Owner, esp, note};
use crate::state::{AppState, lock};

/// Effects the Linux renderer draws: the names of the ESPHome firmware's, so the page is the same.
pub const LINUX_EFFECTS: [&str; 7] = ["Pulse", "Random", "Strobe", "Scan", "Color Wipe", "Fireworks", "Rainbow"];
/// Frames per second while something moves.
const FRAME: Duration = Duration::from_millis(40);

/// What the ring shows when no signal is on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Look {
    pub on: bool,
    pub color: [u8; 3],
    /// 0–100 %.
    pub brightness: u8,
    /// `None` or an effect name.
    pub effect: String,
    /// 1–100 (Linux only).
    pub speed: u8,
    /// Twelve colours (Linux only, effect `None`).
    pub pixels: Option<Vec<[u8; 3]>>,
}

impl Default for Look {
    fn default() -> Self {
        Self { on: false, color: [255, 180, 90], brightness: 50, effect: "None".into(), speed: 50, pixels: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SignalKind {
    Working,
    Done,
    Failed,
    Attention,
    End,
}

#[derive(Debug, Clone)]
pub struct Active {
    pub kind: SignalKind,
    /// ESPHome: the look to put back when the signal ends (`None` once somebody else changed it).
    saved: Option<Look>,
    /// ESPHome: what the signal asked for, to tell its own state from somebody else's change.
    shown: Look,
}

#[derive(Debug, Clone, Default)]
pub struct Light {
    pub owner: Owner,
    pub known: bool,
    pub look: Look,
    /// ESPHome's effects under the hub's names, and the firmware's own names in the same order.
    pub effects: Vec<String>,
    pub fw_effects: Vec<String>,
    pub signal: Option<Active>,
    dark_saved: Option<Look>,
    pub error: Option<String>,
}

/// What the Linux renderer is told.
#[derive(Debug, Clone)]
pub enum Frame {
    Look(Look),
    Overlay(Option<SignalKind>),
    Dark(bool),
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

pub(crate) fn json(st: &Inner, present: bool) -> Value {
    let l = &st.light;
    let linux = l.owner == Owner::Linux;
    json!({
        "available": present && matches!(l.owner, Owner::Esphome | Owner::Linux),
        "owner": l.owner,
        "known": l.known,
        "on": l.look.on,
        "color": hex(l.look.color),
        "brightness": l.look.brightness,
        "effect": l.look.effect,
        "effects": if linux { LINUX_EFFECTS.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>() } else { l.effects.clone() },
        "speed": if linux { Some(l.look.speed) } else { None },
        "pixels": if linux { l.look.pixels.as_ref().map(|p| p.iter().map(|c| hex(*c)).collect::<Vec<_>>()) } else { None },
        "per_led": linux,
        "signal": l.signal.as_ref().map(|s| s.kind),
        "error": l.error,
    })
}

/// The hub's name for an effect of the firmware, so the API names an effect the same whoever
/// draws it: ESPHome's firmware calls four of them "Rainbow Effect" and the like.
pub fn hub_effect(fw: &str) -> String {
    let base = fw.trim();
    let base = base.strip_suffix(" Effect").or_else(|| base.strip_suffix(" effect")).unwrap_or(base);
    let n = |s: &str| s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
    LINUX_EFFECTS.iter().find(|e| n(e) == n(base)).map_or_else(|| base.to_owned(), |e| (*e).to_owned())
}

/// The firmware's name for an effect the hub names.
fn fw_effect(name: &str, fw: &[String]) -> String {
    fw.iter().find(|e| hub_effect(e) == name).cloned().unwrap_or_else(|| name.to_owned())
}

fn command(key: u32, look: &Look, fw: &[String]) -> Command {
    if !look.on {
        return Command::Light(LightCommand { key, on: Some(false), ..Default::default() });
    }
    let c = look.color;
    Command::Light(LightCommand {
        key,
        on: Some(true),
        brightness: Some(f32::from(look.brightness.max(1)) / 100.0),
        rgb: Some([f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0]),
        effect: Some(fw_effect(&look.effect, fw)),
        // ESPHome refuses a transition next to an effect (it logs a warning and drops it), and
        // fades a steady colour by its own default when none is given
        transition_ms: None,
        flash_ms: None,
    })
}

fn close(a: &Look, b: &Look) -> bool {
    a.on == b.on && (!a.on || (a.effect.eq_ignore_ascii_case(&b.effect) && a.color.iter().zip(b.color).all(|(x, y)| x.abs_diff(y) <= 24)))
}

/// A state from ESPHome.
pub fn esp_state(hub: &AppState, on: bool, brightness: f32, rgb: [f32; 3], effect: &str) {
    let max = rgb.iter().copied().fold(0.0f32, f32::max).max(0.001);
    let to8 = |v: f32| ((v / max).clamp(0.0, 1.0) * 255.0).round() as u8;
    let look = Look {
        on,
        color: [to8(rgb[0]), to8(rgb[1]), to8(rgb[2])],
        brightness: (brightness.clamp(0.0, 1.0) * 100.0).round() as u8,
        effect: if effect.is_empty() { "None".into() } else { hub_effect(effect) },
        speed: 50,
        pixels: None,
    };
    esp_report(&mut lock(&hub.hw.st).light, look);
}

/// ESPHome's report is the ring's state only while ESPHome drives the ring. When the hub drives
/// it, ESPHome still lists its light (marked failed) and still reports it: the OS asks it for
/// blue at the end of every start.
fn esp_report(l: &mut Light, look: Look) {
    if l.owner != Owner::Esphome {
        return;
    }
    if let Some(s) = l.signal.as_mut() {
        if !close(&look, &s.shown) {
            // somebody else changed the ring during the signal: their choice stays
            s.saved = None;
        }
    }
    if look.on {
        l.dark_saved = None;
    }
    l.look = look;
    l.known = true;
}

/// A change of the look, from a person or a button.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookPatch {
    pub on: Option<bool>,
    pub color: Option<String>,
    pub brightness: Option<u8>,
    pub effect: Option<String>,
    pub speed: Option<u8>,
    /// Twelve `#rrggbb`; an empty list removes the picture.
    pub pixels: Option<Vec<String>>,
}

pub async fn set(hub: &AppState, p: LookPatch, by: &str) -> Result<()> {
    let (owner, mut look, effects, fw) = {
        let st = lock(&hub.hw.st);
        (st.light.owner, st.light.look.clone(), st.light.effects.clone(), st.light.fw_effects.clone())
    };
    let linux = owner == Owner::Linux;
    if !matches!(owner, Owner::Esphome | Owner::Linux) {
        return Err(Error::Unavailable("the light is not available on this hub now".into()));
    }
    if let Some(on) = p.on {
        look.on = on;
    }
    if let Some(c) = &p.color {
        look.color = parse_hex(c).ok_or_else(|| Error::invalid("color", "use #rrggbb"))?;
        look.on = p.on.unwrap_or(true);
    }
    if let Some(b) = p.brightness {
        if b > 100 {
            return Err(Error::invalid("brightness", "0 to 100"));
        }
        look.brightness = b;
        look.on = p.on.unwrap_or(true);
    }
    if let Some(e) = &p.effect {
        let known = if linux { LINUX_EFFECTS.iter().any(|x| x == e) } else { effects.iter().any(|x| x == e) };
        if e != "None" && !known {
            return Err(Error::invalid("effect", "not an effect of this light"));
        }
        look.effect = e.clone();
        look.on = p.on.unwrap_or(true);
        if e != "None" {
            look.pixels = None;
        }
    }
    if let Some(s) = p.speed {
        if !linux {
            return Err(Error::NotSupported("the ESPHome firmware has no effect speed".into()));
        }
        look.speed = s.clamp(1, 100);
    }
    if let Some(px) = &p.pixels {
        if !linux {
            return Err(Error::NotSupported("the ESPHome firmware cannot light single LEDs".into()));
        }
        if px.is_empty() {
            look.pixels = None;
        } else {
            if px.len() != RING_LEDS {
                return Err(Error::invalid("pixels", "twelve colours"));
            }
            let v: Option<Vec<[u8; 3]>> = px.iter().map(|c| parse_hex(c)).collect();
            look.pixels = Some(v.ok_or_else(|| Error::invalid("pixels", "use #rrggbb"))?);
            look.effect = "None".into();
            look.on = true;
        }
    }
    match owner {
        Owner::Esphome => {
            let key = lock(&hub.hw.st).esp.map.light.ok_or_else(|| Error::Unavailable("ESPHome has no light".into()))?;
            esp::send(hub, command(key, &look, &fw))?;
            // shown at once; ESPHome's report follows. The person's change ends a signal without
            // putting the old look back.
            let mut st = lock(&hub.hw.st);
            st.light.look = Look { speed: st.light.look.speed, pixels: None, ..look.clone() };
            st.light.signal = None;
        }
        Owner::Linux => {
            {
                let mut st = lock(&hub.hw.st);
                st.light.look = look.clone();
                st.light.signal = None;
            }
            render(hub, Frame::Overlay(None));
            render(hub, Frame::Look(look.clone()));
            if let Ok(text) = serde_json::to_string(&look) {
                let _ = hub.store.run(move |db| db.set_hub_setting("light_look", &text)).await;
            }
        }
        _ => {}
    }
    let what = if look.on { format!("{} {}% {}", hex(look.color), look.brightness, look.effect) } else { "off".into() };
    note(hub, "light", what, by, "done");
    Ok(())
}

fn render(hub: &AppState, f: Frame) {
    if let Some(tx) = lock(&hub.hw.light_tx).as_ref() {
        let _ = tx.try_send(f);
    }
}

fn signal_look(kind: SignalKind, base: &Look, effects: &[String]) -> Look {
    let (color, effect) = match kind {
        SignalKind::Working => ([80, 160, 255], "Pulse"),
        SignalKind::Attention => ([255, 140, 0], "Pulse"),
        SignalKind::Done => ([0, 255, 0], "None"),
        SignalKind::Failed => ([255, 0, 0], "None"),
        SignalKind::End => (base.color, "None"),
    };
    let effect = if effect == "None" || effects.iter().any(|e| e == effect) { effect } else { "None" };
    Look { on: true, color, brightness: base.brightness.max(50), effect: effect.into(), speed: 50, pixels: None }
}

/// Show a hub signal. Working and Attention last until the next signal or End; Done and Failed
/// are short. Hub-state signals (Working, Done, Failed) show only when the user switched them on.
pub fn signal(hub: &AppState, kind: SignalKind) {
    let (owner, enabled, active, look, (effects, fw), key, dark) = {
        let st = lock(&hub.hw.st);
        let l = &st.light;
        (l.owner, st.settings.light.signals, l.signal.clone(), l.look.clone(), (l.effects.clone(), l.fw_effects.clone()), st.esp.map.light, st.dark)
    };
    let hub_state = matches!(kind, SignalKind::Working | SignalKind::Done | SignalKind::Failed);
    if (hub_state && !enabled) || (dark && kind != SignalKind::End) {
        return;
    }
    match owner {
        Owner::Linux => {
            let overlay = match kind {
                SignalKind::End => None,
                k => Some(k),
            };
            lock(&hub.hw.st).light.signal = overlay.map(|k| Active { kind: k, saved: None, shown: Look::default() });
            render(hub, Frame::Overlay(overlay));
            if matches!(kind, SignalKind::Done | SignalKind::Failed) {
                let h = Arc::clone(hub);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(1600)).await;
                    let mut st = lock(&h.hw.st);
                    if st.light.signal.as_ref().is_some_and(|s| s.kind == kind) {
                        st.light.signal = None;
                    }
                });
            }
        }
        Owner::Esphome => {
            let Some(key) = key else { return };
            // end what shows: put the saved look back
            let restore = active.as_ref().and_then(|a| a.saved.clone());
            match kind {
                SignalKind::Working | SignalKind::Attention => {
                    if active.as_ref().is_some_and(|a| a.kind == kind) {
                        return;
                    }
                    let saved = match &active {
                        Some(a) => a.saved.clone(),
                        None => lock(&hub.hw.st).light.known.then(|| look.clone()),
                    };
                    let shown = signal_look(kind, &look, &effects);
                    if esp::send(hub, command(key, &shown, &fw)).is_ok() {
                        lock(&hub.hw.st).light.signal = Some(Active { kind, saved, shown });
                    }
                }
                SignalKind::Done | SignalKind::Failed | SignalKind::End => {
                    lock(&hub.hw.st).light.signal = None;
                    if let Some(r) = restore {
                        let _ = esp::send(hub, command(key, &r, &fw));
                    }
                    if kind != SignalKind::End {
                        let s = signal_look(kind, &look, &effects);
                        let c = s.color;
                        let _ = esp::send(
                            hub,
                            Command::Light(LightCommand {
                                key,
                                on: Some(true),
                                brightness: Some(0.8),
                                rgb: Some([f32::from(c[0]) / 255.0, f32::from(c[1]) / 255.0, f32::from(c[2]) / 255.0]),
                                flash_ms: Some(1500),
                                ..Default::default()
                            }),
                        );
                    }
                }
            }
        }
        _ => return,
    }
    hub.publish_later(&["hardware"]);
}

/// Night mode and "all off" for the ring.
pub async fn set_dark(hub: &AppState, dark: bool) {
    let (owner, look, known, key, fw) = {
        let st = lock(&hub.hw.st);
        (st.light.owner, st.light.look.clone(), st.light.known, st.esp.map.light, st.light.fw_effects.clone())
    };
    match owner {
        Owner::Linux => render(hub, Frame::Dark(dark)),
        Owner::Esphome => {
            let Some(key) = key else { return };
            if dark {
                if known && look.on {
                    lock(&hub.hw.st).light.dark_saved = Some(look);
                    let _ = esp::send(hub, command(key, &Look { on: false, ..Look::default() }, &fw));
                }
            } else {
                let saved = lock(&hub.hw.st).light.dark_saved.take();
                if let Some(s) = saved {
                    let _ = esp::send(hub, command(key, &s, &fw));
                }
            }
        }
        _ => {}
    }
}

/// Start the Linux renderer when Linux owns the ring; stop it otherwise.
pub fn start(hub: &AppState) {
    let (owner, look, dark) = {
        let st = lock(&hub.hw.st);
        (st.light.owner, st.light.look.clone(), st.dark)
    };
    *lock(&hub.hw.light_tx) = None;
    if owner != Owner::Linux {
        return;
    }
    {
        let mut st = lock(&hub.hw.st);
        st.light.known = true;
    }
    let (tx, rx) = mpsc::channel(16);
    *lock(&hub.hw.light_tx) = Some(tx);
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        let ring = match h.sys.ring_open() {
            Ok(r) => r,
            Err(e) => {
                lock(&h.hw.st).light.error = Some(e.message().to_owned());
                tracing::warn!("ring: {e}");
                return;
            }
        };
        renderer(ring, look, dark, rx).await;
    });
}

/// A tiny random source for the Random and Fireworks effects.
struct Rand(u64);

impl Rand {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn color(&mut self) -> [u8; 3] {
        hsv((self.next() % 360) as f32 / 360.0)
    }
}

/// A fully saturated colour from a hue (0–1).
fn hsv(h: f32) -> [u8; 3] {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    [(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8]
}

fn scale(c: [u8; 3], f: f32) -> [u8; 3] {
    let f = f.clamp(0.0, 1.0);
    [(f32::from(c[0]) * f) as u8, (f32::from(c[1]) * f) as u8, (f32::from(c[2]) * f) as u8]
}

/// One frame of a look at `t` ms after it started. `state` carries Random and Fireworks.
fn draw(look: &Look, t: u64, rand: &mut Rand, state: &mut [[u8; 3]; RING_LEDS]) -> [[u8; 3]; RING_LEDS] {
    let mut px = [[0u8; 3]; RING_LEDS];
    if !look.on {
        return px;
    }
    let bright = f32::from(look.brightness) / 100.0;
    let period = 400 + u64::from(100 - look.speed.clamp(1, 100)) * 36;
    let phase = (t % period) as f32 / period as f32;
    let c = look.color;
    match look.effect.as_str() {
        "Pulse" => {
            let level = 0.15 + 0.85 * (0.5 - 0.5 * (phase * std::f32::consts::TAU).cos());
            px = [scale(c, bright * level); RING_LEDS];
        }
        "Strobe" => {
            if (t / (period / 4).max(1)).is_multiple_of(2) {
                px = [scale(c, bright); RING_LEDS];
            }
        }
        "Rainbow" => {
            for (i, p) in px.iter_mut().enumerate() {
                *p = scale(hsv(i as f32 / RING_LEDS as f32 + phase), bright);
            }
        }
        "Scan" => {
            let steps = (RING_LEDS * 2 - 2) as u64;
            let s = (t / (period / steps).max(1)) % steps;
            let pos = if s < RING_LEDS as u64 { s } else { steps - s };
            for (i, p) in px.iter_mut().enumerate() {
                let d = (i as i64 - pos as i64).unsigned_abs();
                *p = scale(c, bright * if d == 0 { 1.0 } else if d == 1 { 0.25 } else { 0.0 });
            }
        }
        "Color Wipe" => {
            let steps = (RING_LEDS * 2) as u64;
            let s = (t / (period / RING_LEDS as u64).max(1)) % steps;
            for (i, p) in px.iter_mut().enumerate() {
                let i = i as u64;
                let lit = if s < RING_LEDS as u64 { i <= s } else { i > s - RING_LEDS as u64 };
                if lit {
                    *p = scale(c, bright);
                }
            }
        }
        "Random" => {
            if t % period < FRAME.as_millis() as u64 || state[0] == [0, 0, 0] {
                *state = [rand.color(); RING_LEDS];
            }
            px = state.map(|s| scale(s, bright));
        }
        "Fireworks" => {
            for s in state.iter_mut() {
                *s = scale(*s, 0.85);
            }
            if rand.next() % 100 < 25 {
                let i = (rand.next() % RING_LEDS as u64) as usize;
                if let Some(s) = state.get_mut(i) {
                    *s = rand.color();
                }
            }
            px = state.map(|s| scale(s, bright));
        }
        _ => match &look.pixels {
            Some(p) => {
                for (o, c) in px.iter_mut().zip(p) {
                    *o = scale(*c, bright);
                }
            }
            None => px = [scale(c, bright); RING_LEDS],
        },
    }
    px
}

fn overlay_look(kind: SignalKind, base: &Look) -> Look {
    signal_look(kind, base, &LINUX_EFFECTS.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
}

/// The Linux renderer: draws only while something moves, else waits for the next change.
async fn renderer(mut ring: smhub_sys::periph::Ring, mut look: Look, mut dark: bool, mut rx: mpsc::Receiver<Frame>) {
    let mut overlay: Option<(SignalKind, Instant)> = None;
    let mut started = Instant::now();
    let mut rand = Rand(u64::try_from(smhub_model::time::now_unix()).unwrap_or(1) | 1);
    let mut state = [[0u8; 3]; RING_LEDS];
    loop {
        let (px, moving) = if dark {
            ([[0u8; 3]; RING_LEDS], false)
        } else if let Some((kind, since)) = overlay {
            let t = since.elapsed().as_millis() as u64;
            let px = match kind {
                SignalKind::Done | SignalKind::Failed => {
                    if t >= 1500 {
                        overlay = None;
                        continue;
                    }
                    let on = (t / 250).is_multiple_of(2);
                    let s = overlay_look(kind, &look);
                    if on { [scale(s.color, f32::from(s.brightness) / 100.0); RING_LEDS] } else { [[0u8; 3]; RING_LEDS] }
                }
                k => draw(&overlay_look(k, &look), t, &mut rand, &mut state),
            };
            (px, true)
        } else {
            let t = started.elapsed().as_millis() as u64;
            (draw(&look, t, &mut rand, &mut state), look.on && look.effect != "None")
        };
        if let Err(e) = ring.show(&px) {
            tracing::warn!("ring: {e}");
        }
        let next = if moving {
            match tokio::time::timeout(FRAME, rx.recv()).await {
                Ok(m) => m,
                Err(_) => continue,
            }
        } else {
            rx.recv().await
        };
        let Some(f) = next else { return };
        match f {
            Frame::Look(l) => {
                look = l;
                started = Instant::now();
                state = [[0u8; 3]; RING_LEDS];
            }
            Frame::Overlay(o) => overlay = o.map(|k| (k, Instant::now())),
            Frame::Dark(d) => dark = d,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_names() {
        // the firmware's list, as ESPHome reports it on the hub
        let fw: Vec<String> = ["Pulse", "Strobe", "Random", "Rainbow Effect", "Color Wipe Effect", "Scan Effect", "Fireworks Effect"].map(String::from).to_vec();
        let hub: Vec<String> = fw.iter().map(|e| hub_effect(e)).collect();
        assert_eq!(hub, ["Pulse", "Strobe", "Random", "Rainbow", "Color Wipe", "Scan", "Fireworks"]);
        assert!(hub.iter().all(|e| LINUX_EFFECTS.contains(&e.as_str())));
        assert_eq!(fw_effect("Color Wipe", &fw), "Color Wipe Effect");
        assert_eq!(fw_effect("None", &fw), "None");
        assert_eq!(hub_effect("Twinkle"), "Twinkle");
        let c = command(1, &Look { on: true, effect: "Rainbow".into(), ..Look::default() }, &fw);
        assert!(matches!(c, Command::Light(LightCommand { effect: Some(ref e), .. }) if e == "Rainbow Effect"));
    }

    #[test]
    fn colours_and_frames() {
        assert_eq!(parse_hex("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex("ff8000"), None);
        let mut r = Rand(7);
        let mut s = [[0u8; 3]; RING_LEDS];
        let solid = Look { on: true, color: [200, 100, 0], brightness: 100, ..Look::default() };
        assert_eq!(draw(&solid, 0, &mut r, &mut s), [[200, 100, 0]; RING_LEDS]);
        let off = Look { on: false, ..solid.clone() };
        assert_eq!(draw(&off, 0, &mut r, &mut s), [[0, 0, 0]; RING_LEDS]);
        for e in LINUX_EFFECTS {
            let l = Look { effect: e.into(), ..solid.clone() };
            for t in [0, 40, 1000, 99_999] {
                let _ = draw(&l, t, &mut r, &mut s);
            }
        }
        let pic = Look { pixels: Some(vec![[10, 20, 30]; RING_LEDS]), ..solid };
        assert_eq!(draw(&pic, 0, &mut r, &mut s)[0], [10, 20, 30]);
    }

    #[test]
    fn esphome_reports_only_while_it_drives_the_ring() {
        // the OS's end-of-boot blue, reported by ESPHome's light that no longer drives the ring
        let blue = Look { on: true, color: [0, 0, 255], brightness: 50, ..Look::default() };
        let mut l = Light { owner: Owner::Linux, ..Light::default() };
        l.look.on = false;
        esp_report(&mut l, blue.clone());
        assert!(!l.look.on);
        l.owner = Owner::Esphome;
        esp_report(&mut l, blue.clone());
        assert_eq!(l.look, blue);
        assert!(l.known);
    }
}
