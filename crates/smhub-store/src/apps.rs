//! Tables `apps`, `appsettings`, `configsource` and `configsetting`.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, Row, params};
use smhub_model::{Error, Result};
use smhub_model::time::db_now;

use crate::{Db, b, sql_err};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRow {
    pub id: i64,
    pub key: String,
    pub description: Option<String>,
    pub version: String,
    pub installed: bool,
    pub is_service: bool,
    pub start_at_boot: bool,
    pub watchdog: Option<bool>,
    pub show_in_sidebar: bool,
    pub iframe_port: Option<i64>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigSourceRow {
    pub name: String,
    pub provider: String,
    pub path: String,
    pub params: Option<serde_json::Value>,
    pub app_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSettingRow {
    pub name: String,
    pub source_name: String,
    pub path: String,
    pub value: Option<String>,
    pub app_id: Option<i64>,
}

const APP_COLS: &str =
    "id, \"key\", description, version, installed, is_service, start_at_boot, watchdog, show_in_sidebar, iframe_port, icon";

fn app(r: &Row<'_>) -> rusqlite::Result<AppRow> {
    Ok(AppRow {
        id: r.get(0)?,
        key: r.get(1)?,
        description: r.get(2)?,
        version: r.get(3)?,
        installed: r.get(4)?,
        is_service: r.get(5)?,
        start_at_boot: r.get(6)?,
        watchdog: r.get(7)?,
        show_in_sidebar: r.get(8)?,
        iframe_port: r.get(9)?,
        icon: r.get(10)?,
    })
}

/// Fields of `apps` that can be changed.
#[derive(Debug, Clone, Default)]
pub struct AppUpdate {
    pub installed: Option<bool>,
    pub is_service: Option<bool>,
    pub start_at_boot: Option<bool>,
    pub watchdog: Option<bool>,
    pub show_in_sidebar: Option<bool>,
    pub iframe_port: Option<Option<i64>>,
    pub version: Option<String>,
    pub description: Option<String>,
}

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

impl Db {
    pub fn apps(&self) -> Result<Vec<AppRow>> {
        let mut st = self.main.prepare_cached(&format!("SELECT {APP_COLS} FROM apps ORDER BY \"key\"")).map_err(sql_err)?;
        let rows = st.query_map([], app).map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    pub fn app(&self, key: &str) -> Result<Option<AppRow>> {
        self.main
            .query_row(&format!("SELECT {APP_COLS} FROM apps WHERE \"key\" = ?1"), [key], app)
            .optional()
            .map_err(sql_err)
    }

    /// Create the row for a package, as the old `check_and_create_apps` did. `name` and `key`
    /// are the package name; `version` is the feed's latest version.
    pub fn create_app(&self, key: &str, description: Option<&str>, version: &str, installed: bool) -> Result<AppRow> {
        self.main
            .execute(
                "INSERT INTO apps (\"key\", name, description, enabled, start_at_boot, created_at, updated_at, version,
                   watchdog, installed, iframe_port, icon, is_service, show_in_sidebar)
                 VALUES (?1, ?2, ?3, 0, 0, ?4, NULL, ?5, NULL, ?6, NULL, NULL, 0, 1)",
                params![
                    trunc(key, 32),
                    trunc(key, 32),
                    description.map(|d| trunc(d, 1024)),
                    db_now(),
                    trunc(version, 16),
                    b(installed)
                ],
            )
            .map_err(sql_err)?;
        self.app(key)?.ok_or_else(|| Error::internal("app row missing after insert"))
    }

    pub fn update_app(&self, key: &str, u: &AppUpdate) -> Result<()> {
        let mut sets: Vec<String> = Vec::new();
        let mut vals: Vec<rusqlite::types::Value> = Vec::new();
        let mut add = |col: &str, v: rusqlite::types::Value| {
            sets.push(format!("{col} = ?"));
            vals.push(v);
        };
        use rusqlite::types::Value as V;
        if let Some(v) = u.installed {
            add("installed", V::Integer(b(v)));
        }
        if let Some(v) = u.is_service {
            add("is_service", V::Integer(b(v)));
        }
        if let Some(v) = u.start_at_boot {
            add("start_at_boot", V::Integer(b(v)));
        }
        if let Some(v) = u.watchdog {
            add("watchdog", V::Integer(b(v)));
        }
        if let Some(v) = u.show_in_sidebar {
            add("show_in_sidebar", V::Integer(b(v)));
        }
        if let Some(v) = u.iframe_port {
            add("iframe_port", v.map_or(V::Null, V::Integer));
        }
        if let Some(v) = &u.version {
            add("version", V::Text(trunc(v, 16)));
        }
        if let Some(v) = &u.description {
            add("description", V::Text(trunc(v, 1024)));
        }
        if sets.is_empty() {
            return Ok(());
        }
        sets.push("updated_at = ?".into());
        vals.push(V::Text(db_now()));
        vals.push(V::Text(key.to_owned()));
        let sql = format!("UPDATE apps SET {} WHERE \"key\" = ?", sets.join(", "));
        self.main.execute(&sql, rusqlite::params_from_iter(vals)).map_err(sql_err)?;
        Ok(())
    }

    pub fn app_settings(&self, app_id: i64) -> Result<BTreeMap<String, String>> {
        let mut st =
            self.main.prepare_cached("SELECT \"key\", value FROM appsettings WHERE app_id = ?1").map_err(sql_err)?;
        let rows = st.query_map([app_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(sql_err)?;
        rows.collect::<rusqlite::Result<BTreeMap<_, _>>>().map_err(sql_err)
    }

    /// Write app settings in one transaction; unchanged values are not rewritten.
    pub fn save_app_settings(&mut self, app_id: i64, values: &[(String, String)]) -> Result<()> {
        let now = db_now();
        let tx = self.main.transaction().map_err(sql_err)?;
        for (k, v) in values {
            let current: Option<String> = tx
                .query_row(
                    "SELECT value FROM appsettings WHERE app_id = ?1 AND \"key\" = ?2",
                    params![app_id, k],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql_err)?;
            let v = trunc(v, 1024);
            match current {
                Some(c) if c == v => {}
                Some(_) => {
                    tx.execute(
                        "UPDATE appsettings SET value = ?1, updated_at = ?2 WHERE app_id = ?3 AND \"key\" = ?4",
                        params![v, now, app_id, k],
                    )
                    .map_err(sql_err)?;
                }
                None => {
                    tx.execute(
                        "INSERT INTO appsettings (\"key\", value, updated_at, app_id) VALUES (?1, ?2, ?3, ?4)",
                        params![k, v, now, app_id],
                    )
                    .map_err(sql_err)?;
                }
            }
        }
        tx.commit().map_err(sql_err)
    }

    /// Add settings that do not exist yet, keeping existing values (old `sync_app_settings`).
    pub fn add_missing_app_settings(&mut self, app_id: i64, defaults: &[(String, String)]) -> Result<()> {
        let tx = self.main.transaction().map_err(sql_err)?;
        for (k, v) in defaults {
            tx.execute(
                "INSERT OR IGNORE INTO appsettings (\"key\", value, updated_at, app_id) VALUES (?1, ?2, ?3, ?4)",
                params![k, trunc(v, 1024), db_now(), app_id],
            )
            .map_err(sql_err)?;
        }
        tx.commit().map_err(sql_err)
    }

    pub fn delete_app_settings(&self, app_id: i64) -> Result<()> {
        self.main.execute("DELETE FROM appsettings WHERE app_id = ?1", [app_id]).map_err(sql_err)?;
        Ok(())
    }

    pub fn config_sources(&self) -> Result<Vec<ConfigSourceRow>> {
        let mut st =
            self.main.prepare_cached("SELECT name, provider, path, params, app_id FROM configsource").map_err(sql_err)?;
        let rows = st
            .query_map([], |r| {
                let params: Option<String> = r.get(3)?;
                Ok(ConfigSourceRow {
                    name: r.get(0)?,
                    provider: r.get(1)?,
                    path: r.get(2)?,
                    params: params.and_then(|p| serde_json::from_str(&p).ok()).filter(|v: &serde_json::Value| !v.is_null()),
                    app_id: r.get(4)?,
                })
            })
            .map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    pub fn config_settings(&self) -> Result<Vec<ConfigSettingRow>> {
        let mut st = self
            .main
            .prepare_cached("SELECT name, source_name, path, value, app_id FROM configsetting")
            .map_err(sql_err)?;
        let rows = st
            .query_map([], |r| {
                Ok(ConfigSettingRow {
                    name: r.get(0)?,
                    source_name: r.get(1)?,
                    path: r.get(2)?,
                    value: r.get(3)?,
                    app_id: r.get(4)?,
                })
            })
            .map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    /// Insert or update a config source by name (old `crud.upsert_config_source`).
    pub fn upsert_config_source(&self, s: &ConfigSourceRow) -> Result<()> {
        // SQLAlchemy's JSON type stored Python's json.dumps text, and JSON null for None
        let params_json = py_json(s.params.as_ref().unwrap_or(&serde_json::Value::Null));
        let current: Option<(String, String, Option<String>, Option<i64>)> = self
            .main
            .query_row("SELECT provider, path, params, app_id FROM configsource WHERE name = ?1", [&s.name], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()
            .map_err(sql_err)?;
        if let Some((provider, path, params, app_id)) = &current {
            let same_params = params.as_deref().and_then(|p| serde_json::from_str::<serde_json::Value>(p).ok())
                == Some(s.params.clone().unwrap_or(serde_json::Value::Null));
            if *provider == s.provider && *path == s.path && same_params && *app_id == s.app_id {
                return Ok(());
            }
        }
        let updated = self
            .main
            .execute(
                "UPDATE configsource SET provider = ?1, path = ?2, params = ?3, app_id = ?4 WHERE name = ?5",
                params![s.provider, s.path, params_json, s.app_id, s.name],
            )
            .map_err(sql_err)?;
        if updated == 0 {
            self.main
                .execute(
                    "INSERT INTO configsource (name, provider, path, params, app_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![s.name, s.provider, s.path, params_json, s.app_id],
                )
                .map_err(sql_err)?;
        }
        Ok(())
    }

    /// Insert or update a config setting by (name, source) (old `crud.upsert_config_setting`).
    pub fn upsert_config_setting(&self, s: &ConfigSettingRow) -> Result<()> {
        let updated = self
            .main
            .execute(
                "UPDATE configsetting SET path = ?1, value = ?2, app_id = ?3 WHERE name = ?4 AND source_name = ?5",
                params![s.path, s.value, s.app_id, s.name, s.source_name],
            )
            .map_err(sql_err)?;
        if updated == 0 {
            self.main
                .execute(
                    "INSERT INTO configsetting (name, source_name, path, value, app_id) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![s.name, s.source_name, s.path, s.value, s.app_id],
                )
                .map_err(sql_err)?;
        }
        Ok(())
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::memory;

    #[test]
    fn apps_round_trip() -> Result<()> {
        let mut db = memory()?;
        let a = db.create_app("zigbee2mqtt", Some("Zigbee"), "2.13.0-1", true)?;
        assert!(a.show_in_sidebar);
        db.update_app("zigbee2mqtt", &AppUpdate { iframe_port: Some(Some(8080)), is_service: Some(true), ..Default::default() })?;
        let a = db.app("zigbee2mqtt")?.ok_or_else(|| Error::internal("missing"))?;
        assert_eq!(a.iframe_port, Some(8080));
        db.add_missing_app_settings(a.id, &[("adapter".into(), "zstack".into())])?;
        db.add_missing_app_settings(a.id, &[("adapter".into(), "ember".into())])?;
        assert_eq!(db.app_settings(a.id)?.get("adapter").map(String::as_str), Some("zstack"));
        db.upsert_config_source(&ConfigSourceRow {
            name: "zigbee2mqtt".into(),
            provider: "yaml".into(),
            path: "/x.yaml".into(),
            params: None,
            app_id: Some(a.id),
        })?;
        db.upsert_config_setting(&ConfigSettingRow {
            name: "adapter".into(),
            source_name: "zigbee2mqtt".into(),
            path: "serial.adapter".into(),
            value: Some("zstack".into()),
            app_id: Some(a.id),
        })?;
        assert_eq!(db.config_sources()?.len(), 1);
        assert_eq!(db.config_settings()?.len(), 1);
        Ok(())
    }
}

/// JSON text as Python's `json.dumps` writes it by default: `", "` and `": "` separators and
/// non-ASCII characters escaped, so rows look the same whichever backend wrote them.
pub fn py_json(v: &serde_json::Value) -> String {
    fn esc(s: &str, out: &mut String) {
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str(r#"\""#),
                '\\' => out.push_str(r"\\"),
                '\n' => out.push_str(r"\n"),
                '\r' => out.push_str(r"\r"),
                '\t' => out.push_str(r"\t"),
                '\u{08}' => out.push_str(r"\b"),
                '\u{0c}' => out.push_str(r"\f"),
                c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                    let mut buf = [0u16; 2];
                    for u in c.encode_utf16(&mut buf) {
                        out.push_str(&format!(r"\u{u:04x}"));
                    }
                }
                c => out.push(c),
            }
        }
        out.push('"');
    }
    fn go(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Null => out.push_str("null"),
            serde_json::Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            serde_json::Value::Number(n) => out.push_str(&n.to_string()),
            serde_json::Value::String(s) => esc(s, out),
            serde_json::Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    go(x, out);
                }
                out.push(']');
            }
            serde_json::Value::Object(m) => {
                out.push('{');
                for (i, (k, x)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    esc(k, out);
                    out.push_str(": ");
                    go(x, out);
                }
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    go(v, &mut out);
    out
}

#[cfg(test)]
mod py_json_tests {
    use super::py_json;

    #[test]
    fn matches_python() {
        let v: serde_json::Value =
            serde_json::from_str(r##"{"comment_prefixes":["#"],"x":null,"é":1.5,"q":"a\"b"}"##).unwrap_or_default();
        let bs = '\\';
        let expected = format!(r##"{{"comment_prefixes": ["#"], "x": null, "{bs}u00e9": 1.5, "q": "a{bs}"b"}}"##);
        assert_eq!(py_json(&v), expected);
        assert_eq!(py_json(&serde_json::Value::Null), "null");
    }
}
