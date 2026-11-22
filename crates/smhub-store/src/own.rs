//! The daemon's own file, `smhubd.db`: sessions, new preferences and bookkeeping. The old
//! backend never reads it, so going back to the previous versions ignores it (D21).

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use smhub_model::Result;

use crate::{Db, sql_err};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    created INTEGER NOT NULL,
    expires INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS preferences (
    user_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (user_id, key)
);
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS hub_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS ir_codes (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    device TEXT NOT NULL DEFAULT '',
    protocol TEXT,
    scancode INTEGER,
    carrier INTEGER NOT NULL DEFAULT 38000,
    raw TEXT NOT NULL DEFAULT '[]',
    favourite INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS api_tokens (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    user_id TEXT NOT NULL,
    created INTEGER NOT NULL,
    last_used INTEGER
);
PRAGMA user_version = 2;
";

pub(crate) fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let c = Connection::open(path).map_err(sql_err)?;
    c.busy_timeout(std::time::Duration::from_secs(5)).map_err(sql_err)?;
    c.execute_batch("PRAGMA synchronous = FULL; PRAGMA cache_size = -64;").map_err(sql_err)?;
    init(&c)?;
    Ok(c)
}

pub(crate) fn init(c: &Connection) -> Result<()> {
    c.execute_batch(SCHEMA).map_err(sql_err)
}

/// A learned or entered infrared code.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IrCodeRow {
    pub id: String,
    pub name: String,
    pub device: String,
    /// Kernel protocol name (`nec`, `rc-5`, ...) when the code was decoded.
    pub protocol: Option<String>,
    pub scancode: Option<i64>,
    pub carrier: i64,
    /// Pulse and space lengths in microseconds, starting with a pulse.
    pub raw: Vec<u32>,
    pub favourite: bool,
    pub created: i64,
}

/// An API token; only its hash is kept.
#[derive(Debug, Clone)]
pub struct TokenRow {
    pub id: String,
    pub name: String,
    pub token_hash: String,
    pub user_id: String,
    pub created: i64,
    pub last_used: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub token_hash: String,
    pub user_id: String,
    pub created: i64,
    pub expires: i64,
}

