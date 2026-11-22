//! WebSocket endpoints: the event connection (decision D9), the terminal and log following.

use std::collections::BTreeSet;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use smhub_model::Error;
use tokio::sync::broadcast::error::RecvError;

use crate::error::ApiResult;
use crate::state::{AppState, BusEvent, Hub, limits, resource_name};

fn text(v: &Value) -> Message {
    Message::Text(v.to_string().into())
}

pub async fn events(State(hub): State<AppState>, ws: WebSocketUpgrade) -> ApiResult<Response> {
    if !Hub::inc(&hub.event_clients, limits::EVENT_CLIENTS) {
        return Err(Error::Busy("too many open windows".into()).into());
    }
    Ok(ws.max_message_size(64 * 1024).on_upgrade(move |socket| async move {
        events_loop(Arc::clone(&hub), socket).await;
        Hub::dec(&hub.event_clients);
    }))
}

async fn events_loop(hub: AppState, socket: WebSocket) {
    let (mut tx, mut rx) = socket.split();
    let mut bus = hub.bus.subscribe();
    let mut subs: BTreeSet<&'static str> = BTreeSet::new();
    loop {
        tokio::select! {
            msg = rx.next() => {
                let Some(Ok(msg)) = msg else { break };
                let Message::Text(t) = msg else {
                    if matches!(msg, Message::Close(_)) { break } else { continue }
                };
                let Ok(v) = serde_json::from_str::<Value>(t.as_str()) else { continue };
                let names: Vec<&'static str> = v
                    .get("resources")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.as_str().and_then(resource_name)).collect())
                    .unwrap_or_default();
                match v.get("op").and_then(Value::as_str) {
                    Some("sub") => {
                        let new: Vec<&'static str> = names.into_iter().filter(|n| subs.insert(n)).collect();
                        hub.subscribe_counts(&new, 1);
                        crate::watchers::subscriptions_changed(&hub);
                        // First states are worked out together and each is sent when ready: the
                        // slow ones (network, Wi-Fi: NetworkManager) no longer hold up the rest.
                        let mut first: futures_util::stream::FuturesUnordered<_> = new
                            .into_iter()
                            .map(|r| {
                                let hub = Arc::clone(&hub);
                                async move { (r, crate::resources::state(&hub, r).await) }
                            })
                            .collect();
                        while let Some((r, res)) = first.next().await {
                            match res {
                                Ok(data) => {
                                    if tx.send(text(&json!({"type": "state", "resource": r, "data": data}))).await.is_err() {
                                        break;
                                    }
                                }
                                Err(e) => tracing::warn!("state of {r}: {e}"),
                            }
                        }
                    }
                    Some("unsub") => {
                        let gone: Vec<&'static str> = names.into_iter().filter(|n| subs.remove(n)).collect();
                        hub.subscribe_counts(&gone, -1);
                        crate::watchers::subscriptions_changed(&hub);
                    }
                    Some("ping") => {
                        if tx.send(text(&json!({"type": "pong"}))).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            ev = bus.recv() => {
                let out = match ev {
                    Ok(BusEvent::State(r, data)) if subs.contains(r) => json!({"type": "state", "resource": r, "data": *data}),
                    Ok(BusEvent::Operation(op)) => json!({"type": "operation", "data": *op}),
                    Ok(_) => continue,
                    Err(RecvError::Lagged(_)) => json!({"type": "reload"}),
                    Err(RecvError::Closed) => break,
                };
                if tx.send(text(&out)).await.is_err() {
                    break;
                }
            }
        }
    }
    let all: Vec<&'static str> = subs.into_iter().collect();
    hub.subscribe_counts(&all, -1);
    crate::watchers::subscriptions_changed(&hub);
}

#[derive(Deserialize)]
pub struct Size {
    cols: Option<u16>,
    rows: Option<u16>,
}

pub async fn terminal(State(hub): State<AppState>, Query(q): Query<Size>, ws: WebSocketUpgrade) -> ApiResult<Response> {
    if !Hub::inc(&hub.terminals, limits::TERMINALS) {
        return Err(Error::Busy("two terminals are open already".into()).into());
    }
    let term = match hub.sys.open_terminal(q.cols.unwrap_or(80), q.rows.unwrap_or(24)) {
        Ok(t) => t,
        Err(e) => {
            Hub::dec(&hub.terminals);
            return Err(e.into());
        }
    };
    Ok(ws.on_upgrade(move |socket| async move {
        let (mut tx, mut rx) = socket.split();
        let term = Arc::new(term);
        let reader = {
            let term = Arc::clone(&term);
            async move {
                let mut buf = vec![0u8; 4096];
                loop {
                    match term.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if tx.send(Message::Binary(buf[..n].to_vec().into())).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                let _ = tx.close().await;
            }
        };
        let writer = {
            let term = Arc::clone(&term);
            let hub = Arc::clone(&hub);
            async move {
                while let Some(Ok(msg)) = rx.next().await {
                    match msg {
                        Message::Binary(b) => {
                            if term.write(&b).await.is_err() {
                                break;
                            }
                        }
                        Message::Text(t) => {
                            let s = t.as_str();
                            if let Ok(v) = serde_json::from_str::<Value>(s) {
                                if v.get("type").and_then(Value::as_str) == Some("resize") {
                                    let cols = v.get("cols").and_then(Value::as_u64).and_then(|c| u16::try_from(c).ok()).unwrap_or(80);
                                    let rows = v.get("rows").and_then(Value::as_u64).and_then(|r| u16::try_from(r).ok()).unwrap_or(24);
                                    term.resize(&hub.sys, cols, rows).await;
                                    continue;
                                }
                            }
                            if term.write(s.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                        Message::Close(_) => break,
                        _ => {}
                    }
                }
            }
        };
        tokio::select! {
            () = reader => {}
            () = writer => {}
        }
        if let Ok(t) = Arc::try_unwrap(term) {
            t.close().await;
        }
        Hub::dec(&hub.terminals);
    }))
}

#[derive(Deserialize)]
pub struct LogQuery {
    filter: Option<String>,
}

pub async fn logs(State(hub): State<AppState>, Path(name): Path<String>, Query(q): Query<LogQuery>, ws: WebSocketUpgrade) -> ApiResult<Response> {
    let path = hub.sys.log_path(&name)?;
    // a log of the hub or of an installed app is followed even before its first line
    if !path.exists() && !crate::routes::log_expected(&hub, &name) {
        return Err(Error::not_found("log").into());
    }
    if !Hub::inc(&hub.log_followers, limits::LOG_FOLLOWERS) {
        return Err(Error::Busy("too many logs are being followed".into()).into());
    }
    let initial = {
        let sys = hub.sys.clone();
        let n = name.clone();
        let f = q.filter.clone();
        tokio::task::spawn_blocking(move || sys.log_tail(&n, 200, f.as_deref())).await.ok().and_then(Result::ok).unwrap_or_default()
    };
    let follow = match hub.sys.log_follow(&name, q.filter) {
        Ok(f) => f,
        Err(e) => {
            Hub::dec(&hub.log_followers);
            return Err(e.into());
        }
    };
    Ok(ws.on_upgrade(move |socket| async move {
        let (mut tx, mut rx) = socket.split();
        let mut follow = follow;
        let mut ok = true;
        for l in initial {
            if tx.send(text(&json!({ "line": l }))).await.is_err() {
                ok = false;
                break;
            }
        }
        if ok {
            loop {
                tokio::select! {
                    line = follow.recv() => match line {
                        Some(l) => if tx.send(text(&json!({ "line": l }))).await.is_err() { break },
                        None => break,
                    },
                    msg = rx.next() => match msg {
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        _ => {}
                    },
                }
            }
        }
        Hub::dec(&hub.log_followers);
    }))
}
