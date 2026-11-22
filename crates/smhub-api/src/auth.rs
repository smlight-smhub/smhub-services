//! Sign-in (decisions D40–D42): deny by default, sessions, the open hub, the local socket,
//! accounts, and the security switch.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::Json;
use axum::extract::{Extension, Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use smhub_model::Error;
use smhub_model::time::now_unix;
use smhub_store::{SYSTEM_USER, SessionRow, UserRow, bool_str, parse_bool};

use crate::error::{ApiError, ApiResult};
use crate::state::{AppState, Hub, Session, limits, lock};

pub const COOKIE: &str = "smhub_session";
/// The factory account every hub ships with; an open hub acts as it.
/// The address is fixed: it is the account in every 1.x database, and the 1.x
/// UI signs itself in with it on an open hub. A different value would lose
/// the account on hubs already in use and break going back to 1.x (D41).
pub const FACTORY_ADMIN: &str = "admin@smlight.tech";

/// Marker put on requests that arrived over the local socket from an accepted user.
#[derive(Clone, Copy, Debug)]
pub struct LocalSocket;

/// Who is calling.
#[derive(Clone, Debug)]
pub struct Caller {
    pub user_id: String,
    pub username: String,
    pub admin: bool,
    /// `password`, `open`, `local` or `token`.
    pub mode: &'static str,
    pub token_hash: Option<String>,
    /// The API token's name, for a caller that sent one.
    pub token_name: Option<String>,
}

impl Caller {
    /// Who did something, for the activity lists: the token's name, else the account.
    pub fn label(&self) -> String {
        self.token_name.clone().unwrap_or_else(|| self.username.clone())
    }

    pub fn user_json(&self) -> Value {
        json!({ "id": self.user_id, "username": self.username, "admin": self.admin })
    }

    pub fn session_json(&self) -> Value {
        json!({ "user": self.user_json(), "mode": if self.mode == "password" { "password" } else { "open" } })
    }
}

fn is_public(method: &Method, path: &str) -> bool {
    matches!(
        (method.as_str(), path),
        ("GET", "/api/v2/health")
            | ("GET", "/api/v2/system")
            | ("GET" | "POST", "/api/v2/session")
            | ("GET" | "HEAD", "/api/v1/login/user-session")
            | ("POST", "/api/v1/login/cookie")
    )
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    for v in headers.get_all(header::COOKIE) {
        let Ok(s) = v.to_str() else { continue };
        for part in s.split(';') {
            if let Some(t) = part.trim().strip_prefix(&format!("{COOKIE}=")) {
                if !t.is_empty() {
                    return Some(t.to_owned());
                }
            }
        }
    }
    None
}

/// A state-changing request from another site is refused: the `Origin` must name this host.
fn origin_ok(req: &Request) -> bool {
    if matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        return true;
    }
    let Some(origin) = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return true };
    let host = req.headers().get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    let authority = origin.split_once("://").map_or(origin, |(_, a)| a).trim_end_matches('/');
    let strip = |s: &str| s.trim_end_matches(":80").trim_end_matches(":443").to_owned();
    strip(authority) == strip(host) || host.is_empty()
}

async fn user_row(hub: &AppState, id: String) -> Option<UserRow> {
    hub.store.run(move |db| db.user_by_id(&id)).await.ok().flatten()
}

/// The account an open hub acts as: the factory admin, else the first administrator.
pub async fn open_user(hub: &AppState) -> Option<UserRow> {
    if let Some(u) = lock(&hub.open_user).clone() {
        return Some(u);
    }
    let u = hub
        .store
        .run(|db| {
            if let Some(u) = db.user_by_email(FACTORY_ADMIN)? {
                return Ok(Some(u));
            }
            Ok(db.users()?.into_iter().find(|u| u.is_superuser && u.is_active))
        })
        .await
        .ok()
        .flatten();
    // A database without any administrator (created here rather than from the package's seed)
    // gets the factory account, with a random password nobody knows: an open hub needs an
    // account to act as, and turning sign-in on sets a password.
    let u = match u {
        Some(u) => Some(u),
        None => {
            let password = smhub_auth::hex(&smhub_auth::random_bytes::<18>().ok()?);
            let hash = hash_password(hub, password).await.ok()?;
            let id = smhub_auth::new_uuid().ok()?;
            let made = hub.store.run(move |db| db.create_user(&id, FACTORY_ADMIN, &hash, true)).await;
            match made {
                Ok(u) => {
                    tracing::info!("no administrator account: created {FACTORY_ADMIN}");
                    Some(u)
                }
                Err(e) => {
                    tracing::warn!("creating {FACTORY_ADMIN}: {e}");
                    None
                }
            }
        }
    };
    *lock(&hub.open_user) = u.clone();
    u
}

