//! Networking through NetworkManager's `nmcli` (the plan's fallback to D-Bus in D24, chosen for
//! maturity and zero idle memory). Connection names follow the old backend: Ethernet `eth0`,
//! Wi-Fi named after the SSID, WireGuard `wg0`; existing connections are modified, never
//! duplicated.

use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::Serialize;
use smhub_model::{Error, Result};

use crate::{Sys, fsx};

const NM_TIMEOUT: Duration = Duration::from_secs(20);
const UP_TIMEOUT: Duration = Duration::from_secs(60);
pub const WIFI_IF: &str = "wlan0";
pub const ETH_IF: &str = "eth0";
pub const WG_NAME: &str = "wg0";
const CONN_DIR: &str = "/etc/NetworkManager/system-connections";

/// Split one line of `nmcli -t` output on unescaped colons.
pub fn split_terse(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            ':' => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Interface {
    pub name: String,
    pub kind: &'static str,
    pub state: &'static str,
    pub connection: Option<String>,
    pub mac: Option<String>,
    pub addresses: Vec<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub speed_mbps: Option<u32>,
}

fn kind_of(name: &str, nm_type: &str) -> &'static str {
    match nm_type {
        "wifi" => "wifi",
        "wireguard" | "tun" => "vpn",
        "gsm" | "modem" => "lte",
        "ethernet" if name.starts_with("usb") => "usb",
        "ethernet" => "ethernet",
        _ if name.starts_with("wwan") || name.starts_with("ppp") => "lte",
        _ => "other",
    }
}

fn state_of(s: &str) -> &'static str {
    let code: u32 = s.split_whitespace().next().and_then(|c| c.parse().ok()).unwrap_or(0);
    match code {
        100 => "connected",
        40..=90 => "connecting",
        30 => "disconnected",
        20 => "unavailable",
        10 => "unmanaged",
        _ => "unavailable",
    }
}

/// Parse `nmcli -t -f GENERAL.DEVICE,... device show` (blocks separated by blank lines).
pub fn parse_devices(text: &str) -> Vec<Interface> {
    let mut out = Vec::new();
    let mut cur: Option<(Interface, String)> = None;
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.split('[').next().unwrap_or(k);
        match key {
            "GENERAL.DEVICE" => {
                if let Some((i, _)) = cur.take() {
                    out.push(i);
                }
                cur = Some((Interface { name: v.to_owned(), ..Default::default() }, String::new()));
            }
            _ => {
                let Some((i, t)) = cur.as_mut() else { continue };
                match key {
                    "GENERAL.TYPE" => *t = v.to_owned(),
                    "GENERAL.HWADDR" if !v.is_empty() && v != "00:00:00:00:00:00" => i.mac = Some(v.to_owned()),
                    "GENERAL.STATE" => i.state = state_of(v),
                    "GENERAL.CONNECTION" if !v.is_empty() => i.connection = Some(v.to_owned()),
                    "IP4.ADDRESS" | "IP6.ADDRESS" if !v.is_empty() => i.addresses.push(v.to_owned()),
                    "IP4.GATEWAY" if !v.is_empty() => i.gateway = Some(v.to_owned()),
                    "IP4.DNS" | "IP6.DNS" if !v.is_empty() => i.dns.push(v.to_owned()),
                    _ => {}
                }
                i.kind = kind_of(&i.name, t);
            }
        }
    }
    if let Some((i, _)) = cur {
        out.push(i);
    }
    out.retain(|i| i.name != "lo");
    out
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct IpSettings {
    pub method: String,
    pub addresses: Vec<String>,
    pub gateway: Option<String>,
    pub dns: Vec<String>,
    pub mtu: Option<u32>,
    pub autoconnect: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub frequency_mhz: Option<u32>,
    pub saved: bool,
    pub in_use: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SavedWifi {
    pub name: String,
    pub ssid: String,
    pub autoconnect: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CurrentWifi {
    pub ssid: String,
    pub signal: Option<u8>,
    pub security: Option<String>,
    pub frequency_mhz: Option<u32>,
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WifiState {
    pub available: bool,
    pub enabled: bool,
    pub state: &'static str,
    pub current: Option<CurrentWifi>,
    pub saved: Vec<SavedWifi>,
    pub networks: Vec<WifiNetwork>,
}

/// Validate an IPv4 or IPv6 address with an optional prefix.
pub fn valid_cidr(s: &str, need_prefix: bool) -> bool {
    let (addr, prefix) = match s.split_once('/') {
        Some((a, p)) => (a, Some(p)),
        None => (s, None),
    };
    if need_prefix && prefix.is_none() {
        return false;
    }
    let ip: std::result::Result<std::net::IpAddr, _> = addr.parse();
    let Ok(ip) = ip else { return false };
    match prefix {
        None => true,
        Some(p) => p.parse::<u8>().is_ok_and(|n| n <= if ip.is_ipv4() { 32 } else { 128 }),
    }
}

fn yes(b: bool) -> &'static str {
    if b { "yes" } else { "no" }
}

/// WireGuard settings as stored in the `wg0` keyfile.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Wireguard {
    pub addresses: Vec<String>,
    #[serde(skip)]
    pub private_key: String,
    pub private_key_set: bool,
    pub public_key: String,
    pub endpoint: String,
    pub allowed_ips: Vec<String>,
    pub dns: Vec<String>,
    pub keepalive: Option<u32>,
    #[serde(skip)]
    pub preshared_key: String,
    pub preshared_key_set: bool,
    pub autoconnect: bool,
}

fn parse_keyfile(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some(s) = l.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = s.to_owned();
            out.entry(section.clone()).or_default();
        } else if let Some((k, v)) = l.split_once('=') {
            out.entry(section.clone()).or_default().insert(k.trim().to_owned(), v.trim().to_owned());
        }
    }
    out
}

