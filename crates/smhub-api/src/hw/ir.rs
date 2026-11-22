//! Infrared (full-size SMHUB): learn a code from any remote, keep it under a name, send it, and
//! optionally listen for codes. Always the Linux side: the kernel's receiver and transmitter.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use smhub_model::time::{now_unix, rfc3339};
use smhub_model::{Error, Result};
use smhub_store::IrCodeRow;
use tokio::sync::{mpsc, oneshot};

use super::note;
use crate::state::{AppState, lock};

/// Saved codes.
pub const MAX_CODES: usize = 256;
/// Sends waiting for the transmitter.
pub const SEND_QUEUE: usize = 32;
/// At most five sends a second.
const SEND_GAP: Duration = Duration::from_millis(200);
/// How long learning waits for a code.
pub const LEARN_FOR: Duration = Duration::from_secs(30);

/// A code as received.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Received {
    pub at: String,
    pub protocol: Option<String>,
    pub scancode: Option<u64>,
    pub raw: Vec<u32>,
    pub carrier: u32,
    /// The saved code it matches.
    pub matched: Option<String>,
}

#[derive(Debug, Default)]
pub struct Ir {
    pub available: bool,
    pub learning: Option<i64>,
    pub last: Option<Received>,
    pub listening: bool,
    learn_task: Option<tokio::task::JoinHandle<()>>,
}

pub struct SendJob {
    protocol: Option<String>,
    scancode: Option<u64>,
    raw: Vec<u32>,
    carrier: u32,
    reply: oneshot::Sender<Result<()>>,
}

fn sc(v: Option<u64>) -> Option<String> {
    v.map(|s| format!("{s:#x}"))
}

fn code_json(c: &IrCodeRow) -> Value {
    json!({
        "id": c.id,
        "name": c.name,
        "device": c.device,
        "protocol": c.protocol,
        "scancode": sc(c.scancode.and_then(|s| u64::try_from(s).ok())),
        "carrier": c.carrier,
        "edges": c.raw.len(),
        "favourite": c.favourite,
    })
}

/// The `ir` resource.
pub async fn state_json(hub: &AppState) -> Result<Value> {
    let codes = hub.store.run(|db| db.ir_codes()).await?;
    let st = lock(&hub.hw.st);
    let ir = &st.ir;
    Ok(json!({
        "available": ir.available,
        "learning": ir.learning.map(|u| json!({ "until": rfc3339(u) })),
        "listening": ir.listening,
        "last": ir.last.as_ref().map(|l| json!({
            "at": l.at, "protocol": l.protocol, "scancode": sc(l.scancode), "edges": l.raw.len(), "carrier": l.carrier,
            "matched": l.matched.as_ref().and_then(|id| codes.iter().find(|c| &c.id == id)).map(|c| json!({ "id": c.id, "name": c.name, "device": c.device })),
        })),
        "codes": codes.iter().map(code_json).collect::<Vec<_>>(),
        "max_codes": MAX_CODES,
    }))
}

fn available(hub: &AppState) -> Result<()> {
    if lock(&hub.hw.st).ir.available { Ok(()) } else { Err(Error::Unavailable("this hub has no infrared".into())) }
}

/// The transmitter's queue, and the listener if it is on.
pub async fn start(hub: &AppState) {
    if !lock(&hub.hw.st).ir.available {
        return;
    }
    let (tx, mut rx) = mpsc::channel::<SendJob>(SEND_QUEUE);
    *lock(&hub.hw.ir_tx) = Some(tx);
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        while let Some(j) = rx.recv().await {
            let r = match (&j.protocol, j.scancode) {
                (Some(p), Some(s)) => h.sys.ir_send_scancode(p, s).await,
                _ => h.sys.ir_send_raw(&j.raw, j.carrier).await,
            };
            let _ = j.reply.send(r);
            tokio::time::sleep(SEND_GAP).await;
        }
    });
    listen_refresh(hub);
}

