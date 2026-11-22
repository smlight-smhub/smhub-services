//! Per-user preferences. The old `usersettings` columns keep their meaning; what is new lives in
//! the daemon's own file (decision D21).

use axum::Json;
use axum::extract::{Extension, State};
use serde::Deserialize;
use serde_json::{Value, json};
use smhub_model::Error;
use smhub_store::UserSettingsRow;

use crate::auth::Caller;
use crate::error::ApiResult;
use crate::state::AppState;

fn region_of(locale: Option<&str>, language: &str) -> String {
    match locale.map(|l| l.replace('_', "-")) {
        Some(l) if l.len() >= 2 => l,
        _ => match language {
            "uk" => "uk-UA".into(),
            "de" => "de-DE".into(),
            _ => "en-US".into(),
        },
    }
}

/// The usual currency of a region (`ll-CC`), for users who have not chosen one.
fn currency_of(region: &str) -> &'static str {
    let country = region.rsplit(['-', '_']).next().unwrap_or("").to_ascii_uppercase();
    match country.as_str() {
        "US" | "EC" | "SV" | "PR" => "USD",
        "GB" => "GBP",
        "UA" => "UAH",
        "CH" | "LI" => "CHF",
        "PL" => "PLN",
        "CZ" => "CZK",
        "HU" => "HUF",
        "RO" => "RON",
        "SE" => "SEK",
        "NO" => "NOK",
        "DK" => "DKK",
        "IS" => "ISK",
        "TR" => "TRY",
        "CA" => "CAD",
        "AU" => "AUD",
        "NZ" => "NZD",
        "JP" => "JPY",
        "CN" => "CNY",
        "KR" => "KRW",
        "IN" => "INR",
        "BR" => "BRL",
        "MX" => "MXN",
        "ZA" => "ZAR",
        "IL" => "ILS",
        "SG" => "SGD",
        "HK" => "HKD",
        "TW" => "TWD",
        _ => "EUR",
    }
}

fn build(us: &UserSettingsRow, own: &std::collections::BTreeMap<String, String>) -> Value {
    let get = |k: &str| own.get(k).cloned();
    let language = us.language.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| "en".into());
    let language = if language == "ua" { "uk".to_owned() } else { language };
    let units = if us.metric_units == Some(false) { "imperial" } else { "metric" };
    let theme = get("theme").unwrap_or_else(|| "auto".into());
    let clock = get("clock").unwrap_or_else(|| if us.use_12h_clock == Some(true) { "12h".into() } else { "auto".into() });
    let temperature = get("temperature").unwrap_or_else(|| if units == "imperial" { "f".into() } else { "c".into() });
    let dashboard: Value = get("dashboard").and_then(|d| serde_json::from_str(&d).ok()).unwrap_or_else(|| json!([]));
    let region = get("region").unwrap_or_else(|| region_of(us.locale.as_deref(), &language));
    let currency = get("currency").filter(|c| !c.is_empty()).unwrap_or_else(|| currency_of(&region).to_owned());
    json!({
        "language": language,
        "region": region,
        "theme": theme,
        "units": units,
        "temperature": temperature,
        "clock": clock,
        "timezone": get("timezone").unwrap_or_else(|| "hub".into()),
        "level": get("level").unwrap_or_else(|| "simple".into()),
        "dashboard": dashboard,
        "currency": currency,
        "text_size": get("text_size").unwrap_or_else(|| "normal".into()),
        "density": get("density").unwrap_or_else(|| "comfortable".into()),
        "tone": get("tone").unwrap_or_else(|| "green".into()),
    })
}

pub async fn get(State(hub): State<AppState>, Extension(c): Extension<Caller>) -> ApiResult<Json<Value>> {
    let id = c.user_id.clone();
    let v = hub.store.run(move |db| Ok(build(&db.user_settings(&id)?, &db.preferences(&id)?))).await?;
    Ok(Json(v))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    language: Option<String>,
    region: Option<String>,
    theme: Option<String>,
    units: Option<String>,
    temperature: Option<String>,
    clock: Option<String>,
    timezone: Option<String>,
    level: Option<String>,
    dashboard: Option<Vec<String>>,
    currency: Option<String>,
    text_size: Option<String>,
    density: Option<String>,
    tone: Option<String>,
}

