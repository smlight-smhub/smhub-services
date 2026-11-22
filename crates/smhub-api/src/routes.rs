//! HTTP handlers of API v2 (contract `api/openapi.yaml`).

use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use smhub_model::Error;
use smhub_model::hardware::RadioMode;
use smhub_model::time::now_unix;
use smhub_store::{bool_str, parse_bool};

use crate::apps;
use crate::error::ApiResult;
use crate::resources;
use crate::state::{AppState, Operation, lock};

fn op_response(op: Option<Operation>) -> Response {
    (StatusCode::ACCEPTED, Json(json!(op))).into_response()
}

// ---------------------------------------------------------------- system

pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

pub async fn system(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(resources::system(&hub).await?))
}

/// The first-run setup was finished or skipped: it is not offered again, on any browser.
pub async fn setup_done(State(hub): State<AppState>) -> ApiResult<StatusCode> {
    hub.store.run(|db| db.set_meta("setup", "done")).await?;
    hub.setup_needed.store(false, std::sync::atomic::Ordering::Relaxed);
    hub.publish_later(&["system"]);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reboot(State(hub): State<AppState>) -> ApiResult<StatusCode> {
    crate::hw::event(&hub, "power");
    let sys = hub.sys.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(800)).await;
        if let Err(e) = sys.reboot().await {
            tracing::error!("reboot: {e}");
        }
    });
    Ok(StatusCode::ACCEPTED)
}

pub async fn shutdown(State(hub): State<AppState>) -> ApiResult<StatusCode> {
    crate::hw::event(&hub, "power");
    let sys = hub.sys.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(800)).await;
        if let Err(e) = sys.poweroff().await {
            tracing::error!("poweroff: {e}");
        }
    });
    Ok(StatusCode::ACCEPTED)
}

pub async fn metrics(State(hub): State<AppState>) -> Json<Value> {
    Json(serde_json::to_value(hub.sys.metrics()).unwrap_or(Value::Null))
}

pub async fn services(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(resources::services(&hub).await?))
}

pub async fn service_action(State(hub): State<AppState>, Path((name, action)): Path<(String, String)>) -> ApiResult<Json<Value>> {
    if name == "smhub-services" {
        return Err(Error::Forbidden("the hub's own service is controlled by the hub".into()).into());
    }
    // Tailscale is an app the user may stop on purpose (from its app page); the system
    // services in the list have no such page.
    if action == "stop" && name != "tailscale" && smhub_sys::services::ACCESS_SERVICES.contains(&name.as_str()) {
        return Err(Error::Forbidden(format!("stopping {name} would cut off access to the hub; restart it instead")).into());
    }
    hub.sys.service_action(&name, &action).await?;
    hub.publish_later(&["services", "apps"]);
    let all = resources::services(&hub).await?;
    let one = all["services"].as_array().and_then(|a| a.iter().find(|s| s["name"] == name.as_str()).cloned());
    Ok(Json(one.unwrap_or_else(|| json!({ "name": name, "state": "unknown", "autostart": false, "kind": "system" }))))
}

pub async fn operations(State(hub): State<AppState>) -> Json<Value> {
    Json(json!({ "operations": hub.operations() }))
}

pub async fn operation(State(hub): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(json!(hub.operation(&id).ok_or_else(|| Error::not_found("operation"))?)))
}

// ---------------------------------------------------------------- apps

pub async fn apps_list(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(apps::list(&hub).await?))
}

pub async fn app_detail(State(hub): State<AppState>, Path(name): Path<String>) -> ApiResult<Json<Value>> {
    Ok(Json(apps::detail(&hub, &name).await?))
}

#[derive(Deserialize)]
pub struct AppFlags {
    autostart: Option<bool>,
    watchdog: Option<bool>,
    sidebar: Option<bool>,
}

pub async fn app_patch(State(hub): State<AppState>, Path(name): Path<String>, Json(b): Json<AppFlags>) -> ApiResult<Json<Value>> {
    apps::set_flags(&hub, &name, b.autostart, b.watchdog, b.sidebar).await?;
    hub.publish_later(&["apps", "services"]);
    Ok(Json(apps::detail(&hub, &name).await?))
}

pub async fn app_settings(State(hub): State<AppState>, Path(name): Path<String>, Json(b): Json<Map<String, Value>>) -> ApiResult<Json<Value>> {
    Ok(Json(apps::save_settings(&hub, &name, b).await?))
}

fn run_opkg(hub: &AppState, kind: &'static str, target: Option<String>, args: Vec<String>) -> ApiResult<Response> {
    let op = hub.start_op(kind, target.clone())?;
    let h2 = Arc::clone(hub);
    tokio::spawn(async move {
        op.progress(None, Some(match kind {
            "apps_refresh" => "updating_lists",
            "app_remove" => "removing",
            _ => "installing",
        }));
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let mut r = h2.sys.opkg(&refs, Duration::from_secs(1800)).await;
        if let Err(e) = &r {
            let tail = h2.sys.opkg_log_tail(4);
            r = Err(Error::internal(format!("{}\n{tail}", e.message())));
        }
        apps::after_change(&h2, target.as_deref()).await;
        op.finish(&r);
    });
    let id = op_id_snapshot(hub, kind);
    Ok(op_response(id))
}

fn op_id_snapshot(hub: &AppState, kind: &str) -> Option<Operation> {
    hub.operations().into_iter().rev().find(|o| o.kind == kind && o.state == "running")
}

pub async fn apps_refresh(State(hub): State<AppState>) -> ApiResult<Response> {
    run_opkg(&hub, "apps_refresh", None, vec!["update".into()])
}

pub async fn apps_upgrade(State(hub): State<AppState>) -> ApiResult<Response> {
    run_opkg(&hub, "apps_upgrade", None, vec!["upgrade".into()])
}