fn caller_of(u: &UserRow, mode: &'static str, token_hash: Option<String>) -> Caller {
    Caller { user_id: u.id.clone(), username: u.email.clone(), admin: u.is_superuser, mode, token_hash, token_name: None }
}

/// `Authorization: Bearer <token>`.
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let t = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer "))?.trim();
    (!t.is_empty() && t.len() <= 128).then(|| t.to_owned())
}

/// A caller with an API token. Its last use is written at most once a minute.
async fn token_caller(hub: &AppState, token: &str) -> Option<Caller> {
    let h = smhub_auth::token_hash(token);
    let info = lock(&hub.tokens).get(&h).cloned()?;
    let u = user_row(hub, info.user_id.clone()).await.filter(|u| u.is_active)?;
    let now = now_unix();
    if now - info.last_written >= 60 {
        if let Some(t) = lock(&hub.tokens).get_mut(&h) {
            t.last_written = now;
        }
        let id = info.id.clone();
        let st = Arc::clone(hub);
        tokio::spawn(async move {
            let _ = st.store.run(move |db| db.token_used(&id, now)).await;
        });
    }
    let mut c = caller_of(&u, "token", None);
    c.token_name = Some(info.name);
    Some(c)
}

/// What a token may not do: manage sign-in, accounts and tokens, or open a shell.
fn token_forbidden(path: &str) -> bool {
    ["/api/v2/tokens", "/api/v2/users", "/api/v2/security", "/api/v2/session", "/ws/v2/terminal"].iter().any(|p| path == *p || path.starts_with(&format!("{p}/")))
}

async fn resolve(hub: &AppState, token: Option<String>, local: bool) -> Option<Caller> {
    if let Some(token) = token {
        let h = smhub_auth::token_hash(&token);
        let sess = lock(&hub.sessions).get(&h).cloned();
        if let Some(s) = sess {
            if s.expires > now_unix() {
                if let Some(u) = user_row(hub, s.user_id).await.filter(|u| u.is_active) {
                    return Some(caller_of(&u, "password", Some(h)));
                }
            } else {
                lock(&hub.sessions).remove(&h);
            }
        }
    }
    if local {
        return open_user(hub).await.map(|u| caller_of(&u, "local", None));
    }
    if !hub.security_required.load(Ordering::Relaxed) {
        return open_user(hub).await.map(|u| caller_of(&u, "open", None));
    }
    None
}

/// The sign-in layer in front of every route: everything is private unless listed in
/// `is_public` (decision D40).
pub async fn layer(State(hub): State<AppState>, mut req: Request, next: Next) -> Response {
    if !origin_ok(&req) {
        return ApiError(Error::Forbidden("requests from other sites are refused".into())).into_response();
    }
    let public = is_public(req.method(), req.uri().path());
    if let Some(bearer) = bearer_token(req.headers()) {
        let Some(c) = token_caller(&hub, &bearer).await else {
            return ApiError(Error::Unauthorized("the API token is not valid".into())).into_response();
        };
        if token_forbidden(req.uri().path()) {
            return ApiError(Error::Forbidden("an API token cannot do this; sign in instead".into())).into_response();
        }
        req.extensions_mut().insert(c);
        let mut res = next.run(req).await;
        res.headers_mut().entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("no-store"));
        return res;
    }
    let token = cookie_token(req.headers());
    let local = req.extensions().get::<LocalSocket>().is_some();
    match resolve(&hub, token, local).await {
        Some(c) => {
            req.extensions_mut().insert(c);
        }
        None if !public => return ApiError(Error::Unauthorized("sign in first".into())).into_response(),
        None => {}
    }
    let mut res = next.run(req).await;
    res.headers_mut().entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("no-store"));
    res
}