impl Db {
    pub fn sessions(&self) -> Result<Vec<SessionRow>> {
        let mut st =
            self.own.prepare_cached("SELECT token_hash, user_id, created, expires FROM sessions").map_err(sql_err)?;
        let rows = st
            .query_map([], |r| {
                Ok(SessionRow { token_hash: r.get(0)?, user_id: r.get(1)?, created: r.get(2)?, expires: r.get(3)? })
            })
            .map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    pub fn add_session(&self, s: &SessionRow) -> Result<()> {
        self.own
            .execute(
                "INSERT OR REPLACE INTO sessions (token_hash, user_id, created, expires) VALUES (?1, ?2, ?3, ?4)",
                params![s.token_hash, s.user_id, s.created, s.expires],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    pub fn delete_session(&self, token_hash: &str) -> Result<()> {
        self.own.execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash]).map_err(sql_err)?;
        Ok(())
    }

    /// Remove a user's sessions, optionally keeping one.
    pub fn delete_user_sessions(&self, user_id: &str, keep: Option<&str>) -> Result<()> {
        self.own
            .execute(
                "DELETE FROM sessions WHERE user_id = ?1 AND token_hash != ?2",
                params![user_id, keep.unwrap_or("")],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    pub fn delete_expired_sessions(&self, now: i64) -> Result<()> {
        self.own.execute("DELETE FROM sessions WHERE expires < ?1", [now]).map_err(sql_err)?;
        Ok(())
    }

    pub fn preferences(&self, user_id: &str) -> Result<BTreeMap<String, String>> {
        let mut st = self.own.prepare_cached("SELECT key, value FROM preferences WHERE user_id = ?1").map_err(sql_err)?;
        let rows = st.query_map([user_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(sql_err)?;
        rows.collect::<rusqlite::Result<BTreeMap<_, _>>>().map_err(sql_err)
    }

    pub fn save_preferences(&mut self, user_id: &str, values: &[(String, String)]) -> Result<()> {
        let tx = self.own.transaction().map_err(sql_err)?;
        for (k, v) in values {
            tx.execute(
                "INSERT INTO preferences (user_id, key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT(user_id, key) DO UPDATE SET value = excluded.value",
                params![user_id, k, v],
            )
            .map_err(sql_err)?;
        }
        tx.commit().map_err(sql_err)
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        self.own.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional().map_err(sql_err)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.own
            .execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, value],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    pub fn hub_setting(&self, key: &str) -> Result<Option<String>> {
        self.own.query_row("SELECT value FROM hub_settings WHERE key = ?1", [key], |r| r.get(0)).optional().map_err(sql_err)
    }

    pub fn set_hub_setting(&self, key: &str, value: &str) -> Result<()> {
        self.own
            .execute(
                "INSERT INTO hub_settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [key, value],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    pub fn delete_hub_setting(&self, key: &str) -> Result<()> {
        self.own.execute("DELETE FROM hub_settings WHERE key = ?1", [key]).map_err(sql_err)?;
        Ok(())
    }

    pub fn ir_codes(&self) -> Result<Vec<IrCodeRow>> {
        let mut st = self
            .own
            .prepare_cached("SELECT id, name, device, protocol, scancode, carrier, raw, favourite, created FROM ir_codes ORDER BY device, name, created")
            .map_err(sql_err)?;
        let rows = st
            .query_map([], |r| {
                let raw: String = r.get(6)?;
                Ok(IrCodeRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    device: r.get(2)?,
                    protocol: r.get(3)?,
                    scancode: r.get(4)?,
                    carrier: r.get(5)?,
                    raw: serde_json::from_str(&raw).unwrap_or_default(),
                    favourite: r.get::<_, i64>(7)? != 0,
                    created: r.get(8)?,
                })
            })
            .map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    pub fn save_ir_code(&self, c: &IrCodeRow) -> Result<()> {
        self.own
            .execute(
                "INSERT INTO ir_codes (id, name, device, protocol, scancode, carrier, raw, favourite, created)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, device = excluded.device, protocol = excluded.protocol,
                   scancode = excluded.scancode, carrier = excluded.carrier, raw = excluded.raw, favourite = excluded.favourite",
                params![c.id, c.name, c.device, c.protocol, c.scancode, c.carrier, serde_json::to_string(&c.raw)?, i64::from(c.favourite), c.created],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    pub fn delete_ir_code(&self, id: &str) -> Result<bool> {
        let n = self.own.execute("DELETE FROM ir_codes WHERE id = ?1", [id]).map_err(sql_err)?;
        Ok(n > 0)
    }

    pub fn tokens(&self) -> Result<Vec<TokenRow>> {
        let mut st = self.own.prepare_cached("SELECT id, name, token_hash, user_id, created, last_used FROM api_tokens ORDER BY created").map_err(sql_err)?;
        let rows = st
            .query_map([], |r| {
                Ok(TokenRow { id: r.get(0)?, name: r.get(1)?, token_hash: r.get(2)?, user_id: r.get(3)?, created: r.get(4)?, last_used: r.get(5)? })
            })
            .map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    pub fn add_token(&self, t: &TokenRow) -> Result<()> {
        self.own
            .execute(
                "INSERT INTO api_tokens (id, name, token_hash, user_id, created, last_used) VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                params![t.id, t.name, t.token_hash, t.user_id, t.created],
            )
            .map_err(sql_err)?;
        Ok(())
    }

    pub fn delete_token(&self, id: &str) -> Result<bool> {
        let n = self.own.execute("DELETE FROM api_tokens WHERE id = ?1", [id]).map_err(sql_err)?;
        Ok(n > 0)
    }

    /// Record a token's use, at most once a minute per token (the caller decides when).
    pub fn token_used(&self, id: &str, at: i64) -> Result<()> {
        self.own.execute("UPDATE api_tokens SET last_used = ?2 WHERE id = ?1", params![id, at]).map_err(sql_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::memory;

    #[test]
    fn sessions_and_preferences() -> Result<()> {
        let mut db = memory()?;
        db.add_session(&SessionRow { token_hash: "h".into(), user_id: "u".into(), created: 1, expires: 10 })?;
        assert_eq!(db.sessions()?.len(), 1);
        db.delete_expired_sessions(20)?;
        assert!(db.sessions()?.is_empty());
        db.save_preferences("u", &[("level".into(), "advanced".into())])?;
        assert_eq!(db.preferences("u")?.get("level").map(String::as_str), Some("advanced"));
        db.set_meta("k", "v")?;
        assert_eq!(db.meta("k")?.as_deref(), Some("v"));
        Ok(())
    }

    #[test]
    fn hub_settings_codes_and_tokens() -> Result<()> {
        let db = memory()?;
        db.set_hub_setting("hardware", "{}")?;
        assert_eq!(db.hub_setting("hardware")?.as_deref(), Some("{}"));
        let c = IrCodeRow {
            id: "a".into(),
            name: "Power".into(),
            device: "TV".into(),
            protocol: Some("nec".into()),
            scancode: Some(0x0408),
            carrier: 38000,
            raw: vec![9000, 4500, 560],
            favourite: true,
            created: 5,
        };
        db.save_ir_code(&c)?;
        assert_eq!(db.ir_codes()?, vec![c]);
        assert!(db.delete_ir_code("a")?);
        assert!(db.ir_codes()?.is_empty());
        db.add_token(&TokenRow { id: "t".into(), name: "HA".into(), token_hash: "h".into(), user_id: "u".into(), created: 1, last_used: None })?;
        db.token_used("t", 9)?;
        assert_eq!(db.tokens()?.first().and_then(|t| t.last_used), Some(9));
        assert!(db.delete_token("t")?);
        Ok(())
    }
}