pub async fn app_action(State(hub): State<AppState>, Path((name, action)): Path<(String, String)>) -> ApiResult<Response> {
    smhub_sys::packages::check_package(&name)?;
    let system = apps::SYSTEM_PACKAGES.contains(&name.as_str());
    match action.as_str() {
        "install" => {
            if hub.sys.package(&name).is_none() {
                return Err(Error::not_found(format!("app {name}")).into());
            }
            run_opkg(&hub, "app_install", Some(name.clone()), vec!["install".into(), name])
        }
        "upgrade" => run_opkg(&hub, "app_upgrade", Some(name.clone()), vec!["upgrade".into(), name]),
        "remove" => {
            if system {
                return Err(Error::Forbidden("this package is part of the hub".into()).into());
            }
            run_opkg(&hub, "app_remove", Some(name.clone()), vec!["remove".into(), name])
        }
        "start" | "stop" | "restart" => {
            if name == "smhub-services" {
                return Err(Error::Forbidden("the hub's own service is controlled by the hub".into()).into());
            }
            apps::service(&hub, &name, &action).await?;
            hub.publish_later(&["apps", "services"]);
            Ok(Json(apps::detail(&hub, &name).await?).into_response())
        }
        "reset" => {
            apps::reset(&hub, &name).await?;
            hub.publish_later(&["apps", "services"]);
            Ok(Json(apps::detail(&hub, &name).await?).into_response())
        }
        _ => Err(Error::not_found("action").into()),
    }
}

pub async fn app_changelog(State(hub): State<AppState>, Path(name): Path<String>) -> ApiResult<Response> {
    let text = hub.sys.package_changelog(&name).ok_or_else(|| Error::not_found("changelog"))?;
    Ok(([(header::CONTENT_TYPE, "text/markdown; charset=utf-8")], text).into_response())
}

pub async fn app_icon(State(hub): State<AppState>, Path(name): Path<String>) -> ApiResult<Response> {
    let path = hub.sys.package_icon(&name, "icon").ok_or_else(|| Error::not_found("icon"))?;
    let data = tokio::fs::read(&path).await.map_err(Error::from)?;
    let ct = match path.extension().and_then(|e| e.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    };
    Ok((
        [(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "max-age=3600"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff"), (header::CONTENT_SECURITY_POLICY, "default-src 'none'; style-src 'unsafe-inline'")],
        data,
    )
        .into_response())
}

pub async fn app_tailscale(State(hub): State<AppState>, Path(name): Path<String>) -> ApiResult<Json<Value>> {
    if name != "tailscale" || !hub.sys.installed_packages().contains_key("tailscale") {
        return Err(Error::not_found("Tailscale is not installed").into());
    }
    Ok(Json(serde_json::to_value(hub.sys.tailscale().await).unwrap_or(Value::Null)))
}

#[derive(Deserialize, Default)]
pub struct TsBody {
    auth_key: Option<String>,
}

pub async fn app_tailscale_action(State(hub): State<AppState>, Path((name, action)): Path<(String, String)>, body: Option<Json<TsBody>>) -> ApiResult<Json<Value>> {
    if name != "tailscale" || !hub.sys.installed_packages().contains_key("tailscale") {
        return Err(Error::not_found("Tailscale is not installed").into());
    }
    match action.as_str() {
        "login" => {
            let key = body.and_then(|b| b.0.auth_key).unwrap_or_default();
            if !hub.sys.services().await.get("tailscale").is_some_and(|s| *s == smhub_sys::services::State::Started) {
                let _ = hub.sys.service_action("tailscale", "start").await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            hub.sys.tailscale_login(&key).await?;
        }
        "logout" => hub.sys.tailscale_logout().await?,
        _ => return Err(Error::not_found("action").into()),
    }
    Ok(Json(serde_json::to_value(hub.sys.tailscale().await).unwrap_or(Value::Null)))
}

pub async fn app_thread(State(hub): State<AppState>, Path(name): Path<String>) -> ApiResult<Json<Value>> {
    if name != "openthread" || !hub.sys.installed_packages().contains_key("openthread") {
        return Err(Error::not_found("OpenThread is not installed").into());
    }
    let out = hub.sys.exec("curl", &["-s", "-m", "2", "-H", "Accept: application/json", "http://localhost:8081/node"], Duration::from_secs(4)).await?;
    let v: Value = serde_json::from_str(&out.stdout).map_err(|_| Error::Unavailable("the Thread border router does not answer".into()))?;
    Ok(Json(v))
}

// ---------------------------------------------------------------- network

pub async fn network(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(resources::network(&hub).await?))
}

pub async fn ethernet(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(serde_json::to_value(hub.sys.ethernet().await?).unwrap_or(Value::Null)))
}

#[derive(Deserialize)]
pub struct IpBody {
    method: Option<String>,
    addresses: Option<Vec<String>>,
    gateway: Option<String>,
    dns: Option<Vec<String>>,
    mtu: Option<u32>,
    autoconnect: Option<bool>,
}

impl IpBody {
    fn into_settings(self, base: smhub_sys::network::IpSettings) -> smhub_sys::network::IpSettings {
        smhub_sys::network::IpSettings {
            method: self.method.unwrap_or(base.method),
            addresses: self.addresses.unwrap_or(base.addresses),
            gateway: self.gateway.or(base.gateway).filter(|g| !g.is_empty()),
            dns: self.dns.unwrap_or(base.dns),
            mtu: self.mtu.or(base.mtu),
            autoconnect: self.autoconnect.unwrap_or(base.autoconnect),
        }
    }
}

pub async fn ethernet_patch(State(hub): State<AppState>, Json(b): Json<IpBody>) -> ApiResult<Json<Value>> {
    let base = hub.sys.ethernet().await?;
    let s = b.into_settings(base);
    if s.method != "auto" && s.method != "manual" {
        return Err(Error::invalid("method", "use auto or manual").into());
    }
    let page: Vec<(String, String)> = vec![
        ("ip_method".into(), s.method.clone()),
        ("addresses".into(), serde_json::to_string(&s.addresses).unwrap_or_default()),
        ("gateway".into(), s.gateway.clone().unwrap_or_default()),
        ("dns".into(), serde_json::to_string(&s.dns).unwrap_or_default()),
        ("mtu".into(), s.mtu.map(|m| m.to_string()).unwrap_or_default()),
        ("auto_connect_on_boot".into(), if s.autoconnect { "True".into() } else { "False".into() }),
        ("connection".into(), "eth0".into()),
    ];
    let r = hub.sys.set_ethernet(&s).await?;
    hub.store.run(move |db| db.save_page_settings("ethernet", "Ethernet Settings", &page)).await?;
    hub.publish_later(&["network"]);
    Ok(Json(serde_json::to_value(r).unwrap_or(Value::Null)))
}

pub async fn wifi(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(resources::wifi(&hub).await?))
}

