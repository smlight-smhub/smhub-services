//! The old backend's schema. A missing database is created with exactly the DDL the old backend
//! has after its last migration (`4cc919e10054`), so the previous versions accept it. An existing
//! database is never altered here.

use rusqlite::Connection;
use smhub_model::Result;

use crate::sql_err;

/// Last Alembic revision of the old backend.
pub const ALEMBIC_HEAD: &str = "4cc919e10054";

/// DDL as read from a hub's `backend.db` at that revision.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS alembic_version (
	version_num VARCHAR(32) NOT NULL,
	CONSTRAINT alembic_version_pkc PRIMARY KEY (version_num)
);
CREATE TABLE IF NOT EXISTS user (
	email VARCHAR(255) NOT NULL,
	is_active BOOLEAN NOT NULL,
	is_superuser BOOLEAN NOT NULL,
	full_name VARCHAR(255),
	id VARCHAR NOT NULL,
	hashed_password VARCHAR NOT NULL,
	PRIMARY KEY (id)
);
CREATE UNIQUE INDEX IF NOT EXISTS ix_user_email ON user (email);
CREATE TABLE IF NOT EXISTS apps (
	id INTEGER NOT NULL,
	"key" VARCHAR(32) NOT NULL,
	name VARCHAR(32) NOT NULL,
	description VARCHAR(1024),
	enabled BOOLEAN NOT NULL,
	start_at_boot BOOLEAN NOT NULL,
	created_at DATETIME NOT NULL,
	updated_at DATETIME, version VARCHAR(16) NOT NULL, watchdog BOOLEAN, installed BOOLEAN DEFAULT 0 NOT NULL, iframe_port INTEGER, icon VARCHAR(64), is_service BOOLEAN DEFAULT '0' NOT NULL, show_in_sidebar BOOLEAN DEFAULT '1' NOT NULL,
	PRIMARY KEY (id),
	UNIQUE ("key")
);
CREATE TABLE IF NOT EXISTS appsettings (
	id INTEGER NOT NULL,
	"key" VARCHAR(255) NOT NULL,
	value VARCHAR(1024) NOT NULL,
	updated_at DATETIME,
	app_id INTEGER NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(app_id) REFERENCES apps (id),
	CONSTRAINT uix_app_id_key UNIQUE (app_id, "key")
);
CREATE INDEX IF NOT EXISTS ix_appsettings_updated_at ON appsettings (updated_at);
CREATE TABLE IF NOT EXISTS usersettings (
	id VARCHAR NOT NULL,
	light_theme BOOLEAN,
	metric_units BOOLEAN,
	notifications BOOLEAN,
	language VARCHAR(8),
	locale VARCHAR(8),
	timezone VARCHAR(64),
	use_12h_clock BOOLEAN,
	PRIMARY KEY (id),
	FOREIGN KEY(id) REFERENCES user (id)
);
CREATE TABLE IF NOT EXISTS pages (
	id INTEGER NOT NULL,
	"key" VARCHAR(32) NOT NULL,
	name VARCHAR(32) NOT NULL,
	description VARCHAR(1024),
	created_at DATETIME NOT NULL,
	updated_at DATETIME,
	PRIMARY KEY (id),
	UNIQUE ("key")
);
CREATE TABLE IF NOT EXISTS devicesettings (
	id INTEGER NOT NULL,
	"key" VARCHAR(255) NOT NULL,
	value VARCHAR(1024) NOT NULL,
	updated_at DATETIME,
	page_id INTEGER NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(page_id) REFERENCES pages (id),
	CONSTRAINT uix_dev_id_key UNIQUE (page_id, "key")
);
CREATE INDEX IF NOT EXISTS ix_devicesettings_updated_at ON devicesettings (updated_at);
CREATE TABLE IF NOT EXISTS fileitem (
	name VARCHAR(255) NOT NULL,
	is_directory BOOLEAN NOT NULL,
	path VARCHAR(1024) NOT NULL,
	parent_id VARCHAR,
	size INTEGER,
	mime_type VARCHAR(128),
	id VARCHAR NOT NULL,
	created_at DATETIME NOT NULL,
	updated_at DATETIME, synced_at DATETIME, owner VARCHAR(128) DEFAULT 'smlight' NOT NULL,
	PRIMARY KEY (id),
	FOREIGN KEY(parent_id) REFERENCES fileitem (id)
);
CREATE TABLE IF NOT EXISTS configsource (
	id INTEGER NOT NULL,
	name VARCHAR(64) NOT NULL,
	provider VARCHAR(32) NOT NULL,
	path VARCHAR(255) NOT NULL,
	params JSON,
	app_id INTEGER,
	PRIMARY KEY (id),
	FOREIGN KEY(app_id) REFERENCES apps (id)
);
CREATE UNIQUE INDEX IF NOT EXISTS ix_configsource_name ON configsource (name);
CREATE TABLE IF NOT EXISTS configsetting (
	id INTEGER NOT NULL,
	name VARCHAR(64) NOT NULL,
	source_name VARCHAR(64) NOT NULL,
	path VARCHAR(255) NOT NULL,
	value VARCHAR(1024),
	app_id INTEGER,
	PRIMARY KEY (id),
	FOREIGN KEY(app_id) REFERENCES apps (id),
	FOREIGN KEY(source_name) REFERENCES configsource (name) ON DELETE CASCADE
);
"#;