/// Start or stop listening, as the settings say.
pub fn listen_refresh(hub: &AppState) {
    if let Some(t) = lock(&hub.hw.ir_listen).take() {
        t.abort();
    }
    let wanted = {
        let mut st = lock(&hub.hw.st);
        let w = st.ir.available && st.settings.ir.listen;
        st.ir.listening = w;
        w
    };
    if !wanted {
        if lock(&hub.hw.st).ir.learning.is_none() {
            let _ = hub.sys.ir_decoders(false);
        }
        hub.publish_later(&["ir"]);
        return;
    }
    if let Err(e) = hub.sys.ir_decoders(true) {
        tracing::warn!("infrared: {e}");
    }
    let mut rx = match hub.sys.ir_decoded_stream() {
        Ok(rx) => rx,
        Err(e) => {
            tracing::warn!("infrared: {e}");
            return;
        }
    };
    let h = Arc::clone(hub);
    let t = tokio::spawn(async move {
        while let Some((protocol, scancode)) = rx.recv().await {
            let codes = h.store.run(|db| db.ir_codes()).await.unwrap_or_default();
            let matched = codes.iter().find(|c| c.protocol.as_deref() == Some(protocol.as_str()) && c.scancode == i64::try_from(scancode).ok());
            let name = matched.map(|c| format!("{} · {}", c.device, c.name));
            let r = Received { at: rfc3339(now_unix()), protocol: Some(protocol.clone()), scancode: Some(scancode), raw: Vec::new(), carrier: 38_000, matched: matched.map(|c| c.id.clone()) };
            lock(&h.hw.st).ir.last = Some(r);
            note(&h, "ir_received", name.unwrap_or_else(|| format!("{protocol} {scancode:#x}")), "remote", "received");
            h.publish("ir").await;
        }
    });
    *lock(&hub.hw.ir_listen) = Some(t);
    hub.publish_later(&["ir"]);
}

/// Start learning: the next code the receiver sees becomes `last`.
pub fn learn(hub: &AppState) -> Result<()> {
    available(hub)?;
    let until = now_unix() + i64::try_from(LEARN_FOR.as_secs()).unwrap_or(30);
    {
        let mut st = lock(&hub.hw.st);
        if st.ir.learning.is_some() {
            return Err(Error::Busy("the receiver is already learning".into()));
        }
        st.ir.learning = Some(until);
        st.ir.last = None;
    }
    let h = Arc::clone(hub);
    let t = tokio::spawn(async move {
        let got = h.sys.ir_receive(LEARN_FOR).await;
        let listening = {
            let mut st = lock(&h.hw.st);
            st.ir.learning = None;
            st.ir.learn_task = None;
            if let Ok(Some(r)) = &got {
                st.ir.last = Some(Received { at: rfc3339(now_unix()), protocol: r.protocol.clone(), scancode: r.scancode, raw: r.raw.clone(), carrier: 38_000, matched: None });
            }
            st.ir.listening
        };
        if !listening {
            let _ = h.sys.ir_decoders(false);
        }
        if let Err(e) = got {
            tracing::warn!("infrared learning: {e}");
        }
        h.publish("ir").await;
    });
    lock(&hub.hw.st).ir.learn_task = Some(t);
    hub.publish_later(&["ir"]);
    Ok(())
}

pub fn cancel_learn(hub: &AppState) {
    let (t, listening) = {
        let mut st = lock(&hub.hw.st);
        st.ir.learning = None;
        (st.ir.learn_task.take(), st.ir.listening)
    };
    if let Some(t) = t {
        t.abort();
        // the aborted task does not get to switch the decoders back itself
        if !listening {
            let _ = hub.sys.ir_decoders(false);
        }
    }
    hub.publish_later(&["ir"]);
}

/// A new code: from the last one received, or given in full.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewCode {
    pub name: String,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub from_last: bool,
    pub protocol: Option<String>,
    pub scancode: Option<Value>,
    pub raw: Option<Vec<u32>>,
    pub carrier: Option<u32>,
    #[serde(default)]
    pub favourite: bool,
}

fn scancode_of(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => {
            let s = s.trim();
            match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                Some(h) => u64::from_str_radix(h, 16).ok(),
                None => s.parse().ok(),
            }
        }
        _ => None,
    }
}

fn check_names(name: &str, device: &str) -> Result<()> {
    if name.trim().is_empty() || name.chars().count() > 40 {
        return Err(Error::invalid("name", "1 to 40 characters"));
    }
    if device.chars().count() > 40 {
        return Err(Error::invalid("device", "at most 40 characters"));
    }
    Ok(())
}

fn check_protocol(p: &str) -> Result<()> {
    if p.is_empty() || p.len() > 16 || !p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-') {
        return Err(Error::invalid("protocol", "not a protocol name"));
    }
    Ok(())
}

