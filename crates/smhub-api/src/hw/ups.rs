//! A UPS board on J10 (full-size SMHUB, docs/peripherals.md "UPS board"). Nothing on the board
//! tells the hub reliably that it is there, so the owner says so (`PATCH /ups`). Then the `system`
//! resource reports it, and the hub turns I2C2 on J10's pins 1 and 3 on for talking to it: an
//! overlay, from the next start. Saved in `smhubd.db` (`hub_settings`, key `ups`).

use std::sync::atomic::Ordering;

use serde_json::{Value, json};
use smhub_model::hardware::Family;
use smhub_model::{Error, Result};

use crate::state::{AppState, lock};

const KEY: &str = "ups";
/// I2C2's controller.
const I2C2: &str = "4020000.i2c";

fn has_j10(hub: &AppState) -> bool {
    hub.sys.model().family == Family::Smhub
}

/// Whether the owner marked a UPS board as connected.
pub fn installed(hub: &AppState) -> bool {
    hub.hw.ups.load(Ordering::SeqCst)
}

/// Load the setting; the boot overlay follows it (a restored backup, an overlay removed by hand).
pub async fn start(hub: &AppState) {
    if !has_j10(hub) {
        return;
    }
    let on = hub.store.run(|db| db.hub_setting(KEY)).await.ok().flatten().is_some_and(|v| smhub_store::parse_bool(&v));
    hub.hw.ups.store(on, Ordering::SeqCst);
    if let Err(e) = hub.sys.set_ups_overlay(on) {
        tracing::warn!("UPS overlay: {e}");
    }
}

/// `GET /ups`.
pub fn state_json(hub: &AppState) -> Value {
    let on = installed(hub);
    let bus = hub.sys.i2c_bus(I2C2);
    json!({
        "available": has_j10(hub),
        "installed": on,
        "bus": bus,
        // the overlay applies at the next start, in either direction
        "restart_needed": has_j10(hub) && on != bus.is_some(),
    })
}

/// Mark a UPS board as connected or not.
pub async fn set_installed(hub: &AppState, on: bool) -> Result<Value> {
    if !has_j10(hub) {
        return Err(Error::Conflict("this model has no UPS connector".into()));
    }
    hub.sys.set_ups_overlay(on)?;
    let text = if on { "true" } else { "false" };
    hub.store.run(move |db| db.set_hub_setting(KEY, text)).await?;
    hub.hw.ups.store(on, Ordering::SeqCst);
    *lock(&hub.features) = None;
    hub.publish_later(&["system"]);
    Ok(state_json(hub))
}

/// The addresses that answer on I2C2, while the bus is on.
pub async fn scan(hub: &AppState) -> Result<Value> {
    let bus = hub.sys.i2c_bus(I2C2).ok_or_else(|| Error::Conflict("the UPS board's bus is not on yet: mark the board as connected and restart the hub".into()))?;
    Ok(json!({ "addresses": hub.sys.i2c_scan(bus).await? }))
}