#[derive(Deserialize)]
pub struct Enabled {
    enabled: bool,
}

pub async fn wifi_patch(State(hub): State<AppState>, Json(b): Json<Enabled>) -> ApiResult<Json<Value>> {
    hub.sys.set_wifi_radio(b.enabled).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    hub.publish_later(&["network", "wifi"]);
    Ok(Json(resources::wifi(&hub).await?))
}

pub async fn wifi_scan(State(hub): State<AppState>) -> ApiResult<StatusCode> {
    hub.sys.wifi_rescan().await?;
    *lock(&hub.wifi_scanned) = Some(now_unix());
    let h2 = Arc::clone(&hub);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(4)).await;
        h2.publish("wifi").await;
    });
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
pub struct WifiConnect {
    ssid: String,
    password: Option<String>,
    #[serde(default)]
    security: Option<String>,
    #[serde(default)]
    hidden: bool,
    ip: Option<IpBody>,
}

pub async fn wifi_connect(State(hub): State<AppState>, Json(b): Json<WifiConnect>) -> ApiResult<Json<Value>> {
    let base = smhub_sys::network::IpSettings { method: "auto".into(), autoconnect: true, ..Default::default() };
    let ip = b.ip.map_or(base.clone(), |i| i.into_settings(base));
    let security = b.security.unwrap_or_else(|| "auto".into());
    hub.sys.wifi_connect(&b.ssid, b.password.as_deref(), &security, b.hidden, &ip).await?;
    let ssid = b.ssid.clone();
    let page: Vec<(String, String)> = vec![
        ("ssid".into(), ssid),
        ("hidden_network".into(), if b.hidden { "True".into() } else { "False".into() }),
        ("security".into(), security),
        ("ip_method".into(), if ip.method == "manual" { "STATIC".into() } else { "AUTO".into() }),
    ];
    hub.store.run(move |db| db.save_page_settings("wifi", "Wi-Fi", &page)).await?;
    hub.publish_later(&["network", "wifi"]);
    Ok(Json(resources::wifi(&hub).await?))
}

pub async fn wifi_disconnect(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    hub.sys.wifi_disconnect().await?;
    hub.publish_later(&["network", "wifi"]);
    Ok(Json(resources::wifi(&hub).await?))
}

/// Query parameters by name.
type Params = Query<std::collections::HashMap<String, String>>;

/// A required query parameter, exactly as sent. Values from outside the hub (a Wi-Fi name, a
/// broker user name, a key fingerprint) travel in the query: the hub's nginx decodes `%2F` in a
/// path before passing the request on, which would split such a value (API conventions).
fn param(q: &std::collections::HashMap<String, String>, key: &str) -> smhub_model::Result<String> {
    q.get(key).filter(|v| !v.is_empty()).cloned().ok_or_else(|| Error::invalid(key, "missing"))
}

pub async fn wifi_forget(State(hub): State<AppState>, Query(q): Params) -> ApiResult<StatusCode> {
    hub.sys.wifi_forget(&param(&q, "name")?).await?;
    hub.publish_later(&["network", "wifi"]);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn vpn(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(resources::vpn(&hub).await?))
}

#[derive(Deserialize)]
pub struct WgPatch {
    addresses: Option<Vec<String>>,
    private_key: Option<String>,
    public_key: Option<String>,
    endpoint: Option<String>,
    allowed_ips: Option<Vec<String>>,
    dns: Option<Vec<String>>,
    keepalive: Option<Option<u32>>,
    preshared_key: Option<Option<String>>,
    autoconnect: Option<bool>,
}

pub async fn vpn_patch(State(hub): State<AppState>, Json(b): Json<WgPatch>) -> ApiResult<Json<Value>> {
    let mut w = hub.sys.wireguard().unwrap_or_default();
    if let Some(v) = b.addresses {
        w.addresses = v;
    }
    if let Some(v) = b.private_key.filter(|k| !k.is_empty()) {
        w.private_key = v.trim().to_owned();
    }
    if let Some(v) = b.public_key {
        w.public_key = v.trim().to_owned();
    }
    if let Some(v) = b.endpoint {
        w.endpoint = v.trim().to_owned();
    }
    if let Some(v) = b.allowed_ips {
        w.allowed_ips = v;
    }
    if let Some(v) = b.dns {
        w.dns = v;
    }
    if let Some(v) = b.keepalive {
        w.keepalive = v;
    }
    if let Some(v) = b.preshared_key {
        w.preshared_key = v.unwrap_or_default().trim().to_owned();
    }
    if let Some(v) = b.autoconnect {
        w.autoconnect = v;
    }
    hub.sys.set_wireguard(&w).await?;
    let page: Vec<(String, String)> = vec![
        ("autoconnect".into(), if w.autoconnect { "True".into() } else { "False".into() }),
        ("addresses".into(), serde_json::to_string(&w.addresses).unwrap_or_default()),
        ("endpoint_address".into(), w.endpoint.clone()),
        ("public_key".into(), w.public_key.clone()),
        ("allowed_ips".into(), serde_json::to_string(&w.allowed_ips).unwrap_or_default()),
        ("keepalive".into(), w.keepalive.map(|k| k.to_string()).unwrap_or_else(|| "None".into())),
    ];
    hub.store.run(move |db| db.save_page_settings("wireguard", "WireGuard VPN", &page)).await?;
    hub.publish_later(&["vpn", "network"]);
    Ok(Json(resources::vpn(&hub).await?))
}

pub async fn vpn_delete(State(hub): State<AppState>) -> ApiResult<StatusCode> {
    hub.sys.wireguard_delete().await?;
    hub.publish_later(&["vpn", "network"]);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn vpn_action(State(hub): State<AppState>, Path(action): Path<String>) -> ApiResult<Json<Value>> {
    match action.as_str() {
        "connect" => hub.sys.wireguard_up(true).await?,
        "disconnect" => hub.sys.wireguard_up(false).await?,
        _ => return Err(Error::not_found("action").into()),
    }
    hub.publish_later(&["vpn", "network"]);
    Ok(Json(resources::vpn(&hub).await?))
}

pub async fn lte(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(resources::lte(&hub).await?))
}

