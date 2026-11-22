//! Tables `pages` and `devicesettings`: the key/value store of the settings pages. Values are
//! strings, booleans as "true"/"false", lists and objects as JSON, as the old backend stored them.

use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, params};
use smhub_model::Result;
use smhub_model::time::db_now;

use crate::{Db, sql_err};

impl Db {
    /// The page's id, creating the page when it does not exist (the old UI created pages on
    /// first use with `POST /pages`).
    pub fn page_id(&self, key: &str, name: &str, description: &str) -> Result<i64> {
        if let Some(id) = self
            .main
            .query_row("SELECT id FROM pages WHERE \"key\" = ?1", [key], |r| r.get(0))
            .optional()
            .map_err(sql_err)?
        {
            return Ok(id);
        }
        self.main
            .execute(
                "INSERT INTO pages (\"key\", name, description, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, NULL)",
                params![key, name, description, db_now()],
            )
            .map_err(sql_err)?;
        Ok(self.main.last_insert_rowid())
    }

    /// All settings of a page. A missing page has none.
    pub fn page_settings(&self, key: &str) -> Result<BTreeMap<String, String>> {
        let mut st = self
            .main
            .prepare_cached(
                "SELECT d.\"key\", d.value FROM devicesettings d JOIN pages p ON p.id = d.page_id WHERE p.\"key\" = ?1",
            )
            .map_err(sql_err)?;
        let rows = st.query_map([key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(sql_err)?;
        rows.collect::<rusqlite::Result<BTreeMap<_, _>>>().map_err(sql_err)
    }

    /// One setting of a page.
    pub fn page_setting(&self, key: &str, setting: &str) -> Result<Option<String>> {
        self.main
            .query_row(
                "SELECT d.value FROM devicesettings d JOIN pages p ON p.id = d.page_id WHERE p.\"key\" = ?1 AND d.\"key\" = ?2",
                [key, setting],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql_err)
    }

    /// Write several settings of a page in one transaction. Unchanged values are not rewritten,
    /// which keeps `updated_at` meaningful, as the old upsert did.
    pub fn save_page_settings(&mut self, page: &str, name: &str, values: &[(String, String)]) -> Result<()> {
        let page_id = self.page_id(page, name, "")?;
        let now = db_now();
        let tx = self.main.transaction().map_err(sql_err)?;
        for (k, v) in values {
            let current: Option<String> = tx
                .query_row(
                    "SELECT value FROM devicesettings WHERE page_id = ?1 AND \"key\" = ?2",
                    params![page_id, k],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql_err)?;
            match current {
                Some(c) if &c == v => {}
                Some(_) => {
                    tx.execute(
                        "UPDATE devicesettings SET value = ?1, updated_at = ?2 WHERE page_id = ?3 AND \"key\" = ?4",
                        params![v, now, page_id, k],
                    )
                    .map_err(sql_err)?;
                }
                None => {
                    tx.execute(
                        "INSERT INTO devicesettings (\"key\", value, updated_at, page_id) VALUES (?1, ?2, ?3, ?4)",
                        params![k, v, now, page_id],
                    )
                    .map_err(sql_err)?;
                }
            }
        }
        tx.commit().map_err(sql_err)
    }

    /// Remove one setting of a page.
    pub fn delete_page_setting(&self, page: &str, setting: &str) -> Result<()> {
        self.main
            .execute(
                "DELETE FROM devicesettings WHERE \"key\" = ?2 AND page_id = (SELECT id FROM pages WHERE \"key\" = ?1)",
                [page, setting],
            )
            .map_err(sql_err)?;
        Ok(())
    }
}

/// A boolean the way the old backend stored it.
pub fn bool_str(v: bool) -> String {
    if v { "true".into() } else { "false".into() }
}

/// Read a stored boolean leniently ("true", "True", "1").
pub fn parse_bool(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on")
}

#[cfg(test)]
mod tests {
    use crate::testutil::memory;

    #[test]
    fn page_settings() -> smhub_model::Result<()> {
        let mut db = memory()?;
        assert!(db.page_settings("general")?.is_empty());
        db.save_page_settings("general", "General Settings", &[("hostname".into(), "SMHUB".into())])?;
        db.save_page_settings("general", "General Settings", &[("hostname".into(), "hub2".into())])?;
        assert_eq!(db.page_setting("general", "hostname")?.as_deref(), Some("hub2"));
        db.delete_page_setting("general", "hostname")?;
        assert!(db.page_settings("general")?.is_empty());
        Ok(())
    }
}
