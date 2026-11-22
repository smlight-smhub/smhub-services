//! Writing settings into the config files of apps and the OS, driven by the registry the
//! database keeps (`configsource`, `configsetting`). Providers match the old backend:
//! `yaml`, `json`, `flat` (line-preserving `key value` files) and `single` (one value per file).

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};
use smhub_model::{Error, Result};

use crate::flatconf::FlatConf;
use crate::{Sys, fsx};

/// A config file and how to edit it.
#[derive(Debug, Clone)]
pub struct Source {
    pub name: String,
    pub provider: String,
    pub path: String,
    pub params: Option<Value>,
}

/// A setting name and where it lives in its source.
#[derive(Debug, Clone)]
pub struct Setting {
    pub name: String,
    pub path: String,
}

fn load_tree(provider: &str, path: &Path) -> Result<Value> {
    let Some(text) = fsx::read_opt(path)? else { return Ok(Value::Object(Map::new())) };
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    let v: Value = match provider {
        "yaml" => serde_saphyr::from_str(&text).map_err(|e| Error::internal(format!("YAML in {}: {e}", path.display())))?,
        _ => serde_json::from_str(&text).map_err(|e| Error::internal(format!("JSON in {}: {e}", path.display())))?,
    };
    Ok(if v.is_object() { v } else { Value::Object(Map::new()) })
}

fn get_path<'a>(tree: &'a Value, dotted: &str) -> Option<&'a Value> {
    let mut cur = tree;
    for k in dotted.split('.') {
        cur = cur.get(k)?;
    }
    Some(cur)
}

/// Set a value at a dotted path, creating objects on the way. An empty string removes the key,
/// as the old `ConfigProvider.set` did.
fn set_path(tree: &mut Value, dotted: &str, value: Value) {
    let keys: Vec<&str> = dotted.split('.').collect();
    let Some((last, parents)) = keys.split_last() else { return };
    let mut cur = tree;
    for k in parents {
        if !cur.get(*k).is_some_and(Value::is_object) {
            if let Some(obj) = cur.as_object_mut() {
                obj.insert((*k).to_owned(), Value::Object(Map::new()));
            }
        }
        let Some(next) = cur.get_mut(*k) else { return };
        cur = next;
    }
    if let Some(obj) = cur.as_object_mut() {
        if value.as_str() == Some("") {
            obj.remove(*last);
        } else {
            obj.insert((*last).to_owned(), value);
        }
    }
}

/// YAML text as a tree, `None` when it is not valid YAML (the serial bridge reads its file's
/// entries one by one with it).
pub(crate) fn parse_yaml(text: &str) -> Option<Value> {
    serde_saphyr::from_str(text).ok()
}

pub(crate) fn dump_yaml(tree: &Value, header: Option<&str>) -> Result<String> {
    let body = serde_saphyr::to_string(tree).map_err(|e| Error::internal(format!("YAML: {e}")))?;
    Ok(match header {
        Some(h) => {
            let mut h = h.trim_end().to_owned();
            if body.starts_with("---\n") && h.ends_with("---") {
                h.truncate(h.len() - 3);
                h = h.trim_end().to_owned();
            } else if h.starts_with("%YAML") && !h.ends_with("---") {
                h.push_str("\n---");
            }
            if h.is_empty() { body } else { format!("{h}\n{body}") }
        }
        None => body,
    })
}

/// Text of a value for flat and single files: booleans as `true`/`false`.
fn flat_text(v: &Value) -> Vec<String> {
    match v {
        Value::Null => vec![String::new()],
        Value::Bool(b) => vec![if *b { "true".into() } else { "false".into() }],
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().map(|x| flat_text(x).join(" ")).collect(),
        other => vec![other.to_string()],
    }
}

fn comment_prefixes(params: Option<&Value>) -> Vec<String> {
    match params.and_then(|p| p.get("comment_prefixes")) {
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => vec!["#".into(), ";".into()],
    }
}