#[derive(Deserialize)]
pub struct LteBody {
    apn: Option<String>,
    autostart: Option<bool>,
    led: Option<bool>,
}

pub async fn lte_patch(State(hub): State<AppState>, Json(b): Json<LteBody>) -> ApiResult<Json<Value>> {
    let mut page = Vec::new();
    if let Some(a) = &b.apn {
        if a.len() > 64 || a.contains(char::is_whitespace) {
            return Err(Error::invalid("apn", "an APN has no spaces").into());
        }
        page.push(("apn".to_owned(), a.clone()));
    }
    if let Some(a) = b.autostart {
        hub.sys.set_package_flag("lte", "autostart", a)?;
        page.push(("start_on_boot".to_owned(), bool_str(a)));
    }
    if let Some(l) = b.led {
        page.push(("led_control".to_owned(), if l { "False".into() } else { "True".into() }));
        if hub.sys.model().lte_slot {
            let _ = hub.sys.modem_led(l).await;
        }
    }
    hub.store.run(move |db| db.save_page_settings("lte", "LTE Settings", &page)).await?;
    *lock(&hub.modem) = None;
    Ok(Json(resources::lte(&hub).await?))
}

pub async fn lte_action(State(hub): State<AppState>, Path(action): Path<String>) -> ApiResult<Json<Value>> {
    match action.as_str() {
        "power" => hub.sys.modem_pulse("on").await?,
        "reset" => hub.sys.modem_pulse("reset").await?,
        "connect" | "disconnect" | "refresh" => {}
        _ => return Err(Error::not_found("action").into()),
    }
    *lock(&hub.modem) = None;
    *lock(&hub.features) = None;
    if action == "power" || action == "reset" {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    hub.publish_later(&["lte"]);
    Ok(Json(resources::lte(&hub).await?))
}

// ---------------------------------------------------------------- radios

pub async fn radios(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(resources::radios(&hub).await?))
}

#[derive(Deserialize)]
pub struct ModeQuery {
    mode: String,
}

fn slot_of(hub: &AppState, index: u8) -> ApiResult<smhub_model::hardware::RadioSlot> {
    Ok(hub.sys.model().radios.iter().find(|r| r.index == index).cloned().ok_or_else(|| Error::not_found("radio"))?)
}

fn mode_for(slot: &smhub_model::hardware::RadioSlot, m: &str) -> ApiResult<RadioMode> {
    let mode = RadioMode::parse(m).ok_or_else(|| Error::invalid("mode", "unknown radio mode"))?;
    if !smhub_model::hardware::ModelInfo::modes_for(slot).contains(&mode) {
        return Err(Error::invalid("mode", "this radio cannot run that mode").into());
    }
    Ok(mode)
}

pub async fn radio_firmware(State(hub): State<AppState>, Path(index): Path<u8>, Query(q): Query<ModeQuery>) -> ApiResult<Json<Value>> {
    let slot = slot_of(&hub, index)?;
    let mode = mode_for(&slot, &q.mode)?;
    let list = hub.sys.radio_firmware(&slot, mode).await?;
    Ok(Json(serde_json::to_value(list).unwrap_or(Value::Null)))
}

#[derive(Deserialize)]
pub struct FlashBody {
    mode: String,
    version: String,
    ieee: Option<String>,
}

pub async fn radio_flash(State(hub): State<AppState>, Path(index): Path<u8>, Json(b): Json<FlashBody>) -> ApiResult<Response> {
    let slot = slot_of(&hub, index)?;
    let mode = mode_for(&slot, &b.mode)?;
    if let Some(i) = b.ieee.as_deref().filter(|i| !i.is_empty()) {
        let hex = i.replace(':', "");
        if hex.len() != 16 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::invalid("ieee", "use 16 hexadecimal digits").into());
        }
        if mode.is_zwave() {
            return Err(Error::invalid("ieee", "Z-Wave radios have no IEEE address to set").into());
        }
    }
    if !hub.sys.radio_helper_ready() {
        return Err(Error::Unavailable("the radio flashing helper is not installed on this hub".into()).into());
    }
    let list = hub.sys.radio_firmware(&slot, mode).await?;
    let fw = list.into_iter().find(|f| f.version == b.version).ok_or_else(|| Error::invalid("version", "this firmware version is not offered"))?;
    let op = hub.start_op("radio_flash", Some(index.to_string()))?;
    let h2 = Arc::clone(&hub);
    let ieee = b.ieee.filter(|i| !i.is_empty());
    tokio::spawn(async move {
        let file = h2.sys.firmware_cache(index);
        op.progress(Some(0.0), Some("downloading"));
        let mut r = h2.sys.radio_download(&fw, &file).await;
        if r.is_ok() {
            let holder = h2.sys.service_holding(&slot.port);
            if let Some(svc) = &holder {
                op.progress(None, Some("stopping"));
                let _ = h2.sys.service_action(svc, "stop").await;
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            op.progress(Some(0.0), Some("preparing"));
            r = h2
                .sys
                .radio_flash(&slot, &file, ieee.as_deref(), |p, s| op.progress(p, s.as_deref()))
                .await;
            if let Some(svc) = &holder {
                tokio::time::sleep(Duration::from_secs(2)).await;
                op.progress(None, Some("starting"));
                let _ = h2.sys.service_action(svc, "start").await;
            }
            if r.is_ok() {
                let i = index - 1;
                let vals = vec![
                    (format!("mode{i}"), mode.legacy_type().to_string()),
                    (format!("version{i}"), fw.version.clone()),
                    (format!("ieee{i}"), ieee.clone().unwrap_or_default()),
                ];
                if let Err(e) = h2.store.run(move |db| db.save_page_settings("radios", "Radio Settings", &vals)).await {
                    tracing::warn!("radios page: {e}");
                }
            }
        }
        let _ = std::fs::remove_file(&file);
        op.finish(&r);
        h2.publish_later(&["radios", "services", "apps"]);
    });
    Ok(op_response(op_id_snapshot(&hub, "radio_flash")))
}

// ---------------------------------------------------------------- updates

pub async fn updates(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(resources::updates(&hub).await?))
}

#[derive(Deserialize)]
pub struct UpdatesPatch {
    channel: Option<String>,
    auto_update: Option<bool>,
}

