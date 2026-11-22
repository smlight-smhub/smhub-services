//! HTTP handlers of the hub's hardware (API contract: `/hardware`, `/settings/hardware`, `/ir`).

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use smhub_model::Error;

use super::{buzzer, find, ir, leds, light, patch_settings, settings_json, state_json, ups};
use crate::auth::Caller;
use crate::error::ApiResult;
use crate::state::{AppState, lock};

pub async fn hardware(State(hub): State<AppState>) -> Json<Value> {
    Json(state_json(&hub))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Play {
    /// A preset id or one of the user's melodies.
    sound: Option<String>,
    rtttl: Option<String>,
}

pub async fn buzzer_play(State(hub): State<AppState>, Extension(c): Extension<Caller>, Json(b): Json<Play>) -> ApiResult<Json<Value>> {
    let (label, text) = match (b.sound, b.rtttl) {
        (Some(s), None) => {
            let t = buzzer::melody_text(&lock(&hub.hw.st), &s).ok_or_else(|| Error::invalid("sound", "no such sound"))?;
            (s, t)
        }
        (None, Some(r)) => {
            let name = smhub_model::rtttl::parse(&r).map_err(|e| Error::invalid("rtttl", e))?.name;
            (if name.is_empty() { "melody".to_owned() } else { name }, r)
        }
        _ => return Err(Error::bad("give sound or rtttl").into()),
    };
    let out = buzzer::request(&hub, &label, &text, buzzer::Prio::Person, &c.label()).await?;
    Ok(Json(serde_json::to_value(out).map_err(Error::from)?))
}

#[derive(Deserialize, Default)]
pub struct Stop {
    #[serde(default = "yes")]
    clear: bool,
}

fn yes() -> bool {
    true
}

pub async fn buzzer_stop(State(hub): State<AppState>, body: Option<Json<Stop>>) -> Json<Value> {
    let clear = body.is_none_or(|Json(b)| b.clear);
    buzzer::stop(&hub, clear).await;
    Json(state_json(&hub))
}

pub async fn light_patch(State(hub): State<AppState>, Extension(c): Extension<Caller>, Json(p): Json<light::LookPatch>) -> ApiResult<Json<Value>> {
    light::set(&hub, p, &c.label()).await?;
    Ok(Json(state_json(&hub)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LedsPatch {
    power: Option<bool>,
    blue: Option<bool>,
}

pub async fn leds_patch(State(hub): State<AppState>, Extension(c): Extension<Caller>, Json(p): Json<LedsPatch>) -> ApiResult<Json<Value>> {
    if p.power.is_none() && p.blue.is_none() {
        return Err(Error::bad("give power or blue").into());
    }
    leds::set(&hub, p.power, p.blue, &c.label()).await?;
    Ok(Json(state_json(&hub)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Find {
    on: bool,
}

pub async fn find_post(State(hub): State<AppState>, Extension(c): Extension<Caller>, Json(b): Json<Find>) -> ApiResult<Json<Value>> {
    find(&hub, b.on, &c.label()).await?;
    Ok(Json(state_json(&hub)))
}

pub async fn settings_get(State(hub): State<AppState>) -> Json<Value> {
    Json(settings_json(&hub))
}

pub async fn settings_patch(State(hub): State<AppState>, Json(p): Json<Value>) -> ApiResult<Json<Value>> {
    if !p.is_object() {
        return Err(Error::bad("send an object").into());
    }
    Ok(Json(patch_settings(&hub, &p).await?))
}

pub async fn ir_get(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(ir::state_json(&hub).await?))
}

pub async fn ir_learn(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    ir::learn(&hub)?;
    Ok(Json(ir::state_json(&hub).await?))
}

pub async fn ir_learn_cancel(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    ir::cancel_learn(&hub);
    Ok(Json(ir::state_json(&hub).await?))
}

pub async fn ir_code_add(State(hub): State<AppState>, Json(b): Json<ir::NewCode>) -> ApiResult<Response> {
    let v = ir::save(&hub, b).await?;
    Ok((StatusCode::CREATED, Json(v)).into_response())
}

pub async fn ir_code_patch(State(hub): State<AppState>, Path(id): Path<String>, Json(p): Json<ir::CodePatch>) -> ApiResult<Json<Value>> {
    Ok(Json(ir::update(&hub, id, p).await?))
}

pub async fn ir_code_delete(State(hub): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    ir::delete(&hub, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn ir_code_send(State(hub): State<AppState>, Extension(c): Extension<Caller>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    ir::send_saved(&hub, &id, &c.label()).await?;
    Ok(Json(json!({ "result": "sent" })))
}

pub async fn ir_send(State(hub): State<AppState>, Extension(c): Extension<Caller>, Json(s): Json<ir::Spec>) -> ApiResult<Json<Value>> {
    ir::send_spec(&hub, s, &c.label()).await?;
    Ok(Json(json!({ "result": "sent" })))
}

pub async fn ir_export(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(ir::export(&hub).await?))
}

pub async fn ir_import(State(hub): State<AppState>, Json(b): Json<ir::Import>) -> ApiResult<Json<Value>> {
    let n = ir::import(&hub, b).await?;
    Ok(Json(json!({ "imported": n })))
}

pub async fn ups_get(State(hub): State<AppState>) -> Json<Value> {
    Json(ups::state_json(&hub))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsPatch {
    installed: bool,
}

pub async fn ups_patch(State(hub): State<AppState>, Json(b): Json<UpsPatch>) -> ApiResult<Json<Value>> {
    Ok(Json(ups::set_installed(&hub, b.installed).await?))
}

pub async fn ups_scan(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    Ok(Json(ups::scan(&hub).await?))
}
