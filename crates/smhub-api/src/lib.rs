//! API v2 of `smhubd`: the router, the sign-in layer, and the servers on loopback TCP and the
//! local socket. The contract is `api/openapi.yaml`.

pub mod apps;
pub mod auth;
pub mod error;
pub mod hw;
pub mod prefs;
pub mod resources;
pub mod routes;
pub mod state;
pub mod transfer;
pub mod watchers;
pub mod ws;

use std::time::Duration;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Extension};
use axum::routing::{get, post, put};
use tower_http::timeout::TimeoutLayer;

use crate::routes as r;
use crate::state::{AppState, limits};

/// All routes. Every route is private unless `auth::is_public` lists it (decision D40).
pub fn router(hub: AppState) -> Router {
    let piece = DefaultBodyLimit::max(limits::PIECE + 4096);
    let api = Router::new()
        .route("/api/v2/health", get(r::health))
        .route("/api/v2/system", get(r::system))
        .route("/api/v2/system/setup-done", post(r::setup_done))
        .route("/api/v2/system/reboot", post(r::reboot))
        .route("/api/v2/system/shutdown", post(r::shutdown))
        .route("/api/v2/session", get(auth::session_get).post(auth::session_post).delete(auth::session_delete))
        .route("/api/v2/security", get(auth::security_get))
        .route("/api/v2/security/enable", post(auth::security_enable))
        .route("/api/v2/security/disable", post(auth::security_disable))
        .route("/api/v2/users", get(auth::users_list).post(auth::users_create))
        .route("/api/v2/users/{id}", axum::routing::patch(auth::users_patch).delete(auth::users_delete))
        .route("/api/v2/preferences", get(prefs::get).patch(prefs::patch))
        .route("/api/v2/metrics", get(r::metrics))
        .route("/api/v2/services", get(r::services))
        .route("/api/v2/services/{name}/{action}", post(r::service_action))
        .route("/api/v2/operations", get(r::operations))
        .route("/api/v2/operations/{id}", get(r::operation))
        .route("/api/v2/apps", get(r::apps_list))
        .route("/api/v2/apps/refresh", post(r::apps_refresh))
        .route("/api/v2/apps/upgrade", post(r::apps_upgrade))
        .route("/api/v2/apps/{name}", get(r::app_detail).patch(r::app_patch))
        .route("/api/v2/apps/{name}/settings", axum::routing::patch(r::app_settings))
        .route("/api/v2/apps/{name}/changelog", get(r::app_changelog))
        .route("/api/v2/apps/{name}/icon", get(r::app_icon))
        .route("/api/v2/apps/{name}/tailscale", get(r::app_tailscale))
        .route("/api/v2/apps/{name}/tailscale/{action}", post(r::app_tailscale_action))
        .route("/api/v2/apps/{name}/thread", get(r::app_thread))
        .route("/api/v2/apps/{name}/{action}", post(r::app_action))
        .route("/api/v2/network", get(r::network))
        .route("/api/v2/network/ethernet", get(r::ethernet).patch(r::ethernet_patch))
        .route("/api/v2/wifi", get(r::wifi).patch(r::wifi_patch))
        .route("/api/v2/wifi/scan", post(r::wifi_scan))
        .route("/api/v2/wifi/connect", post(r::wifi_connect))
        .route("/api/v2/wifi/disconnect", post(r::wifi_disconnect))
        .route("/api/v2/wifi/saved", axum::routing::delete(r::wifi_forget))
        .route("/api/v2/vpn", get(r::vpn).patch(r::vpn_patch).delete(r::vpn_delete))
        .route("/api/v2/vpn/{action}", post(r::vpn_action))
        .route("/api/v2/lte", get(r::lte).patch(r::lte_patch))
        .route("/api/v2/lte/{action}", post(r::lte_action))
        .route("/api/v2/radios", get(r::radios))
        .route("/api/v2/radios/{index}/firmware", get(r::radio_firmware))
        .route("/api/v2/radios/{index}/flash", post(r::radio_flash))
        .route("/api/v2/updates", get(r::updates).patch(r::updates_patch))
        .route("/api/v2/updates/check", post(r::updates_check))
        .route("/api/v2/updates/os/install", post(r::os_install))
        .route("/api/v2/settings/general", get(r::general).patch(r::general_patch))
        .route("/api/v2/settings/timezones", get(r::timezones))
        .route("/api/v2/settings/mqtt", get(r::mqtt).patch(r::mqtt_patch))
        .route("/api/v2/settings/mqtt/users", post(r::mqtt_user_add).delete(r::mqtt_user_delete))
        .route("/api/v2/settings/usb", get(r::usb).patch(r::usb_patch))
        .route("/api/v2/settings/ssh", get(r::ssh).patch(r::ssh_patch))
        .route("/api/v2/settings/ssh/keys", post(r::ssh_key_add).delete(r::ssh_key_delete))
        .route("/api/v2/settings/serial-bridge", get(r::serial).patch(r::serial_patch))
        .route("/api/v2/settings/esphome", get(r::esphome).patch(r::esphome_patch))
        .route("/api/v2/settings/esphome/restart", post(r::esphome_restart))
        .route("/api/v2/settings/backups", get(r::backup_settings).patch(r::backup_settings_patch))
        .route("/api/v2/backups/archive", post(transfer::backup_archive))
        .route("/api/v2/backups/restore", post(transfer::backup_restore))
        .route("/api/v2/backups/factory", get(transfer::factory).delete(transfer::factory_delete))
        .route("/api/v2/backups/factory/archive", get(transfer::factory_archive))
        .route("/api/v2/backups/factory/restore", post(transfer::factory_restore))
        .route("/api/v2/uploads", post(transfer::upload_start))
        .route("/api/v2/uploads/{id}", put(transfer::upload_piece).layer(piece))
        .route("/api/v2/files", get(transfer::files_list).delete(transfer::files_delete))
        .route("/api/v2/files/content", get(transfer::file_get).put(transfer::file_put).layer(piece))
        .route("/api/v2/files/upload", post(transfer::file_upload))
        .route("/api/v2/files/folder", post(transfer::file_folder))
        .route("/api/v2/files/move", post(transfer::file_move))
        .route("/api/v2/tokens", get(auth::tokens_list).post(auth::tokens_create))
        .route("/api/v2/tokens/{id}", axum::routing::delete(auth::tokens_delete))
        .route("/api/v2/hardware", get(hw::routes::hardware))
        .route("/api/v2/hardware/buzzer/play", post(hw::routes::buzzer_play))
        .route("/api/v2/hardware/buzzer/stop", post(hw::routes::buzzer_stop))
        .route("/api/v2/hardware/light", axum::routing::patch(hw::routes::light_patch))
        .route("/api/v2/hardware/leds", axum::routing::patch(hw::routes::leds_patch))
        .route("/api/v2/hardware/find", post(hw::routes::find_post))
        .route("/api/v2/settings/hardware", get(hw::routes::settings_get).patch(hw::routes::settings_patch))
        .route("/api/v2/ir", get(hw::routes::ir_get))
        .route("/api/v2/ir/learn", post(hw::routes::ir_learn).delete(hw::routes::ir_learn_cancel))
        .route("/api/v2/ir/codes", post(hw::routes::ir_code_add))
        .route("/api/v2/ir/codes/{id}", axum::routing::patch(hw::routes::ir_code_patch).delete(hw::routes::ir_code_delete))
        .route("/api/v2/ir/codes/{id}/send", post(hw::routes::ir_code_send))
        .route("/api/v2/ir/send", post(hw::routes::ir_send))
        .route("/api/v2/ir/export", get(hw::routes::ir_export))
        .route("/api/v2/ir/import", post(hw::routes::ir_import))
        .route("/api/v2/ups", get(hw::routes::ups_get).patch(hw::routes::ups_patch))
        .route("/api/v2/ups/scan", post(hw::routes::ups_scan))
        .route("/api/v2/logs", get(r::logs))
        .route("/api/v2/logs/{name}", get(r::log_tail))
        .route("/api/v2/logs/{name}/download", get(r::log_download))
        .route("/api/v1/login/user-session", get(auth::legacy_ready).head(auth::legacy_ready))
        .route("/api/v1/login/cookie", post(auth::legacy_sign_in))
        .layer(axum::middleware::from_fn(finish_writes))
        .layer(TimeoutLayer::with_status_code(axum::http::StatusCode::REQUEST_TIMEOUT, Duration::from_secs(150)));
    let sockets = Router::new()
        .route("/ws/v2/events", get(ws::events))
        .route("/ws/v2/terminal", get(ws::terminal))
        .route("/ws/v2/logs/{name}", get(ws::logs));
    api.merge(sockets)
        .layer(DefaultBodyLimit::max(limits::BODY))
        .layer(axum::middleware::from_fn_with_state(hub.clone(), auth::layer))
        .with_state(hub)
}

