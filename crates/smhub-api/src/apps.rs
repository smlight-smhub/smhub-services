//! Apps: the store, each app's state and settings form, and the sync between opkg, the
//! database rows the old backend used, and the apps' config files (reference §1.6, §3).

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value, json};
use smhub_model::{Error, Result};
use smhub_store::{AppRow, AppUpdate, ConfigSettingRow, ConfigSourceRow, parse_bool};
use smhub_sys::appconfig::{Setting, Source};
use smhub_sys::packages::{Package, check_package, compare_versions};
use smhub_sys::services::{Snapshot, service_for_package};
use smhub_sys::stunnel::WebApp;

use crate::state::AppState;

/// Packages that are part of the hub itself: shown, never removable from the UI.
pub const SYSTEM_PACKAGES: &[&str] =
    &["smhub-services", "smhub-ui", "smhub-broker", "smhub-os-base", "nodejs", "python3", "libopenssl-abi", "esphome-bin", "uv"];

/// Built-in config sources the old backend registered at every start (its `system_schema.json`).
fn system_registry() -> (Vec<ConfigSourceRow>, Vec<ConfigSettingRow>) {
    let src = |name: &str, provider: &str, path: &str, params: Option<Value>| ConfigSourceRow {
        name: name.into(),
        provider: provider.into(),
        path: path.into(),
        params,
        app_id: None,
    };
    let sources = vec![
        src("mosquitto", "flat", "/etc/mosquitto/mosquitto.conf", Some(json!({"comment_prefixes": ["#"]}))),
        src("hostname", "single", "/etc/hostname", None),
        src("timezone", "single", "/etc/timezone", None),
        src("usb_mode", "single", "/etc/usb_mode", None),
    ];
    let set = |name: &str, source: &str, path: &str, v: &str| ConfigSettingRow {
        name: name.into(),
        source_name: source.into(),
        path: path.into(),
        value: Some(v.into()),
        app_id: None,
    };
    let settings = vec![
        set("hostname", "hostname", "hostname", "smhub"),
        set("timezone", "timezone", "timezone", "UTC"),
        set("usb_mode", "usb_mode", "usb_mode", "device"),
        set("listener_port", "mosquitto", "listener", "1883"),
        set("bind_interface", "mosquitto", "bind_interface", "localhost"),
        set("persistence_file", "mosquitto", "persistence_file", "mosquitto.db"),
        set("persistence_location", "mosquitto", "persistence_location", "/var/lib/mosquitto/"),
        set("log_dest", "mosquitto", "log_dest", "file /var/log/mosquitto.log"),
        set("password_file", "mosquitto", "password_file", "/var/lib/mosquitto/passwd"),
        set("allow_anonymous", "mosquitto", "allow_anonymous", "false"),
        set("remote_connection", "mosquitto", "connection", ""),
        set("remote_address", "mosquitto", "address", ""),
        set("remote_username", "mosquitto", "remote_username", ""),
        set("remote_password", "mosquitto", "remote_password", ""),
        set("clean_session", "mosquitto", "cleansession", "true"),
        set("bridge_topic", "mosquitto", "topic", ""),
    ];
    (sources, settings)
}

