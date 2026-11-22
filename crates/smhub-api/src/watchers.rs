//! Background work. Always on: inotify watches on OpenRC's and opkg's state (no wake-ups
//! while nothing changes) and daily housekeeping. Only while somebody listens: the metrics
//! ticker and `nmcli monitor` (decision D17).

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::state::{AppState, lock};

/// How often live metrics are sent while somebody watches them.
const METRICS_EVERY: Duration = Duration::from_secs(3);

pub fn start(hub: &AppState) {
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        let dirs: Vec<PathBuf> = ["started", "starting", "stopping", "inactive", "failed", "daemons"]
            .iter()
            .map(|d| h.sys.p(&format!("/run/openrc/{d}")))
            .collect();
        let mut rx = smhub_sys::watch::watch(dirs, smhub_sys::watch::changes());
        while rx.recv().await.is_some() {
            tokio::time::sleep(Duration::from_millis(400)).await;
            while rx.try_recv().is_ok() {}
            h.sys.services_changed();
            let crashed = h.sys.services().await.into_iter().filter(|(_, s)| *s == smhub_sys::services::State::Crashed).map(|(n, _)| n).collect();
            crate::hw::services_changed(&h, crashed);
            h.publish("services").await;
            h.publish("apps").await;
        }
    });
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        let dirs = vec![h.sys.p("/var/lib/opkg"), h.sys.p(smhub_sys::packages::LISTS)];
        let mut rx = smhub_sys::watch::watch(dirs, smhub_sys::watch::changes());
        while rx.recv().await.is_some() {
            tokio::time::sleep(Duration::from_secs(2)).await;
            while rx.try_recv().is_ok() {}
            if h.operations().iter().any(|o| o.state == "running" && o.kind.starts_with("app")) {
                continue;
            }
            h.publish("apps").await;
            h.publish("updates").await;
        }
    });
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(600)).await;
        loop {
            let ch = crate::resources::channel(&h);
            h.sys.os_releases(ch, true).await;
            *lock(&h.updates_checked) = Some(smhub_model::time::now_unix());
            h.publish("updates").await;
            crate::auth::prune_sessions(&h).await;
            tokio::time::sleep(Duration::from_secs(86_400)).await;
        }
    });
    crate::transfer::clear_stale_uploads(hub);
    let h = Arc::clone(hub);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1800)).await;
            crate::transfer::expire_uploads(&h);
        }
    });
}

fn ensure(hub: &AppState, name: &'static str, wanted: bool, f: impl FnOnce(AppState) -> tokio::task::JoinHandle<()>) {
    let mut w = lock(&hub.watchers);
    match (wanted, w.contains_key(name)) {
        (true, false) => {
            w.insert(name, f(Arc::clone(hub)));
        }
        (false, true) => {
            if let Some(j) = w.remove(name) {
                j.abort();
            }
        }
        _ => {}
    }
}

/// Start or stop the on-demand watchers after subscriptions changed.
pub fn subscriptions_changed(hub: &AppState) {
    ensure(hub, "metrics", hub.has_subscribers("metrics"), |h| {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(METRICS_EVERY).await;
                h.publish("metrics").await;
            }
        })
    });
    let net = hub.has_subscribers("network") || hub.has_subscribers("wifi") || hub.has_subscribers("vpn");
    ensure(hub, "nmcli", net, |h| {
        tokio::spawn(async move {
            let mut c = h.sys.command("nmcli");
            c.arg("monitor").stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
            let Ok(mut child) = c.spawn() else { return };
            let Some(out) = child.stdout.take() else { return };
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(_)) = lines.next_line().await {
                // changes come in bursts: wait for the burst to end, then publish once
                let deadline = tokio::time::sleep(Duration::from_millis(800));
                tokio::pin!(deadline);
                loop {
                    tokio::select! {
                        () = &mut deadline => break,
                        l = lines.next_line() => if !matches!(l, Ok(Some(_))) { return },
                    }
                }
                *lock(&h.internet) = None;
                h.publish("network").await;
                h.publish("wifi").await;
                h.publish("vpn").await;
            }
        })
    });
}