/// A change the hub has started is finished even if the browser goes away: closing a tab while
/// a service restarts or a network profile is written must not stop the command half-way (they
/// run with kill-on-drop so that a time-out can stop a hung command). Reads are dropped as usual.
async fn finish_writes(req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    use axum::response::IntoResponse;
    if matches!(*req.method(), axum::http::Method::GET | axum::http::Method::HEAD) {
        return next.run(req).await;
    }
    match tokio::spawn(next.run(req)).await {
        Ok(r) => r,
        Err(e) => error::ApiError(smhub_model::Error::internal(format!("the request stopped: {e}"))).into_response(),
    }
}

/// The same router for the local socket, where accepted callers count as signed in.
pub fn local_router(hub: AppState) -> Router {
    router(hub).layer(Extension(auth::LocalSocket))
}

/// A Unix-socket listener that accepts only root and the device user (decision D42).
pub struct LocalListener {
    pub inner: tokio::net::UnixListener,
    pub allowed: Vec<u32>,
}

impl axum::serve::Listener for LocalListener {
    type Io = tokio::net::UnixStream;
    type Addr = tokio::net::unix::SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.inner.accept().await {
                Ok((s, a)) => match s.peer_cred() {
                    Ok(c) if self.allowed.contains(&c.uid()) => return (s, a),
                    _ => tracing::warn!("refused a local connection"),
                },
                Err(e) => {
                    tracing::warn!("local socket: {e}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}
