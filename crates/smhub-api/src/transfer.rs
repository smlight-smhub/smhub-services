//! Uploads in pieces (each under nginx's 1 MiB limit), the file manager, and backups.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use smhub_model::Error;
use smhub_model::time::{now_unix, rfc3339_system};
use smhub_sys::services::{State as Svc, service_for_package};

use crate::error::ApiResult;
use crate::routes::{async_stream, tokio_util_stream};
use crate::state::{AppState, Upload, limits, lock};

// ---------------------------------------------------------------- uploads

#[derive(Deserialize)]
pub struct NewUpload {
    size: u64,
    name: Option<String>,
}

pub async fn upload_start(State(hub): State<AppState>, Json(b): Json<NewUpload>) -> ApiResult<Response> {
    if b.size > limits::UPLOAD_MAX {
        return Err(Error::TooLarge("the file is larger than 2 GiB".into()).into());
    }
    if lock(&hub.uploads).len() >= 8 {
        return Err(Error::Busy("too many uploads at once".into()).into());
    }
    let id = smhub_auth::new_id()?;
    std::fs::create_dir_all(&hub.cfg.upload_dir).map_err(Error::from)?;
    let path = hub.cfg.upload_dir.join(format!("upload-{id}"));
    std::fs::File::create(&path).map_err(Error::from)?;
    lock(&hub.uploads).insert(id.clone(), Upload { path, size: b.size, received: 0, name: b.name, created: now_unix() });
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "size": b.size, "received": 0 }))).into_response())
}

#[derive(Deserialize)]
pub struct Offset {
    offset: u64,
}

pub async fn upload_piece(State(hub): State<AppState>, Path(id): Path<String>, Query(q): Query<Offset>, body: Bytes) -> ApiResult<Json<Value>> {
    let up = lock(&hub.uploads).get(&id).cloned().ok_or_else(|| Error::not_found("upload"))?;
    if q.offset != up.received {
        return Err(Error::Conflict(format!("expected offset {}", up.received)).into());
    }
    if up.received + body.len() as u64 > up.size {
        return Err(Error::TooLarge("more data than announced".into()).into());
    }
    let path = up.path.clone();
    let data = body.clone();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
        f.write_all(&data)
    })
    .await
    .map_err(|e| Error::internal(e.to_string()))?
    .map_err(Error::from)?;
    let received = {
        let mut m = lock(&hub.uploads);
        let u = m.get_mut(&id).ok_or_else(|| Error::not_found("upload"))?;
        u.received += body.len() as u64;
        u.received
    };
    Ok(Json(json!({ "id": id, "size": up.size, "received": received })))
}

/// Take a finished upload out of the table.
fn take_upload(hub: &AppState, id: &str) -> ApiResult<Upload> {
    let mut m = lock(&hub.uploads);
    let up = m.get(id).cloned().ok_or_else(|| Error::not_found("upload"))?;
    if up.received != up.size {
        return Err(Error::invalid("upload_id", "the upload is not complete").into());
    }
    m.remove(id);
    Ok(up)
}