/// A default value the way the old startup sync stored it (raw JSON bound to TEXT).
fn default_text(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "1".into() } else { "0".into() },
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// A saved value the way the old settings route stored it (Python `str()`).
fn saved_text(v: &Value) -> String {
    match v {
        Value::Bool(b) => if *b { "True".into() } else { "False".into() },
        Value::String(s) => s.clone(),
        Value::Null => "None".into(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn schema_config(schema: &Value) -> Map<String, Value> {
    schema.get("config").and_then(Value::as_object).cloned().unwrap_or_default()
}

fn int_of(v: Option<&Value>) -> Option<i64> {
    v.and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
}

/// Bring the database rows in line with the installed packages (old startup steps 4.1–4.7).
/// With `only`, just that package. Skipped when nothing changed since the last run.
pub async fn sync(hub: &AppState, only: Option<String>) -> Result<()> {
    let sys = hub.sys.clone();
    let packages = sys.packages();
    let installed = sys.installed_packages();
    let mut schemas: BTreeMap<String, Value> = BTreeMap::new();
    for p in installed.keys() {
        if let Some(s) = sys.package_schema(p) {
            schemas.insert(p.clone(), s);
        }
    }
    let mut autostart: BTreeMap<String, bool> = BTreeMap::new();
    for p in installed.keys() {
        autostart.insert(p.clone(), sys.package_autostart(p));
    }
    let fingerprint = {
        let mut f = String::new();
        for (k, v) in &installed {
            f.push_str(&format!("{k}={v};"));
        }
        for (k, s) in &schemas {
            f.push_str(&format!("{k}:{};", s.to_string().len()));
        }
        f.push_str(&format!("{};{}", packages.len(), hub.cfg.version));
        f
    };
    let model = sys.model().clone();
    let usb_ports = sys.usb_serial_ports();
    let (sys_sources, sys_settings) = system_registry();
    hub.store
        .run(move |db| {
            if only.is_none() && db.meta("apps_fingerprint")?.as_deref() == Some(fingerprint.as_str()) {
                return Ok(());
            }
            // like the old backend: a row for every package in the feed indexes, none for
            // packages that are only installed (OS packages, files installed by hand)
            let all: Vec<(String, Option<String>, String)> =
                packages.iter().map(|p| (p.package.clone(), p.description.clone(), p.latest_version.clone())).collect();
            for (name, desc, ver) in &all {
                if only.as_ref().is_some_and(|o| o != name) {
                    continue;
                }
                let is_inst = installed.contains_key(name);
                let row = match db.app(name)? {
                    Some(r) => {
                        if r.installed != is_inst {
                            db.update_app(name, &AppUpdate { installed: Some(is_inst), ..Default::default() })?;
                        }
                        r
                    }
                    None => db.create_app(name, desc.as_deref(), ver, is_inst)?,
                };
                if !is_inst {
                    continue;
                }
                let mut u = AppUpdate::default();
                if let Some(a) = autostart.get(name) {
                    if *a != row.start_at_boot {
                        u.start_at_boot = Some(*a);
                    }
                }
                let Some(schema) = schemas.get(name) else {
                    db.update_app(name, &u)?;
                    continue;
                };
                if let Some(port) = int_of(schema.pointer("/system/iframe_port/default")) {
                    if row.iframe_port != Some(port) {
                        u.iframe_port = Some(Some(port));
                    }
                }
                if let Some(s) = schema.pointer("/system/service/default").and_then(Value::as_bool) {
                    if s != row.is_service {
                        u.is_service = Some(s);
                    }
                }
                db.update_app(name, &u)?;
                let config = schema_config(schema);
                let defaults: Vec<(String, String)> = config
                    .iter()
                    .filter(|(_, v)| v.get("toggle").is_none() || v.get("default").is_some() || v.get("type").and_then(Value::as_str) != Some("separator"))
                    .map(|(k, v)| (k.clone(), v.get("default").map(default_text).unwrap_or_default()))
                    .collect();
                db.add_missing_app_settings(row.id, &defaults)?;
                if let Some(srcs) = schema.pointer("/system/config_sources").and_then(Value::as_array) {
                    for s in srcs {
                        let Some(n) = s.get("name").and_then(Value::as_str) else { continue };
                        db.upsert_config_source(&ConfigSourceRow {
                            name: n.to_owned(),
                            provider: s.get("provider").and_then(Value::as_str).unwrap_or("json").to_owned(),
                            path: s.get("path").and_then(Value::as_str).unwrap_or("").to_owned(),
                            params: s.get("params").cloned().filter(|p| !p.is_null()),
                            app_id: Some(row.id),
                        })?;
                    }
                }
                for (k, v) in &config {
                    let source = v.get("source").and_then(Value::as_str).unwrap_or(name).to_owned();
                    if let Some(path) = v.get("path").and_then(Value::as_str) {
                        let value = v.get("value").or_else(|| v.get("default")).map(default_text);
                        db.upsert_config_setting(&ConfigSettingRow { name: k.clone(), source_name: source.clone(), path: path.into(), value, app_id: Some(row.id) })?;
                    }
                    if let Some(t) = v.get("toggle").filter(|t| t.is_object()) {
                        if let (Some(field), Some(path)) = (t.get("field").and_then(Value::as_str), t.get("path").and_then(Value::as_str)) {
                            let value = v.get("value").or_else(|| v.get("default")).or_else(|| t.get("default")).map(default_text);
                            db.upsert_config_setting(&ConfigSettingRow { name: field.into(), source_name: source.clone(), path: path.into(), value, app_id: Some(row.id) })?;
                        }
                    }
                }
                if (name == "zigbee2mqtt" || name == "openthread") && model.model.starts_with("nano") {
                    let s = db.app_settings(row.id)?;
                    if let Some(port) = s.get("serial_port") {
                        let only_s1 = !model.model.starts_with("nanomr");
                        let bad = if port.starts_with("/dev/ttyS") {
                            if only_s1 { port != "/dev/ttyS1" } else { port != "/dev/ttyS1" && port != "/dev/ttyS2" }
                        } else {
                            !(port.starts_with("/dev/ttyUSB") || port.starts_with("/dev/serial/by-id/"))
                        };
                        if bad {
                            db.save_app_settings(row.id, &[("serial_port".into(), "/dev/ttyS1".into())])?;
                        }
                    }
                }
                let _ = &usb_ports;
            }
            if only.is_none() {
                for s in &sys_sources {
                    db.upsert_config_source(s)?;
                }
                for s in &sys_settings {
                    db.upsert_config_setting(s)?;
                }
                db.set_meta("apps_fingerprint", &fingerprint)?;
            }
            Ok(())
        })
        .await
}

/// Bring stunnel's entries in line with the apps.
pub async fn sync_stunnel(hub: &AppState) -> Result<bool> {
    let rows = hub.store.run(|db| db.apps()).await?;
    let apps: Vec<WebApp> = rows
        .iter()
        .filter_map(|r| {
            let port = u32::try_from(r.iframe_port?).ok()?;
            Some(WebApp { key: r.key.clone(), port, installed: r.installed })
        })
        .collect();
    hub.sys.sync_stunnel(&apps).await
}

/// Data each listing needs once.
struct Ctx {
    packages: Vec<Package>,
    rows: BTreeMap<String, AppRow>,
    services: Snapshot,
    https: BTreeMap<String, u32>,
    os_version: String,
}

async fn ctx(hub: &AppState) -> Result<Ctx> {
    let rows = hub.store.run(|db| db.apps()).await?;
    Ok(Ctx {
        packages: hub.sys.packages(),
        rows: rows.into_iter().map(|r| (r.key.clone(), r)).collect(),
        services: hub.sys.services().await,
        https: hub.sys.https_ports(),
        os_version: hub.sys.os_version(),
    })
}

fn web_of(c: &Ctx, name: &str, schema: Option<&Value>) -> Value {
    let port = c
        .rows
        .get(name)
        .and_then(|r| r.iframe_port)
        .or_else(|| schema.and_then(|s| int_of(s.pointer("/system/iframe_port/default"))))
        .filter(|p| *p > 0);
    match port {
        Some(p) => json!({ "port": p, "https_port": c.https.get(name) }),
        None => Value::Null,
    }
}

fn summary(hub: &AppState, c: &Ctx, p: &Package) -> Value {
    let schema = if p.version.is_some() { hub.sys.package_schema(&p.package) } else { None };
    let row = c.rows.get(&p.package);
    let is_service = row.map(|r| r.is_service).unwrap_or(false)
        || schema.as_ref().and_then(|s| s.pointer("/system/service/default")).and_then(Value::as_bool).unwrap_or(false);
    let svc = service_for_package(schema.as_ref(), &p.package);
    let state = if p.version.is_some() && is_service {
        Some(c.services.get(&svc).map_or("stopped", |s| s.as_str()))
    } else {
        None
    };
    let update = p.version.as_deref().is_some_and(|v| compare_versions(&p.latest_version, v) > 0);
    let compatible = p.required_os.as_deref().is_none_or(|r| smhub_sys::updates::os_version_cmp(&c.os_version, r) != std::cmp::Ordering::Less);
    let icon = (p.version.is_some() && hub.sys.package_icon(&p.package, "icon").is_some()).then(|| format!("/api/v2/apps/{}/icon", p.package));
    json!({
        "name": p.package,
        "description": p.description,
        "section": if p.feed == "community" { Some("community".to_owned()) } else { p.section.clone() },
        "installed": p.version.is_some(),
        "installed_version": p.version,
        "latest_version": p.latest_version,
        "update_available": update,
        "state": state,
        "is_service": is_service,
        "autostart": p.version.is_some() && hub.sys.package_autostart(&p.package),
        "sidebar": row.map(|r| r.show_in_sidebar).unwrap_or(true),
        "web": web_of(c, &p.package, schema.as_ref()),
        "icon": icon,
        "homepage": p.homepage,
        "size": p.size,
        "required_os": p.required_os,
        "compatible": compatible,
        "system": SYSTEM_PACKAGES.contains(&p.package.as_str()),
    })
}

/// The `apps` resource.
pub async fn list(hub: &AppState) -> Result<Value> {
    let c = ctx(hub).await?;
    let mut apps: Vec<Value> = c.packages.iter().map(|p| summary(hub, &c, p)).collect();
    let installed = hub.sys.installed_packages();
    for (name, ver) in &installed {
        if !c.packages.iter().any(|p| &p.package == name) {
            let p = Package {
                package: name.clone(),
                latest_version: ver.clone(),
                version: Some(ver.clone()),
                description: None,
                depends: None,
                size: None,
                required_os: None,
                section: None,
                homepage: None,
                feed: String::new(),
            };
            apps.push(summary(hub, &c, &p));
        }
    }
    let updates = apps.iter().filter(|a| a["update_available"].as_bool() == Some(true)).count();
    Ok(json!({
        "apps": apps,
        "channel": hub.sys.app_channel(),
        "updates": updates,
        "refreshed_at": hub.sys.lists_refreshed().map(smhub_model::time::rfc3339_system),
    }))
}

fn field_type(t: &str) -> &'static str {
    match t {
        "boolean" | "bool" => "boolean",
        "integer" | "int" => "integer",
        "number" | "float" => "number",
        "enum" | "select" => "enum",
        "password" | "secret" => "password",
        "separator" => "separator",
        "text-list" | "list" | "array" | "address-list" => "list",
        _ => "text",
    }
}

fn options_of(v: &Value) -> Vec<Value> {
    let raw = v.get("options").or_else(|| v.get("enum")).and_then(Value::as_array).cloned().unwrap_or_default();
    raw.into_iter()
        .map(|o| match o {
            Value::Object(m) if m.contains_key("value") => Value::Object(m),
            other => json!({ "value": other }),
        })
        .collect()
}

/// The settings form from a package's schema, in order, with the serial-port list patched in
/// for Zigbee2MQTT and OpenThread (old `get_hotpatched_schema`).
pub fn form(name: &str, schema: &Value, model: &str, usb_ports: &[String]) -> Vec<Value> {
    let mut out = Vec::new();
    for (key, v) in schema_config(schema) {
        let mut t = field_type(v.get("type").and_then(Value::as_str).unwrap_or("text"));
        let mut options = options_of(&v);
        let mut default = v.get("default").cloned();
        if key == "serial_port" && (name == "zigbee2mqtt" || name == "openthread") {
            t = "enum";
            let internal: Vec<&str> = if model.starts_with("nano") && !model.starts_with("nanomr") {
                default = Some(json!("/dev/ttyS1"));
                vec!["/dev/ttyS1"]
            } else {
                vec!["/dev/ttyS1", "/dev/ttyS2"]
            };
            let mut seen = BTreeSet::new();
            options = internal
                .into_iter()
                .map(str::to_owned)
                .chain(usb_ports.iter().cloned())
                .filter(|p| seen.insert(p.clone()))
                .map(|p| json!({ "value": p }))
                .collect();
        }
        let mut f = json!({ "key": key, "type": t });
        let obj = f.as_object_mut().map(std::mem::take).unwrap_or_default();
        let mut obj = obj;
        if let Some(d) = default {
            obj.insert("default".into(), d);
        }
        if !options.is_empty() {
            obj.insert("options".into(), Value::Array(options));
        }
        for k in ["min", "max", "pattern", "required", "label"] {
            if let Some(x) = v.get(k) {
                obj.insert(k.into(), x.clone());
            }
        }
        if let Some(d) = v.get("data_description").or_else(|| v.get("description")) {
            obj.insert("description".into(), d.clone());
        }
        if let Some(adv) = v.get("advanced").and_then(Value::as_bool) {
            obj.insert("advanced".into(), Value::Bool(adv));
        }
        // `read-only` in the package schema (app-builder `docs/schema-json.md`); the old UI
        // disabled such fields, for example Z-Wave JS UI's security keys.
        if v.get("read-only").and_then(Value::as_bool) == Some(true) {
            obj.insert("read_only".into(), Value::Bool(true));
        }
        if let Some(tg) = v.get("toggle").filter(|t| t.is_object()) {
            obj.insert("toggle".into(), json!({ "field": tg.get("field"), "default": tg.get("default").and_then(Value::as_bool).unwrap_or(false) }));
        }
        f = Value::Object(obj);
        out.push(f);
    }
    out
}

fn parse_text_list(s: &str) -> Value {
    let t = s.trim();
    if t.is_empty() {
        return json!([]);
    }
    if let Ok(v @ Value::Array(_)) = serde_json::from_str::<Value>(t) {
        return v;
    }
    if t.starts_with('[') {
        let fixed = t.replace('\'', "\"");
        if let Ok(v @ Value::Array(_)) = serde_json::from_str::<Value>(&fixed) {
            return v;
        }
    }
    json!([t])
}

/// A stored text value as the form's type.
fn typed(t: &str, s: &str, options: Option<&Value>) -> Value {
    match t {
        "boolean" => Value::Bool(parse_bool(s)),
        "integer" => s.trim().parse::<i64>().map(Value::from).unwrap_or(Value::Null),
        "number" => s.trim().parse::<f64>().map(Value::from).unwrap_or(Value::Null),
        "list" => parse_text_list(s),
        "enum" => {
            let numeric = options.and_then(Value::as_array).is_some_and(|o| o.iter().all(|x| x["value"].is_number()));
            if numeric { s.trim().parse::<f64>().map(|n| if n.fract() == 0.0 { Value::from(n as i64) } else { Value::from(n) }).unwrap_or(Value::Null) } else { Value::String(s.to_owned()) }
        }
        _ => if s == "None" { Value::String(String::new()) } else { Value::String(s.to_owned()) },
    }
}

/// Coerce a JSON value from the UI to the form's type, or explain why not.
fn coerce(field: &Value, v: &Value) -> Result<Value> {
    let key = field["key"].as_str().unwrap_or("");
    let t = field["type"].as_str().unwrap_or("text");
    let bad = |m: &str| Error::invalid(key, m.to_owned());
    let num = |v: &Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()));
    let out = match t {
        "boolean" => Value::Bool(v.as_bool().or_else(|| v.as_str().map(parse_bool)).ok_or_else(|| bad("must be on or off"))?),
        "integer" | "number" => {
            let n = num(v).ok_or_else(|| bad("must be a number"))?;
            if let Some(min) = field.get("min").and_then(Value::as_f64) {
                if n < min {
                    return Err(bad(&format!("must be at least {min}")));
                }
            }
            if let Some(max) = field.get("max").and_then(Value::as_f64) {
                if n > max {
                    return Err(bad(&format!("must be at most {max}")));
                }
            }
            if t == "integer" {
                if n.fract() != 0.0 {
                    return Err(bad("must be a whole number"));
                }
                Value::from(n as i64)
            } else {
                Value::from(n)
            }
        }
        "enum" => {
            let opts = field.get("options").and_then(Value::as_array).cloned().unwrap_or_default();
            let numeric = !opts.is_empty() && opts.iter().all(|o| o["value"].is_number());
            let val = if numeric { Value::from(num(v).ok_or_else(|| bad("must be one of the choices"))?) } else { Value::String(v.as_str().map_or_else(|| v.to_string(), str::to_owned)) };
            let allowed_extra = key == "serial_port";
            let matches = |o: &Value| o["value"] == val || (numeric && o["value"].as_f64() == val.as_f64());
            if !opts.is_empty() && !allowed_extra && !opts.iter().any(matches) {
                return Err(bad("must be one of the choices"));
            }
            if let Some(f) = val.as_f64().filter(|f| numeric && f.fract() == 0.0) { Value::from(f as i64) } else { val }
        }
        "list" => match v {
            Value::Array(a) => Value::Array(a.iter().map(|x| Value::String(x.as_str().map_or_else(|| x.to_string(), str::to_owned))).collect()),
            Value::String(s) if s.is_empty() => json!([]),
            Value::String(s) => json!([s]),
            _ => return Err(bad("must be a list")),
        },
        _ => {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            if s.len() > 1024 || s.contains(['\n', '\r']) && t == "password" {
                return Err(bad("is too long or has line breaks"));
            }
            Value::String(s)
        }
    };
    Ok(out)
}

