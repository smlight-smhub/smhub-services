//! Tables `user` and `usersettings`, as the old backend uses them.

use rusqlite::{OptionalExtension, Row, params};
use smhub_model::{Error, Result};

use crate::{Db, b, sql_err};

/// The account the old backend used for requests over the Unix socket. It is never shown.
pub const SYSTEM_USER: &str = "system@localhost";
/// Where the factory account's own password hash waits (in `smhubd.db`) while sign-in is on.
const FACTORY_HASH_KEY: &str = "factory_admin_hash";

#[derive(Debug, Clone)]
pub struct UserRow {
    pub id: String,
    pub email: String,
    pub is_active: bool,
    pub is_superuser: bool,
    pub hashed_password: String,
}

/// Columns of `usersettings`. All nullable in the old schema.
#[derive(Debug, Clone, Default)]
pub struct UserSettingsRow {
    pub light_theme: Option<bool>,
    pub metric_units: Option<bool>,
    pub notifications: Option<bool>,
    pub language: Option<String>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub use_12h_clock: Option<bool>,
}

fn user(r: &Row<'_>) -> rusqlite::Result<UserRow> {
    Ok(UserRow {
        id: r.get(0)?,
        email: r.get(1)?,
        is_active: r.get(2)?,
        is_superuser: r.get(3)?,
        hashed_password: r.get(4)?,
    })
}

const USER_COLS: &str = "id, email, is_active, is_superuser, hashed_password";

impl Db {
    pub fn user_by_email(&self, email: &str) -> Result<Option<UserRow>> {
        self.main
            .query_row(&format!("SELECT {USER_COLS} FROM user WHERE email = ?1"), [email], user)
            .optional()
            .map_err(sql_err)
    }

    pub fn user_by_id(&self, id: &str) -> Result<Option<UserRow>> {
        self.main
            .query_row(&format!("SELECT {USER_COLS} FROM user WHERE id = ?1"), [id], user)
            .optional()
            .map_err(sql_err)
    }

    /// Every account except the internal system user.
    pub fn users(&self) -> Result<Vec<UserRow>> {
        let mut st = self
            .main
            .prepare_cached(&format!("SELECT {USER_COLS} FROM user WHERE email != ?1 ORDER BY email"))
            .map_err(sql_err)?;
        let rows = st.query_map([SYSTEM_USER], user).map_err(sql_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql_err)
    }

    /// Create an account and its empty `usersettings` row, as the old `crud.create_user` did.
    pub fn create_user(&mut self, id: &str, email: &str, hash: &str, superuser: bool) -> Result<UserRow> {
        if self.user_by_email(email)?.is_some() {
            return Err(Error::Conflict(format!("an account named {email} exists")));
        }
        let tx = self.main.transaction().map_err(sql_err)?;
        tx.execute(
            "INSERT INTO user (email, is_active, is_superuser, full_name, id, hashed_password) VALUES (?1, 1, ?2, NULL, ?3, ?4)",
            params![email, b(superuser), id, hash],
        )
        .map_err(sql_err)?;
        tx.execute("INSERT OR IGNORE INTO usersettings (id) VALUES (?1)", [id]).map_err(sql_err)?;
        tx.commit().map_err(sql_err)?;
        Ok(UserRow {
            id: id.to_owned(),
            email: email.to_owned(),
            is_active: true,
            is_superuser: superuser,
            hashed_password: hash.to_owned(),
        })
    }

    pub fn set_password_hash(&self, id: &str, hash: &str) -> Result<()> {
        let n = self.main.execute("UPDATE user SET hashed_password = ?1 WHERE id = ?2", [hash, id]).map_err(sql_err)?;
        if n == 0 { Err(Error::not_found("account")) } else { Ok(()) }
    }

    /// Sign-in is being turned on: the factory account `email` (whose factory password is
    /// public) gets `hash`, and the hash it had is kept aside, once, so that `factory_back` can
    /// return it. Nothing happens when the hub has no such account.
    pub fn factory_aside(&self, email: &str, hash: &str) -> Result<()> {
        let Some(u) = self.user_by_email(email)? else { return Ok(()) };
        if self.hub_setting(FACTORY_HASH_KEY)?.is_none() {
            self.set_hub_setting(FACTORY_HASH_KEY, &u.hashed_password)?;
        }
        self.set_password_hash(&u.id, hash)
    }

