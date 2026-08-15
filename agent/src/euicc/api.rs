//! HTTP surface for the eUICC module.
//!
//! Kept separate from the transport and ES10 layers so those can be reused by
//! another agent without pulling in this project's handler conventions.

use serde_json::{json, Value};

use super::{eid, es10::ProfileInfo, mask, profiles, status};

/// GET /api/euicc/status
pub fn euicc_status() -> (u16, Value) {
    match status() {
        Ok(s) => (
            200,
            json!({"ok": true, "data": {
                "card_present": s.card_present,
                "euicc_available": s.isdr_available,
                "detail": s.detail,
            }}),
        ),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// GET /api/euicc/eid — masked by default.
///
/// `?full=true` returns the complete EID. It is still an authenticated,
/// LAN-only route, but the default keeps the identifier out of casual screen
/// shares and dashboard screenshots.
pub fn euicc_eid(query: Option<&str>) -> (u16, Value) {
    match eid() {
        Ok(value) => {
            let full = wants_full(query);
            (
                200,
                json!({"ok": true, "data": {
                    "eid": if full { value.clone() } else { mask(&value) },
                    "masked": !full,
                }}),
            )
        }
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// GET /api/euicc/profiles — ICCIDs masked by default.
pub fn euicc_profiles(query: Option<&str>) -> (u16, Value) {
    match profiles() {
        Ok(list) => {
            let full = wants_full(query);
            let items: Vec<Value> = list.iter().map(|p| profile_json(p, full)).collect();
            (
                200,
                json!({"ok": true, "data": {
                    "profiles": items,
                    "count": list.len(),
                    "masked": !full,
                }}),
            )
        }
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

fn profile_json(profile: &ProfileInfo, full: bool) -> Value {
    let iccid = profile
        .iccid
        .as_ref()
        .map(|v| if full { v.clone() } else { mask(v) });
    json!({
        "iccid": iccid,
        "isdp_aid": profile.isdp_aid,
        "state": profile.state_label(),
        "enabled": profile.is_enabled(),
        "class": profile.class_label(),
        "nickname": profile.nickname,
        "service_provider": profile.service_provider,
        "name": profile.name,
        "disable_blocked": profile.policy.disable_blocked,
        "delete_blocked": profile.policy.delete_blocked,
    })
}

/// Parse `full=true` out of a query string.
fn wants_full(query: Option<&str>) -> bool {
    let Some(query) = query else {
        return false;
    };
    query.split('&').any(|pair| {
        matches!(
            pair.split_once('='),
            Some(("full", "true" | "1" | "yes"))
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_flag() {
        assert!(wants_full(Some("full=true")));
        assert!(wants_full(Some("x=1&full=1")));
        assert!(wants_full(Some("full=yes")));
        assert!(!wants_full(Some("full=false")));
        assert!(!wants_full(Some("fullish=true")));
        assert!(!wants_full(Some("")));
        assert!(!wants_full(None));
    }

    #[test]
    fn masks_profile_identifiers_by_default() {
        let profile = ProfileInfo {
            iccid: Some("89490102186110201029".into()),
            state: Some(1),
            class: Some(2),
            name: Some("Mobitel".into()),
            ..Default::default()
        };

        let masked = profile_json(&profile, false);
        assert_eq!(masked["iccid"], "8949************1029");
        assert_eq!(masked["state"], "enabled");
        assert_eq!(masked["enabled"], true);
        assert_eq!(masked["class"], "operational");
        assert_eq!(masked["name"], "Mobitel");

        let full = profile_json(&profile, true);
        assert_eq!(full["iccid"], "89490102186110201029");
    }

    #[test]
    fn renders_profile_without_iccid() {
        let profile = ProfileInfo::default();
        let value = profile_json(&profile, false);
        assert!(value["iccid"].is_null());
        assert_eq!(value["state"], "unknown");
        assert_eq!(value["enabled"], false);
    }
}

// --- Write operations ---
//
// All of these change card state and are gated on `X-Confirm: true` by the
// route table, alongside reboot and shutdown.

use super::{
    chip_info, delete_profile, disable_profile, download, enable_profile, notifications,
    process_notifications, process_notifications_via_relay, relay, remove_notifications,
    set_nickname, DownloadRequest,
};
use std::time::Duration;
use super::lpac::{self, LpacResult};

fn parse_body(body: &[u8]) -> Result<Value, (u16, Value)> {
    serde_json::from_slice(body)
        .map_err(|_| (400, json!({"ok": false, "error": "invalid JSON"})))
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Turn an lpac result into a response. lpac reports failure inside its own
/// payload as a non-zero `code`, so a zero exit is not on its own success.
fn lpac_response(result: Result<LpacResult, String>) -> (u16, Value) {
    match result {
        Ok(result) => {
            let code = result.payload["code"].as_i64().unwrap_or(0);
            if code != 0 {
                let message = result.payload["message"]
                    .as_str()
                    .unwrap_or("operation failed")
                    .to_string();
                return (
                    502,
                    json!({"ok": false, "error": message, "data": {"progress": result.progress}}),
                );
            }
            (
                200,
                json!({"ok": true, "data": {
                    "result": result.payload["data"],
                    "progress": result.progress,
                }}),
            )
        }
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// Flag a successful switch as needing a reboot to take effect.
///
/// Without REFRESH the card is switched but the modem still reads the old
/// profile, so the user sees no change and assumes it failed. Saying so is the
/// difference between a confusing no-op and a clear next step.
fn with_reboot_notice((status, mut body): (u16, Value), refreshed: bool) -> (u16, Value) {
    if status == 200 && !refreshed {
        if let Some(data) = body["data"].as_object_mut() {
            data.insert("reboot_required".into(), Value::Bool(true));
            data.insert(
                "notice".into(),
                Value::String(
                    "The profile was switched on the card, but this modem cannot be \
                     refreshed live. Reboot the router for it to take effect."
                        .into(),
                ),
            );
        }
    }
    (status, body)
}

/// Guard every write route: without lpac there is no write path at all, and a
/// clear message beats a confusing failure deeper in.
fn require_lpac() -> Option<(u16, Value)> {
    if lpac::available() {
        return None;
    }
    Some((
        501,
        json!({"ok": false, "error": format!(
            "lpac is not installed at {} — profile management is unavailable",
            lpac::binary_path().display()
        )}),
    ))
}

/// POST /api/euicc/download
///
/// Body: `{"activation_code": "LPA:1$..."}` or
/// `{"smdp": "...", "matching_id": "...", "confirmation_code": "...", "imei": "..."}`
pub fn euicc_download(body: &[u8]) -> (u16, Value) {
    if let Some(unavailable) = require_lpac() {
        return unavailable;
    }
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let request = DownloadRequest {
        activation_code: string_field(&parsed, "activation_code"),
        smdp: string_field(&parsed, "smdp"),
        matching_id: string_field(&parsed, "matching_id"),
        confirmation_code: string_field(&parsed, "confirmation_code"),
        imei: string_field(&parsed, "imei"),
    };

    // Validate before running so a bad field is a 400, not a 502 from lpac.
    if let Err(e) = request.to_args() {
        return (400, json!({"ok": false, "error": e}));
    }

    // `relay: true` means a client on the LAN will carry the ES9+ traffic,
    // which is the normal case on a router whose WAN does not exist yet.
    let use_relay = parsed["relay"].as_bool().unwrap_or(false);
    if let Some(refused) = require_relay_client(use_relay) {
        return refused;
    }
    lpac_response(download(&request, use_relay))
}

/// POST /api/euicc/enable — body: `{"iccid": "..."}`
pub fn euicc_enable(body: &[u8]) -> (u16, Value) {
    if let Some(unavailable) = require_lpac() {
        return unavailable;
    }
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(iccid) = string_field(&parsed, "iccid") else {
        return (400, json!({"ok": false, "error": "missing 'iccid'"}));
    };
    // Off by default — this device rejects EnableProfile with the refresh flag.
    // See enable_profile.
    let refresh = parsed["refresh"].as_bool().unwrap_or(false);
    // The card queues a notification for this switch and the agent delivers it
    // before returning; on a router with no WAN yet that needs a relay client,
    // exactly as a download does.
    let use_relay = parsed["relay"].as_bool().unwrap_or(false);
    if let Some(refused) = require_relay_client(use_relay) {
        return refused;
    }
    with_reboot_notice(
        lpac_response(enable_profile(&iccid, refresh, use_relay)),
        refresh,
    )
}

/// POST /api/euicc/disable — body: `{"iccid": "...", "force": false}`
pub fn euicc_disable(body: &[u8]) -> (u16, Value) {
    if let Some(unavailable) = require_lpac() {
        return unavailable;
    }
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(iccid) = string_field(&parsed, "iccid") else {
        return (400, json!({"ok": false, "error": "missing 'iccid'"}));
    };
    let force = parsed["force"].as_bool().unwrap_or(false);
    let refresh = parsed["refresh"].as_bool().unwrap_or(false);
    let use_relay = parsed["relay"].as_bool().unwrap_or(false);
    if let Some(refused) = require_relay_client(use_relay) {
        return refused;
    }
    match disable_profile(&iccid, force, refresh, use_relay) {
        // The last-enabled-profile guard is a client mistake, not a card fault.
        Err(e) if e.starts_with("refusing") => (409, json!({"ok": false, "error": e})),
        other => with_reboot_notice(lpac_response(other), refresh),
    }
}

/// POST /api/euicc/delete — body: `{"iccid": "..."}`
pub fn euicc_delete(body: &[u8]) -> (u16, Value) {
    if let Some(unavailable) = require_lpac() {
        return unavailable;
    }
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(iccid) = string_field(&parsed, "iccid") else {
        return (400, json!({"ok": false, "error": "missing 'iccid'"}));
    };
    // Without this the delete notification never reaches the SM-DP+, and the
    // profile stays spent there: no app on any device can reinstall it.
    let use_relay = parsed["relay"].as_bool().unwrap_or(false);
    if let Some(refused) = require_relay_client(use_relay) {
        return refused;
    }
    match delete_profile(&iccid, use_relay) {
        Err(e) if e.starts_with("disable the profile") => (409, json!({"ok": false, "error": e})),
        other => lpac_response(other),
    }
}

/// POST /api/euicc/nickname — body: `{"iccid": "...", "nickname": "..."}`
pub fn euicc_nickname(body: &[u8]) -> (u16, Value) {
    if let Some(unavailable) = require_lpac() {
        return unavailable;
    }
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(iccid) = string_field(&parsed, "iccid") else {
        return (400, json!({"ok": false, "error": "missing 'iccid'"}));
    };
    // An empty nickname is meaningful: it clears the current one.
    let nickname = parsed["nickname"].as_str().unwrap_or("");
    lpac_response(set_nickname(&iccid, nickname))
}

/// Refuse a relay-backed operation when nothing is there to carry it.
///
/// Without this the operation runs, every HTTP leg fails, and the user is shown
/// whichever ES9+ step died — `es9p_handle_notification` and the like — which
/// says nothing about the actual problem. It also used to take minutes to get
/// there, one response timeout per leg.
fn require_relay_client(use_relay: bool) -> Option<(u16, Value)> {
    if !use_relay || relay::client_connected() {
        return None;
    }
    Some((
        409,
        json!({"ok": false, "error":
            "no relay client is connected. Start one on a device that can reach both this \
             router and the internet, then try again."}),
    ))
}

/// GET /api/euicc/chip
pub fn euicc_chip() -> (u16, Value) {
    if let Some(unavailable) = require_lpac() {
        return unavailable;
    }
    lpac_response(chip_info())
}

/// GET /api/euicc/notifications — ICCIDs masked by default.
///
/// Each notification names the profile it belongs to, so this list carries the
/// same identifiers `/api/euicc/profiles` masks. Leaving them in the clear here
/// would make the masking elsewhere pointless.
pub fn euicc_notifications(query: Option<&str>) -> (u16, Value) {
    if let Some(unavailable) = require_lpac() {
        return unavailable;
    }
    let full = wants_full(query);
    let (status, mut body) = lpac_response(notifications());
    if status == 200 {
        mask_notification_iccids(&mut body, full);
    }
    (status, body)
}

/// Mask the ICCID on each notification in place, and record that it happened.
fn mask_notification_iccids(body: &mut Value, full: bool) {
    if !full {
        if let Some(list) = body["data"]["result"].as_array_mut() {
            for item in list {
                if let Some(iccid) = item["iccid"].as_str() {
                    item["iccid"] = Value::String(mask(iccid));
                }
            }
        }
    }
    if let Some(data) = body["data"].as_object_mut() {
        data.insert("masked".into(), Value::Bool(!full));
    }
}

fn sequence_numbers(parsed: &Value) -> Result<Vec<u64>, (u16, Value)> {
    parsed["sequences"]
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_u64).collect())
        .ok_or_else(|| (400, json!({"ok": false, "error": "missing 'sequences' array"})))
}

/// POST /api/euicc/notifications/process — body: `{"sequences": [0, 1]}`
pub fn euicc_notifications_process(body: &[u8]) -> (u16, Value) {
    if let Some(unavailable) = require_lpac() {
        return unavailable;
    }
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let use_relay = parsed["relay"].as_bool().unwrap_or(false);
    if let Some(refused) = require_relay_client(use_relay) {
        return refused;
    }
    match sequence_numbers(&parsed) {
        Ok(sequences) if use_relay => lpac_response(process_notifications_via_relay(&sequences)),
        Ok(sequences) => lpac_response(process_notifications(&sequences)),
        Err(e) => e,
    }
}

// --- Relay endpoints ---
//
// Used by a native client (the Android app, or the test client in scripts/)
// that has internet and performs the ES9+ requests on the router's behalf.

/// GET /api/euicc/relay/pending[?wait=25]
///
/// Long-polls for the next request to carry. Returns `{"request": null}` when
/// nothing is waiting, so the client can simply poll again.
pub fn relay_pending(query: Option<&str>) -> (u16, Value) {
    let wait = query
        .and_then(|q| {
            q.split('&')
                .filter_map(|pair| pair.split_once('='))
                .find(|(k, _)| *k == "wait")
                .and_then(|(_, v)| v.parse::<u64>().ok())
        })
        .unwrap_or(25);

    match relay::take_pending(Duration::from_secs(wait)) {
        Some(request) => (
            200,
            json!({"ok": true, "data": {"request": {
                "id": request.id,
                "url": request.url,
                "headers": request.headers,
                "body_hex": request.body_hex,
            }}}),
        ),
        None => (200, json!({"ok": true, "data": {"request": null}})),
    }
}

/// POST /api/euicc/relay/response
///
/// Body: `{"id": 1, "status": 200, "body_hex": "7B..."}`
pub fn relay_response(body: &[u8]) -> (u16, Value) {
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(id) = parsed["id"].as_u64() else {
        return (400, json!({"ok": false, "error": "missing 'id'"}));
    };
    let Some(status) = parsed["status"].as_u64() else {
        return (400, json!({"ok": false, "error": "missing 'status'"}));
    };
    let body_hex = parsed["body_hex"].as_str().unwrap_or("").to_string();
    if lpac::decode_hex(&body_hex).is_err() {
        return (400, json!({"ok": false, "error": "body_hex is not valid hex"}));
    }

    match relay::put_response(id, status as u32, body_hex) {
        Ok(()) => (200, json!({"ok": true})),
        Err(e) => (409, json!({"ok": false, "error": e})),
    }
}

/// GET /api/euicc/relay/status
pub fn relay_status() -> (u16, Value) {
    let (active, waiting, connected) = relay::status();
    (
        200,
        json!({"ok": true, "data": {
            "active": active,
            "waiting_for_client": waiting,
            // Lets the switch say whether anything is actually listening,
            // instead of the user finding out only when an operation fails.
            "client_connected": connected,
        }}),
    )
}

/// POST /api/euicc/notifications/remove — body: `{"sequences": [0, 1]}`
pub fn euicc_notifications_remove(body: &[u8]) -> (u16, Value) {
    if let Some(unavailable) = require_lpac() {
        return unavailable;
    }
    let parsed = match parse_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match sequence_numbers(&parsed) {
        Ok(sequences) => lpac_response(remove_notifications(&sequences)),
        Err(e) => e,
    }
}

#[cfg(test)]
mod write_api_tests {
    use super::*;

    #[test]
    fn rejects_invalid_json() {
        assert!(parse_body(b"not json").is_err());
        assert_eq!(parse_body(b"not json").unwrap_err().0, 400);
        assert!(parse_body(br#"{"iccid":"1"}"#).is_ok());
    }

    #[test]
    fn write_routes_refuse_when_lpac_is_absent() {
        // The availability guard runs before body parsing, so a missing lpac is
        // reported as such rather than as a confusing failure further in. On a
        // dev host lpac is not installed, which is what this asserts.
        if !lpac::available() {
            let (status, body) = euicc_enable(br#"{"iccid":"89490102186110201029"}"#);
            assert_eq!(status, 501);
            assert!(
                body["error"].as_str().unwrap_or_default().contains("lpac"),
                "{body}"
            );
        }
    }

    #[test]
    fn reads_string_fields() {
        let v = json!({"a": " x ", "b": "  ", "c": 1});
        assert_eq!(string_field(&v, "a").as_deref(), Some("x"));
        assert_eq!(string_field(&v, "b"), None);
        assert_eq!(string_field(&v, "c"), None);
        assert_eq!(string_field(&v, "missing"), None);
    }

    #[test]
    fn maps_lpac_failure_payloads_to_errors() {
        let failed = LpacResult {
            payload: json!({"code": -1, "message": "profile not found"}),
            progress: vec![],
        };
        let (status, body) = lpac_response(Ok(failed));
        assert_eq!(status, 502);
        assert_eq!(body["error"], "profile not found");
        assert_eq!(body["ok"], false);
    }

    #[test]
    fn maps_lpac_success_payloads() {
        let ok = LpacResult {
            payload: json!({"code": 0, "data": {"eid": "89"}}),
            progress: vec!["downloading".into()],
        };
        let (status, body) = lpac_response(Ok(ok));
        assert_eq!(status, 200);
        assert_eq!(body["ok"], true);
        assert_eq!(body["data"]["result"]["eid"], "89");
        assert_eq!(body["data"]["progress"][0], "downloading");
    }

    #[test]
    fn flags_a_switch_that_needs_a_reboot() {
        let ok = (200, json!({"ok": true, "data": {"result": null}}));
        let (status, body) = with_reboot_notice(ok, false);
        assert_eq!(status, 200);
        assert_eq!(body["data"]["reboot_required"], true);
        assert!(body["data"]["notice"].as_str().unwrap().contains("Reboot"));
    }

    #[test]
    fn omits_reboot_notice_when_refreshed_or_failed() {
        // A card that accepted REFRESH needs no reboot.
        let ok = (200, json!({"ok": true, "data": {"result": null}}));
        assert!(with_reboot_notice(ok, true).1["data"]["reboot_required"].is_null());

        // A failure is not a switch, so there is nothing to reboot for.
        let failed = (502, json!({"ok": false, "error": "nope"}));
        assert!(with_reboot_notice(failed, false).1["data"]["reboot_required"].is_null());
    }

    #[test]
    fn parses_sequence_numbers() {
        assert_eq!(sequence_numbers(&json!({"sequences": [0, 2]})).unwrap(), vec![0, 2]);
        assert!(sequence_numbers(&json!({})).is_err());
    }

    fn notification_body() -> Value {
        json!({"ok": true, "data": {"result": [
            {"seqNumber": 0, "iccid": "89940102186101209299", "profileManagementOperation": "install"},
            {"seqNumber": 1, "iccid": "8944476500017411534", "profileManagementOperation": "enable"},
        ], "progress": []}})
    }

    #[test]
    fn masks_iccids_on_notifications() {
        let mut body = notification_body();
        mask_notification_iccids(&mut body, false);
        let list = body["data"]["result"].as_array().unwrap();
        assert_eq!(list[0]["iccid"], "8994************9299");
        assert_eq!(list[1]["iccid"], "8944***********1534");
        assert_eq!(body["data"]["masked"], true);
        // The rest of the notification must survive: the sequence number is how
        // the client addresses it back.
        assert_eq!(list[0]["seqNumber"], 0);
        assert_eq!(list[1]["profileManagementOperation"], "enable");
    }

    #[test]
    fn leaves_iccids_alone_when_full_was_asked_for() {
        let mut body = notification_body();
        mask_notification_iccids(&mut body, true);
        assert_eq!(body["data"]["result"][0]["iccid"], "89940102186101209299");
        assert_eq!(body["data"]["masked"], false);
    }
}