/// Appearance choices that are stored as given: field, allowed values.
const LOOK: [(&str, &[&str]); 3] =
    [("text_size", &["small", "normal", "large"]), ("density", &["comfortable", "compact"]), ("tone", &["green", "teal", "blue", "violet", "pink"])];

fn one_of(field: &str, v: &str, allowed: &[&str]) -> Result<(), Error> {
    if allowed.contains(&v) { Ok(()) } else { Err(Error::invalid(field, format!("use one of: {}", allowed.join(", ")))) }
}

fn token(field: &str, v: &str, max: usize) -> Result<(), Error> {
    if v.is_empty() || v.len() > max || !v.chars().all(|c| c.is_ascii_alphanumeric() || "-_/+".contains(c)) {
        Err(Error::invalid(field, "not a valid value"))
    } else {
        Ok(())
    }
}

pub async fn patch(State(hub): State<AppState>, Extension(c): Extension<Caller>, Json(p): Json<Patch>) -> ApiResult<Json<Value>> {
    if let Some(v) = &p.language {
        token("language", v, 8)?;
    }
    if let Some(v) = &p.region {
        token("region", v, 16)?;
    }
    if let Some(v) = &p.theme {
        one_of("theme", v, &["auto", "light", "dark"])?;
    }
    if let Some(v) = &p.units {
        one_of("units", v, &["metric", "imperial"])?;
    }
    if let Some(v) = &p.temperature {
        one_of("temperature", v, &["c", "f"])?;
    }
    if let Some(v) = &p.clock {
        one_of("clock", v, &["auto", "12h", "24h"])?;
    }
    if let Some(v) = &p.timezone {
        token("timezone", v, 64)?;
    }
    if let Some(v) = &p.level {
        one_of("level", v, &["simple", "advanced"])?;
    }
    if let Some(d) = &p.dashboard {
        if d.len() > 32 || d.iter().any(|x| x.is_empty() || x.len() > 32) {
            return Err(Error::invalid("dashboard", "too many or invalid cards").into());
        }
    }
    if let Some(v) = &p.currency {
        if !v.is_empty() && (v.len() != 3 || !v.chars().all(|c| c.is_ascii_uppercase())) {
            return Err(Error::invalid("currency", "use a three-letter code such as EUR").into());
        }
    }
    let look: Vec<(&str, String)> =
        [("text_size", p.text_size), ("density", p.density), ("tone", p.tone)].into_iter().filter_map(|(k, v)| v.map(|v| (k, v))).collect();
    for (k, v) in &look {
        let allowed = LOOK.iter().find(|(f, _)| f == k).map_or(&[][..], |(_, a)| *a);
        one_of(k, v, allowed)?;
    }
    let id = c.user_id.clone();
    let v = hub
        .store
        .run(move |db| {
            let mut us = db.user_settings(&id)?;
            let mut own: Vec<(String, String)> = Vec::new();
            if let Some(l) = p.language {
                us.language = Some(l);
            }
            if let Some(r) = p.region {
                if r.len() <= 8 {
                    us.locale = Some(r.clone());
                }
                own.push(("region".into(), r));
            }
            if let Some(t) = p.theme {
                match t.as_str() {
                    "light" => us.light_theme = Some(true),
                    "dark" => us.light_theme = Some(false),
                    _ => {}
                }
                own.push(("theme".into(), t));
            }
            if let Some(u) = p.units {
                us.metric_units = Some(u == "metric");
            }
            if let Some(t) = p.temperature {
                own.push(("temperature".into(), t));
            }
            if let Some(cl) = p.clock {
                us.use_12h_clock = Some(cl == "12h");
                own.push(("clock".into(), cl));
            }
            if let Some(z) = p.timezone {
                if z != "hub" && z != "browser" {
                    us.timezone = Some(z.clone());
                }
                own.push(("timezone".into(), z));
            }
            if let Some(l) = p.level {
                own.push(("level".into(), l));
            }
            if let Some(d) = p.dashboard {
                own.push(("dashboard".into(), serde_json::to_string(&d)?));
            }
            if let Some(cu) = p.currency {
                own.push(("currency".into(), cu));
            }
            for (k, v) in look {
                own.push((k.into(), v));
            }
            db.save_user_settings(&id, &us)?;
            db.save_preferences(&id, &own)?;
            Ok(build(&db.user_settings(&id)?, &db.preferences(&id)?))
        })
        .await?;
    Ok(Json(v))
}