fn row(b: NewCode, last: Option<Received>) -> Result<IrCodeRow> {
    check_names(&b.name, &b.device)?;
    let (protocol, scancode, raw, carrier) = if b.from_last {
        let l = last.ok_or_else(|| Error::Conflict("no code was received yet".into()))?;
        (l.protocol, l.scancode, l.raw, l.carrier)
    } else {
        let protocol = b.protocol.filter(|p| !p.is_empty());
        let scancode = match &b.scancode {
            Some(v) => Some(scancode_of(v).ok_or_else(|| Error::invalid("scancode", "a number, decimal or 0x hex"))?),
            None => None,
        };
        (protocol, scancode, b.raw.unwrap_or_default(), b.carrier.unwrap_or(38_000))
    };
    if let Some(p) = &protocol {
        check_protocol(p)?;
    }
    let decoded = protocol.is_some() && scancode.is_some();
    // a decoded code may come without timings; an undecoded one needs them
    if !decoded || !raw.is_empty() {
        smhub_sys::periph::check_raw(&raw)?;
    }
    if !(20_000..=60_000).contains(&carrier) {
        return Err(Error::invalid("carrier", "20000 to 60000 Hz"));
    }
    Ok(IrCodeRow {
        id: smhub_auth::new_id()?,
        name: b.name.trim().to_owned(),
        device: b.device.trim().to_owned(),
        protocol: if decoded { protocol } else { None },
        scancode: if decoded { scancode.and_then(|s| i64::try_from(s).ok()) } else { None },
        carrier: i64::from(carrier),
        raw,
        favourite: b.favourite,
        created: now_unix(),
    })
}

pub async fn save(hub: &AppState, b: NewCode) -> Result<Value> {
    available(hub)?;
    let last = lock(&hub.hw.st).ir.last.clone();
    let r = row(b, last)?;
    let r2 = r.clone();
    hub.store
        .run(move |db| {
            if db.ir_codes()?.len() >= MAX_CODES {
                return Err(Error::Conflict(format!("at most {MAX_CODES} codes")));
            }
            db.save_ir_code(&r2)
        })
        .await?;
    hub.publish_later(&["ir"]);
    Ok(code_json(&r))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodePatch {
    pub name: Option<String>,
    pub device: Option<String>,
    pub favourite: Option<bool>,
}

pub async fn update(hub: &AppState, id: String, p: CodePatch) -> Result<Value> {
    let r = hub
        .store
        .run(move |db| {
            let mut c = db.ir_codes()?.into_iter().find(|c| c.id == id).ok_or_else(|| Error::not_found("code"))?;
            if let Some(n) = p.name {
                c.name = n.trim().to_owned();
            }
            if let Some(d) = p.device {
                c.device = d.trim().to_owned();
            }
            if let Some(f) = p.favourite {
                c.favourite = f;
            }
            check_names(&c.name, &c.device)?;
            db.save_ir_code(&c)?;
            Ok(c)
        })
        .await?;
    hub.publish_later(&["ir"]);
    Ok(code_json(&r))
}

pub async fn delete(hub: &AppState, id: String) -> Result<()> {
    if !hub.store.run(move |db| db.delete_ir_code(&id)).await? {
        return Err(Error::not_found("code"));
    }
    hub.publish_later(&["ir"]);
    Ok(())
}

async fn transmit(hub: &AppState, protocol: Option<String>, scancode: Option<u64>, raw: Vec<u32>, carrier: u32) -> Result<()> {
    available(hub)?;
    let tx = lock(&hub.hw.ir_tx).clone().ok_or_else(|| Error::Unavailable("the transmitter is not ready".into()))?;
    let (rtx, rrx) = oneshot::channel();
    tx.try_send(SendJob { protocol, scancode, raw, carrier, reply: rtx }).map_err(|_| Error::Busy("too many codes waiting to be sent".into()))?;
    tokio::time::timeout(Duration::from_secs(15), rrx).await.map_err(|_| Error::Unavailable("the transmitter did not answer".into()))?.map_err(|_| Error::internal("the transmitter stopped"))?
}

pub async fn send_saved(hub: &AppState, id: &str, by: &str) -> Result<()> {
    let id2 = id.to_owned();
    let c = hub.store.run(move |db| Ok(db.ir_codes()?.into_iter().find(|c| c.id == id2))).await?.ok_or_else(|| Error::not_found("code"))?;
    let r = transmit(hub, c.protocol.clone(), c.scancode.and_then(|s| u64::try_from(s).ok()), c.raw.clone(), u32::try_from(c.carrier).unwrap_or(38_000)).await;
    note(hub, "ir_send", format!("{} · {}", c.device, c.name), by, if r.is_ok() { "sent" } else { "failed" });
    r
}

/// A code given in the request, not saved.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub protocol: Option<String>,
    pub scancode: Option<Value>,
    pub raw: Option<Vec<u32>>,
    pub carrier: Option<u32>,
}

