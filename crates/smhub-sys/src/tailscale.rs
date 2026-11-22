//! The Tailscale app's state and log-in, through its own command (old routes `tailscale.py`).

use std::time::Duration;

use serde::Serialize;
use smhub_model::{Error, Result};

use crate::Sys;

const TAILSCALE: &str = "/opt/bin/tailscale";

#[derive(Debug, Clone, Serialize)]
pub struct Tailscale {
    pub running: bool,
    pub state: String,
    pub online: Option<bool>,
    pub hostname: Option<String>,
    pub addresses: Vec<String>,
}

impl Sys {
    pub async fn tailscale(&self) -> Tailscale {
        let out = self.exec(&self.p(TAILSCALE).to_string_lossy(), &["status", "--json"], Duration::from_secs(10)).await;
        let v: Option<serde_json::Value> = out.ok().and_then(|o| serde_json::from_str(&o.stdout).ok());
        match v {
            Some(v) => Tailscale {
                running: true,
                state: v.get("BackendState").and_then(|s| s.as_str()).unwrap_or("Unknown").to_owned(),
                online: v.pointer("/Self/Online").and_then(serde_json::Value::as_bool),
                hostname: v.pointer("/Self/DNSName").and_then(|s| s.as_str()).map(|s| s.trim_end_matches('.').to_owned()),
                addresses: v
                    .get("TailscaleIPs")
                    .and_then(|a| a.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default(),
            },
            None => Tailscale { running: false, state: "Stopped".into(), online: None, hostname: None, addresses: Vec::new() },
        }
    }

    pub async fn tailscale_login(&self, auth_key: &str) -> Result<()> {
        let key = auth_key.trim();
        if !key.starts_with("tskey-") || key.len() > 200 || key.chars().any(char::is_whitespace) {
            return Err(Error::invalid("auth_key", "a Tailscale auth key starts with tskey-"));
        }
        let out = self.exec(&self.p(TAILSCALE).to_string_lossy(), &["up", "--authkey", key], Duration::from_secs(60)).await?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("tailscale up: {}", out.error_text()))) }
    }

    pub async fn tailscale_logout(&self) -> Result<()> {
        let out = self.exec(&self.p(TAILSCALE).to_string_lossy(), &["logout"], Duration::from_secs(30)).await?;
        if out.ok() { Ok(()) } else { Err(Error::Unavailable(format!("tailscale logout: {}", out.error_text()))) }
    }
}