pub async fn updates_patch(State(hub): State<AppState>, Json(b): Json<UpdatesPatch>) -> ApiResult<Json<Value>> {
    let mut page = Vec::new();
    if let Some(c) = &b.channel {
        if c != "stable" && c != "dev" {
            return Err(Error::invalid("channel", "use stable or dev").into());
        }
        hub.sys.set_app_channel(c)?;
        page.push(("os_update_channel".to_owned(), if c == "dev" { "development".into() } else { "stable".into() }));
    }
    if let Some(a) = b.auto_update {
        page.push(("auto_update".to_owned(), if a { "True".into() } else { "False".into() }));
    }
    hub.store.run(move |db| db.save_page_settings("updates", "Update Settings", &page)).await?;
    if b.channel.is_some() {
        let h2 = Arc::clone(&hub);
        tokio::spawn(async move {
            let ch = resources::channel(&h2);
            h2.sys.os_releases(ch, true).await;
            h2.publish("updates").await;
        });
    }
    hub.publish_later(&["apps"]);
    Ok(Json(resources::updates(&hub).await?))
}

pub async fn updates_check(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    let ch = resources::channel(&hub);
    hub.sys.os_releases(ch, true).await;
    *lock(&hub.updates_checked) = Some(now_unix());
    hub.publish_later(&["updates"]);
    Ok(Json(resources::updates(&hub).await?))
}

#[derive(Deserialize)]
pub struct Version {
    version: String,
}

/// RAUC's progress messages ("Checking bundle", "Copying image to rootfs.1", ...) as the
/// operation step names of the API.
fn rauc_step(message: &str) -> &'static str {
    let m = message.to_ascii_lowercase();
    if m.contains("download") || m.contains("fetch") {
        "downloading"
    } else if m.contains("check") || m.contains("verif") || m.contains("signature") {
        "verifying"
    } else if m.contains("copy") || m.contains("writ") || m.contains("updating slot") {
        "writing"
    } else if m.contains("determin") || m.contains("mount") || m.contains("prepar") {
        "preparing"
    } else {
        "installing"
    }
}

pub async fn os_install(State(hub): State<AppState>, Json(b): Json<Version>) -> ApiResult<Response> {
    let ch = resources::channel(&hub);
    let releases = hub.sys.os_releases(ch, false).await;
    let rel = releases.releases.into_iter().find(|r| r.version == b.version).ok_or_else(|| Error::invalid("version", "this release is not offered"))?;
    let op = hub.start_op("os_update", Some(rel.version.clone()))?;
    let h2 = Arc::clone(&hub);
    tokio::spawn(async move {
        op.progress(Some(0.0), Some("preparing"));
        let r = h2.sys.rauc_install(&rel.bundle_url, |p, s| op.progress(Some(f64::from(p)), Some(rauc_step(&s)))).await;
        if r.is_ok() {
            op.reboot_needed();
        }
        op.finish(&r);
    });
    Ok(op_response(op_id_snapshot(&hub, "os_update")))
}

// ---------------------------------------------------------------- settings

pub async fn general(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({
        "hostname": hub.sys.hostname(),
        "timezone": hub.sys.timezone(),
        "sdcard_config": hub.sys.p("/mnt/user/.sdcard-config-enabled").exists(),
    })))
}

#[derive(Deserialize)]
pub struct GeneralBody {
    hostname: Option<String>,
    timezone: Option<String>,
    sdcard_config: Option<bool>,
}

pub async fn general_patch(State(hub): State<AppState>, Json(b): Json<GeneralBody>) -> ApiResult<Json<Value>> {
    let mut page = Vec::new();
    if let Some(z) = &b.timezone {
        if *z != hub.sys.timezone() && !hub.sys.settable(z) {
            return Err(Error::invalid("timezone", "unknown time zone").into());
        }
    }
    if let Some(h) = &b.hostname {
        hub.sys.set_hostname(h.trim()).await?;
        page.push(("hostname".to_owned(), h.trim().to_owned()));
    }
    if let Some(z) = &b.timezone {
        let z = hub.sys.set_timezone(z)?;
        crate::hw::zone_changed(&hub);
        page.push(("timezone".to_owned(), z));
    }
    if let Some(s) = b.sdcard_config {
        let flag = hub.sys.p("/mnt/user/.sdcard-config-enabled");
        if s {
            smhub_sys::fsx::write_plain(&flag, b"")?;
        } else {
            let _ = std::fs::remove_file(&flag);
        }
        page.push(("sdcard-config".to_owned(), if s { "True".into() } else { "False".into() }));
    }
    hub.store.run(move |db| db.save_page_settings("general", "General Settings", &page)).await?;
    hub.publish_later(&["system", "network"]);
    general(State(hub)).await
}

pub async fn timezones(State(hub): State<AppState>) -> Json<Value> {
    Json(json!(hub.sys.timezones()))
}

async fn mqtt_json(hub: &AppState) -> Value {
    let m = hub.sys.mqtt();
    let state = hub.sys.services().await.get("mosquitto").copied().unwrap_or(smhub_sys::services::State::Stopped);
    let mut v = serde_json::to_value(&m).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        o.insert("users".into(), json!(hub.sys.mqtt_users()));
        o.insert("state".into(), json!(state));
    }
    v
}

pub async fn mqtt(State(hub): State<AppState>) -> Json<Value> {
    Json(mqtt_json(&hub).await)
}

#[derive(Deserialize)]
pub struct BridgeBody {
    enabled: Option<bool>,
    connection: Option<String>,
    address: Option<String>,
    username: Option<String>,
    password: Option<Option<String>>,
    topics: Option<Vec<String>>,
    clean_session: Option<bool>,
}

#[derive(Deserialize)]
pub struct MqttBody {
    port: Option<u16>,
    allow_external: Option<bool>,
    allow_anonymous: Option<bool>,
    bridge: Option<BridgeBody>,
}