/// Load the security switch and the sessions at start.
pub async fn load(hub: &AppState) -> smhub_model::Result<()> {
    let now = now_unix();
    let (required, sessions, setup_needed, tokens) = hub
        .store
        .run(move |db| {
            db.delete_expired_sessions(now)?;
            let required = db.page_setting("security", "enabled")?.is_some_and(|v| parse_bool(&v));
            // First-run setup: only on a hub the old backend never ran on. The old backend
            // created `system@localhost` at every start; the package's seed database has no
            // such account. A hub with sign-in turned on was set up too.
            if db.meta("setup")?.is_none() && (required || db.user_by_email(SYSTEM_USER)?.is_some()) {
                db.set_meta("setup", "upgrade")?;
            }
            let setup_needed = db.meta("setup")?.is_none();
            Ok((required, db.sessions()?, setup_needed, db.tokens()?))
        })
        .await?;
    let tokens = tokens_of(tokens);
    hub.security_required.store(required, Ordering::Relaxed);
    hub.setup_needed.store(setup_needed, Ordering::Relaxed);
    let mut m = lock(&hub.sessions);
    for s in sessions {
        m.insert(s.token_hash, Session { user_id: s.user_id, expires: s.expires });
    }
    *lock(&hub.tokens) = tokens;
    Ok(())
}

fn tokens_of(rows: Vec<smhub_store::TokenRow>) -> std::collections::BTreeMap<String, crate::state::TokenInfo> {
    rows.into_iter()
        .map(|t| (t.token_hash, crate::state::TokenInfo { id: t.id, user_id: t.user_id, name: t.name, last_written: t.last_used.unwrap_or(0) }))
        .collect()
}

fn token_json(t: &smhub_store::TokenRow) -> Value {
    json!({
        "id": t.id,
        "name": t.name,
        "created": smhub_model::time::rfc3339(t.created),
        "last_used": t.last_used.map(smhub_model::time::rfc3339),
    })
}

pub async fn tokens_list(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    let rows = hub.store.run(|db| db.tokens()).await?;
    Ok(Json(Value::Array(rows.iter().map(token_json).collect())))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewToken {
    name: String,
}

/// Tokens kept at most.
pub const MAX_TOKENS: usize = 32;

/// A new token. Its value is shown once, in this answer; the hub keeps only its hash.
pub async fn tokens_create(State(hub): State<AppState>, Extension(c): Extension<Caller>, Json(b): Json<NewToken>) -> ApiResult<Response> {
    let name = b.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
        return Err(Error::invalid("name", "1 to 40 characters").into());
    }
    let (raw, _) = smhub_auth::new_token()?;
    let token = format!("smhub_{raw}");
    let hash = smhub_auth::token_hash(&token);
    let row = smhub_store::TokenRow { id: smhub_auth::new_id()?, name, token_hash: hash, user_id: c.user_id.clone(), created: now_unix(), last_used: None };
    let r2 = row.clone();
    hub.store
        .run(move |db| {
            if db.tokens()?.len() >= MAX_TOKENS {
                return Err(Error::Conflict(format!("at most {MAX_TOKENS} tokens")));
            }
            db.add_token(&r2)
        })
        .await?;
    lock(&hub.tokens).insert(row.token_hash.clone(), crate::state::TokenInfo { id: row.id.clone(), user_id: row.user_id.clone(), name: row.name.clone(), last_written: 0 });
    let mut v = token_json(&row);
    if let Some(o) = v.as_object_mut() {
        o.insert("token".into(), Value::String(token));
    }
    Ok((StatusCode::CREATED, Json(v)).into_response())
}

pub async fn tokens_delete(State(hub): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let i2 = id.clone();
    if !hub.store.run(move |db| db.delete_token(&i2)).await? {
        return Err(Error::not_found("token").into());
    }
    lock(&hub.tokens).retain(|_, t| t.id != id);
    Ok(StatusCode::NO_CONTENT)
}