/// What the old migrations needed from the hub itself.
pub struct Upgrade<'a> {
    /// `ember` when the raw EEPROM model contains "mg", else `zstack` (revision `ac49055b2b34`).
    pub zigbee_adapter: &'a str,
    /// The ESPHome factory key preseeded by revision `4cc919e10054`.
    pub rtos_key: &'a str,
}

/// The Alembic chain of the old backend, oldest first (reference §19).
const CHAIN: &[&str] = &[
    "6e89cce31fda",
    "65450f9768ca",
    "ac49055b2b34",
    "e1ad05962702",
    "826b9fba7761",
    "b3f1c2d4e5a6",
    "0958a1a2873c",
    "71b1eaf89bfe",
    "7e4b48ebfc95",
    "f2a4d8c9b1e7",
    "9375c2db72a0",
    ALEMBIC_HEAD,
];

/// Create the database if it is empty; bring a database at an older revision of the old backend
/// to its last one with the same changes Alembic made, in one transaction. A database at an
/// unknown revision is left as it is.
pub fn ensure(c: &mut Connection, up: &Upgrade<'_>) -> Result<()> {
    let tables: i64 = c
        .query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'user'", [], |r| r.get(0))
        .map_err(sql_err)?;
    if tables == 0 {
        let tx = c.transaction().map_err(sql_err)?;
        tx.execute_batch(SCHEMA).map_err(sql_err)?;
        tx.execute("INSERT INTO alembic_version (version_num) VALUES (?1)", [ALEMBIC_HEAD]).map_err(sql_err)?;
        tx.commit().map_err(sql_err)?;
        tracing::info!("created backend.db at revision {ALEMBIC_HEAD}");
        return Ok(());
    }
    let rev: Option<String> = c.query_row("SELECT version_num FROM alembic_version LIMIT 1", [], |r| r.get(0)).ok();
    let Some(rev) = rev else {
        tracing::warn!("backend.db has no Alembic revision; left as it is");
        return Ok(());
    };
    if rev == ALEMBIC_HEAD {
        return Ok(());
    }
    let Some(at) = CHAIN.iter().position(|r| *r == rev) else {
        tracing::warn!(revision = %rev, "backend.db is at a revision this version does not know; left as it is");
        return Ok(());
    };
    let tx = c.transaction().map_err(sql_err)?;
    for (i, next) in CHAIN.iter().enumerate().skip(at + 1) {
        step(&tx, next, up)?;
        tx.execute("UPDATE alembic_version SET version_num = ?1 WHERE version_num = ?2", [*next, CHAIN[i - 1]])
            .map_err(sql_err)?;
    }
    tx.commit().map_err(sql_err)?;
    tracing::info!("upgraded backend.db from revision {rev} to {ALEMBIC_HEAD}");
    Ok(())
}