impl Sys {
    /// Write values (setting name to value) into one source file.
    pub fn write_config(&self, src: &Source, settings: &[Setting], values: &[(String, Value)]) -> Result<()> {
        let path = self.p(&src.path);
        let by_name: BTreeMap<&str, &str> = settings.iter().map(|s| (s.name.as_str(), s.path.as_str())).collect();
        let by_path: BTreeMap<&str, &str> = settings.iter().map(|s| (s.path.as_str(), s.name.as_str())).collect();
        let resolve = |k: &str| -> Option<String> {
            by_name.get(k).map(|p| (*p).to_owned()).or_else(|| by_path.contains_key(k).then(|| k.to_owned()))
        };
        match src.provider.as_str() {
            "yaml" | "json" => {
                let mut tree = load_tree(&src.provider, &path)?;
                for (k, v) in values {
                    if let Some(p) = resolve(k) {
                        set_path(&mut tree, &p, v.clone());
                    } else {
                        tracing::warn!("setting {k} not found in {}", src.name);
                    }
                }
                let text = if src.provider == "yaml" {
                    let header = src.params.as_ref().and_then(|p| p.get("header")).and_then(Value::as_str);
                    dump_yaml(&tree, header)?
                } else {
                    serde_json::to_string_pretty(&tree)?
                };
                fsx::write_atomic(&path, text.as_bytes())
            }
            "flat" => {
                let text = fsx::read_opt(&path)?.unwrap_or_default();
                let prefixes = comment_prefixes(src.params.as_ref());
                let refs: Vec<&str> = prefixes.iter().map(String::as_str).collect();
                let mut conf = FlatConf::parse(&text, &refs);
                for (k, v) in values {
                    match resolve(k) {
                        Some(p) => match v {
                            Value::String(s) => conf.set_str(&p, s),
                            other => conf.set(&p, &flat_text(other)),
                        },
                        None => tracing::warn!("setting {k} not found in {}", src.name),
                    }
                }
                fsx::write_plain(&path, conf.render().as_bytes())
            }
            "single" => {
                let Some((_, v)) = values.iter().find(|(k, _)| resolve(k).is_some()) else { return Ok(()) };
                let text = flat_text(v).join(" ");
                let body = if text.is_empty() { String::new() } else { format!("{text}\n") };
                fsx::write_plain(&path, body.as_bytes())?;
                if src.path.ends_with("timezone") && !text.is_empty() {
                    self.link_localtime(&text)?;
                }
                Ok(())
            }
            other => Err(Error::NotSupported(format!("config provider {other}"))),
        }
    }

    /// Read the current values of settings from one source file.
    pub fn read_config(&self, src: &Source, settings: &[Setting]) -> BTreeMap<String, Value> {
        let path = self.p(&src.path);
        let mut out = BTreeMap::new();
        match src.provider.as_str() {
            "yaml" | "json" => {
                if let Ok(tree) = load_tree(&src.provider, &path) {
                    for s in settings {
                        if let Some(v) = get_path(&tree, &s.path) {
                            out.insert(s.name.clone(), v.clone());
                        }
                    }
                }
            }
            "flat" => {
                let text = fsx::read_opt(&path).ok().flatten().unwrap_or_default();
                let prefixes = comment_prefixes(src.params.as_ref());
                let refs: Vec<&str> = prefixes.iter().map(String::as_str).collect();
                let conf = FlatConf::parse(&text, &refs);
                for s in settings {
                    let vals = conf.get_active(&s.path);
                    match vals.len() {
                        0 => {}
                        1 => {
                            out.insert(s.name.clone(), Value::String(vals[0].clone()));
                        }
                        _ => {
                            out.insert(s.name.clone(), Value::Array(vals.into_iter().map(Value::String).collect()));
                        }
                    }
                }
            }
            "single" => {
                if let Some(v) = fsx::read_trim(&path) {
                    for s in settings {
                        out.insert(s.name.clone(), Value::String(v.clone()));
                    }
                }
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_paths() -> Result<()> {
        let mut tree: Value = serde_saphyr::from_str("serial:\n  port: /dev/ttyS1\nmqtt:\n  server: mqtt://x\n")
            .map_err(|e| Error::internal(e.to_string()))?;
        set_path(&mut tree, "serial.adapter", Value::String("ember".into()));
        set_path(&mut tree, "mqtt.server", Value::String(String::new()));
        set_path(&mut tree, "advanced.channel", Value::from(15));
        assert_eq!(get_path(&tree, "serial.adapter").and_then(Value::as_str), Some("ember"));
        assert!(get_path(&tree, "mqtt.server").is_none());
        let y = dump_yaml(&tree, Some("# header"))?;
        assert!(y.starts_with("# header\n"));
        assert!(y.contains("channel: 15"));
        Ok(())
    }
}