async fn registry_for(hub: &AppState, name: &str, app_id: i64) -> Result<Vec<(Source, Vec<Setting>)>> {
    let n = name.to_owned();
    let (sources, settings) = hub.store.run(|db| Ok((db.config_sources()?, db.config_settings()?))).await?;
    let mut out = Vec::new();
    for s in sources.into_iter().filter(|s| s.app_id == Some(app_id) || s.name == n) {
        let sets: Vec<Setting> =
            settings.iter().filter(|x| x.source_name == s.name).map(|x| Setting { name: x.name.clone(), path: x.path.clone() }).collect();
        out.push((Source { name: s.name, provider: s.provider, path: s.path, params: s.params }, sets));
    }
    Ok(out)
}

/// One app with its form and current values.
pub async fn detail(hub: &AppState, name: &str) -> Result<Value> {
    check_package(name)?;
    let c = ctx(hub).await?;
    let pkg = c
        .packages
        .iter()
        .find(|p| p.package == name)
        .cloned()
        .or_else(|| {
            hub.sys.installed_packages().get(name).map(|v| Package {
                package: name.to_owned(),
                latest_version: v.clone(),
                version: Some(v.clone()),
                description: None,
                depends: None,
                size: None,
                required_os: None,
                section: None,
                homepage: None,
                feed: String::new(),
            })
        })
        .ok_or_else(|| Error::not_found(format!("app {name}")))?;
    let mut s = summary(hub, &c, &pkg);
    let schema = hub.sys.package_schema(name);
    let form = schema.as_ref().map(|sc| form(name, sc, &hub.sys.model().model, &hub.sys.usb_serial_ports())).unwrap_or_default();
    let mut values = Map::new();
    let mut secrets = Vec::new();
    if let (Some(row), true) = (c.rows.get(name), pkg.version.is_some()) {
        let id = row.id;
        let stored = hub.store.run(move |db| db.app_settings(id)).await?;
        let mut from_files: BTreeMap<String, Value> = BTreeMap::new();
        for (src, sets) in registry_for(hub, name, row.id).await? {
            from_files.extend(hub.sys.read_config(&src, &sets));
        }
        for f in &form {
            let key = f["key"].as_str().unwrap_or("");
            let t = f["type"].as_str().unwrap_or("text");
            if t == "separator" {
                if let Some(tf) = f.pointer("/toggle/field").and_then(Value::as_str) {
                    let v = from_files.get(tf).map(|v| Value::Bool(v.as_bool().unwrap_or_else(|| v.as_str().is_some_and(parse_bool))))
                        .or_else(|| stored.get(tf).map(|s| Value::Bool(parse_bool(s))))
                        .unwrap_or_else(|| f.pointer("/toggle/default").cloned().unwrap_or(Value::Bool(false)));
                    values.insert(tf.to_owned(), v);
                }
                continue;
            }
            let file_v = from_files.get(key).map(|v| match v {
                Value::String(s) => typed(t, s, f.get("options")),
                other if t == "list" && !other.is_array() => json!([other]),
                other => other.clone(),
            });
            let v = file_v
                .or_else(|| stored.get(key).map(|s| typed(t, s, f.get("options"))))
                .or_else(|| f.get("default").cloned())
                .unwrap_or(Value::Null);
            if t == "password" {
                if v.as_str().is_some_and(|s| !s.is_empty()) {
                    secrets.push(Value::String(key.to_owned()));
                }
                continue;
            }
            values.insert(key.to_owned(), v);
        }
    }
    let data_dirs = schema.as_ref().map(smhub_sys::backups::data_dirs).unwrap_or_default();
    let watchdog = schema.as_ref().and_then(|sc| sc.pointer("/system/watchdog")).map(|_| hub.sys.package_flag(name, "watchdog"));
    let can_reset = schema.as_ref().is_some_and(|sc| sc.pointer("/system/config_reset/default").and_then(Value::as_bool).unwrap_or(false) || sc.pointer("/system/data_dir").is_some());
    let mut extras = Vec::new();
    if name == "tailscale" {
        extras.push("tailscale");
    }
    if name == "openthread" {
        extras.push("thread");
    }
    if let Some(o) = s.as_object_mut() {
        o.insert("settings".into(), Value::Object(values));
        o.insert("secrets_set".into(), Value::Array(secrets));
        o.insert("form".into(), Value::Array(form));
        o.insert("data_dirs".into(), json!(data_dirs));
        o.insert("watchdog".into(), json!(watchdog));
        o.insert("can_reset".into(), Value::Bool(can_reset && pkg.version.is_some()));
        o.insert("has_changelog".into(), Value::Bool(hub.sys.package_changelog(name).is_some()));
        o.insert("log".into(), json!(pkg.version.as_ref().map(|_| smhub_sys::logs::log_for_package(name))));
        o.insert("extras".into(), json!(extras));
    }
    Ok(s)
}