/// The paths the session cookie is sent to. Cookies are not separated by port, so a cookie for
/// `/` would also reach the web UIs of apps on other ports of the hub; these two paths are the
/// only ones that need it.
const COOKIE_PATHS: [&str; 2] = ["/api/v2", "/ws/v2"];

/// `Set-Cookie` headers carrying a session token.
pub struct Cookies(Vec<HeaderValue>);

impl Cookies {
    fn set(token: &str) -> Self {
        let max = limits::SESSION_DAYS * 86_400;
        Self(
            COOKIE_PATHS
                .iter()
                .filter_map(|p| HeaderValue::from_str(&format!("{COOKIE}={token}; Path={p}; HttpOnly; SameSite=Strict; Max-Age={max}")).ok())
                .collect(),
        )
    }

    fn clear() -> Self {
        // also the cookie for `/` that development builds set
        Self(
            COOKIE_PATHS
                .iter()
                .chain(&["/"])
                .filter_map(|p| HeaderValue::from_str(&format!("{COOKIE}=; Path={p}; HttpOnly; SameSite=Strict; Max-Age=0")).ok())
                .collect(),
        )
    }

    fn apply(self, r: &mut Response) {
        for c in self.0 {
            r.headers_mut().append(header::SET_COOKIE, c);
        }
    }
}

/// Start a session for a user and return the cookie headers.
async fn new_session(hub: &AppState, user_id: &str) -> ApiResult<Cookies> {
    let (token, hash) = smhub_auth::new_token()?;
    let now = now_unix();
    let row = SessionRow { token_hash: hash.clone(), user_id: user_id.to_owned(), created: now, expires: now + limits::SESSION_DAYS * 86_400 };
    let r2 = row.clone();
    hub.store.run(move |db| db.add_session(&r2)).await?;
    lock(&hub.sessions).insert(hash, Session { user_id: row.user_id, expires: row.expires });
    Ok(Cookies::set(&token))
}

/// Check a password on the blocking pool, one at a time.
async fn check_password(hub: &AppState, password: String, hash: String) -> bool {
    let Ok(_permit) = hub.password_checks.acquire().await else { return false };
    tokio::task::spawn_blocking(move || smhub_auth::verify_password(&password, &hash)).await.unwrap_or(false)
}

async fn hash_password(hub: &AppState, password: String) -> ApiResult<String> {
    let _permit = hub.password_checks.acquire().await.map_err(|_| Error::internal("password check closed"))?;
    tokio::task::spawn_blocking(move || smhub_auth::hash_password(&password))
        .await
        .map_err(|e| Error::internal(e.to_string()))?
        .map_err(ApiError)
}

fn too_many_failures(hub: &Hub) -> bool {
    let now = now_unix();
    let mut f = lock(&hub.auth_failures);
    f.retain(|t| now - t < 60);
    f.len() >= 10
}

#[derive(Deserialize)]
pub struct SignIn {
    username: String,
    password: String,
}

pub async fn session_get(caller: Option<Extension<Caller>>) -> ApiResult<Json<Value>> {
    match caller {
        Some(Extension(c)) => Ok(Json(c.session_json())),
        None => Err(Error::Unauthorized("not signed in".into()).into()),
    }
}

pub async fn session_post(State(hub): State<AppState>, Json(b): Json<SignIn>) -> ApiResult<Response> {
    if too_many_failures(&hub) {
        return Err(Error::RateLimited("too many failed sign-ins; wait a minute".into()).into());
    }
    let name = b.username.trim().to_owned();
    let n2 = name.clone();
    let user = hub
        .store
        .run(move |db| Ok(db.user_by_email(&n2)?.or(db.user_by_email(&n2.to_lowercase())?)))
        .await?
        .filter(|u| u.is_active && u.email != SYSTEM_USER);
    let ok = match &user {
        Some(u) => check_password(&hub, b.password.clone(), u.hashed_password.clone()).await,
        None => {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            false
        }
    };
    let Some(user) = user.filter(|_| ok) else {
        lock(&hub.auth_failures).push(now_unix());
        return Err(Error::AuthFailed("wrong user name or password".into()).into());
    };
    if smhub_auth::is_legacy(&user.hashed_password) {
        let new = hash_password(&hub, b.password).await?;
        let id = user.id.clone();
        hub.store.run(move |db| db.set_password_hash(&id, &new)).await?;
    }
    let cookie = new_session(&hub, &user.id).await?;
    let c = caller_of(&user, "password", None);
    let mut r = Json(c.session_json()).into_response();
    cookie.apply(&mut r);
    Ok(r)
}

