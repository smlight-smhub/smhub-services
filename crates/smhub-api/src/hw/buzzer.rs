//! The buzzer: one sound at a time from a queue with priorities, the same rules for every caller
//! (docs/peripherals.md, "One place for every caller"). ESPHome plays the RTTTL text itself; on
//! the Linux side the daemon plays the notes on the PWM output.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use smhub_model::time::{now_unix, rfc3339};
use smhub_model::{Error, Result};

use super::{Inner, Owner, esp, note};
use crate::state::{AppState, lock};

/// Sounds waiting.
pub const QUEUE: usize = 16;
/// The same sound is not repeated sooner.
const SAME_GAP: Duration = Duration::from_secs(1);
/// At most this much sound per minute.
const BUDGET_MS: u32 = 60_000;
/// A hub event sound waits at most this long for its turn.
const EVENT_WAIT: Duration = Duration::from_secs(10);
/// A single short rest: replaces whatever ESPHome plays, which stops it.
const SILENT: &str = "stop:d=32,o=5,b=900:p";
/// The Linux player's duty cycle (loudness).
const DUTY: u32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Prio {
    /// Feedback clicks.
    Click,
    /// Hub event sounds.
    Event,
    /// Sounds a person started (the page, Home Assistant, a button).
    Person,
    /// Find my hub.
    Alarm,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: u64,
    pub label: String,
    #[serde(skip)]
    pub rtttl: String,
    pub ms: u32,
    pub prio: Prio,
    pub by: String,
    #[serde(skip)]
    queued: Instant,
    pub started: Option<String>,
}

#[derive(Debug, Default)]
pub struct Buzzer {
    pub owner: Owner,
    pub playing: Option<Job>,
    pub queue: VecDeque<Job>,
    recent: VecDeque<(Instant, String, u32)>,
    next_id: u64,
    interrupt: bool,
    stop_now: bool,
}

/// What happened to a request.
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    /// `playing`, `queued` or `dropped`
    pub result: &'static str,
    pub position: Option<usize>,
    /// For `dropped`: `quiet`, `repeat`, `limit` or `full`.
    pub reason: Option<&'static str>,
    pub ms: u32,
}

pub(crate) fn json(st: &Inner, present: bool) -> Value {
    let b = &st.buzzer;
    json!({
        "available": present && matches!(b.owner, Owner::Esphome | Owner::Linux),
        "owner": b.owner,
        "playing": b.playing,
        "queue": b.queue,
        "can_stop": matches!(b.owner, Owner::Esphome | Owner::Linux),
    })
}

/// Ask for a sound. The melody is checked first; then the queue rules decide.
pub async fn request(hub: &AppState, label: &str, rtttl: &str, prio: Prio, by: &str) -> Result<Outcome> {
    let melody = smhub_model::rtttl::parse(rtttl).map_err(|e| Error::invalid("rtttl", e))?;
    let ms = melody.duration_ms();
    let owner = lock(&hub.hw.st).buzzer.owner;
    if !matches!(owner, Owner::Esphome | Owner::Linux) {
        return Err(Error::Unavailable("the buzzer is not available on this hub now".into()));
    }
    if prio <= Prio::Event && super::events_quiet(hub).await {
        return Ok(Outcome { result: "dropped", position: None, reason: Some("quiet"), ms });
    }
    let outcome = {
        let mut st = lock(&hub.hw.st);
        let b = &mut st.buzzer;
        let now = Instant::now();
        b.recent.retain(|(t, _, _)| now.duration_since(*t) < Duration::from_secs(60));
        let used: u32 = b.recent.iter().map(|(_, _, m)| *m).sum();
        if b.recent.iter().any(|(t, l, _)| l == label && now.duration_since(*t) < SAME_GAP) {
            Outcome { result: "dropped", position: None, reason: Some("repeat"), ms }
        } else if used.saturating_add(ms) > BUDGET_MS && prio < Prio::Alarm {
            Outcome { result: "dropped", position: None, reason: Some("limit"), ms }
        } else {
            b.next_id += 1;
            let job = Job { id: b.next_id, label: label.to_owned(), rtttl: rtttl.trim().to_owned(), ms, prio, by: by.to_owned(), queued: now, started: None };
            let busy = b.playing.as_ref().map(|p| p.prio);
            let out = match busy {
                None => {
                    b.queue.push_front(job);
                    Outcome { result: "playing", position: None, reason: None, ms }
                }
                Some(p) if p < prio => {
                    b.interrupt = true;
                    b.queue.push_front(job);
                    Outcome { result: "playing", position: None, reason: None, ms }
                }
                Some(_) if b.queue.len() >= QUEUE => Outcome { result: "dropped", position: None, reason: Some("full"), ms },
                Some(_) => {
                    let at = b.queue.iter().position(|j| j.prio < prio).unwrap_or(b.queue.len());
                    b.queue.insert(at, job);
                    Outcome { result: "queued", position: Some(at + 1), reason: None, ms }
                }
            };
            if out.result != "dropped" {
                b.recent.push_back((now, label.to_owned(), ms));
            }
            out
        }
    };
    hub.hw.buzzer_wake.notify_one();
    if prio > Prio::Click {
        note(hub, "sound", label, by, outcome.reason.unwrap_or(outcome.result));
    }
    Ok(outcome)
}