pub async fn mqtt_patch(State(hub): State<AppState>, Json(b): Json<MqttBody>) -> ApiResult<Json<Value>> {
    let mut m = hub.sys.mqtt();
    if let Some(p) = b.port {
        m.port = p;
    }
    if let Some(v) = b.allow_external {
        m.allow_external = v;
    }
    if let Some(v) = b.allow_anonymous {
        m.allow_anonymous = v;
    }
    if let Some(br) = b.bridge {
        if let Some(v) = br.enabled {
            m.bridge.enabled = v;
        }
        if let Some(v) = br.connection {
            m.bridge.connection = v;
        }
        if let Some(v) = br.address {
            m.bridge.address = v;
        }
        if let Some(v) = br.username {
            m.bridge.username = v;
        }
        if let Some(v) = br.password {
            m.bridge.password = v.unwrap_or_default();
        }
        if let Some(v) = br.topics {
            m.bridge.topics = v.into_iter().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect();
        }
        if let Some(v) = br.clean_session {
            m.bridge.clean_session = v;
        }
    }
    let page = hub.sys.set_mqtt(&m)?;
    hub.store.run(move |db| db.save_page_settings("mqtt", "Mosquitto MQTT Broker", &page)).await?;
    if hub.sys.service_exists("mosquitto") {
        if let Err(e) = hub.sys.service_action("mosquitto", "restart").await {
            tracing::warn!("mosquitto restart: {e}");
        }
    }
    hub.publish_later(&["services"]);
    Ok(Json(mqtt_json(&hub).await))
}

#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}

async fn restart_mosquitto(hub: &AppState) {
    if hub.sys.service_exists("mosquitto") {
        let _ = hub.sys.service_action("mosquitto", "restart").await;
    }
}

pub async fn mqtt_user_add(State(hub): State<AppState>, Json(b): Json<Credentials>) -> ApiResult<Json<Value>> {
    hub.sys.mqtt_add_user(b.username.trim(), &b.password)?;
    restart_mosquitto(&hub).await;
    Ok(Json(mqtt_json(&hub).await))
}

pub async fn mqtt_user_delete(State(hub): State<AppState>, Query(q): Params) -> ApiResult<Json<Value>> {
    hub.sys.mqtt_delete_user(&param(&q, "username")?)?;
    restart_mosquitto(&hub).await;
    Ok(Json(mqtt_json(&hub).await))
}

pub async fn usb(State(hub): State<AppState>) -> Json<Value> {
    let configured = hub.sys.usb_mode();
    let now = hub.sys.usb_role_now().unwrap_or_else(|| configured.clone());
    Json(json!({
        "mode": now,
        "pending_mode": if now != configured { Some(configured) } else { None },
        "devices": hub.sys.usb_devices().await,
    }))
}

#[derive(Deserialize)]
pub struct UsbBody {
    mode: String,
}

pub async fn usb_patch(State(hub): State<AppState>, Json(b): Json<UsbBody>) -> ApiResult<Json<Value>> {
    hub.sys.set_usb_mode(&b.mode)?;
    let m = b.mode.clone();
    hub.store.run(move |db| db.save_page_settings("usb", "USB Settings", &[("usb_mode".into(), m)])).await?;
    tokio::time::sleep(Duration::from_millis(800)).await;
    *lock(&hub.features) = None;
    Ok(usb(State(hub)).await)
}

async fn ssh_json(hub: &AppState) -> Value {
    let a = hub.sys.ssh_access();
    let state = hub.sys.services().await.get("sshd").copied().unwrap_or(smhub_sys::services::State::Stopped);
    json!({
        "password_auth": a.password_auth,
        "root_login": a.root_login,
        "pubkey_auth": a.pubkey_auth,
        "state": state,
        "keys": hub.sys.ssh_keys(),
    })
}

pub async fn ssh(State(hub): State<AppState>) -> Json<Value> {
    Json(ssh_json(&hub).await)
}

#[derive(Deserialize)]
pub struct SshBody {
    password_auth: Option<bool>,
    root_login: Option<bool>,
    pubkey_auth: Option<bool>,
}

pub async fn ssh_patch(State(hub): State<AppState>, Json(b): Json<SshBody>) -> ApiResult<Json<Value>> {
    let current = hub.sys.ssh_access();
    let pw = b.password_auth.unwrap_or(current.password_auth);
    let pk = b.pubkey_auth.unwrap_or(current.pubkey_auth);
    if !pw && !pk {
        return Err(Error::invalid("password_auth", "keep at least one way to sign in over SSH").into());
    }
    if !pw && hub.sys.ssh_keys().is_empty() {
        return Err(Error::invalid("password_auth", "add a public key before switching password sign-in off").into());
    }
    if hub.sys.set_ssh_access(b.password_auth, b.root_login, b.pubkey_auth)? && hub.sys.service_exists("sshd") {
        hub.sys.service_action("sshd", "restart").await?;
    }
    Ok(Json(ssh_json(&hub).await))
}

#[derive(Deserialize)]
pub struct KeyBody {
    key: String,
}

pub async fn ssh_key_add(State(hub): State<AppState>, Json(b): Json<KeyBody>) -> ApiResult<Json<Value>> {
    hub.sys.ssh_add_key(&b.key)?;
    Ok(Json(ssh_json(&hub).await))
}

pub async fn ssh_key_delete(State(hub): State<AppState>, Query(q): Params) -> ApiResult<Json<Value>> {
    hub.sys.ssh_delete_key(&param(&q, "fingerprint")?)?;
    Ok(Json(ssh_json(&hub).await))
}

async fn serial_json(hub: &AppState) -> Value {
    let state = hub.sys.services().await.get("ser2net").copied().unwrap_or(smhub_sys::services::State::Stopped);
    let model = hub.sys.model();
    let mut available: Vec<Value> = Vec::new();
    for r in &model.radios {
        available.push(json!({ "path": r.port, "label": format!("{} ({})", r.chip, r.index), "in_use_by": hub.sys.service_holding(&r.port).filter(|s| s != "ser2net") }));
    }
    for p in hub.sys.usb_serial_ports() {
        available.push(json!({ "path": p, "label": null, "in_use_by": hub.sys.service_holding(&p).filter(|s| s != "ser2net") }));
    }
    let ports = hub.sys.ser2net_ports();
    let open = hub.sys.tcp_listening(&ports.iter().map(|p| p.tcp_port).collect::<Vec<_>>());
    let ports: Vec<Value> = ports
        .iter()
        .map(|p| json!({ "device": p.device, "tcp_port": p.tcp_port, "baud": p.baud, "listening": open.contains(&p.tcp_port) }))
        .collect();
    json!({
        // started and stopped with POST /services/ser2net/{action}
        "service": "ser2net",
        "autostart": hub.sys.package_flag("ser2net", "autostart") || hub.sys.service_autostart("ser2net"),
        "state": state,
        "ports": ports,
        "available_ports": available,
    })
}