/// Save settings: validate everything first, then the database (old text forms) and the app's
/// config files (typed values; an empty text removes the key).
pub async fn save_settings(hub: &AppState, name: &str, body: Map<String, Value>) -> Result<Value> {
    check_package(name)?;
    let schema = hub.sys.package_schema(name).ok_or_else(|| Error::not_found("this app has no settings"))?;
    let row = {
        let n = name.to_owned();
        hub.store.run(move |db| db.app(&n)).await?.filter(|r| r.installed).ok_or_else(|| Error::not_found("the app is not installed"))?
    };
    let fields = form(name, &schema, &hub.sys.model().model, &hub.sys.usb_serial_ports());
    let mut by_key: BTreeMap<String, Value> = BTreeMap::new();
    for f in &fields {
        if let Some(k) = f["key"].as_str() {
            by_key.insert(k.to_owned(), f.clone());
        }
        if let Some(tf) = f.pointer("/toggle/field").and_then(Value::as_str) {
            by_key.insert(tf.to_owned(), json!({ "key": tf, "type": "boolean", "read_only": f["read_only"] == true }));
        }
    }
    let mut typed_values: Vec<(String, Value)> = Vec::new();
    for (k, v) in &body {
        let f = by_key.get(k).ok_or_else(|| Error::invalid(k, "this app has no such setting"))?;
        // The app owns these values (Z-Wave network keys among them): the hub never writes them.
        if f["read_only"] == true {
            return Err(Error::invalid(k, "this setting is managed by the app and can't be changed here"));
        }
        if f["type"] == "separator" {
            continue;
        }
        if f["type"] == "password" && (v.is_null() || v.as_str() == Some("")) {
            continue;
        }
        typed_values.push((k.clone(), coerce(f, v)?));
    }
    let db_values: Vec<(String, String)> = typed_values
        .iter()
        .map(|(k, v)| (k.clone(), if v.is_array() { v.to_string() } else { saved_text(v) }))
        .collect();
    let id = row.id;
    hub.store.run(move |db| db.save_app_settings(id, &db_values)).await?;
    let mut registry = registry_for(hub, name, row.id).await?;
    if registry.is_empty() {
        sync(hub, Some(name.to_owned())).await?;
        registry = registry_for(hub, name, row.id).await?;
    }
    for (src, sets) in &registry {
        let mine: Vec<(String, Value)> = typed_values.iter().filter(|(k, _)| sets.iter().any(|s| &s.name == k)).cloned().collect();
        if !mine.is_empty() {
            hub.sys.write_config(src, sets, &mine)?;
        }
    }
    let svc = service_for_package(Some(&schema), name);
    let running = hub.sys.services().await.get(&svc).is_some_and(|s| *s == smhub_sys::services::State::Started);
    let detail = detail(hub, name).await?;
    Ok(json!({ "settings": detail["settings"], "restart_needed": running }))
}