pub async fn session_delete(State(hub): State<AppState>, Extension(c): Extension<Caller>) -> ApiResult<Response> {
    if let Some(h) = c.token_hash {
        lock(&hub.sessions).remove(&h);
        hub.store.run(move |db| db.delete_session(&h)).await?;
    }
    let mut r = StatusCode::NO_CONTENT.into_response();
    Cookies::clear().apply(&mut r);
    Ok(r)
}

/// `GET /api/v1/login/user-session`: the OS's readiness check asks this before apps start.
/// It answers 401 without a session, which the check accepts as "ready".
/// A browser can keep the 1.x UI in its cache for days after the update (nginx sends no
/// cache headers). That UI signs in with this call and reloads the page when it succeeds; the
/// reload fetches the new UI. Nothing is signed in here.
pub async fn legacy_sign_in() -> Response {
    (StatusCode::OK, Json(json!({ "detail": "The hub was updated; reloading" }))).into_response()
}

pub async fn legacy_ready() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "detail": "Not authenticated" }))).into_response()
}

pub async fn security_get(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    let email = hub.store.run(|db| db.page_setting("security", "email")).await?;
    Ok(Json(json!({ "required": hub.security_required.load(Ordering::Relaxed), "username": email.filter(|e| !e.is_empty()) })))
}

fn check_new_password(p: &str) -> ApiResult<()> {
    if p.chars().count() < 8 || p.len() > 128 {
        return Err(Error::invalid("password", "use 8 to 128 characters").into());
    }
    Ok(())
}

fn check_username(u: &str) -> ApiResult<()> {
    if u.is_empty() || u.len() > 255 || u.chars().any(char::is_control) || u == SYSTEM_USER {
        return Err(Error::invalid("username", "choose another user name").into());
    }
    Ok(())
}

pub async fn security_enable(State(hub): State<AppState>, Json(b): Json<SignIn>) -> ApiResult<Response> {
    let username = b.username.trim().to_owned();
    check_username(&username)?;
    check_new_password(&b.password)?;
    let hash = hash_password(&hub, b.password).await?;
    // The factory account's password is public (the previous UI carries it). While sign-in is
    // on it must not open the hub: it gets the owner's password when it is the owner's account,
    // otherwise a random one nobody knows; its own waits in `smhubd.db` for sign-in to go off.
    let factory = username == FACTORY_ADMIN;
    let locked = if factory { hash.clone() } else { hash_password(&hub, smhub_auth::hex(&smhub_auth::random_bytes::<18>()?)).await? };
    let id = smhub_auth::new_uuid()?;
    let u2 = username.clone();
    let user = hub
        .store
        .run(move |db| {
            db.factory_aside(FACTORY_ADMIN, &locked)?;
            let user = match db.user_by_email(&u2)? {
                Some(u) if factory => u,
                Some(u) => {
                    db.set_password_hash(&u.id, &hash)?;
                    u
                }
                None => db.create_user(&id, &u2, &hash, true)?,
            };
            db.save_page_settings(
                "security",
                "Authorization Settings",
                &[("enabled".into(), bool_str(true)), ("email".into(), u2.clone())],
            )?;
            Ok(user)
        })
        .await?;
    hub.security_required.store(true, Ordering::Relaxed);
    *lock(&hub.open_user) = None;
    let cookie = new_session(&hub, &user.id).await?;
    let c = caller_of(&user, "password", None);
    let mut r = Json(c.session_json()).into_response();
    cookie.apply(&mut r);
    Ok(r)
}

