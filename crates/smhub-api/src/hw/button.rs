//! The hub's buttons, counted from the left: BTN1 (full-size SMHUB) and BTN2 (every model; held
//! 10 s it resets the hub to factory settings, which `smhub-reset-daemon` does and nothing here
//! changes). The third, RST, restarts the chip and no software sees it. Both are read from the
//! kernel's key events on every hub, with or without ESPHome; gestures (short, double, long) are
//! worked out here, and each can carry out the action the user chose.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use smhub_model::time::{now_unix, rfc3339};
use smhub_sys::periph::{KEY_BTN1, KEY_BTN2};

use super::{Action, ButtonSettings, Inner, Owner, buzzer, ir, leds, light, note};
use crate::state::{AppState, lock};

/// A press held this long is a long press.
const LONG: Duration = Duration::from_millis(1000);
/// BTN2's long press ends here: a longer hold may be on its way to the factory reset (10 s).
const LONG_MAX_BTN2: Duration = Duration::from_millis(3000);
/// A second press within this gap makes a double press.
const DOUBLE_GAP: Duration = Duration::from_millis(400);
/// The feedback click: one 32nd note at 200 bpm, about 38 ms, high and short.
const CLICK: &str = "click:d=32,o=6,b=200:c";

/// Which button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Btn1,
    Btn2,
}

impl Which {
    fn of(code: u16) -> Option<Self> {
        match code {
            KEY_BTN1 => Some(Self::Btn1),
            KEY_BTN2 => Some(Self::Btn2),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Btn1 => "btn1",
            Self::Btn2 => "btn2",
        }
    }

    fn state(self, st: &mut Inner) -> &mut Button {
        match self {
            Self::Btn1 => &mut st.button,
            Self::Btn2 => &mut st.button2,
        }
    }

    fn settings(self, st: &Inner) -> &ButtonSettings {
        match self {
            Self::Btn1 => &st.settings.button,
            Self::Btn2 => &st.settings.button2,
        }
    }

    /// The gesture for a press held this long, `None` while it may still become a double press
    /// (`Short`), or no gesture at all.
    fn held(self, held: Duration) -> Held {
        if held < LONG {
            Held::Short
        } else if self == Self::Btn2 && held > LONG_MAX_BTN2 {
            Held::Ignored
        } else {
            Held::Long
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    Short,
    Long,
    /// BTN2 held past its long press, towards the factory reset: nothing happens here.
    Ignored,
}

/// The last gesture: which, when, and how long the button was held.
#[derive(Debug, Clone)]
pub struct Last {
    pub gesture: &'static str,
    pub at: i64,
    pub held: Duration,
}

#[derive(Debug, Clone, Default)]
pub struct Button {
    pub owner: Owner,
    pub pressed: bool,
    pub last: Option<Last>,
    down_at: Option<Instant>,
    /// A short press waiting to see whether a second one follows.
    pending: Option<(u64, Instant)>,
    seq: u64,
}

pub(crate) fn json(b: &Button) -> Value {
    json!({
        "available": b.owner == Owner::Linux,
        "owner": b.owner,
        "pressed": b.pressed,
        "last": b.last.as_ref().map(|l| json!({
            "gesture": l.gesture,
            "at": rfc3339(l.at),
            "held_ms": u64::try_from(l.held.as_millis()).unwrap_or(u64::MAX),
        })),
    })
}

/// A press (`true`) or release of a button.
pub fn edge(hub: &AppState, which: Which, pressed: bool) {
    let now = Instant::now();
    let gesture = {
        let mut st = lock(&hub.hw.st);
        let b = which.state(&mut st);
        if pressed == b.pressed {
            return;
        }
        b.pressed = pressed;
        if pressed {
            b.down_at = Some(now);
            None
        } else {
            let held = b.down_at.take().map_or(Duration::ZERO, |d| now.duration_since(d));
            match which.held(held) {
                Held::Long => {
                    b.pending = None;
                    Some(("long", held))
                }
                Held::Ignored => {
                    b.pending = None;
                    None
                }
                Held::Short if b.pending.take().is_some_and(|(_, at)| now.duration_since(at) <= DOUBLE_GAP + held) => Some(("double", held)),
                Held::Short => {
                    b.seq += 1;
                    b.pending = Some((b.seq, now));
                    let seq = b.seq;
                    let h = Arc::clone(hub);
                    tokio::spawn(async move {
                        tokio::time::sleep(DOUBLE_GAP).await;
                        let fire = {
                            let mut st = lock(&h.hw.st);
                            let b = which.state(&mut st);
                            if b.pending.is_some_and(|(s, _)| s == seq) && !b.pressed {
                                b.pending = None;
                                true
                            } else {
                                false
                            }
                        };
                        if fire {
                            gesture_done(&h, which, "short", held);
                        }
                    });
                    None
                }
            }
        }
    };
    if let Some((g, held)) = gesture {
        gesture_done(hub, which, g, held);
    }
    hub.publish_later(&["hardware"]);
}

fn gesture_done(hub: &AppState, which: Which, g: &'static str, held: Duration) {
    let (action, feedback) = {
        let mut st = lock(&hub.hw.st);
        which.state(&mut st).last = Some(Last { gesture: g, at: now_unix(), held });
        let s = which.settings(&st);
        let a = match g {
            "short" => s.short.clone(),
            "double" => s.double.clone(),
            _ => s.long.clone(),
        };
        (a, s.feedback)
    };
    hub.publish_later(&["hardware"]);
    if action.action == "none" {
        return;
    }
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        let result = run(&h, which, g, &action, feedback).await;
        let name = which.id().to_ascii_uppercase();
        note(&h, "button", format!("{name} {g}: {}", action.action), "button", if result { "done" } else { "failed" });
    });
}

