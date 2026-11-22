//! The Mosquitto broker: `mosquitto.conf` through the line-preserving editor (old `mqtt` page
//! handler, reference §4.8) and the password file in mosquitto's `$7$` format (§2.6).

use serde::Serialize;
use smhub_model::{Error, Result};

use crate::flatconf::FlatConf;
use crate::{Sys, fsx};

pub const CONF: &str = "/etc/mosquitto/mosquitto.conf";
pub const PASSWD: &str = "/var/lib/mosquitto/passwd";

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct Bridge {
    pub enabled: bool,
    pub connection: String,
    pub address: String,
    pub username: String,
    #[serde(skip)]
    pub password: String,
    pub password_set: bool,
    pub topics: Vec<String>,
    pub clean_session: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct Mqtt {
    pub port: u16,
    pub allow_external: bool,
    pub allow_anonymous: bool,
    pub bridge: Bridge,
}

fn first(c: &FlatConf, k: &str) -> Option<String> {
    c.get_active(k).into_iter().next()
}

/// Where the broker listens, if not on every interface.
const LOCAL: &str = "127.0.0.1";

/// The broker's settings as Mosquitto 2 actually applies them.
///
/// - No `listener` line (the OS image's file): Mosquitto runs in "local only mode", listening
///   on the loopback interface and letting clients in without a password. Apps on the hub, such
///   as Zigbee2MQTT as installed, connect that way.
/// - A `listener` line: without an address (or with 0.0.0.0) it listens on every interface;
///   `allow_anonymous` then defaults to false.
///
/// The old backend read "no 0.0.0.0" as "not reachable from the network" and an absent
/// `allow_anonymous` as false, so its first save locked such apps out (and the switch did not
/// change where the broker listened).
pub fn read_mqtt(c: &FlatConf) -> Mqtt {
    let listener = first(c, "listener");
    let (port, allow_external, allow_anonymous) = match &listener {
        None => (1883, false, true),
        Some(l) => {
            let mut parts = l.split_whitespace();
            let port = parts.next().and_then(|p| p.parse().ok()).unwrap_or(1883);
            let bind = parts.next().unwrap_or("");
            let external = matches!(bind, "" | "0.0.0.0" | "::");
            (port, external, first(c, "allow_anonymous").is_some_and(|v| v == "true"))
        }
    };
    let connection = first(c, "connection").unwrap_or_default();
    let password = first(c, "remote_password").unwrap_or_default();
    Mqtt {
        port,
        allow_external,
        allow_anonymous,
        bridge: Bridge {
            enabled: !connection.is_empty(),
            connection,
            address: first(c, "address").unwrap_or_default(),
            username: first(c, "remote_username").unwrap_or_default(),
            password_set: !password.is_empty(),
            password,
            topics: c.get_active("topic"),
            clean_session: first(c, "cleansession").is_none_or(|v| v == "true"),
        },
    }
}

/// The `listener` value for these settings, keeping the form the file already has: a bare
/// port (every interface, as the old backend wrote it) stays bare.
fn listener_value(c: &FlatConf, m: &Mqtt) -> String {
    if !m.allow_external {
        return format!("{} {LOCAL}", m.port);
    }
    let bare = first(c, "listener").is_some_and(|l| l.split_whitespace().nth(1).is_none());
    if bare { m.port.to_string() } else { format!("{} 0.0.0.0", m.port) }
}

impl Sys {
    fn mqtt_conf(&self) -> FlatConf {
        let text = fsx::read_opt(&self.p(CONF)).ok().flatten().unwrap_or_default();
        FlatConf::parse(&text, &["#"])
    }

    /// Settings as the broker applies them (see `read_mqtt`).
    pub fn mqtt(&self) -> Mqtt {
        read_mqtt(&self.mqtt_conf())
    }

    /// Write the broker's file as the old handler did: fixed defaults plus the settings; an empty
    /// value comments a directive out. Returns the old-style page values for the database.
    pub fn set_mqtt(&self, m: &Mqtt) -> Result<Vec<(String, String)>> {
        if m.port == 0 {
            return Err(Error::invalid("port", "use a port from 1 to 65535"));
        }
        let b = &m.bridge;
        if b.enabled {
            if b.connection.trim().is_empty() || b.connection.contains(char::is_whitespace) {
                return Err(Error::invalid("bridge.connection", "give the bridge a name without spaces"));
            }
            if b.address.trim().is_empty() {
                return Err(Error::invalid("bridge.address", "give the remote broker's host:port"));
            }
            if b.topics.is_empty() {
                return Err(Error::invalid("bridge.topics", "bridge at least one topic, e.g. # both 0"));
            }
        }
        let on = |s: &str| if b.enabled { s.to_owned() } else { String::new() };
        let mut c = self.mqtt_conf();
        let listener = listener_value(&c, m);
        // the old page value: the port, with 0.0.0.0 when other devices may connect
        let page_listener = if m.allow_external { format!("{} 0.0.0.0", m.port) } else { m.port.to_string() };
        let set = |c: &mut FlatConf, k: &str, v: &str| c.set(k, &[v.to_owned()]);
        set(&mut c, "persistence_file", "mosquitto.db");
        set(&mut c, "persistence_location", "/var/lib/mosquitto/");
        set(&mut c, "log_dest", "file /var/log/mosquitto.log");
        set(&mut c, "password_file", PASSWD);
        set(&mut c, "listener", &listener);
        set(&mut c, "allow_anonymous", if m.allow_anonymous { "true" } else { "false" });
        set(&mut c, "connection", &on(b.connection.trim()));
        set(&mut c, "address", &on(b.address.trim()));
        set(&mut c, "remote_username", &on(&b.username));
        set(&mut c, "remote_password", &on(&b.password));
        set(&mut c, "cleansession", &on(if b.clean_session { "true" } else { "false" }));
        let topics: Vec<String> = if b.enabled { b.topics.iter().map(|t| t.trim().to_owned()).collect() } else { vec![String::new()] };
        c.set("topic", &topics);
        fsx::write_plain(&self.p(CONF), c.render().as_bytes())?;
        let tf = |v: bool| if v { "True".to_owned() } else { "False".to_owned() };
        Ok(vec![
            ("listener_port".into(), page_listener),
            ("allow_anonymous".into(), tf(m.allow_anonymous)),
            ("enable_bridge_mode".into(), tf(b.enabled)),
            ("remote_connection".into(), on(&b.connection)),
            ("remote_address".into(), on(&b.address)),
            ("remote_username".into(), on(&b.username)),
            ("remote_password".into(), on(&b.password)),
            ("bridge_topic".into(), if b.enabled { serde_json::to_string(&b.topics).unwrap_or_default() } else { String::new() }),
            ("clean_session".into(), if b.enabled { tf(b.clean_session) } else { String::new() }),
        ])
    }

    /// Broker accounts in the password file.
    pub fn mqtt_users(&self) -> Vec<String> {
        fsx::read_opt(&self.p(PASSWD))
            .ok()
            .flatten()
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().split_once(':').map(|(u, _)| u.to_owned()))
            .filter(|u| !u.is_empty())
            .collect()
    }

    fn write_passwd(&self, lines: &[String]) -> Result<()> {
        let mut text = lines.join("\n");
        text.push('\n');
        let path = self.p(PASSWD);
        fsx::write_atomic_private(&path, text.as_bytes(), 0o600)?;
        if let Some((uid, gid, _)) = fsx::user_ids(&self.p("/etc/passwd"), "mosquitto") {
            fsx::chown(&path, uid, gid);
        }
        Ok(())
    }

    /// Add a broker account or replace its password.
    pub fn mqtt_add_user(&self, username: &str, password: &str) -> Result<()> {
        let entry = smhub_auth::mosquitto_entry(username, password)?;
        let mut lines: Vec<String> = fsx::read_opt(&self.p(PASSWD))?
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && l.split_once(':').is_none_or(|(u, _)| u != username))
            .map(str::to_owned)
            .collect();
        lines.push(entry);
        self.write_passwd(&lines)
    }

    pub fn mqtt_delete_user(&self, username: &str) -> Result<()> {
        let text = fsx::read_opt(&self.p(PASSWD))?.unwrap_or_default();
        let before = text.lines().filter(|l| !l.trim().is_empty()).count();
        let lines: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && l.split_once(':').is_none_or(|(u, _)| u != username))
            .map(str::to_owned)
            .collect();
        if lines.len() == before {
            return Err(Error::not_found("broker account"));
        }
        self.write_passwd(&lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The relevant lines of the OS image's file: everything commented out.
    const STOCK: &str = "# Place your local configuration in /etc/mosquitto/conf.d/\n#listener\n#allow_anonymous false\n#password_file\n#connection <name>\n#address <host>\n#topic <topic>\n";

    fn sys_with(name: &str, conf: &str) -> (Sys, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("smhub-mqtt-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("etc/mosquitto")).unwrap_or_default();
        std::fs::write(root.join("etc/mosquitto/mosquitto.conf"), conf).unwrap_or_default();
        (Sys::new(&root, None), root)
    }

    fn active(root: &std::path::Path, key: &str) -> Option<String> {
        let t = std::fs::read_to_string(root.join("etc/mosquitto/mosquitto.conf")).unwrap_or_default();
        t.lines().find_map(|l| l.strip_prefix(&format!("{key} ")).map(str::to_owned))
    }

    #[test]
    fn stock_file_is_local_only_without_password() {
        let m = read_mqtt(&FlatConf::parse(STOCK, &["#"]));
        assert_eq!((m.port, m.allow_external, m.allow_anonymous), (1883, false, true));
    }

    #[test]
    fn saving_the_stock_file_keeps_apps_on_the_hub_working() -> Result<()> {
        let (sys, root) = sys_with("stock-save", STOCK);
        let m = sys.mqtt();
        sys.set_mqtt(&m)?;
        // still loopback only and no password: an app connecting to localhost without one works
        assert_eq!(active(&root, "listener").as_deref(), Some("1883 127.0.0.1"));
        assert_eq!(active(&root, "allow_anonymous").as_deref(), Some("true"));
        assert_eq!(sys.mqtt(), m);
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn a_file_the_old_backend_saved_keeps_its_behaviour() -> Result<()> {
        // the old backend wrote a bare port for "not external": that listens on every interface
        let (sys, root) = sys_with("old-file", "listener 1883\nallow_anonymous false\npassword_file /var/lib/mosquitto/passwd\n");
        let m = sys.mqtt();
        assert_eq!((m.allow_external, m.allow_anonymous), (true, false));
        sys.set_mqtt(&m)?;
        assert_eq!(active(&root, "listener").as_deref(), Some("1883"));
        assert_eq!(active(&root, "allow_anonymous").as_deref(), Some("false"));
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn switches_change_where_and_how() -> Result<()> {
        let (sys, root) = sys_with("switches", STOCK);
        let mut m = sys.mqtt();
        m.allow_external = true;
        m.port = 1884;
        sys.set_mqtt(&m)?;
        assert_eq!(active(&root, "listener").as_deref(), Some("1884 0.0.0.0"));
        m.allow_external = false;
        m.allow_anonymous = false;
        sys.set_mqtt(&m)?;
        assert_eq!(active(&root, "listener").as_deref(), Some("1884 127.0.0.1"));
        assert_eq!(active(&root, "allow_anonymous").as_deref(), Some("false"));
        assert_eq!(sys.mqtt(), m);
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }
}