fn list(v: Option<&String>) -> Vec<String> {
    v.map(|s| s.split([';', ',']).map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

impl Sys {
    async fn nmcli(&self, args: &[&str], limit: Duration) -> Result<String> {
        let out = self.exec("nmcli", args, limit).await?;
        if out.ok() {
            Ok(out.stdout)
        } else {
            let msg = out.error_text();
            let msg = msg.strip_prefix("Error: ").unwrap_or(&msg).to_owned();
            Err(Error::Unavailable(msg))
        }
    }

    /// Interfaces with their addresses.
    pub async fn interfaces(&self) -> Vec<Interface> {
        let text = self
            .nmcli(
                &["-t", "-f", "GENERAL.DEVICE,GENERAL.TYPE,GENERAL.HWADDR,GENERAL.STATE,GENERAL.CONNECTION,IP4.ADDRESS,IP4.GATEWAY,IP4.DNS,IP6.ADDRESS", "device", "show"],
                NM_TIMEOUT,
            )
            .await
            .unwrap_or_default();
        let mut list = parse_devices(&text);
        for i in &mut list {
            if i.kind == "ethernet" || i.kind == "usb" {
                i.speed_mbps = fsx::read_trim(&self.p(&format!("/sys/class/net/{}/speed", i.name)))
                    .and_then(|s| s.parse::<i64>().ok())
                    .and_then(|s| u32::try_from(s).ok());
            }
        }
        list
    }

    /// Whether the internet answers: NetworkManager's connectivity state, else a quick TCP
    /// connection to well-known resolvers.
    pub async fn internet(&self) -> Option<bool> {
        if let Ok(s) = self.nmcli(&["networking", "connectivity"], Duration::from_secs(5)).await {
            match s.trim() {
                "full" => return Some(true),
                "none" | "limited" | "portal" => return Some(false),
                _ => {}
            }
        }
        let ok = tokio::task::spawn_blocking(|| {
            ["1.1.1.1:443", "8.8.8.8:53"].iter().any(|a| {
                a.parse::<SocketAddr>().is_ok_and(|sa| TcpStream::connect_timeout(&sa, Duration::from_secs(2)).is_ok())
            })
        })
        .await
        .unwrap_or(false);
        Some(ok)
    }

    async fn connection_exists(&self, name: &str) -> bool {
        self.nmcli(&["-t", "-f", "NAME", "connection", "show"], NM_TIMEOUT)
            .await
            .map(|t| t.lines().any(|l| split_terse(l).first().map(String::as_str) == Some(name)))
            .unwrap_or(false)
    }

    /// Ethernet settings of connection `eth0`.
    pub async fn ethernet(&self) -> Result<IpSettings> {
        if !self.connection_exists(ETH_IF).await {
            return Ok(IpSettings { method: "auto".into(), autoconnect: true, ..Default::default() });
        }
        let text = self
            .nmcli(
                &["-t", "-f", "ipv4.method,ipv4.addresses,ipv4.gateway,ipv4.dns,802-3-ethernet.mtu,connection.autoconnect", "connection", "show", ETH_IF],
                NM_TIMEOUT,
            )
            .await?;
        let mut s = IpSettings::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else { continue };
            let v = v.trim();
            match k {
                "ipv4.method" => s.method = if v == "manual" { "manual".into() } else { "auto".into() },
                "ipv4.addresses" => s.addresses = v.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect(),
                "ipv4.gateway" if !v.is_empty() && v != "--" => s.gateway = Some(v.to_owned()),
                "ipv4.dns" => s.dns = v.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect(),
                "802-3-ethernet.mtu" => s.mtu = v.parse().ok(),
                "connection.autoconnect" => s.autoconnect = v == "yes",
                _ => {}
            }
        }
        Ok(s)
    }

    fn ip_args(s: &IpSettings) -> Result<Vec<String>> {
        let mut a: Vec<String> = Vec::new();
        if s.method == "manual" {
            if s.addresses.is_empty() {
                return Err(Error::invalid("addresses", "a manual configuration needs an address"));
            }
            for x in &s.addresses {
                if !valid_cidr(x, true) {
                    return Err(Error::invalid("addresses", format!("{x} is not an address with a prefix, e.g. 192.168.1.20/24")));
                }
            }
            if let Some(g) = &s.gateway {
                if !g.is_empty() && !valid_cidr(g, false) {
                    return Err(Error::invalid("gateway", "not an IP address"));
                }
            }
            for d in &s.dns {
                if !valid_cidr(d, false) {
                    return Err(Error::invalid("dns", format!("{d} is not an IP address")));
                }
            }
            a.extend(["ipv4.method".into(), "manual".into(), "ipv4.addresses".into(), s.addresses.join(",")]);
            a.extend(["ipv4.gateway".into(), s.gateway.clone().unwrap_or_default()]);
            a.extend(["ipv4.dns".into(), s.dns.join(",")]);
        } else {
            a.extend(["ipv4.method".into(), "auto".into(), "ipv4.addresses".into(), String::new(), "ipv4.gateway".into(), String::new(), "ipv4.dns".into(), String::new()]);
        }
        Ok(a)
    }

    /// Apply Ethernet settings to connection `eth0` and bring it up.
    pub async fn set_ethernet(&self, s: &IpSettings) -> Result<IpSettings> {
        let mut args: Vec<String> = Self::ip_args(s)?;
        if let Some(m) = s.mtu {
            if !(576..=9000).contains(&m) {
                return Err(Error::invalid("mtu", "use a value from 576 to 9000"));
            }
            args.extend(["802-3-ethernet.mtu".into(), m.to_string()]);
        }
        args.extend(["connection.autoconnect".into(), yes(s.autoconnect).into()]);
        let mut cmd: Vec<String> = if self.connection_exists(ETH_IF).await {
            vec!["connection".into(), "modify".into(), ETH_IF.into()]
        } else {
            vec!["connection".into(), "add".into(), "type".into(), "ethernet".into(), "ifname".into(), ETH_IF.into(), "con-name".into(), ETH_IF.into()]
        };
        cmd.extend(args);
        let refs: Vec<&str> = cmd.iter().map(String::as_str).collect();
        self.nmcli(&refs, NM_TIMEOUT).await?;
        self.nmcli(&["connection", "up", ETH_IF], UP_TIMEOUT).await?;
        self.ethernet().await
    }

    /// Wi-Fi state, saved networks and the last scan.
    pub async fn wifi(&self) -> WifiState {
        let ifaces = self.interfaces().await;
        let wlan = ifaces.iter().find(|i| i.name == WIFI_IF);
        let enabled = self.nmcli(&["radio", "wifi"], NM_TIMEOUT).await.is_ok_and(|s| s.trim() == "enabled");
        let saved = self.saved_wifi().await;
        let mut networks: Vec<WifiNetwork> = Vec::new();
        if enabled && wlan.is_some() {
            let text = self
                .nmcli(&["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY,FREQ", "device", "wifi", "list", "ifname", WIFI_IF, "--rescan", "no"], NM_TIMEOUT)
                .await
                .unwrap_or_default();
            for line in text.lines() {
                let f = split_terse(line);
                if f.len() < 5 || f[1].is_empty() {
                    continue;
                }
                let signal = f[2].parse::<u8>().unwrap_or(0);
                let freq = f[4].split_whitespace().next().and_then(|v| v.parse().ok());
                let security = if f[3].trim().is_empty() { "none".to_owned() } else { f[3].replace(' ', "/") };
                let in_use = f[0].trim() == "*";
                match networks.iter_mut().find(|n| n.ssid == f[1]) {
                    Some(n) => {
                        if signal > n.signal || in_use {
                            n.signal = n.signal.max(signal);
                            if in_use {
                                n.in_use = true;
                                n.frequency_mhz = freq;
                            }
                        }
                    }
                    None => networks.push(WifiNetwork {
                        ssid: f[1].clone(),
                        signal,
                        security,
                        frequency_mhz: freq,
                        saved: saved.iter().any(|s| s.ssid == f[1]),
                        in_use,
                    }),
                }
            }
            networks.sort_by(|a, b| b.in_use.cmp(&a.in_use).then(b.signal.cmp(&a.signal)));
        }
        let current = wlan.filter(|w| w.state == "connected").map(|w| {
            let ssid = networks
                .iter()
                .find(|n| n.in_use)
                .map(|n| n.ssid.clone())
                .or_else(|| saved.iter().find(|s| Some(&s.name) == w.connection.as_ref()).map(|s| s.ssid.clone()))
                .or_else(|| w.connection.clone())
                .unwrap_or_default();
            let net = networks.iter().find(|n| n.ssid == ssid);
            CurrentWifi {
                ssid,
                signal: net.map(|n| n.signal),
                security: net.map(|n| n.security.clone()),
                frequency_mhz: net.and_then(|n| n.frequency_mhz),
                addresses: w.addresses.clone(),
            }
        });
        WifiState {
            available: wlan.is_some(),
            enabled,
            state: wlan.map_or("unavailable", |w| match w.state {
                "connected" => "connected",
                "connecting" => "connecting",
                "disconnected" => "disconnected",
                _ => "unavailable",
            }),
            current,
            saved,
            networks,
        }
    }

    async fn saved_wifi(&self) -> Vec<SavedWifi> {
        let text = self.nmcli(&["-t", "-f", "NAME,TYPE,AUTOCONNECT", "connection", "show"], NM_TIMEOUT).await.unwrap_or_default();
        let mut out = Vec::new();
        for line in text.lines() {
            let f = split_terse(line);
            if f.len() >= 3 && f[1] == "802-11-wireless" {
                let ssid = self
                    .nmcli(&["-g", "802-11-wireless.ssid", "connection", "show", &f[0]], NM_TIMEOUT)
                    .await
                    .map(|s| s.trim().replace("\\:", ":"))
                    .unwrap_or_else(|_| f[0].clone());
                out.push(SavedWifi { name: f[0].clone(), ssid, autoconnect: f[2] == "yes" });
            }
        }
        out
    }

    pub async fn set_wifi_radio(&self, on: bool) -> Result<()> {
        self.nmcli(&["radio", "wifi", if on { "on" } else { "off" }], NM_TIMEOUT).await.map(|_| ())
    }

    /// Ask the Wi-Fi interface to scan again.
    pub async fn wifi_rescan(&self) -> Result<()> {
        self.nmcli(&["device", "wifi", "rescan", "ifname", WIFI_IF], NM_TIMEOUT).await.map(|_| ())
    }

    /// Create or update the connection named after the SSID, then connect to it.
    pub async fn wifi_connect(&self, ssid: &str, password: Option<&str>, security: &str, hidden: bool, ip: &IpSettings) -> Result<()> {
        if ssid.is_empty() || ssid.len() > 32 {
            return Err(Error::invalid("ssid", "a network name has 1 to 32 characters"));
        }
        let pw = password.unwrap_or("");
        let open = security == "none" || security == "None";
        if !open && !pw.is_empty() && !(8..=63).contains(&pw.len()) {
            return Err(Error::invalid("password", "a Wi-Fi password has 8 to 63 characters"));
        }
        let mut args: Vec<String> = Vec::new();
        args.extend(["802-11-wireless.ssid".into(), ssid.into(), "802-11-wireless.hidden".into(), yes(hidden).into()]);
        if open {
            args.extend(["remove".into(), "wifi-sec".into()]);
        } else if !pw.is_empty() {
            let key_mgmt = match security {
                "WPA3" => "sae",
                _ => "wpa-psk",
            };
            args.extend(["wifi-sec.key-mgmt".into(), key_mgmt.into(), "wifi-sec.psk".into(), pw.into()]);
        }
        args.extend(Self::ip_args(ip)?);
        args.extend(["connection.autoconnect".into(), "yes".into()]);
        let exists = self.connection_exists(ssid).await;
        let mut cmd: Vec<String> = if exists {
            vec!["connection".into(), "modify".into(), ssid.into()]
        } else {
            vec!["connection".into(), "add".into(), "type".into(), "wifi".into(), "ifname".into(), WIFI_IF.into(), "con-name".into(), ssid.into()]
        };
        if !exists {
            if let Some(i) = args.iter().position(|a| a == "remove") {
                args.drain(i..i + 2);
            }
        }
        cmd.extend(args);
        let refs: Vec<&str> = cmd.iter().map(String::as_str).collect();
        self.nmcli(&refs, NM_TIMEOUT).await?;
        let up = self.nmcli(&["connection", "up", ssid], UP_TIMEOUT).await.map(|_| ()).map_err(wifi_failure);
        if up.is_err() && !exists {
            // a profile made for this attempt would keep retrying in the background (it
            // connects automatically); the old backend left it behind
            let _ = self.nmcli(&["connection", "delete", ssid], NM_TIMEOUT).await;
        }
        up
    }

    pub async fn wifi_disconnect(&self) -> Result<()> {
        self.nmcli(&["device", "disconnect", WIFI_IF], NM_TIMEOUT).await.map(|_| ())
    }

    /// Delete a saved Wi-Fi connection.
    pub async fn wifi_forget(&self, name: &str) -> Result<()> {
        let saved = self.saved_wifi().await;
        if !saved.iter().any(|s| s.name == name) {
            return Err(Error::not_found("saved network"));
        }
        self.nmcli(&["connection", "delete", name], NM_TIMEOUT).await.map(|_| ())
    }

    fn wg_file(&self) -> std::path::PathBuf {
        self.p(&format!("{CONN_DIR}/{WG_NAME}.nmconnection"))
    }

    /// WireGuard settings from the `wg0` keyfile.
    pub fn wireguard(&self) -> Option<Wireguard> {
        let text = fsx::read_opt(&self.wg_file()).ok().flatten()?;
        let kf = parse_keyfile(&text);
        let empty = BTreeMap::new();
        let conn = kf.get("connection").unwrap_or(&empty);
        let wg = kf.get("wireguard").unwrap_or(&empty);
        let ipv4 = kf.get("ipv4").unwrap_or(&empty);
        let peer_section = kf.keys().find(|k| k.starts_with("wireguard-peer."));
        let peer = peer_section.and_then(|k| kf.get(k)).unwrap_or(&empty);
        let mut addresses = Vec::new();
        for (k, v) in ipv4 {
            if k.starts_with("address") {
                addresses.push(v.split(',').next().unwrap_or(v).to_owned());
            }
        }
        let private_key = wg.get("private-key").cloned().unwrap_or_default();
        let preshared_key = peer.get("preshared-key").cloned().unwrap_or_default();
        Some(Wireguard {
            addresses,
            private_key_set: !private_key.is_empty(),
            private_key,
            public_key: peer_section.and_then(|k| k.strip_prefix("wireguard-peer.")).unwrap_or("").to_owned(),
            endpoint: peer.get("endpoint").cloned().unwrap_or_default(),
            allowed_ips: list(peer.get("allowed-ips")),
            dns: list(ipv4.get("dns")),
            keepalive: peer.get("persistent-keepalive").and_then(|v| v.parse().ok()),
            preshared_key_set: !preshared_key.is_empty(),
            preshared_key,
            autoconnect: conn.get("autoconnect").is_none_or(|v| v != "false"),
        })
    }

    /// Write the `wg0` keyfile and load it into NetworkManager.
    pub async fn set_wireguard(&self, w: &Wireguard) -> Result<()> {
        let key_ok = |k: &str| k.len() == 44 && k.ends_with('=') && k.chars().all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c));
        if !key_ok(&w.private_key) {
            return Err(Error::invalid("private_key", "a WireGuard key is 44 characters of base64"));
        }
        if !key_ok(&w.public_key) {
            return Err(Error::invalid("public_key", "a WireGuard key is 44 characters of base64"));
        }
        if !w.preshared_key.is_empty() && !key_ok(&w.preshared_key) {
            return Err(Error::invalid("preshared_key", "a WireGuard key is 44 characters of base64"));
        }
        if w.addresses.is_empty() || w.addresses.iter().any(|a| !valid_cidr(a, true)) {
            return Err(Error::invalid("addresses", "use an address with a prefix, e.g. 10.0.0.2/32"));
        }
        let ep = w.endpoint.rsplit_once(':');
        if ep.is_none_or(|(h, p)| h.is_empty() || p.parse::<u16>().is_err()) {
            return Err(Error::invalid("endpoint", "use host:port, e.g. vpn.example.com:51820"));
        }
        let allowed = if w.allowed_ips.is_empty() { vec!["0.0.0.0/0".to_owned()] } else { w.allowed_ips.clone() };
        if allowed.iter().any(|a| !valid_cidr(a, true)) {
            return Err(Error::invalid("allowed_ips", "use networks with a prefix, e.g. 10.0.0.0/24"));
        }
        if w.dns.iter().any(|d| !valid_cidr(d, false)) {
            return Err(Error::invalid("dns", "not an IP address"));
        }
        let uuid = fsx::read_opt(&self.wg_file())
            .ok()
            .flatten()
            .and_then(|t| parse_keyfile(&t).get("connection").and_then(|c| c.get("uuid").cloned()))
            .map_or_else(smhub_auth::new_uuid, Ok)?;
        let mut t = String::new();
        t.push_str(&format!("[connection]\nid={WG_NAME}\nuuid={uuid}\ntype=wireguard\nautoconnect={}\ninterface-name={WG_NAME}\n\n", w.autoconnect));
        t.push_str(&format!("[wireguard]\nprivate-key={}\n\n", w.private_key));
        t.push_str(&format!("[wireguard-peer.{}]\nendpoint={}\nallowed-ips={};\n", w.public_key, w.endpoint, allowed.join(";")));
        if let Some(k) = w.keepalive.filter(|k| *k > 0) {
            t.push_str(&format!("persistent-keepalive={k}\n"));
        }
        if !w.preshared_key.is_empty() {
            t.push_str(&format!("preshared-key={}\npreshared-key-flags=0\n", w.preshared_key));
        }
        t.push_str("\n[ipv4]\n");
        for (i, a) in w.addresses.iter().enumerate() {
            t.push_str(&format!("address{}={a}\n", i + 1));
        }
        if !w.dns.is_empty() {
            t.push_str(&format!("dns={};\n", w.dns.join(";")));
        }
        t.push_str("method=manual\n\n[ipv6]\naddr-gen-mode=default\nmethod=disabled\n");
        let file = self.wg_file();
        // NetworkManager ignores keyfiles others can read; the private key must never be readable
        fsx::write_atomic_private(&file, t.as_bytes(), 0o600)?;
        let path = file.to_string_lossy().into_owned();
        self.nmcli(&["connection", "load", &path], NM_TIMEOUT).await.map(|_| ())
    }

    pub async fn wireguard_active(&self) -> (bool, &'static str) {
        let ifaces = self.interfaces().await;
        match ifaces.iter().find(|i| i.name == WG_NAME) {
            Some(i) if i.state == "connected" => (true, "connected"),
            Some(i) if i.state == "connecting" => (false, "connecting"),
            _ => (false, "disconnected"),
        }
    }

    pub async fn wireguard_up(&self, up: bool) -> Result<()> {
        if up {
            self.nmcli(&["connection", "up", WG_NAME], UP_TIMEOUT).await.map(|_| ())
        } else {
            self.nmcli(&["connection", "down", WG_NAME], NM_TIMEOUT).await.map(|_| ())
        }
    }

    pub async fn wireguard_delete(&self) -> Result<()> {
        if self.connection_exists(WG_NAME).await {
            self.nmcli(&["connection", "delete", WG_NAME], NM_TIMEOUT).await?;
        }
        let _ = std::fs::remove_file(self.wg_file());
        Ok(())
    }
}