pub async fn send_spec(hub: &AppState, s: Spec, by: &str) -> Result<()> {
    let carrier = s.carrier.unwrap_or(38_000);
    if !(20_000..=60_000).contains(&carrier) {
        return Err(Error::invalid("carrier", "20000 to 60000 Hz"));
    }
    
    match (s.protocol, s.scancode) {
        (Some(p), Some(v)) => {
            check_protocol(&p)?;
            let code = scancode_of(&v).ok_or_else(|| Error::invalid("scancode", "a number, decimal or 0x hex"))?;
            let label = format!("{p} {code:#x}");
            let r = transmit(hub, Some(p), Some(code), Vec::new(), carrier).await;
            note(hub, "ir_send", label, by, if r.is_ok() { "sent" } else { "failed" });
            r
        }
        _ => {
            let raw = s.raw.ok_or_else(|| Error::bad("give protocol and scancode, or raw"))?;
            smhub_sys::periph::check_raw(&raw)?;
            let label = format!("raw, {} timings", raw.len());
            let r = transmit(hub, None, None, raw, carrier).await;
            note(hub, "ir_send", label, by, if r.is_ok() { "sent" } else { "failed" });
            r
        }
    }
}

pub async fn export(hub: &AppState) -> Result<Value> {
    let codes = hub.store.run(|db| db.ir_codes()).await?;
    Ok(json!({ "format": "smhub-ir-1", "codes": codes }))
}

#[derive(Debug, Deserialize)]
pub struct Import {
    pub codes: Vec<ImportCode>,
}

#[derive(Debug, Deserialize)]
pub struct ImportCode {
    pub name: String,
    #[serde(default)]
    pub device: String,
    pub protocol: Option<String>,
    pub scancode: Option<i64>,
    #[serde(default = "default_carrier")]
    pub carrier: i64,
    #[serde(default)]
    pub raw: Vec<u32>,
    #[serde(default)]
    pub favourite: bool,
}

fn default_carrier() -> i64 {
    38_000
}

pub async fn import(hub: &AppState, list: Import) -> Result<usize> {
    available(hub)?;
    let mut rows = Vec::new();
    for c in list.codes {
        let b = NewCode {
            name: c.name,
            device: c.device,
            from_last: false,
            protocol: c.protocol,
            scancode: c.scancode.map(|s| json!(s)),
            raw: Some(c.raw),
            carrier: Some(u32::try_from(c.carrier).unwrap_or(38_000)),
            favourite: c.favourite,
        };
        rows.push(row(b, None)?);
    }
    let n = rows.len();
    hub.store
        .run(move |db| {
            if db.ir_codes()?.len() + rows.len() > MAX_CODES {
                return Err(Error::Conflict(format!("at most {MAX_CODES} codes")));
            }
            for r in &rows {
                db.save_ir_code(r)?;
            }
            Ok(())
        })
        .await?;
    hub.publish_later(&["ir"]);
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(scancode_of(&json!("0x408")), Some(0x408));
        assert_eq!(scancode_of(&json!(1032)), Some(1032));
        let b = NewCode { name: "Power".into(), device: "TV".into(), from_last: false, protocol: Some("nec".into()), scancode: Some(json!("0x408")), raw: None, carrier: None, favourite: false };
        let r = row(b, None);
        assert!(r.is_ok_and(|r| r.scancode == Some(0x408) && r.raw.is_empty()));
        let bad = NewCode { name: "x".into(), device: String::new(), from_last: false, protocol: None, scancode: None, raw: Some(vec![1, 2]), carrier: None, favourite: false };
        assert!(row(bad, None).is_err());
        let from = NewCode { name: "x".into(), device: String::new(), from_last: true, protocol: None, scancode: None, raw: None, carrier: None, favourite: false };
        assert!(row(from, None).is_err());
    }
}