/// One Alembic revision as plain SQL.
fn step(tx: &rusqlite::Transaction<'_>, rev: &str, up: &Upgrade<'_>) -> Result<()> {
    let now = smhub_model::time::db_now();
    let sql = |s: &str| tx.execute_batch(s).map_err(sql_err);
    match rev {
        "65450f9768ca" => sql(
            r#"CREATE TABLE pages (id INTEGER NOT NULL, "key" VARCHAR(32) NOT NULL, name VARCHAR(32) NOT NULL, description VARCHAR(1024), created_at DATETIME NOT NULL, updated_at DATETIME, PRIMARY KEY (id), UNIQUE ("key"));
CREATE TABLE devicesettings (id INTEGER NOT NULL, "key" VARCHAR(255) NOT NULL, value VARCHAR(1024) NOT NULL, updated_at DATETIME, page_id INTEGER NOT NULL, PRIMARY KEY (id), FOREIGN KEY(page_id) REFERENCES pages (id), CONSTRAINT uix_dev_id_key UNIQUE (page_id, "key"));
CREATE INDEX ix_devicesettings_updated_at ON devicesettings (updated_at);"#,
        ),
        "ac49055b2b34" => {
            let app: Option<i64> = tx.query_row("SELECT id FROM apps WHERE key = 'zigbee2mqtt'", [], |r| r.get(0)).ok();
            if let Some(app) = app {
                let run = |q: &str, v: &dyn rusqlite::ToSql| tx.execute(q, rusqlite::params![v, app]).map_err(sql_err);
                run("UPDATE appsettings SET value = ?1 WHERE app_id = ?2 AND key = 'serial_port' AND value = '/dev/ttyS4'", &"/dev/ttyS1")?;
                run("UPDATE appsettings SET value = ?1 WHERE app_id = ?2 AND key = 'zigbee_channel' AND updated_at IS NULL", &"11")?;
                run("UPDATE appsettings SET value = ?1 WHERE app_id = ?2 AND key = 'network_key' AND updated_at IS NULL", &"")?;
                run("UPDATE appsettings SET value = ?1 WHERE app_id = ?2 AND key = 'adapter' AND updated_at IS NULL", &up.zigbee_adapter)?;
            }
            Ok(())
        }
        "e1ad05962702" => sql("ALTER TABLE apps ADD COLUMN watchdog BOOLEAN;"),
        "826b9fba7761" => sql(
            "CREATE TABLE fileitem (name VARCHAR(255) NOT NULL, is_directory BOOLEAN NOT NULL, path VARCHAR(1024) NOT NULL, parent_id VARCHAR, size INTEGER, mime_type VARCHAR(128), id VARCHAR NOT NULL, created_at DATETIME NOT NULL, updated_at DATETIME, PRIMARY KEY (id), FOREIGN KEY(parent_id) REFERENCES fileitem (id));",
        ),
        "b3f1c2d4e5a6" => sql(
            "ALTER TABLE apps ADD COLUMN installed BOOLEAN DEFAULT 0 NOT NULL;
ALTER TABLE apps ADD COLUMN iframe_port INTEGER;
ALTER TABLE apps ADD COLUMN icon VARCHAR(64);",
        ),
        "0958a1a2873c" => sql(
            "ALTER TABLE fileitem ADD COLUMN synced_at DATETIME;
ALTER TABLE fileitem ADD COLUMN owner VARCHAR(128) DEFAULT 'smlight' NOT NULL;",
        ),
        "71b1eaf89bfe" => sql("ALTER TABLE apps ADD COLUMN is_service BOOLEAN DEFAULT '0' NOT NULL;"),
        "7e4b48ebfc95" => sql(
            "CREATE TABLE configsource (id INTEGER NOT NULL, name VARCHAR(64) NOT NULL, provider VARCHAR(32) NOT NULL, path VARCHAR(255) NOT NULL, params JSON, app_id INTEGER, PRIMARY KEY (id), FOREIGN KEY(app_id) REFERENCES apps (id));
CREATE UNIQUE INDEX ix_configsource_name ON configsource (name);
CREATE TABLE configsetting (id INTEGER NOT NULL, name VARCHAR(64) NOT NULL, source_name VARCHAR(64) NOT NULL, path VARCHAR(255) NOT NULL, value VARCHAR(1024), app_id INTEGER, PRIMARY KEY (id), FOREIGN KEY(app_id) REFERENCES apps (id), FOREIGN KEY(source_name) REFERENCES configsource (name) ON DELETE CASCADE);",
        ),
        "f2a4d8c9b1e7" => sql("ALTER TABLE apps ADD COLUMN show_in_sidebar BOOLEAN DEFAULT '1' NOT NULL;"),
        ALEMBIC_HEAD => {
            let page: Option<i64> = tx.query_row("SELECT id FROM pages WHERE key = 'esphome'", [], |r| r.get(0)).ok();
            let page = match page {
                Some(p) => p,
                None => {
                    tx.execute(
                        "INSERT INTO pages (key, name, description, created_at) VALUES ('esphome', 'ESPHome', 'Smart Home Hub RTOS settings', ?1)",
                        [&now],
                    )
                    .map_err(sql_err)?;
                    tx.last_insert_rowid()
                }
            };
            for (k, v) in [
                ("ambient_light", "true"),
                ("buzzer", "true"),
                ("user_i2c", "true"),
                ("bluetooth_proxy_mode", "hci"),
                ("encryption_key", up.rtos_key),
            ] {
                tx.execute(
                    "INSERT INTO devicesettings (key, value, updated_at, page_id) SELECT ?1, ?2, ?3, ?4
                     WHERE NOT EXISTS (SELECT 1 FROM devicesettings WHERE page_id = ?4 AND key = ?1)",
                    rusqlite::params![k, v, now, page],
                )
                .map_err(sql_err)?;
            }
            Ok(())
        }
        // 9375c2db72a0 changed only Python defaults
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: Upgrade<'static> = Upgrade { zigbee_adapter: "ember", rtos_key: "KEY" };

    /// The schema of the first revision, as Alembic created it.
    const FIRST: &str = r#"
CREATE TABLE alembic_version (version_num VARCHAR(32) NOT NULL, CONSTRAINT alembic_version_pkc PRIMARY KEY (version_num));
INSERT INTO alembic_version (version_num) VALUES ('6e89cce31fda');
CREATE TABLE user (id VARCHAR NOT NULL, email VARCHAR(255) NOT NULL, is_active BOOLEAN NOT NULL, is_superuser BOOLEAN NOT NULL, full_name VARCHAR(255), hashed_password VARCHAR NOT NULL, PRIMARY KEY (id));
CREATE UNIQUE INDEX ix_user_email ON user (email);
CREATE TABLE usersettings (id VARCHAR NOT NULL, light_theme BOOLEAN, metric_units BOOLEAN, notifications BOOLEAN, language VARCHAR(8), locale VARCHAR(8), timezone VARCHAR(64), use_12h_clock BOOLEAN, PRIMARY KEY (id), FOREIGN KEY(id) REFERENCES user (id));
CREATE TABLE apps (id INTEGER NOT NULL, "key" VARCHAR(32) NOT NULL, name VARCHAR(32) NOT NULL, version VARCHAR(16) NOT NULL, description VARCHAR(1024), enabled BOOLEAN NOT NULL, start_at_boot BOOLEAN NOT NULL, created_at DATETIME NOT NULL, updated_at DATETIME, PRIMARY KEY (id), UNIQUE ("key"));
CREATE TABLE appsettings (id INTEGER NOT NULL, "key" VARCHAR(255) NOT NULL, value VARCHAR(1024) NOT NULL, updated_at DATETIME, app_id INTEGER NOT NULL, PRIMARY KEY (id), FOREIGN KEY(app_id) REFERENCES apps (id), CONSTRAINT uix_app_id_key UNIQUE (app_id, "key"));
CREATE INDEX ix_appsettings_updated_at ON appsettings (updated_at);
INSERT INTO apps (id, key, name, version, enabled, start_at_boot, created_at) VALUES (1, 'zigbee2mqtt', 'Zigbee2MQTT', '1', 1, 1, '2025-01-01 00:00:00');
INSERT INTO appsettings (key, value, app_id) VALUES ('serial_port', '/dev/ttyS4', 1), ('adapter', 'zstack', 1), ('zigbee_channel', '15', 1);
"#;

    #[test]
    fn upgrades_the_first_revision_to_head() -> Result<()> {
        let mut c = Connection::open_in_memory().map_err(sql_err)?;
        c.execute_batch(FIRST).map_err(sql_err)?;
        ensure(&mut c, &UP)?;
        let rev: String = c.query_row("SELECT version_num FROM alembic_version", [], |r| r.get(0)).map_err(sql_err)?;
        assert_eq!(rev, ALEMBIC_HEAD);
        let port: String = c
            .query_row("SELECT value FROM appsettings WHERE key = 'serial_port'", [], |r| r.get(0))
            .map_err(sql_err)?;
        assert_eq!(port, "/dev/ttyS1");
        let adapter: String =
            c.query_row("SELECT value FROM appsettings WHERE key = 'adapter'", [], |r| r.get(0)).map_err(sql_err)?;
        assert_eq!(adapter, "ember");
        let key: String = c
            .query_row("SELECT value FROM devicesettings WHERE key = 'encryption_key'", [], |r| r.get(0))
            .map_err(sql_err)?;
        assert_eq!(key, "KEY");
        // the columns the later revisions added are there and have their defaults
        let (installed, sidebar): (i64, i64) = c
            .query_row("SELECT installed, show_in_sidebar FROM apps WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(sql_err)?;
        assert_eq!((installed, sidebar), (0, 1));
        // and a second start changes nothing
        ensure(&mut c, &UP)?;
        Ok(())
    }

    #[test]
    fn unknown_revision_is_left_alone() -> Result<()> {
        let mut c = Connection::open_in_memory().map_err(sql_err)?;
        c.execute_batch(&FIRST.replace("'6e89cce31fda'", "'aaaaaaaaaaaa'")).map_err(sql_err)?;
        ensure(&mut c, &UP)?;
        let rev: String = c.query_row("SELECT version_num FROM alembic_version", [], |r| r.get(0)).map_err(sql_err)?;
        assert_eq!(rev, "aaaaaaaaaaaa");
        Ok(())
    }
}