/// The two usual reasons a Wi-Fi join fails, as codes the UI translates; anything else as is.
fn wifi_failure(e: Error) -> Error {
    let m = e.message().to_ascii_lowercase();
    if m.contains("could not be found") || m.contains("no network with ssid") {
        Error::Specific { code: "wifi_not_found", field: Some("ssid"), message: e.message().to_owned() }
    } else if m.contains("secrets were required") || m.contains("(7)") || m.contains("4-way handshake") || m.contains("password") {
        Error::Specific { code: "wifi_wrong_password", field: Some("password"), message: e.message().to_owned() }
    } else {
        e
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn wifi_failures() {
        use super::*;
        let nf = wifi_failure(Error::Unavailable("Connection activation failed: The Wi-Fi network could not be found".into()));
        assert_eq!((nf.code(), nf.field()), ("wifi_not_found", Some("ssid")));
        let pw = wifi_failure(Error::Unavailable("Connection activation failed: (7) Secrets were required, but not provided".into()));
        assert_eq!((pw.code(), pw.field()), ("wifi_wrong_password", Some("password")));
        assert_eq!(wifi_failure(Error::Unavailable("other".into())).code(), "unavailable");
    }

    use super::*;

    #[test]
    fn terse() {
        assert_eq!(split_terse(r" :My\:Net:47:WPA2 WPA3:5320 MHz"), vec![" ", "My:Net", "47", "WPA2 WPA3", "5320 MHz"]);
    }

    #[test]
    fn devices() {
        let d = parse_devices("GENERAL.DEVICE:eth0\nGENERAL.TYPE:ethernet\nGENERAL.STATE:100 (connected)\nGENERAL.CONNECTION:eth0\nIP4.ADDRESS[1]:192.168.1.112/24\nIP4.GATEWAY:192.168.1.1\n\nGENERAL.DEVICE:lo\nGENERAL.TYPE:loopback\n\nGENERAL.DEVICE:wlan0\nGENERAL.TYPE:wifi\nGENERAL.STATE:30 (disconnected)\n");
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].state, "connected");
        assert_eq!(d[0].kind, "ethernet");
        assert_eq!(d[1].kind, "wifi");
        assert_eq!(d[0].addresses, vec!["192.168.1.112/24".to_owned()]);
    }

    #[test]
    fn cidr() {
        assert!(valid_cidr("10.0.0.2/32", true));
        assert!(!valid_cidr("10.0.0.2", true));
        assert!(valid_cidr("fe80::1/64", true));
        assert!(!valid_cidr("10.0.0.300/24", true));
    }

    #[test]
    fn keyfile() {
        let k = parse_keyfile("[connection]\nid=wg0\n[wireguard-peer.ABC=]\nendpoint=h:1\nallowed-ips=0.0.0.0/0;\n");
        assert_eq!(k.get("wireguard-peer.ABC=").and_then(|p| p.get("endpoint")).map(String::as_str), Some("h:1"));
    }
}
