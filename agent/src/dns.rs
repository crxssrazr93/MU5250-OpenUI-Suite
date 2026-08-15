//! DNS settings, encrypted DNS, and the resolver cache.
//!
//! `zwrt_router.api` reports the WAN resolvers through `router_get_dns_para`
//! and toggles encrypted DNS through `router_set_secure_dns`. The cache belongs
//! to dnsmasq, whose ubus object exposes `metrics` but nothing to clear it, so
//! that is done the way dnsmasq documents: a HUP.

use std::process::Command;

use serde_json::{json, Value};

use crate::handlers::AppState;
use crate::ubus;

const ROUTER: &str = "zwrt_router.api";

fn parse(body: &[u8]) -> Result<Value, (u16, Value)> {
    serde_json::from_slice(body).map_err(|_| (400, json!({"ok": false, "error": "invalid JSON"})))
}

/// GET /api/doh, GET /api/doh/status
///
/// The encrypted-DNS switch is write-only on this firmware: `router_set_secure_dns`
/// exists with no getter, and nothing in UCI records it. Rather than infer a
/// state and have a UI show a switch that may be lying, `enabled` is reported
/// as null and `state_readable` says why. A toggle that admits it does not know
/// is better than one that is confidently wrong about whether DNS is encrypted.
pub fn doh_get(_state: &AppState) -> (u16, Value) {
    let dns = ubus::call(ROUTER, "router_get_dns_para", Some("{}")).unwrap_or_else(|_| json!({}));
    (
        200,
        json!({"ok": true, "data": {
            "enabled": Value::Null,
            "state_readable": false,
            "notice": "This firmware can turn encrypted DNS on and off but will not report \
                       whether it is on.",
            "mode": dns["wan_dns_mode"],
            "prefer_dns": dns["wan_prefer_dns_manual"],
            "standby_dns": dns["wan_standby_dns_manual"],
            "prefer_dns_v6": dns["ipv6_wan_prefer_dns_manual"],
            "standby_dns_v6": dns["ipv6_wan_standby_dns_manual"],
        }}),
    )
}

/// POST /api/doh — body `{"enabled": true}`
pub fn doh_set(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(enabled) = parsed["enabled"].as_bool() else {
        return (400, json!({"ok": false, "error": "missing 'enabled'"}));
    };

    let params = json!({"enable": i32::from(enabled)}).to_string();
    match ubus::call(ROUTER, "router_set_secure_dns", Some(&params)) {
        Ok(_) => (
            200,
            json!({"ok": true, "data": {
                "enabled": enabled,
                // Said plainly, because the read side cannot confirm it and a
                // client should not present this as verified.
                "notice": "Applied. This firmware does not report the setting back, so it \
                           cannot be read to confirm.",
            }}),
        ),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// GET /api/doh/cache — resolver cache statistics.
pub fn dns_cache_get(_state: &AppState) -> (u16, Value) {
    let metrics = ubus::call("dnsmasq", "metrics", Some("{}")).unwrap_or_else(|_| json!({}));
    (200, json!({"ok": true, "data": metrics}))
}

/// POST /api/doh/cache — clear the resolver cache.
///
/// dnsmasq has no ubus method for this; a HUP is how it documents clearing the
/// cache, and it keeps the process and its leases alive. Restarting the service
/// would also work and would drop every DHCP lease with it, which is a much
/// larger thing to do to a router than forgetting cached lookups.
pub fn dns_cache_clear(_state: &AppState, _body: &[u8]) -> (u16, Value) {
    match Command::new("killall").args(["-HUP", "dnsmasq"]).output() {
        Ok(output) if output.status.success() => (
            200,
            json!({"ok": true, "data": {"cleared": true}}),
        ),
        Ok(output) => (
            503,
            json!({"ok": false, "error": format!(
                "could not signal dnsmasq: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )}),
        ),
        Err(e) => (503, json!({"ok": false, "error": format!("could not signal dnsmasq: {e}")})),
    }
}
