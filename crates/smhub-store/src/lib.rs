//! SQLite access (decisions D20–D22).
//!
//! Two files:
//! - `backend.db`, the database the old backend created. Its tables are read and written exactly
//!   as they are, with the same value formats, so the previous versions keep working with it.
//! - `smhubd.db`, the daemon's own small file for what is new: sessions, new preferences, and
//!   bookkeeping.
//!
//! One thread owns both connections and takes jobs from a bounded queue (64 jobs).

mod apps;
mod legacy;
mod own;
mod pages;
mod users;

use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

use rusqlite::Connection;
use smhub_model::{Error, Result};

pub use apps::{AppRow, AppUpdate, ConfigSettingRow, ConfigSourceRow};
pub use pages::{bool_str, parse_bool};
pub use users::SYSTEM_USER;
pub use legacy::{ALEMBIC_HEAD, Upgrade};
pub use own::{IrCodeRow, SessionRow, TokenRow};
pub use users::{UserRow, UserSettingsRow};

/// Jobs waiting for the store thread.
pub const QUEUE: usize = 64;

/// Both connections, owned by the store thread.
pub struct Db {
    pub main: Connection,
    pub own: Connection,
}

type Job = Box<dyn FnOnce(&mut Db) + Send>;

/// Handle to the store thread.
#[derive(Clone)]
pub struct Store {
    tx: SyncSender<Job>,
}

pub(crate) fn sql_err(e: rusqlite::Error) -> Error {
    Error::internal(format!("database: {e}"))
}

impl Store {
    /// Open both files and start the store thread. A missing `backend.db` is created with the
    /// old backend's schema at its last migration; one at an older migration is brought to it.
    pub fn open(main_path: &Path, own_path: &Path, up: &Upgrade<'_>) -> Result<Self> {
        let mut db = Db { main: open_main(main_path)?, own: own::open(own_path)? };
        legacy::ensure(&mut db.main, up)?;
        let (tx, rx) = sync_channel::<Job>(QUEUE);
        std::thread::Builder::new()
            .name("store".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    if std::panic::catch_unwind(AssertUnwindSafe(|| job(&mut db))).is_err() {
                        tracing::error!("a database job panicked");
                    }
                }
            })
            .map_err(|e| Error::internal(format!("store thread: {e}")))?;
        Ok(Self { tx })
    }

    /// Run a job on the store thread and wait for its result.
    pub async fn run<R, F>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Db) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let (otx, orx) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move |db| {
            let _ = otx.send(f(db));
        });
        match self.tx.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => return Err(Error::Busy("the database queue is full".into())),
            Err(TrySendError::Disconnected(_)) => return Err(Error::internal("the database thread has stopped")),
        }
        orx.await.map_err(|_| Error::internal("the database job did not answer"))?
    }
}

fn open_main(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let c = Connection::open(path).map_err(sql_err)?;
    c.busy_timeout(std::time::Duration::from_secs(5)).map_err(sql_err)?;
    c.execute_batch("PRAGMA synchronous = FULL; PRAGMA cache_size = -256;").map_err(sql_err)?;
    c.set_prepared_statement_cache_capacity(24);
    Ok(c)
}

/// Booleans as SQLAlchemy stores them in SQLite: 0 and 1.
pub(crate) fn b(v: bool) -> i64 {
    i64::from(v)
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    pub fn memory() -> Result<Db> {
        let mut main = Connection::open_in_memory().map_err(sql_err)?;
        legacy::ensure(&mut main, &Upgrade { zigbee_adapter: "zstack", rtos_key: "" })?;
        let own = Connection::open_in_memory().map_err(sql_err)?;
        own::init(&own)?;
        Ok(Db { main, own })
    }
}