/// Carry out an action. Returns whether it worked.
async fn run(hub: &AppState, which: Which, gesture: &str, a: &Action, feedback: bool) -> bool {
    let ok = match a.action.as_str() {
        "none" => return true,
        "stop_sound" => {
            lock(&hub.hw.st).find_until = None;
            buzzer::stop(hub, true).await;
            true
        }
        "toggle_light" => {
            let on = lock(&hub.hw.st).light.look.on;
            light::set(hub, light::LookPatch { on: Some(!on), ..Default::default() }, "button").await.is_ok()
        }
        "toggle_leds" => {
            let on = lock(&hub.hw.st).leds.power.unwrap_or(true);
            leds::set(hub, Some(!on), Some(false), "button").await.is_ok()
        }
        "play_sound" => {
            let text = a.preset.as_deref().and_then(|p| buzzer::melody_text(&lock(&hub.hw.st), p));
            match (text, a.preset.as_deref()) {
                (Some(t), Some(p)) => buzzer::request(hub, p, &t, buzzer::Prio::Person, "button").await.is_ok(),
                _ => false,
            }
        }
        "send_ir" => match a.code.as_deref() {
            Some(id) => ir::send_saved(hub, id, "button").await.is_ok(),
            None => false,
        },
        "call_url" => match a.url.as_deref() {
            Some(url) => {
                let body = json!({ "source": "smhub", "button": which.id(), "gesture": gesture }).to_string();
                hub.sys.exec("curl", &["-fsS", "-m", "5", "-X", "POST", "-H", "Content-Type: application/json", "-d", &body, url], Duration::from_secs(8)).await.is_ok_and(|o| o.ok())
            }
            None => false,
        },
        _ => false,
    };
    if ok && feedback {
        let _ = buzzer::request(hub, "click", CLICK, buzzer::Prio::Click, "button").await;
    }
    ok
}

/// Watch the buttons' key events, once, for as long as the daemon runs. A blocked read: it costs
/// nothing while nobody presses.
pub fn refresh(hub: &AppState) {
    if lock(&hub.hw.button_task).as_ref().is_some_and(|t| !t.is_finished()) {
        return;
    }
    let Some((dev, _)) = hub.sys.keys_device() else { return };
    let mut rx = match hub.sys.keys_watch(&dev) {
        Ok(rx) => rx,
        Err(e) => {
            tracing::warn!("buttons: {e}");
            return;
        }
    };
    let h = Arc::clone(hub);
    let t = tokio::spawn(async move {
        while let Some((code, pressed)) = rx.recv().await {
            if let Some(w) = Which::of(code) {
                edge(&h, w, pressed);
            }
        }
    });
    *lock(&hub.hw.button_task) = Some(t);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btn2_keeps_its_long_hold_for_the_factory_reset() {
        let ms = Duration::from_millis;
        assert_eq!(Which::Btn1.held(ms(300)), Held::Short);
        assert_eq!(Which::Btn1.held(ms(1000)), Held::Long);
        assert_eq!(Which::Btn1.held(ms(9000)), Held::Long);
        assert_eq!(Which::Btn2.held(ms(1500)), Held::Long);
        assert_eq!(Which::Btn2.held(ms(3000)), Held::Long);
        assert_eq!(Which::Btn2.held(ms(3001)), Held::Ignored);
        assert_eq!(Which::Btn2.held(ms(10_000)), Held::Ignored);
        assert_eq!((Which::of(KEY_BTN1), Which::of(KEY_BTN2), Which::of(28)), (Some(Which::Btn1), Some(Which::Btn2), None));
    }

    #[test]
    fn the_click_is_short() {
        let m = smhub_model::rtttl::parse(CLICK);
        assert!(m.as_ref().is_ok_and(|m| m.duration_ms() <= 50), "{m:?}");
    }
}
