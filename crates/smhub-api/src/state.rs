//! Shared state of the daemon: the OS layer, the store, sessions, the event bus, operations.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;
use smhub_model::time::{now_unix, rfc3339};
use smhub_model::{Error, Result};
use smhub_store::Store;
use smhub_sys::Sys;
use tokio::sync::{Semaphore, broadcast};

/// Limits from `docs/architecture.md` ("Limits").
pub mod limits {
    pub const EVENT_CLIENTS: usize = 16;
    pub const BUS: usize = 256;
    pub const LOG_FOLLOWERS: usize = 4;
    pub const TERMINALS: usize = 2;
    pub const OPERATIONS_KEPT: usize = 20;
    pub const BODY: usize = 256 * 1024;
    pub const PIECE: usize = 768 * 1024;
    pub const UPLOAD_MAX: u64 = 2 * 1024 * 1024 * 1024;
    pub const SESSION_DAYS: i64 = 30;
}

/// What travels on the event bus.
#[derive(Debug, Clone)]
pub enum BusEvent {
    /// A resource's new state, computed once for every subscriber.
    State(&'static str, Arc<Value>),
    /// An operation changed.
    Operation(Arc<Value>),
}

/// Resources a client can subscribe to on the event connection.
pub const RESOURCES: [&str; 13] =
    ["system", "metrics", "network", "wifi", "services", "apps", "updates", "lte", "radios", "operations", "vpn", "hardware", "ir"];

/// An API token as the sign-in layer keeps it (its hash is the map key).
#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub id: String,
    pub user_id: String,
    pub name: String,
    /// When its last use was written to the store.
    pub last_written: i64,
}

pub fn resource_name(s: &str) -> Option<&'static str> {
    RESOURCES.iter().copied().find(|r| *r == s)
}

/// Startup configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub version: String,
    /// Directory for upload pieces (on the user partition, not in RAM).
    pub upload_dir: std::path::PathBuf,
}

/// An operation, as the API shows it.
#[derive(Debug, Clone, Serialize)]
pub struct Operation {
    pub id: String,
    pub kind: &'static str,
    pub target: Option<String>,
    pub state: &'static str,
    pub progress: Option<f64>,
    pub step: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    pub reboot_needed: bool,
}

/// A session kept in memory; the store keeps only the token's hash.
#[derive(Debug, Clone)]
pub struct Session {
    pub user_id: String,
    pub expires: i64,
}

/// Everything handlers share.
pub struct Hub {
    pub sys: Arc<Sys>,
    pub store: Store,
    pub cfg: Config,
    pub bus: broadcast::Sender<BusEvent>,
    pub sessions: Mutex<BTreeMap<String, Session>>,
    pub operations: Mutex<Vec<Operation>>,
    /// Subscriber counts per resource; watchers run only while a resource has subscribers.
    pub subscribers: Mutex<BTreeMap<&'static str, usize>>,
    pub event_clients: AtomicUsize,
    pub log_followers: AtomicUsize,
    pub terminals: AtomicUsize,
    /// One password check at a time (Argon2 needs 19 MiB).
    pub password_checks: Semaphore,
    pub auth_failures: Mutex<Vec<i64>>,
    pub uploads: Mutex<BTreeMap<String, Upload>>,
    pub watchers: Mutex<BTreeMap<&'static str, tokio::task::JoinHandle<()>>>,
    /// Sign-in required (page `security`, key `enabled`).
    pub security_required: std::sync::atomic::AtomicBool,
    /// No one has finished or skipped the first-run setup on this hub, and it is not an upgrade.
    pub setup_needed: std::sync::atomic::AtomicBool,
    /// The account an open hub acts as.
    pub open_user: Mutex<Option<smhub_store::UserRow>>,
    /// When the hub's own background check for OS updates last ran.
    pub updates_checked: Mutex<Option<i64>>,
    /// Detected hardware features, refreshed at most every ten minutes.
    pub features: Mutex<Option<(i64, Value)>>,
    /// When Wi-Fi last scanned on request.
    pub wifi_scanned: Mutex<Option<i64>>,
    /// Internet reachability, reused for a minute.
    pub internet: Mutex<Option<(i64, Option<bool>)>>,
    /// The modem's last answers, reused for twenty seconds.
    pub modem: Mutex<Option<(i64, Value)>>,
    /// API tokens by hash.
    pub tokens: Mutex<BTreeMap<String, TokenInfo>>,
    /// The hub's own hardware.
    pub hw: crate::hw::Hw,
}

#[derive(Debug, Clone)]
pub struct Upload {
    pub path: std::path::PathBuf,
    pub size: u64,
    pub received: u64,
    pub name: Option<String>,
    pub created: i64,
}