/// Change autostart, watchdog and sidebar flags (flag files are read by OS scripts).
pub async fn set_flags(hub: &AppState, name: &str, autostart: Option<bool>, watchdog: Option<bool>, sidebar: Option<bool>) -> Result<()> {
    check_package(name)?;
    if let Some(a) = autostart {
        hub.sys.set_package_autostart(name, a).await?;
    }
    if let Some(w) = watchdog {
        hub.sys.set_package_flag(name, "watchdog", w)?;
    }
    let n = name.to_owned();
    hub.store
        .run(move |db| {
            if db.app(&n)?.is_none() {
                return Ok(());
            }
            db.update_app(&n, &AppUpdate { start_at_boot: autostart, watchdog, show_in_sidebar: sidebar, ..Default::default() })
        })
        .await?;
    hub.sys.services_changed();
    Ok(())
}

/// Start, stop or restart the app's service.
pub async fn service(hub: &AppState, name: &str, action: &str) -> Result<()> {
    check_package(name)?;
    let schema = hub.sys.package_schema(name);
    let svc = service_for_package(schema.as_ref(), name);
    hub.sys.service_action(&svc, action).await
}

/// Reset an app's configuration (old `reset_config`): its settings rows, its data folder and
/// its post-install script; the service is stopped and started around it if it ran.
pub async fn reset(hub: &AppState, name: &str) -> Result<()> {
    check_package(name)?;
    let schema = hub.sys.package_schema(name).ok_or_else(|| Error::not_found("this app has no settings"))?;
    let svc = service_for_package(Some(&schema), name);
    let was_running = hub.sys.services().await.get(&svc).is_some_and(|s| *s == smhub_sys::services::State::Started);
    if was_running {
        let _ = hub.sys.service_action(&svc, "stop").await;
    }
    let n = name.to_owned();
    let adapter = hub.sys.model().zigbee_adapter().to_owned();
    hub.store
        .run(move |db| {
            if let Some(r) = db.app(&n)? {
                db.delete_app_settings(r.id)?;
                if n == "zigbee2mqtt" {
                    db.save_app_settings(r.id, &[("adapter".into(), adapter)])?;
                }
            }
            Ok(())
        })
        .await?;
    let result = async {
        if let Some(dir) = schema.pointer("/system/data_dir/default").and_then(Value::as_str) {
            let p = hub.sys.p(dir);
            if p.is_dir() {
                std::fs::remove_dir_all(&p)?;
            }
            let post = hub.sys.package_info_file(name, "postinst");
            if post.exists() {
                let out = hub.sys.exec(&post.to_string_lossy(), &[], std::time::Duration::from_secs(300)).await?;
                if !out.ok() {
                    return Err(Error::internal(format!("the app's setup script failed: {}", out.error_text())));
                }
            }
        }
        Ok(())
    }
    .await;
    sync(hub, Some(name.to_owned())).await?;
    if was_running {
        let _ = hub.sys.service_action(&svc, "start").await;
    }
    result
}