/// Stop the sound that plays; with `clear`, also what waits.
pub async fn stop(hub: &AppState, clear: bool) {
    {
        let mut st = lock(&hub.hw.st);
        if clear {
            st.buzzer.queue.clear();
        }
        if st.buzzer.playing.is_some() {
            st.buzzer.stop_now = true;
        }
    }
    hub.hw.buzzer_wake.notify_one();
    hub.publish_later(&["hardware"]);
}

/// Start playing on the owner. Linux returns the note player's task.
async fn begin(hub: &AppState, owner: Owner, rtttl: &str) -> Option<tokio::task::JoinHandle<()>> {
    match owner {
        Owner::Esphome => {
            let key = lock(&hub.hw.st).esp.map.rtttl;
            let sent = match key {
                Some(key) => esp::send(hub, smhub_sys::esphome::Command::Service { key, strings: vec![rtttl.to_owned()] }),
                None => Err(Error::Unavailable("no RTTTL service".into())),
            };
            if sent.is_err() {
                if let Err(e) = hub.sys.rtos_notify(&["buzzer", rtttl]).await {
                    tracing::warn!("buzzer: {e}");
                }
            }
            None
        }
        Owner::Linux => {
            let melody = smhub_model::rtttl::parse(rtttl).ok()?;
            let sys = Arc::clone(&hub.sys);
            if let Err(e) = sys.pwm_prepare() {
                tracing::warn!("buzzer: {e}");
                return None;
            }
            Some(tokio::spawn(async move {
                for n in melody.notes {
                    if let Err(e) = sys.pwm_tone(n.hz, DUTY) {
                        tracing::warn!("buzzer: {e}");
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(u64::from(n.ms))).await;
                }
                let _ = sys.pwm_tone(0, DUTY);
            }))
        }
        Owner::Other | Owner::None => None,
    }
}

async fn halt(hub: &AppState, owner: Owner, player: Option<tokio::task::JoinHandle<()>>) {
    if let Some(p) = player {
        p.abort();
    }
    match owner {
        Owner::Linux => {
            let _ = hub.sys.pwm_tone(0, DUTY);
        }
        Owner::Esphome => {
            let _ = begin(hub, owner, SILENT).await;
        }
        _ => {}
    }
}

/// The player: takes the next sound, plays it for its length, and reacts to interruptions.
pub fn start(hub: &AppState) {
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        loop {
            let job = {
                let mut st = lock(&h.hw.st);
                let b = &mut st.buzzer;
                let mut next = None;
                while let Some(j) = b.queue.pop_front() {
                    if j.prio <= Prio::Event && j.queued.elapsed() > EVENT_WAIT {
                        continue;
                    }
                    next = Some(j);
                    break;
                }
                if let Some(j) = next.as_mut() {
                    j.started = Some(rfc3339(now_unix()));
                    b.playing = Some(j.clone());
                    b.interrupt = false;
                    b.stop_now = false;
                }
                next.map(|j| (j, b.owner))
            };
            let Some((job, owner)) = job else {
                h.hw.buzzer_wake.notified().await;
                continue;
            };
            h.publish("hardware").await;
            let player = begin(&h, owner, &job.rtttl).await;
            let deadline = tokio::time::Instant::now() + Duration::from_millis(u64::from(job.ms) + 150);
            let mut cut = false;
            loop {
                tokio::select! {
                    () = tokio::time::sleep_until(deadline) => break,
                    () = h.hw.buzzer_wake.notified() => {
                        let (interrupt, stop_now) = {
                            let st = lock(&h.hw.st);
                            (st.buzzer.interrupt, st.buzzer.stop_now)
                        };
                        if interrupt || stop_now {
                            cut = true;
                            break;
                        }
                    }
                }
            }
            if cut {
                let stop_only = lock(&h.hw.st).buzzer.stop_now;
                // an interruption plays the next sound at once, which replaces this one on ESPHome
                if stop_only || owner == Owner::Linux {
                    halt(&h, owner, player).await;
                } else if let Some(p) = player {
                    p.abort();
                }
            }
            {
                let mut st = lock(&h.hw.st);
                st.buzzer.playing = None;
                st.buzzer.interrupt = false;
                st.buzzer.stop_now = false;
            }
            h.publish("hardware").await;
        }
    });
}

/// Play a preset or one of the user's melodies by name.
pub(crate) fn melody_text(st: &Inner, name: &str) -> Option<String> {
    smhub_model::rtttl::preset(name).map(str::to_owned).or_else(|| st.settings.sounds.melodies.iter().find(|m| m.name == name).map(|m| m.rtttl.clone()))
}
