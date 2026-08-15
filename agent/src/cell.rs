//! Radio selection: network mode, band locking and cell locking.
//! All of it is driven by the dashboard's Signal → Mode & Locking tab.

use serde_json::{json, Value};

use crate::handlers::AppState;
use crate::ubus;
use crate::validate::validate_ubus_input;

pub fn cell_lock_nr(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    if let Err(e) = validate_ubus_input(&parsed) {
        return (400, json!({"ok": false, "error": e}));
    }
    match ubus::call(
        "zte_nwinfo_api",
        "nwinfo_lock_nr_cell",
        Some(&parsed.to_string()),
    ) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

pub fn cell_lock_lte(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    if let Err(e) = validate_ubus_input(&parsed) {
        return (400, json!({"ok": false, "error": e}));
    }
    match ubus::call(
        "zte_nwinfo_api",
        "nwinfo_lock_lte_cell",
        Some(&parsed.to_string()),
    ) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

pub fn cell_lock_reset(_state: &AppState) -> (u16, Value) {
    match ubus::call(
        "zte_nwinfo_api",
        "nwinfo_reset_band_cell_setting",
        Some("{}"),
    ) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

pub fn cell_band_nr(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    if let Err(e) = validate_ubus_input(&parsed) {
        return (400, json!({"ok": false, "error": e}));
    }
    let params = parsed.to_string();
    eprintln!("[INFO] [band_nr] ubus call zte_nwinfo_api nwinfo_set_nrbandlock '{params}'");
    match ubus::call("zte_nwinfo_api", "nwinfo_set_nrbandlock", Some(&params)) {
        Ok(data) => {
            eprintln!("[INFO] [band_nr] success: {data}");
            (200, json!({"ok": true, "data": data}))
        }
        Err(e) => {
            eprintln!("[WARN] [band_nr] error: {e}");
            (503, json!({"ok": false, "error": e}))
        }
    }
}

pub fn cell_band_lte(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    if let Err(e) = validate_ubus_input(&parsed) {
        return (400, json!({"ok": false, "error": e}));
    }
    let params = parsed.to_string();
    eprintln!("[INFO] [band_lte] ubus call zte_nwinfo_api nwinfo_set_gwl_bandlock '{params}'");
    match ubus::call("zte_nwinfo_api", "nwinfo_set_gwl_bandlock", Some(&params)) {
        Ok(data) => {
            eprintln!("[INFO] [band_lte] success: {data}");
            (200, json!({"ok": true, "data": data}))
        }
        Err(e) => {
            eprintln!("[WARN] [band_lte] error: {e}");
            (503, json!({"ok": false, "error": e}))
        }
    }
}

pub fn cell_band_reset(_state: &AppState) -> (u16, Value) {
    match ubus::call("zte_nwinfo_api", "nwinfo_rest_band_rat", Some("{}")) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

// ── Smart Tower Connect ──────────────────────────────────────────────────────
//
// STC is supposed to watch which cells the unit sees, build a whitelist of the
// ones worth camping on, and lock to those. On this unit it does nothing that
// can be observed, and these routes are kept only because reading the vendor's
// parameters is still honest reporting. Measured, not assumed:
//
//   - `nwinfo_stc_cell_lock_enable` and `..._disable` both return success and
//     change no field in `zte_nwinfo`. Called straight from the device shell,
//     not just through the agent, in case the agent was at fault.
//   - `cell_white_list_enable_flag` reads 1 before and after either verb, so it
//     is not the toggle it looks like. It is reported below as
//     `whitelist_available`, which is all that can be claimed for it.
//   - After enabling, the collected counts and `collect_cell_white_list_run_time`
//     stayed at 0 for two minutes — well past the 60s `delayed_start_timer` —
//     while the neighbour list showed ten cells to collect from.
//
// So there is no on/off state to read and no evidence the verbs do anything.
// The write routes are still served, because "the firmware ignores it" is a
// fair thing for a client to discover, but nothing here reports a toggle
// position, and the app has no STC screen. See docs/MOBILE-API-GAP.md.
//
// Reading is UCI, writing is ubus. Deliberately not `uci set`: the enable path
// has to go through the vendor's own method so the daemon learns about it, and
// writing these keys directly would leave the two disagreeing.

const STC_CONFIG: &str = "stc_cell_lock_config";
const STC_STATUS: &str = "stc_cell_lock_status";

fn stc_uci() -> std::collections::HashMap<String, String> {
    ubus::uci_show("zte_nwinfo")
}

fn stc_field(uci: &std::collections::HashMap<String, String>, section: &str, key: &str) -> Value {
    match uci.get(&format!("{section}.{key}")) {
        Some(value) => Value::String(value.clone()),
        None => Value::Null,
    }
}

/// GET /api/modem/stc/params — the collection parameters STC runs with.
pub fn modem_stc_params(_state: &AppState) -> (u16, Value) {
    let uci = stc_uci();
    if uci.is_empty() {
        return (503, json!({"ok": false, "error": "zte_nwinfo config unreadable"}));
    }
    (
        200,
        json!({"ok": true, "data": {
            "lte_collect_timer": stc_field(&uci, STC_CONFIG, "collect_lte_cell_white_list_timer_max"),
            "nrsa_collect_timer": stc_field(&uci, STC_CONFIG, "collect_nr5g_cell_white_list_timer_max"),
            "lte_whitelist_max": stc_field(&uci, STC_CONFIG, "collect_lte_cell_white_list_num_max"),
            "nrsa_whitelist_max": stc_field(&uci, STC_CONFIG, "collect_nr5g_cell_white_list_num_max"),
            "delayed_start_timer": stc_field(&uci, STC_CONFIG, "collect_cell_white_list_delayed_start_timer"),
            // Not "stc_enable". This flag does not move when the feature is
            // enabled or disabled, so calling it that would put a toggle on a
            // screen that never changes and never means anything.
            "whitelist_available": stc_field(&uci, STC_STATUS, "cell_white_list_enable_flag"),
        }}),
    )
}

/// GET /api/modem/stc/status — how far collection has got, and whether it is on.
pub fn modem_stc_status(_state: &AppState) -> (u16, Value) {
    let uci = stc_uci();
    if uci.is_empty() {
        return (503, json!({"ok": false, "error": "zte_nwinfo config unreadable"}));
    }
    (
        200,
        json!({"ok": true, "data": {
            // See the note above: constant on this unit, so reported for what
            // it is rather than as the feature's state.
            "whitelist_available": stc_field(&uci, STC_STATUS, "cell_white_list_enable_flag"),
            "cell_available": stc_field(&uci, STC_STATUS, "cell_available_flag"),
            "lte_collected": stc_field(&uci, STC_STATUS, "current_lte_cell_white_list_num"),
            "nrsa_collected": stc_field(&uci, STC_STATUS, "current_nr5g_cell_white_list_num"),
            "lte_timer": stc_field(&uci, STC_STATUS, "current_lte_cell_white_list_timer"),
            "nrsa_timer": stc_field(&uci, STC_STATUS, "current_nr5g_cell_white_list_timer"),
            "run_time": stc_field(&uci, STC_STATUS, "collect_cell_white_list_run_time"),
        }}),
    )
}

/// PUT /api/modem/stc — turn the whitelist cell lock on or off.
pub fn modem_stc_set(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    // Accepts the string form the vendor uses elsewhere as well as a bool, so
    // a caller copying the "1"/"0" convention from the other radio routes works.
    let enable = match &parsed["stc_enable"] {
        Value::Bool(on) => *on,
        Value::String(s) => s == "1" || s.eq_ignore_ascii_case("true"),
        Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
        _ => return (400, json!({"ok": false, "error": "stc_enable must be 1/0 or true/false"})),
    };
    let method = if enable {
        "nwinfo_stc_cell_lock_enable"
    } else {
        "nwinfo_stc_cell_lock_disable"
    };
    match ubus::call("zte_nwinfo_api", method, Some("{}")) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// POST /api/modem/stc/reset — discard the collected whitelist and start over.
pub fn modem_stc_reset(_state: &AppState) -> (u16, Value) {
    match ubus::call("zte_nwinfo_api", "nwinfo_stc_cell_lock_reset", Some("{}")) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// GET /api/modem/network-mode — the preferred RAT currently in force.
///
/// There is no `nwinfo_get_netselect`: the firmware exposes only the setter, so
/// the current value has to be read back out of the general network info, which
/// reports it as `net_select` alongside whether it was chosen automatically.
/// Without this the mode screen had nothing to read and opened on an error.
pub fn modem_network_mode(_state: &AppState) -> (u16, Value) {
    match ubus::call("zte_nwinfo_api", "nwinfo_get_netinfo", Some("{}")) {
        Ok(data) => (
            200,
            json!({"ok": true, "data": {
                "net_select": data.get("net_select").cloned().unwrap_or(Value::Null),
                "net_select_mode": data.get("net_select_mode").cloned().unwrap_or(Value::Null),
                "network_type": data.get("network_type").cloned().unwrap_or(Value::Null),
            }}),
        ),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// PUT /api/modem/network-mode — preferred RAT (5G SA/NSA, LTE-only, …).
pub fn modem_network_mode_set(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    if let Err(e) = validate_ubus_input(&parsed) {
        return (400, json!({"ok": false, "error": e}));
    }
    match ubus::call(
        "zte_nwinfo_api",
        "nwinfo_set_netselect",
        Some(&parsed.to_string()),
    ) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}