pub async fn security_disable(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    hub.store
        .run(|db| {
            db.save_page_settings("security", "Authorization Settings", &[("enabled".into(), bool_str(false))])?;
            // the factory account's own password back, so the previous versions' UI can sign
            // itself in on the open hub after going back
            db.factory_back(FACTORY_ADMIN)
        })
        .await?;
    *lock(&hub.open_user) = None;
    hub.security_required.store(false, Ordering::Relaxed);
    security_get(State(hub)).await
}

fn user_json(u: &UserRow) -> Value {
    json!({ "id": u.id, "username": u.email, "admin": u.is_superuser })
}

pub async fn users_list(State(hub): State<AppState>) -> ApiResult<Json<Value>> {
    let users = hub.store.run(|db| db.users()).await?;
    Ok(Json(Value::Array(users.iter().filter(|u| u.email != SYSTEM_USER).map(user_json).collect())))
}

#[derive(Deserialize)]
pub struct NewUser {
    username: String,
    password: String,
    #[serde(default = "yes")]
    admin: bool,
}

fn yes() -> bool {
    true
}

pub async fn users_create(State(hub): State<AppState>, Json(b): Json<NewUser>) -> ApiResult<Response> {
    let username = b.username.trim().to_owned();
    check_username(&username)?;
    check_new_password(&b.password)?;
    let hash = hash_password(&hub, b.password).await?;
    let id = smhub_auth::new_uuid()?;
    let u = hub.store.run(move |db| db.create_user(&id, &username, &hash, b.admin)).await?;
    Ok((StatusCode::CREATED, Json(user_json(&u))).into_response())
}

#[derive(Deserialize)]
pub struct UserPatch {
    password: Option<String>,
    current_password: Option<String>,
}

pub async fn users_patch(
    State(hub): State<AppState>,
    Extension(c): Extension<Caller>,
    Path(id): Path<String>,
    Json(b): Json<UserPatch>,
) -> ApiResult<Json<Value>> {
    let i2 = id.clone();
    let user = hub.store.run(move |db| db.user_by_id(&i2)).await?.filter(|u| u.email != SYSTEM_USER).ok_or_else(|| Error::not_found("account"))?;
    if let Some(p) = b.password {
        check_new_password(&p)?;
        if c.mode == "password" && c.user_id == user.id {
            let cur = b.current_password.unwrap_or_default();
            if !check_password(&hub, cur, user.hashed_password.clone()).await {
                return Err(Error::invalid("current_password", "the current password is wrong").into());
            }
        }
        let hash = hash_password(&hub, p).await?;
        let uid = user.id.clone();
        let keep = c.token_hash.clone();
        hub.store
            .run(move |db| {
                db.set_password_hash(&uid, &hash)?;
                db.delete_user_sessions(&uid, keep.as_deref())
            })
            .await?;
        let keep = c.token_hash.clone();
        lock(&hub.sessions).retain(|h, s| s.user_id != user.id || Some(h) == keep.as_ref());
    }
    Ok(Json(user_json(&user)))
}

pub async fn users_delete(State(hub): State<AppState>, Extension(c): Extension<Caller>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    if c.user_id == id {
        return Err(Error::Conflict("you cannot remove your own account".into()).into());
    }
    let i2 = id.clone();
    hub.store
        .run(move |db| {
            let users: Vec<UserRow> = db.users()?.into_iter().filter(|u| u.email != SYSTEM_USER).collect();
            if !users.iter().any(|u| u.id == i2) {
                return Err(Error::not_found("account"));
            }
            if users.len() <= 1 {
                return Err(Error::Conflict("the last account cannot be removed".into()));
            }
            db.delete_user(&i2)?;
            db.delete_user_sessions(&i2, None)
        })
        .await?;
    lock(&hub.sessions).retain(|_, s| s.user_id != id);
    *lock(&hub.open_user) = None;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete expired sessions from memory and the store (housekeeping, once a day).
pub async fn prune_sessions(hub: &Arc<Hub>) {
    let now = now_unix();
    lock(&hub.sessions).retain(|_, s| s.expires > now);
    let _ = hub.store.run(move |db| db.delete_expired_sessions(now)).await;
}