pub type AppState = Arc<Hub>;

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Hub {
    pub fn new(sys: Arc<Sys>, store: Store, cfg: Config) -> AppState {
        let (bus, _) = broadcast::channel(limits::BUS);
        Arc::new(Self {
            sys,
            store,
            cfg,
            bus,
            sessions: Mutex::new(BTreeMap::new()),
            operations: Mutex::new(Vec::new()),
            subscribers: Mutex::new(BTreeMap::new()),
            event_clients: AtomicUsize::new(0),
            log_followers: AtomicUsize::new(0),
            terminals: AtomicUsize::new(0),
            password_checks: Semaphore::new(1),
            auth_failures: Mutex::new(Vec::new()),
            uploads: Mutex::new(BTreeMap::new()),
            watchers: Mutex::new(BTreeMap::new()),
            security_required: std::sync::atomic::AtomicBool::new(false),
            setup_needed: std::sync::atomic::AtomicBool::new(false),
            open_user: Mutex::new(None),
            updates_checked: Mutex::new(None),
            features: Mutex::new(None),
            wifi_scanned: Mutex::new(None),
            internet: Mutex::new(None),
            modem: Mutex::new(None),
            tokens: Mutex::new(BTreeMap::new()),
            hw: crate::hw::Hw::default(),
        })
    }

    /// Whether anybody listens to a resource.
    pub fn has_subscribers(&self, r: &'static str) -> bool {
        lock(&self.subscribers).get(r).copied().unwrap_or(0) > 0
    }

    /// Send a resource's state to its subscribers. Nothing is computed when nobody listens.
    pub async fn publish(self: &Arc<Self>, r: &'static str) {
        if !self.has_subscribers(r) {
            return;
        }
        match crate::resources::state(self, r).await {
            Ok(v) => {
                let _ = self.bus.send(BusEvent::State(r, Arc::new(v)));
            }
            Err(e) => tracing::warn!("state of {r}: {e}"),
        }
    }

    /// Publish several resources, in the background.
    pub fn publish_later(self: &Arc<Self>, rs: &[&'static str]) {
        let st = Arc::clone(self);
        let rs = rs.to_vec();
        tokio::spawn(async move {
            for r in rs {
                st.publish(r).await;
            }
        });
    }

    /// Start an operation. Fails with `busy` when one of the same kind and target runs, or when
    /// an OS update or restore runs (they exclude everything else).
    pub fn start_op(self: &Arc<Self>, kind: &'static str, target: Option<String>) -> Result<OpHandle> {
        let mut ops = lock(&self.operations);
        let exclusive = |k: &str| matches!(k, "os_update" | "backup_restore" | "factory_restore");
        if let Some(r) = ops.iter().find(|o| {
            o.state == "running"
                && (exclusive(o.kind) || exclusive(kind) || (o.kind == kind && o.target == target) || (kind.starts_with("app") && o.kind.starts_with("app")))
        }) {
            return Err(Error::Busy(format!("{} is still running", r.kind)));
        }
        let id = smhub_auth::new_id()?;
        let op = Operation {
            id: id.clone(),
            kind,
            target,
            state: "running",
            progress: None,
            step: None,
            started_at: rfc3339(now_unix()),
            finished_at: None,
            error: None,
            reboot_needed: false,
        };
        ops.push(op.clone());
        let excess = ops.len().saturating_sub(limits::OPERATIONS_KEPT);
        if excess > 0 {
            let mut removed = 0;
            ops.retain(|o| {
                if removed < excess && o.state != "running" {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        }
        drop(ops);
        self.op_changed(&op);
        crate::hw::op_started(self, kind);
        Ok(OpHandle { st: Arc::clone(self), id })
    }

    fn op_changed(self: &Arc<Self>, op: &Operation) {
        if let Ok(v) = serde_json::to_value(op) {
            let _ = self.bus.send(BusEvent::Operation(Arc::new(v)));
        }
        let st = Arc::clone(self);
        tokio::spawn(async move { st.publish("operations").await });
    }

    pub fn operations(&self) -> Vec<Operation> {
        lock(&self.operations).clone()
    }

    pub fn operation(&self, id: &str) -> Option<Operation> {
        lock(&self.operations).iter().find(|o| o.id == id).cloned()
    }

    pub fn running_op(&self, kind: &str) -> bool {
        lock(&self.operations).iter().any(|o| o.kind == kind && o.state == "running")
    }

    pub fn subscribe_counts(&self, rs: &[&'static str], delta: isize) {
        let mut m = lock(&self.subscribers);
        for r in rs {
            let e = m.entry(r).or_insert(0);
            *e = if delta >= 0 { *e + delta.unsigned_abs() } else { e.saturating_sub(delta.unsigned_abs()) };
        }
    }

    pub fn inc(c: &AtomicUsize, max: usize) -> bool {
        c.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| if n < max { Some(n + 1) } else { None }).is_ok()
    }

    pub fn dec(c: &AtomicUsize) {
        let _ = c.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| Some(n.saturating_sub(1)));
    }
}

/// Handle to a running operation.
pub struct OpHandle {
    st: AppState,
    id: String,
}

impl OpHandle {
    pub fn id(&self) -> &str {
        &self.id
    }

    fn update<F: FnOnce(&mut Operation)>(&self, f: F) {
        let snapshot = {
            let mut ops = lock(&self.st.operations);
            match ops.iter_mut().find(|o| o.id == self.id) {
                Some(o) => {
                    f(o);
                    o.clone()
                }
                None => return,
            }
        };
        self.st.op_changed(&snapshot);
    }

    pub fn progress(&self, percent: Option<f64>, step: Option<&str>) {
        self.update(|o| {
            if let Some(p) = percent {
                o.progress = Some(p.clamp(0.0, 100.0));
            }
            if let Some(s) = step {
                o.step = Some(s.to_owned());
            }
        });
    }

    pub fn reboot_needed(&self) {
        self.update(|o| o.reboot_needed = true);
    }

    pub fn finish(&self, result: &Result<()>) {
        self.update(|o| {
            o.finished_at = Some(rfc3339(now_unix()));
            match result {
                Ok(()) => {
                    o.state = "succeeded";
                    o.progress = Some(100.0);
                }
                Err(e) => {
                    o.state = "failed";
                    o.error = Some(e.message().to_owned());
                }
            }
        });
        if let Some(op) = self.snapshot() {
            crate::hw::op_finished(&self.st, op.kind, result.is_ok());
        }
    }

    /// The operation as it is now.
    pub fn snapshot(&self) -> Option<Operation> {
        self.st.operation(&self.id)
    }
}