/// After a restart, how long a save waits for ser2net to open its ports, so that its answer
/// shows whether ser2net took the file (OpenRC counts it as started either way).
const SERIAL_SETTLE: Duration = Duration::from_secs(2);

pub async fn serial(State(hub): State<AppState>) -> Json<Value> {
    Json(serial_json(&hub).await)
}

pub async fn serial_patch(State(hub): State<AppState>, Json(b): Json<Value>) -> ApiResult<Json<Value>> {
    let ports: Option<Vec<smhub_sys::ser2net::Port>> = match b.get("ports") {
        Some(Value::Array(a)) => {
            if a.len() > smhub_sys::ser2net::MAX_PORTS {
                return Err(Error::invalid("ports", format!("share at most {} ports", smhub_sys::ser2net::MAX_PORTS)).into());
            }
            let mut v = Vec::new();
            for p in a {
                let device = p.get("device").and_then(Value::as_str).unwrap_or("").to_owned();
                let tcp_port = p.get("tcp_port").and_then(Value::as_u64).and_then(|n| u16::try_from(n).ok()).unwrap_or(0);
                let baud = p.get("baud").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(115_200);
                v.push(smhub_sys::ser2net::Port { device, tcp_port, baud });
            }
            Some(v)
        }
        None => None,
        _ => return Err(Error::invalid("ports", "must be a list").into()),
    };
    let autostart = b.get("autostart").and_then(Value::as_bool);
    let mut page = Vec::new();
    if let Some(ports) = &ports {
        let old = hub.sys.set_ser2net_ports(ports)?;
        page.push(("ports".to_owned(), old));
        if ports.is_empty() {
            let _ = hub.sys.service_action("ser2net", "stop").await;
            let _ = hub.sys.set_package_autostart("ser2net", false).await;
        } else if hub.sys.service_exists("ser2net") && hub.sys.service_action("ser2net", "restart").await.is_ok() {
            let want: Vec<u16> = ports.iter().map(|p| p.tcp_port).collect();
            let until = tokio::time::Instant::now() + SERIAL_SETTLE;
            while hub.sys.tcp_listening(&want).len() < want.len() && tokio::time::Instant::now() < until {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    if let Some(a) = autostart {
        if ports.as_ref().is_none_or(|p| !p.is_empty()) {
            hub.sys.set_package_autostart("ser2net", a).await?;
        }
        page.push(("start_on_boot".to_owned(), bool_str(a)));
    }
    hub.store.run(move |db| db.save_page_settings("ser2net", "Serial Bridge", &page)).await?;
    hub.publish_later(&["services"]);
    Ok(Json(serial_json(&hub).await))
}

async fn esphome_json(hub: &AppState) -> ApiResult<Value> {
    let page = hub.store.run(|db| db.page_settings("esphome")).await?;
    let b = |k: &str, default: bool| page.get(k).map_or(default, |v| parse_bool(v));
    let key = page.get("encryption_key").cloned().unwrap_or_else(|| smhub_sys::rtos::FACTORY_KEY_B64.to_owned());
    let state = hub.sys.services().await.get("smhub-broker").copied().unwrap_or(smhub_sys::services::State::Stopped);
    Ok(json!({
        "available": hub.sys.esphome_available(),
        "ambient_light": b("ambient_light", true),
        "buzzer": b("buzzer", true),
        "user_i2c": b("user_i2c", true),
        "bluetooth_proxy_mode": match page.get("bluetooth_proxy_mode").map(String::as_str) {
            Some("off") => "off",
            Some("bluez" | "proxy") => "proxy",
            _ => "hci",
        },
        "encryption_key": key,
        "state": state,
        "restart_needed": false,
    }))
}

pub async fn esphome(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(esphome_json(&hub).await?))
}

#[derive(Deserialize)]
pub struct EsphomeBody {
    ambient_light: Option<bool>,
    buzzer: Option<bool>,
    user_i2c: Option<bool>,
    bluetooth_proxy_mode: Option<String>,
    encryption_key: Option<String>,
}

pub async fn esphome_patch(State(hub): State<AppState>, Json(b): Json<EsphomeBody>) -> ApiResult<Json<Value>> {
    let mut page: Vec<(String, String)> = Vec::new();
    let mut overlays: Vec<(&str, bool)> = Vec::new();
    for (k, v) in [("ambient_light", b.ambient_light), ("buzzer", b.buzzer), ("user_i2c", b.user_i2c)] {
        if let Some(v) = v {
            overlays.push((k, v));
            page.push((k.to_owned(), bool_str(v)));
        }
    }
    let bt = b.bluetooth_proxy_mode.map(|m| if m == "proxy" { "bluez".to_owned() } else { m });
    if let Some(m) = &bt {
        if !matches!(m.as_str(), "off" | "hci" | "bluez") {
            return Err(Error::invalid("bluetooth_proxy_mode", "use off, hci or proxy").into());
        }
    }
    if let Some(k) = &b.encryption_key {
        use base64::Engine;
        let ok = base64::engine::general_purpose::STANDARD.decode(k.trim()).is_ok_and(|d| d.len() == 32);
        if !ok {
            return Err(Error::invalid("encryption_key", "the key must be 32 bytes in base64").into());
        }
    }
    let overlays_changed = hub.sys.set_overlays(&overlays)?;
    let mut patched = false;
    if let Some(k) = &b.encryption_key {
        patched = hub.sys.patch_esphome_key(k)?;
        page.push(("encryption_key".to_owned(), k.trim().to_owned()));
    }
    let mut restart = patched;
    if let Some(m) = &bt {
        restart |= hub.sys.set_bt_mode(m)?;
        page.push(("bluetooth_proxy_mode".to_owned(), m.clone()));
    }
    hub.store.run(move |db| db.save_page_settings("esphome", "ESPHome", &page)).await?;
    if restart && hub.sys.service_exists("smhub-broker") {
        if let Err(e) = hub.sys.service_action("smhub-broker", "restart").await {
            tracing::warn!("smhub-broker restart: {e}");
        }
    }
    if patched {
        // the hub's own connection to ESPHome needs the new key
        crate::hw::esp::start(&hub).await;
    }
    let mut v = esphome_json(&hub).await?;
    if let Some(o) = v.as_object_mut() {
        o.insert("restart_needed".into(), Value::Bool(overlays_changed));
    }
    Ok(Json(v))
}

pub async fn esphome_restart(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    hub.sys.service_action("smhub-broker", "restart").await?;
    Ok(Json(esphome_json(&hub).await?))
}

/// Apply the stored ESPHome settings at start, as the old backend did (only changes act).
pub async fn esphome_sync(hub: &AppState) {
    let Ok(page) = hub.store.run(|db| db.page_settings("esphome")).await else { return };
    if page.is_empty() || !hub.sys.esphome_available() {
        return;
    }
    let overlays: Vec<(&str, bool)> = ["ambient_light", "buzzer", "user_i2c", "adc1", "adc2"]
        .into_iter()
        .filter_map(|k| page.get(k).map(|v| (k, parse_bool(v))))
        .collect();
    if let Err(e) = hub.sys.set_overlays(&overlays) {
        tracing::warn!("overlays: {e}");
    }
    let mut restart = false;
    if let Some(k) = page.get("encryption_key").filter(|k| !k.is_empty()) {
        match hub.sys.patch_esphome_key(k) {
            Ok(p) => restart |= p,
            Err(e) => tracing::warn!("ESPHome key: {e}"),
        }
    }
    if let Some(m) = page.get("bluetooth_proxy_mode") {
        match hub.sys.set_bt_mode(m) {
            Ok(c) => restart |= c,
            Err(e) => tracing::warn!("Bluetooth mode: {e}"),
        }
    }
    if restart && hub.sys.service_exists("smhub-broker") {
        let _ = hub.sys.service_action("smhub-broker", "restart").await;
    }
}

pub async fn backup_settings(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    let page = hub.store.run(|db| db.page_settings("backups")).await?;
    let extra: Vec<String> = page
        .get("extra_dirs")
        .map(|s| {
            serde_json::from_str::<Vec<String>>(s).unwrap_or_else(|_| s.split([',', ' ']).filter(|x| !x.is_empty()).map(str::to_owned).collect())
        })
        .unwrap_or_default();
    Ok(Json(json!({ "include_home": page.get("include_home").is_some_and(|v| parse_bool(v)), "extra_dirs": extra })))
}

#[derive(Deserialize)]
pub struct BackupBody {
    include_home: Option<bool>,
    extra_dirs: Option<Vec<String>>,
}

pub async fn backup_settings_patch(State(hub): State<AppState>, Json(b): Json<BackupBody>) -> ApiResult<Json<Value>> {
    let mut page = Vec::new();
    if let Some(h) = b.include_home {
        page.push(("include_home".to_owned(), if h { "True".into() } else { "False".into() }));
    }
    if let Some(e) = &b.extra_dirs {
        if e.iter().any(|d| !d.starts_with('/') || d.contains("..")) {
            return Err(Error::invalid("extra_dirs", "use absolute folder paths").into());
        }
        page.push(("extra_dirs".to_owned(), e.join(",")));
    }
    hub.store.run(move |db| db.save_page_settings("backups", "Backup Settings", &page)).await?;
    backup_settings(State(hub)).await
}

// ---------------------------------------------------------------- logs

pub async fn logs(State(hub): State<AppState>) -> Json<Value> {
    let installed: Vec<String> = hub.sys.installed_packages().into_keys().collect();
    Json(json!(hub.sys.logs(&installed)))
}

#[derive(Deserialize)]
pub struct TailQuery {
    lines: Option<usize>,
    filter: Option<String>,
}

pub async fn log_tail(State(hub): State<AppState>, Path(name): Path<String>, Query(q): Query<TailQuery>) -> ApiResult<Json<Value>> {
    let sys = hub.sys.clone();
    let lines = q.lines.unwrap_or(500).clamp(1, 5000);
    if !hub.sys.log_path(&name)?.exists() && log_expected(&hub, &name) {
        return Ok(Json(json!({ "lines": [] })));
    }
    let v = tokio::task::spawn_blocking(move || sys.log_tail(&name, lines, q.filter.as_deref()))
        .await
        .map_err(|e| Error::internal(e.to_string()))??;
    Ok(Json(json!({ "lines": v })))
}

/// The log is the hub's own or an installed package's, so a missing file means "nothing yet".
pub fn log_expected(hub: &AppState, name: &str) -> bool {
    let installed: Vec<String> = hub.sys.installed_packages().into_keys().collect();
    hub.sys.log_expected(name, &installed)
}

pub async fn log_download(State(hub): State<AppState>, Path(name): Path<String>) -> ApiResult<Response> {
    let path = hub.sys.log_path(&name)?;
    let f = tokio::fs::File::open(&path).await.map_err(|_| Error::not_found("log"))?;
    let body = Body::from_stream(tokio_util_stream(f));
    let file = format!("attachment; filename=\"{}.log\"", name.replace('/', "-"));
    let mut r = (StatusCode::OK, body).into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain; charset=utf-8"));
    if let Ok(v) = HeaderValue::from_str(&file) {
        r.headers_mut().insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(r)
}

/// A file as a stream of chunks, without loading it into memory.
pub fn tokio_util_stream<R: tokio::io::AsyncRead + Unpin + Send + 'static>(
    mut r: R,
) -> impl futures_util::Stream<Item = std::io::Result<Bytes>> + Send {
    use tokio::io::AsyncReadExt;
    async_stream(move |tx| async move {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match r.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(Ok(Bytes::copy_from_slice(&buf[..n]))).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            }
        }
    })
}

/// A tiny channel-backed stream (no extra crate): the producer runs as a task.
pub fn async_stream<F, Fut>(f: F) -> impl futures_util::Stream<Item = std::io::Result<Bytes>> + Send
where
    F: FnOnce(tokio::sync::mpsc::Sender<std::io::Result<Bytes>>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let (tx, mut rx) = tokio::sync::mpsc::channel::<std::io::Result<Bytes>>(4);
    tokio::spawn(f(tx));
    futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx))
}