/// At start no upload is in progress: remove the pieces a previous run left behind.
pub fn clear_stale_uploads(hub: &AppState) {
    let Ok(rd) = std::fs::read_dir(&hub.cfg.upload_dir) else { return };
    for e in rd.flatten() {
        if e.file_name().to_string_lossy().starts_with("upload-") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Remove uploads older than an hour.
pub fn expire_uploads(hub: &AppState) {
    let now = now_unix();
    let old: Vec<Upload> = {
        let mut m = lock(&hub.uploads);
        let ids: Vec<String> = m.iter().filter(|(_, u)| now - u.created > 3600).map(|(k, _)| k.clone()).collect();
        ids.into_iter().filter_map(|k| m.remove(&k)).collect()
    };
    for u in old {
        let _ = std::fs::remove_file(u.path);
    }
}

// ---------------------------------------------------------------- files

#[derive(Deserialize)]
pub struct PathQuery {
    path: Option<String>,
}

pub async fn files_list(State(hub): State<AppState>, Query(q): Query<PathQuery>) -> ApiResult<Json<Value>> {
    let sys = hub.sys.clone();
    let v = tokio::task::spawn_blocking(move || sys.list_folder(q.path.as_deref())).await.map_err(|e| Error::internal(e.to_string()))??;
    Ok(Json(serde_json::to_value(v).unwrap_or(Value::Null)))
}

pub async fn files_delete(State(hub): State<AppState>, Query(q): Query<PathQuery>) -> ApiResult<StatusCode> {
    let p = q.path.ok_or_else(|| Error::invalid("path", "missing"))?;
    let sys = hub.sys.clone();
    tokio::task::spawn_blocking(move || sys.delete_file(&p)).await.map_err(|e| Error::internal(e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

fn disposition(name: &str) -> HeaderValue {
    let safe: String = name.chars().map(|c| if c.is_ascii_graphic() && c != '"' && c != '\\' { c } else { '_' }).collect();
    HeaderValue::from_str(&format!("attachment; filename=\"{safe}\"")).unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

pub async fn file_get(State(hub): State<AppState>, Query(q): Query<PathQuery>) -> ApiResult<Response> {
    let p = q.path.ok_or_else(|| Error::invalid("path", "missing"))?;
    let (_, real) = hub.sys.resolve_file(&p)?;
    let meta = std::fs::metadata(&real).map_err(|_| Error::not_found("file"))?;
    let name = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "download".into());
    if meta.is_dir() {
        let parent = real.parent().map(PathBuf::from).unwrap_or_default();
        let mut c = hub.sys.command("tar");
        c.arg("-cf").arg("-").arg("-C").arg(&parent).arg(&name).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        let mut child = c.spawn().map_err(|e| Error::Unavailable(format!("tar: {e}")))?;
        let out = child.stdout.take().ok_or_else(|| Error::internal("tar output"))?;
        let stream = async_stream(move |tx| async move {
            use tokio::io::AsyncReadExt;
            let mut out = out;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match out.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(Ok(Bytes::copy_from_slice(&buf[..n]))).await.is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = child.wait().await;
        });
        let mut r = (StatusCode::OK, Body::from_stream(stream)).into_response();
        r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/x-tar"));
        r.headers_mut().insert(header::CONTENT_DISPOSITION, disposition(&format!("{name}.tar")));
        return Ok(r);
    }
    let f = tokio::fs::File::open(&real).await.map_err(Error::from)?;
    let mut r = (StatusCode::OK, Body::from_stream(tokio_util_stream(f))).into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    r.headers_mut().insert(header::CONTENT_LENGTH, HeaderValue::from(meta.len()));
    r.headers_mut().insert(header::CONTENT_DISPOSITION, disposition(&name));
    Ok(r)
}

pub async fn file_put(State(hub): State<AppState>, Query(q): Query<PathQuery>, body: Bytes) -> ApiResult<StatusCode> {
    let p = q.path.ok_or_else(|| Error::invalid("path", "missing"))?;
    let sys = hub.sys.clone();
    tokio::task::spawn_blocking(move || sys.save_file(&p, &body)).await.map_err(|e| Error::internal(e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Place {
    upload_id: String,
    path: String,
}

pub async fn file_upload(State(hub): State<AppState>, Json(b): Json<Place>) -> ApiResult<StatusCode> {
    let up = take_upload(&hub, &b.upload_id)?;
    let sys = hub.sys.clone();
    let r = tokio::task::spawn_blocking(move || sys.place_file(&up.path, &b.path).map_err(|e| (e, up.path)))
        .await
        .map_err(|e| Error::internal(e.to_string()))?;
    if let Err((e, path)) = r {
        let _ = std::fs::remove_file(path);
        return Err(e.into());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct FolderBody {
    path: String,
}

pub async fn file_folder(State(hub): State<AppState>, Json(b): Json<FolderBody>) -> ApiResult<StatusCode> {
    hub.sys.make_folder(&b.path)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct MoveBody {
    from: String,
    to: String,
    #[serde(default)]
    copy: bool,
}

pub async fn file_move(State(hub): State<AppState>, Json(b): Json<MoveBody>) -> ApiResult<StatusCode> {
    let sys = hub.sys.clone();
    tokio::task::spawn_blocking(move || sys.move_file(&b.from, &b.to, b.copy)).await.map_err(|e| Error::internal(e.to_string()))??;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------- backups

/// Services of installed apps that are running, except the hub's own (old `service_context`).
/// Services of installed packages that are running, to stop around a backup or restore
/// (old `service_context`). `keep_access`: leave the services the user is connected through.
async fn running_app_services(hub: &AppState, keep_access: bool) -> Vec<String> {
    let snap = hub.sys.services().await;
    let mut out = Vec::new();
    for (pkg, _) in hub.sys.installed_packages() {
        if pkg == "smhub-services" || pkg == "smhub-ui" {
            continue;
        }
        let svc = service_for_package(hub.sys.package_schema(&pkg).as_ref(), &pkg);
        if keep_access && smhub_sys::services::ACCESS_SERVICES.contains(&svc.as_str()) {
            continue;
        }
        if hub.sys.service_exists(&svc) && snap.get(&svc).is_some_and(|s| *s == Svc::Started) && !out.contains(&svc) {
            out.push(svc);
        }
    }
    out
}

async fn stop_all(hub: &AppState, services: &[String]) {
    for s in services {
        if let Err(e) = hub.sys.service_action(s, "stop").await {
            tracing::warn!("stop {s}: {e}");
        }
    }
    if !services.is_empty() {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn start_all(hub: &AppState, services: &[String]) {
    for s in services.iter().rev() {
        if let Err(e) = hub.sys.service_action(s, "start").await {
            tracing::warn!("start {s}: {e}");
        }
    }
    hub.publish_later(&["services", "apps"]);
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    out.push(v);
                    i += 2;
                } else {
                    out.push(b'%');
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub async fn backup_archive(State(hub): State<AppState>, headers: HeaderMap, body: Bytes) -> ApiResult<Response> {
    let is_json = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|c| c.contains("json"));
    let (mut include_home, mut extra): (bool, Vec<String>) = (false, Vec::new());
    if is_json && !body.is_empty() {
        let v: Value = serde_json::from_slice(&body).map_err(|_| Error::bad("not JSON"))?;
        include_home = v.get("include_home").and_then(Value::as_bool).unwrap_or(false);
        extra = v.get("extra_dirs").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    } else {
        for pair in String::from_utf8_lossy(&body).split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = url_decode(v);
            match k {
                "include_home" => include_home = smhub_store::parse_bool(&v),
                "extra_dirs" => extra = v.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect(),
                _ => {}
            }
        }
    }
    let dirs = hub.sys.backup_dirs(include_home, &extra);
    let services = running_app_services(&hub, true).await;
    stop_all(&hub, &services).await;
    let started = hub.sys.backup_start(&dirs, &hub.cfg.upload_dir).await;
    let (mut child, meta, name) = match started {
        Ok(x) => x,
        Err(e) => {
            start_all(&hub, &services).await;
            return Err(e.into());
        }
    };
    let out = child.stdout.take();
    let h2 = Arc::clone(&hub);
    let stream = async_stream(move |tx| async move {
        use tokio::io::AsyncReadExt;
        if let Some(mut out) = out {
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                match out.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(Ok(Bytes::copy_from_slice(&buf[..n]))).await.is_err() {
                            let _ = child.start_kill();
                            break;
                        }
                    }
                }
            }
        }
        let ok = child.wait().await.is_ok_and(|s| s.success());
        let _ = std::fs::remove_file(&meta);
        start_all(&h2, &services).await;
        crate::hw::event(&h2, if ok { "backup_done" } else { "failed" });
    });
    let mut r = (StatusCode::OK, Body::from_stream(stream)).into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/x-xz"));
    r.headers_mut().insert(header::CONTENT_DISPOSITION, disposition(&name));
    Ok(r)
}

#[derive(Deserialize)]
pub struct RestoreBody {
    upload_id: String,
    #[serde(default)]
    dry_run: bool,
}

fn info_json(hub: &AppState, m: &smhub_sys::backups::Metadata) -> Value {
    let home = format!("/home/{}", smhub_sys::DEVICE_USER);
    let me = hub.sys.eeprom().map(|e| e.uuid.clone());
    json!({
        "created": m.timestamp.as_deref().and_then(smhub_model::time::parse_utc).map(smhub_model::time::rfc3339),
        "device_id": m.device_id,
        "same_device": match (&m.device_id, &me) { (Some(a), Some(b)) => Some(a == b), _ => None },
        "apps": m.apps,
        "data_dirs": m.data_dirs.iter().filter(|d| **d != home).collect::<Vec<_>>(),
    })
}

/// Restore an archive as an operation, then restart the daemon so it reopens the restored
/// database.
fn restore_op(hub: &AppState, kind: &'static str, file: PathBuf, remove_after: bool) -> ApiResult<Response> {
    let op = hub.start_op(kind, None)?;
    let h2 = Arc::clone(hub);
    tokio::spawn(async move {
        op.progress(None, Some("verifying"));
        let r = async {
            let meta = h2.sys.backup_metadata(&file).await?;
            op.progress(None, Some("stopping"));
            let services = running_app_services(&h2, false).await;
            stop_all(&h2, &services).await;
            op.progress(None, Some("restoring"));
            let res = h2.sys.backup_restore(&file, &meta).await;
            op.progress(None, Some("starting"));
            start_all(&h2, &services).await;
            res
        }
        .await;
        if remove_after {
            let _ = std::fs::remove_file(&file);
        }
        if r.is_ok() {
            op.reboot_needed();
        }
        op.finish(&r);
        if r.is_ok() {
            tokio::time::sleep(Duration::from_secs(3)).await;
            tracing::info!("restarting to reopen the restored database");
            let mut c = h2.sys.command("rc-service");
            c.args(["smhub-services", "restart"]).kill_on_drop(false).process_group(0);
            if c.spawn().is_err() {
                std::process::exit(0);
            }
        }
    });
    Ok((StatusCode::ACCEPTED, Json(json!(hub.operations().into_iter().rev().find(|o| o.kind == kind)))).into_response())
}

pub async fn backup_restore(State(hub): State<AppState>, Json(b): Json<RestoreBody>) -> ApiResult<Response> {
    if b.dry_run {
        let up = lock(&hub.uploads).get(&b.upload_id).cloned().ok_or_else(|| Error::not_found("upload"))?;
        if up.received != up.size {
            return Err(Error::invalid("upload_id", "the upload is not complete").into());
        }
        let meta = hub.sys.backup_metadata(&up.path).await?;
        return Ok(Json(info_json(&hub, &meta)).into_response());
    }
    let up = take_upload(&hub, &b.upload_id)?;
    restore_op(&hub, "backup_restore", up.path, true)
}

pub async fn factory(State(hub): State<AppState>) -> Json<Value> {
    match hub.sys.factory_backup() {
        Some(p) => {
            let m = std::fs::metadata(&p).ok();
            Json(json!({
                "available": true,
                "created": m.as_ref().and_then(|m| m.modified().ok()).map(rfc3339_system),
                "size": m.map(|m| m.len()),
            }))
        }
        None => Json(json!({ "available": false, "created": null, "size": null })),
    }
}

pub async fn factory_delete(State(hub): State<AppState>) -> ApiResult<StatusCode> {
    hub.sys.delete_factory_backups()?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn factory_archive(State(hub): State<AppState>) -> ApiResult<Response> {
    let p = hub.sys.factory_backup().ok_or_else(|| Error::not_found("factory backup"))?;
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "factory-backup.tar.xz".into());
    let f = tokio::fs::File::open(&p).await.map_err(Error::from)?;
    let mut r = (StatusCode::OK, Body::from_stream(tokio_util_stream(f))).into_response();
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/x-xz"));
    r.headers_mut().insert(header::CONTENT_DISPOSITION, disposition(&name));
    Ok(r)
}

pub async fn factory_restore(State(hub): State<AppState>) -> ApiResult<Response> {
    let p = hub.sys.factory_backup().ok_or_else(|| Error::not_found("factory backup"))?;
    restore_op(&hub, "factory_restore", p, false)
}