/// After opkg changed packages: rows, stunnel, and listeners.
pub async fn after_change(hub: &AppState, name: Option<&str>) {
    if let Err(e) = sync(hub, name.map(str::to_owned)).await {
        tracing::warn!("app sync: {e}");
    }
    if let Err(e) = sync(hub, None).await {
        tracing::warn!("app sync: {e}");
    }
    if let Err(e) = sync_stunnel(hub).await {
        tracing::warn!("stunnel: {e}");
    }
    hub.sys.services_changed();
    crate::hw::refresh(hub).await;
    hub.publish_later(&["apps", "services", "updates", "system"]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_reaches_the_form() {
        let schema = json!({ "config": {
            "enabled": { "type": "boolean", "default": true, "read-only": true, "path": "zwave.enabled" },
            "port": { "type": "string", "default": "/dev/ttyS3", "path": "zwave.port" },
            "S0_Legacy_key": { "type": "string", "default": "", "read-only": true, "path": "zwave.securityKeys.S0_Legacy" }
        }});
        let f = form("zwavejsui", &schema, "essential", &[]);
        let ro = |k: &str| f.iter().find(|x| x["key"] == k).map(|x| x["read_only"] == true);
        assert_eq!(ro("enabled"), Some(true));
        assert_eq!(ro("S0_Legacy_key"), Some(true));
        assert_eq!(ro("port"), Some(false));
    }
}