    /// Sign-in is being turned off: the factory account gets back the hash kept aside, so the
    /// previous UI, which signs itself in with it on an open hub, works again after going back.
    pub fn factory_back(&self, email: &str) -> Result<()> {
        let Some(h) = self.hub_setting(FACTORY_HASH_KEY)? else { return Ok(()) };
        if let Some(u) = self.user_by_email(email)? {
            self.set_password_hash(&u.id, &h)?;
        }
        self.delete_hub_setting(FACTORY_HASH_KEY)
    }

    pub fn delete_user(&mut self, id: &str) -> Result<()> {
        let tx = self.main.transaction().map_err(sql_err)?;
        tx.execute("DELETE FROM usersettings WHERE id = ?1", [id]).map_err(sql_err)?;
        let n = tx.execute("DELETE FROM user WHERE id = ?1", [id]).map_err(sql_err)?;
        tx.commit().map_err(sql_err)?;
        if n == 0 { Err(Error::not_found("account")) } else { Ok(()) }
    }

    pub fn user_settings(&self, id: &str) -> Result<UserSettingsRow> {
        let row = self
            .main
            .query_row(
                "SELECT light_theme, metric_units, notifications, language, locale, timezone, use_12h_clock FROM usersettings WHERE id = ?1",
                [id],
                |r| {
                    Ok(UserSettingsRow {
                        light_theme: r.get(0)?,
                        metric_units: r.get(1)?,
                        notifications: r.get(2)?,
                        language: r.get(3)?,
                        locale: r.get(4)?,
                        timezone: r.get(5)?,
                        use_12h_clock: r.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(sql_err)?;
        Ok(row.unwrap_or_default())
    }

    /// Write `usersettings` for an account, creating the row when missing.
    pub fn save_user_settings(&self, id: &str, s: &UserSettingsRow) -> Result<()> {
        self.main
            .execute(
                "INSERT INTO usersettings (id, light_theme, metric_units, notifications, language, locale, timezone, use_12h_clock)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET light_theme = excluded.light_theme, metric_units = excluded.metric_units,
                   notifications = excluded.notifications, language = excluded.language, locale = excluded.locale,
                   timezone = excluded.timezone, use_12h_clock = excluded.use_12h_clock",
                params![
                    id,
                    s.light_theme.map(b),
                    s.metric_units.map(b),
                    s.notifications.map(b),
                    s.language,
                    s.locale,
                    s.timezone,
                    s.use_12h_clock.map(b)
                ],
            )
            .map_err(sql_err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::testutil::memory;

    #[test]
    fn users_round_trip() -> smhub_model::Result<()> {
        let mut db = memory()?;
        db.create_user("u1", "admin@example.com", "$argon2id$x", true)?;
        db.create_user("u2", super::SYSTEM_USER, "$argon2id$y", false)?;
        assert!(db.create_user("u3", "admin@example.com", "h", true).is_err());
        assert_eq!(db.users()?.len(), 1);
        let mut s = db.user_settings("u1")?;
        assert_eq!(s.language, None);
        s.language = Some("uk".into());
        s.light_theme = Some(false);
        db.save_user_settings("u1", &s)?;
        assert_eq!(db.user_settings("u1")?.language.as_deref(), Some("uk"));
        db.set_password_hash("u1", "new")?;
        assert_eq!(db.user_by_id("u1")?.map(|u| u.hashed_password), Some("new".into()));
        db.delete_user("u1")?;
        assert!(db.user_by_email("admin@example.com")?.is_none());
        Ok(())
    }

    #[test]
    fn the_factory_password_is_put_aside_and_back() -> smhub_model::Result<()> {
        let mut db = memory()?;
        let hash = |db: &crate::Db| db.user_by_email("admin@smlight.tech").map(|u| u.map(|u| u.hashed_password));
        db.create_user("f", "admin@smlight.tech", "$factory", true)?;
        db.factory_aside("admin@smlight.tech", "$random1")?;
        assert_eq!(hash(&db)?.as_deref(), Some("$random1"));
        // turned on again: the factory hash kept aside the first time stays the one returned
        db.factory_aside("admin@smlight.tech", "$random2")?;
        db.factory_back("admin@smlight.tech")?;
        assert_eq!(hash(&db)?.as_deref(), Some("$factory"));
        // off again with nothing kept aside: unchanged
        db.factory_back("admin@smlight.tech")?;
        assert_eq!(hash(&db)?.as_deref(), Some("$factory"));
        // a hub without the factory account
        db.factory_aside("nobody@example.com", "$x")?;
        assert_eq!(db.hub_setting("factory_admin_hash")?, None);
        Ok(())
    }
}
