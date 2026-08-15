//! Paths the mobile apps ask for.
//!
//! The Android app — and the desktop app that will share its client — were
//! written against the upstream project's agent, which names things
//! differently. Of the endpoints it calls, most are served here under another
//! path, so the app opened on a screen reading `{"error":"not found"}`.
//!
//! These are not blind aliases. The upstream responses are raw vendor objects
//! and the app's parsers read vendor field names straight out of them
//! (`nr5g_rsrp`, `lte_rsrp`, `wan_active_channel`), so each route here returns
//! the same raw source this fork's batched `/api/dashboard` already reads,
//! rather than this fork's reshaped view. Returning the reshaped view would
//! give a 200 the app cannot parse — a quieter failure than a 404, because
//! nothing reports it.
//!
//! See `docs/MOBILE-API-GAP.md` for the full inventory.

use serde_json::{json, Value};

use crate::handlers::AppState;
use crate::{cell, network_ext, operator, router, sim, sms, system, ubus, usb};

/// Wrap a raw ubus object the way the apps expect to receive it.
///
/// A ubus call that fails yields an empty object rather than an error: these
/// feed dashboards that poll every few seconds, and one unavailable source
/// should leave a field blank, not blank the screen.
fn passthrough(object: &str, method: &str) -> (u16, Value) {
    let data = ubus::call(object, method, Some("{}")).unwrap_or_else(|_| json!({}));
    (200, json!({"ok": true, "data": data}))
}

/// GET /api/network/signal — raw `nwinfo_get_netinfo`.
///
/// The screen that failed first. Its parser reads vendor keys directly, so the
/// object is passed through untouched.
pub fn network_signal(_state: &AppState) -> (u16, Value) {
    passthrough("zte_nwinfo_api", "nwinfo_get_netinfo")
}

/// GET /api/network/wan — raw IPv4 WAN interface status.
pub fn network_wan(_state: &AppState) -> (u16, Value) {
    passthrough("network.interface.zte_wan", "status")
}

/// GET /api/network/wan6 — raw IPv6 WAN interface status.
pub fn network_wan6(_state: &AppState) -> (u16, Value) {
    passthrough("network.interface.zte_wan6", "status")
}

/// GET /api/device/thermal — raw CPU temperature.
pub fn device_thermal(_state: &AppState) -> (u16, Value) {
    passthrough("zwrt_bsp.thermal", "get_cpu_temp")
}

/// GET /api/battery — the unreshaped battery object.
///
/// Distinct from `/api/device/battery-info`, which this fork reshapes. The app
/// reads both and expects the raw one here.
pub fn battery(_state: &AppState) -> (u16, Value) {
    passthrough("zwrt_bsp.battery", "list")
}

/// GET /api/device/system — device identity and firmware.
pub fn device_system(state: &AppState) -> (u16, Value) {
    crate::handlers::device(state)
}

/// GET /api/device/imei
pub fn device_imei(state: &AppState) -> (u16, Value) {
    sim::sim_imei(state)
}

/// GET /api/device/usb
pub fn device_usb(state: &AppState) -> (u16, Value) {
    usb::usb_status(state)
}

/// POST /api/device/usb/mode
pub fn device_usb_mode(state: &AppState, body: &[u8]) -> (u16, Value) {
    usb::usb_mode_set(state, body)
}

/// POST /api/device/powerbank
pub fn device_powerbank(state: &AppState, body: &[u8]) -> (u16, Value) {
    usb::usb_powerbank_set(state, body)
}

/// GET/POST /api/network/lan
pub fn network_lan(state: &AppState, method_is_post: bool, body: &[u8]) -> (u16, Value) {
    if method_is_post {
        router::router_lan_set(state, body)
    } else {
        router::router_lan_get(state)
    }
}

/// GET/POST /api/network/dns
pub fn network_dns(state: &AppState, method_is_post: bool, body: &[u8]) -> (u16, Value) {
    if method_is_post {
        router::router_dns_set(state, body)
    } else {
        router::router_dns_get(state)
    }
}

/// GET /api/network/dhcp-leases — connected clients, as the apps name them.
pub fn network_dhcp_leases(state: &AppState) -> (u16, Value) {
    network_ext::network_clients(state)
}

/// GET /api/modem/status — what the apps poll for a connection summary.
///
/// The same raw netinfo object as the signal route: upstream served one object
/// to both, and the app's parsers pick different keys out of it.
pub fn modem_status(_state: &AppState) -> (u16, Value) {
    passthrough("zte_nwinfo_api", "nwinfo_get_netinfo")
}

/// GET /api/network/rmnet — the cellular interfaces' byte counters.
///
/// Both `rmnet_data0` and `rmnet_ipa0` are reported: which one carries traffic
/// varies with how the modem is attached, and a screen showing one of them at
/// zero is worse than one showing the pair.
pub fn network_rmnet(_state: &AppState) -> (u16, Value) {
    let interfaces: Vec<Value> = system::read_network_traffic()
        .into_iter()
        .filter(|iface| iface.name.starts_with("rmnet"))
        .map(|iface| {
            json!({
                "name": iface.name,
                "rx_bytes": iface.rx_bytes,
                "tx_bytes": iface.tx_bytes,
            })
        })
        .collect();
    (200, json!({"ok": true, "data": {"interfaces": interfaces}}))
}

/// GET/POST /api/modem/airplane — the radio's operating mode.
///
/// `low_power` is the modem's own idea of flight mode: the radio is off but the
/// modem stays reachable, which is the same lever the eSIM code uses to get the
/// card released before a profile switch.
pub fn modem_airplane(_state: &AppState, method_is_post: bool, body: &[u8]) -> (u16, Value) {
    if !method_is_post {
        let info = ubus::call("zte_nwinfo_api", "nwinfo_get_netinfo", Some("{}"))
            .unwrap_or_else(|_| json!({}));
        // No getter for the mode itself, so it is inferred: a modem reporting
        // no service at all is the observable half of the radio being down.
        let network = info["network_type"].as_str().unwrap_or("");
        return (
            200,
            json!({"ok": true, "data": {
                "enabled": network.is_empty() || network == "NO_SERVICE",
                "network_type": network,
            }}),
        );
    }

    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let Some(enabled) = parsed["enabled"].as_bool() else {
        return (400, json!({"ok": false, "error": "missing 'enabled'"}));
    };

    let mode = if enabled { "low_power" } else { "online" };
    let params = json!({"operate_mode": mode}).to_string();
    match ubus::call("zte_nwinfo_api", "nwinfo_set_mode", Some(&params)) {
        Ok(_) => (
            200,
            json!({"ok": true, "data": {
                "enabled": enabled,
                "notice": if enabled {
                    "The radio is off. Mobile data will not work until it is switched back on."
                } else {
                    "The radio is on. Registering takes a few seconds."
                },
            }}),
        ),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// POST /api/modem/online — the same switch, phrased the other way round.
///
/// The app has both, and they are opposites rather than duplicates, so this
/// inverts rather than sharing a body format that would silently disagree.
pub fn modem_online(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let Some(online) = parsed["enabled"].as_bool().or_else(|| parsed["online"].as_bool()) else {
        return (400, json!({"ok": false, "error": "missing 'enabled'"}));
    };
    let inverted = json!({"enabled": !online}).to_string();
    modem_airplane(state, true, inverted.as_bytes())
}

// --- APN ---
//
// The app splits what this fork keeps under /api/router/apn.

/// GET/POST /api/modem/apn — the profile list.
pub fn modem_apn(state: &AppState, method_is_post: bool, body: &[u8]) -> (u16, Value) {
    if method_is_post {
        router::router_apn_profiles_add(state, body)
    } else {
        router::router_apn_profiles_get(state)
    }
}

/// POST /api/modem/apn/profile — add or remove a profile.
///
/// The app sends one route for both and distinguishes with `action`, where this
/// fork has separate paths. Dispatched here rather than given its own handler,
/// so there is one implementation of each operation.
pub fn modem_apn_profile(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    match parsed["action"].as_str() {
        Some("delete") | Some("remove") => router::router_apn_profiles_delete(state, body),
        _ => router::router_apn_profiles_add(state, body),
    }
}

/// POST /api/modem/apn/activate
pub fn modem_apn_activate(state: &AppState, body: &[u8]) -> (u16, Value) {
    router::router_apn_profiles_activate(state, body)
}

/// GET/POST /api/modem/apn/mode — automatic or manual APN selection.
pub fn modem_apn_mode(state: &AppState, method_is_post: bool, body: &[u8]) -> (u16, Value) {
    if method_is_post {
        router::router_apn_mode_set(state, body)
    } else {
        router::router_apn_mode_get(state)
    }
}

// --- Band and cell locking ---

/// POST /api/modem/bands/lock — reset, as the app uses it to clear a lock.
pub fn modem_bands_lock(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    // An empty mask is how the app asks for "no lock", and passing it through
    // as a band list would lock the modem to nothing at all.
    let clearing = parsed["bands"].as_array().is_none_or(|b| b.is_empty())
        && parsed["mask"].as_str().is_none_or(str::is_empty);
    if clearing {
        return cell::cell_band_reset(state);
    }
    modem_bands_lte(state, body)
}

/// A list of band numbers, however the caller chose to write it.
///
/// Accepts `[1, 3, 8]`, `"1,3,8"`, and the same with `B`/`n` prefixes, because
/// the two clients label bands differently and neither spelling is wrong.
/// Returns the numbers, not a mask — the mask is built by [`lte_band_mask`].
fn band_numbers(value: &Value) -> Vec<u32> {
    let parse_one = |s: &str| -> Option<u32> {
        s.trim()
            .trim_start_matches(['B', 'b', 'N', 'n'])
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=256).contains(n))
    };
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::Number(n) => n.as_u64().map(|n| n as u32).filter(|n| (1..=256).contains(n)),
                Value::String(s) => parse_one(s),
                _ => None,
            })
            .collect(),
        Value::String(s) => s.split(',').filter_map(parse_one).collect(),
        _ => Vec::new(),
    }
}

/// The vendor's LTE band lock takes a decimal bitmask, band N at bit N-1.
///
/// This lives here rather than in each client because it was already written
/// twice — once in the dashboard, once nowhere, which is why the Android app
/// sent `"1,3,8"` and locked the modem to a mask meaning bands 1, 2, 4, 8, 16
/// and 32. One implementation, with tests.
fn lte_band_mask(bands: &[u32]) -> String {
    let mut mask = 0u128;
    for band in bands {
        if *band >= 1 && *band <= 128 {
            mask |= 1u128 << (band - 1);
        }
    }
    mask.to_string()
}

/// POST /api/modem/bands/lte/lock
///
/// Takes `{"bands": [1, 3, 8]}` and converts, or a ready-made
/// `lte_band_mask` which is passed through untouched.
pub fn modem_bands_lte(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = serde_json::from_slice(body).unwrap_or(Value::Null);

    let mask = match parsed["lte_band_mask"].as_str() {
        // A caller that computed the mask itself knows what it wants. Only a
        // decimal string is a mask; anything with a comma in it is a band list
        // under the wrong key, and locking on it would pick the wrong bands.
        Some(given) if !given.is_empty() && given.chars().all(|c| c.is_ascii_digit()) => {
            given.to_string()
        }
        _ => {
            let bands = if parsed["bands"].is_null() {
                band_numbers(&parsed["lte_band_mask"])
            } else {
                band_numbers(&parsed["bands"])
            };
            if bands.is_empty() {
                return (
                    400,
                    json!({"ok": false, "error": "no LTE bands given; send bands as a list or lte_band_mask as a decimal mask"}),
                );
            }
            lte_band_mask(&bands)
        }
    };

    let call = json!({
        "is_lte_band": "1",
        "lte_band_mask": mask,
        "is_gw_band": "0",
        "gw_band_mask": "0",
    });
    cell::cell_band_lte(state, call.to_string().as_bytes())
}

/// POST /api/modem/bands/nr/lock
///
/// `nr5g_type` is forced to SA. NSA does not take on this unit — the dashboard
/// carries the same note — so accepting an `nr5g_type` of "nsa" would be
/// accepting a request that silently does nothing.
pub fn modem_bands_nr(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let bands = if parsed["bands"].is_null() {
        band_numbers(&parsed["nr5g_band"])
    } else {
        band_numbers(&parsed["bands"])
    };
    if bands.is_empty() {
        return (
            400,
            json!({"ok": false, "error": "no NR bands given; send bands as a list or nr5g_band as a comma-separated list"}),
        );
    }
    let list = bands
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let call = json!({"nr5g_type": "SA", "nr5g_band": list});
    cell::cell_band_nr(state, call.to_string().as_bytes())
}

/// GET/POST /api/modem/cell-lock
///
/// One route where this fork has three. A request naming no cell is a request
/// to clear the lock, which is the only reading that makes an empty body safe.
///
/// The vendor's field names are built here rather than asked of the caller.
/// Forwarding the body verbatim was the earlier behaviour and it meant a lock
/// request that named `nr_pci` reached `nwinfo_lock_nr_cell` with no `pci` at
/// all — dispatched as a reset, so the button labelled Lock unlocked.
pub fn modem_cell_lock(state: &AppState, method_is_post: bool, body: &[u8]) -> (u16, Value) {
    if !method_is_post {
        // No dedicated getter: the current lock is visible in the raw netinfo
        // the signal route already returns.
        return passthrough("zte_nwinfo_api", "nwinfo_get_netinfo");
    }
    let parsed: Value = serde_json::from_slice(body).unwrap_or(Value::Null);

    // `pci`/`rat` is the upstream app's spelling, `nr_pci`/`lte_pci` this
    // fork's screens. Both name the same thing.
    let field = |keys: &[&str]| -> String {
        keys.iter()
            .find_map(|key| match &parsed[*key] {
                Value::String(s) if !s.is_empty() => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .unwrap_or_default()
    };

    let rat = parsed["rat"].as_str().unwrap_or("");
    let nr_pci = field(&["nr_pci"]);
    let lte_pci = field(&["lte_pci"]);
    let generic_pci = field(&["pci"]);

    let wants_nr = matches!(rat, "nr" | "nr5g" | "5g") || !nr_pci.is_empty();

    let pci = if wants_nr {
        if nr_pci.is_empty() { generic_pci.clone() } else { nr_pci }
    } else if lte_pci.is_empty() {
        generic_pci.clone()
    } else {
        lte_pci
    };

    if pci.is_empty() {
        return cell::cell_lock_reset(state);
    }

    let call = if wants_nr {
        json!({
            "lock_nr_pci": pci,
            "lock_nr_earfcn": field(&["nr_earfcn", "earfcn"]),
            "lock_nr_cell_band": field(&["nr_band", "band"]),
        })
    } else {
        json!({
            "lock_lte_pci": pci,
            "lock_lte_earfcn": field(&["lte_earfcn", "earfcn"]),
        })
    };

    if wants_nr {
        cell::cell_lock_nr(state, call.to_string().as_bytes())
    } else {
        cell::cell_lock_lte(state, call.to_string().as_bytes())
    }
}

/// GET /api/modem/neighbors — cells the modem can see besides the serving one.
///
/// `lte_neighbor_cell` and `nr_neighbor_cell` are packed strings, in the same
/// style as the operator scan: records separated by `;`, fields by `,`. They
/// are empty whenever the modem sees only its serving cell, which is the
/// normal case on a fixed router and not an error.
///
/// The raw strings are returned alongside the parsed list. The field order is
/// undocumented and was never observed non-empty on this unit, so a client that
/// finds the parse unconvincing has the original to work from rather than
/// having to call a different endpoint.
pub fn modem_neighbors(_state: &AppState) -> (u16, Value) {
    let info = ubus::call("zte_nwinfo_api", "nwinfo_get_netinfo", Some("{}"))
        .unwrap_or_else(|_| json!({}));
    let lte_raw = info["lte_neighbor_cell"].as_str().unwrap_or("");
    let nr_raw = info["nr_neighbor_cell"].as_str().unwrap_or("");

    (
        200,
        json!({"ok": true, "data": {
            "lte": parse_neighbors(lte_raw),
            "nr": parse_neighbors(nr_raw),
            "lte_raw": lte_raw,
            "nr_raw": nr_raw,
            "serving_lte_pci": info["lte_pci"],
            "serving_nr_pci": info["nr5g_pci"],
        }}),
    )
}

/// Split a packed neighbour list into records.
///
/// Deliberately not mapped onto named fields. The vendor documents no order and
/// this unit has never reported a non-empty list, so naming them would be a
/// guess presented as fact — a client would then filter or sort on a field that
/// might be something else entirely. Positional values are honest about what is
/// known.
pub fn parse_neighbors(raw: &str) -> Vec<Value> {
    raw.split(';')
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .map(|record| {
            let fields: Vec<Value> = record
                .split(',')
                .map(|field| Value::String(field.trim().to_string()))
                .collect();
            json!({"fields": fields})
        })
        .collect()
}

// --- Operator selection ---

/// GET/POST /api/modem/scan
pub fn modem_scan(state: &AppState, method_is_post: bool) -> (u16, Value) {
    if method_is_post {
        operator::operator_scan_start()
    } else {
        let _ = state;
        operator::operator_scan_status()
    }
}

/// POST /api/modem/register
pub fn modem_register(_state: &AppState, body: &[u8]) -> (u16, Value) {
    operator::operator_select(body)
}

// --- Odds and ends ---

/// GET /api/modem/data — connection state, from the same raw netinfo object.
/// GET /api/modem/data — the data call's state, not the radio's.
///
/// `nwinfo_get_netinfo` describes the radio and says nothing about whether a
/// data call is up, so the mobile network screen had no connection status and
/// no data toggle to read back. The wwan interface object is the one that
/// carries connect_status, enable and roam_enable.
pub fn modem_data(_state: &AppState) -> (u16, Value) {
    match ubus::call("zwrt_data", "get_wwaniface", Some(r#"{"cid":1}"#)) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// PUT /api/modem/data — turn the data call on or off, or change roaming.
///
/// Only the three fields a client has any business setting are forwarded. The
/// vendor's setter takes the entire interface description, including addresses
/// and DNS, and passing a caller's body straight through would let it rewrite
/// the routing for the whole unit.
pub fn modem_data_set(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };

    let mut params = json!({"cid": 1});
    let mut touched = false;
    for field in ["enable", "roam_enable", "connect_mode"] {
        let value = match &parsed[field] {
            Value::Bool(on) => Some(i64::from(*on)),
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        };
        if let Some(value) = value {
            if !(0..=3).contains(&value) {
                return (400, json!({"ok": false, "error": format!("{field} out of range")}));
            }
            params[field] = json!(value);
            touched = true;
        }
    }
    if !touched {
        return (
            400,
            json!({"ok": false, "error": "expected enable, roam_enable or connect_mode"}),
        );
    }

    match ubus::call("zwrt_data", "set_wwaniface", Some(&params.to_string())) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// GET /api/network/speed, /speeds, /traffic — live throughput counters.
pub fn network_speed(state: &AppState) -> (u16, Value) {
    let speed = state.speed.sample();
    let interfaces: Vec<Value> = system::read_network_traffic()
        .into_iter()
        .map(|iface| json!({
            "name": iface.name,
            "rx_bytes": iface.rx_bytes,
            "tx_bytes": iface.tx_bytes,
        }))
        .collect();
    (
        200,
        json!({"ok": true, "data": {
            "rx_speed": speed.rx_speed,
            "tx_speed": speed.tx_speed,
            "max_rx_speed": speed.max_rx_speed,
            "max_tx_speed": speed.max_tx_speed,
            "rx_bytes": speed.rx_bytes,
            "tx_bytes": speed.tx_bytes,
            "interfaces": interfaces,
        }}),
    )
}

/// GET /api/sms/capacity — how full the message store is.
pub fn sms_capacity(_state: &AppState) -> (u16, Value) {
    // `zwrt_wms_get_wms_capacity` reports this directly, and returns exactly
    // the `sms_*` keys the apps read.
    //
    // This used to be derived from a listing, reading `body["data"]["total"]`
    // — a field `zte_libwms_get_sms_data` does not return. It replies with
    // `messages` and nothing else, so `used` was null and none of the four
    // `sms_*_total` keys were present at all, which is why the SMS screen's
    // counters sat at zero however many messages were stored. The dedicated
    // method was there the whole time; the derivation was invented rather than
    // looked for.
    //
    // `sms_nvused_total` is the one field the vendor reports as 0 while
    // messages are plainly stored, so the received/sent/draft breakdown is
    // passed through beside it and a usable total is summed from those three.
    let raw = match ubus::call("zwrt_wms", "zwrt_wms_get_wms_capacity", Some("{}")) {
        Ok(v) => v,
        Err(e) => return (503, json!({"ok": false, "error": e})),
    };

    let field = |key: &str| -> u64 { raw[key].as_u64().unwrap_or(0) };
    let nv_used = field("sms_nv_rev_total") + field("sms_nv_send_total") + field("sms_nv_draftbox_total");
    let sim_used = field("sms_sim_rev_total") + field("sms_sim_send_total") + field("sms_sim_draftbox_total");

    (
        200,
        json!({"ok": true, "data": {
            "sms_nv_total": raw["sms_nv_total"],
            "sms_sim_total": raw["sms_sim_total"],
            // Summed, not taken from sms_nvused_total: the vendor reports that
            // as 0 on this unit while sms_nv_rev_total counts the messages the
            // listing returns.
            "sms_nvused_total": nv_used,
            "sms_simused_total": sim_used,
            "sms_dev_unread_num": raw["sms_dev_unread_num"],
            "sms_sim_unread_num": raw["sms_sim_unread_num"],
            "raw": raw,
        }}),
    )
}

/// POST /api/sim/unlock — the app's name for verifying a PIN.
pub fn sim_unlock(state: &AppState, body: &[u8]) -> (u16, Value) {
    crate::simlock::sim_pin_verify(state, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_neighbours_is_an_empty_list_not_a_row_of_blanks() {
        // The normal state on a fixed router seeing only its serving cell.
        assert!(parse_neighbors("").is_empty());
        assert!(parse_neighbors(";;").is_empty());
        assert!(parse_neighbors("   ").is_empty());
    }

    #[test]
    fn splits_records_and_keeps_fields_positional() {
        let parsed = parse_neighbors("227,1725,-99,-11;301,1725,-105,-14;");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0]["fields"][0], "227");
        assert_eq!(parsed[0]["fields"][3], "-11");
        assert_eq!(parsed[1]["fields"][0], "301");
    }

    #[test]
    fn band_numbers_reads_every_spelling_the_clients_use() {
        assert_eq!(band_numbers(&json!([1, 3, 8])), vec![1, 3, 8]);
        assert_eq!(band_numbers(&json!("1,3,8")), vec![1, 3, 8]);
        assert_eq!(band_numbers(&json!("B1, B3 ,B8")), vec![1, 3, 8]);
        assert_eq!(band_numbers(&json!(["n78", "n1"])), vec![78, 1]);
        // Junk is dropped rather than turned into band 0, which is not a band.
        assert_eq!(band_numbers(&json!("1,,x,3")), vec![1, 3]);
        assert_eq!(band_numbers(&json!(null)), Vec::<u32>::new());
    }

    #[test]
    fn lte_mask_puts_band_n_at_bit_n_minus_one() {
        assert_eq!(lte_band_mask(&[1]), "1");
        assert_eq!(lte_band_mask(&[2]), "2");
        assert_eq!(lte_band_mask(&[1, 3, 8]), "133"); // 1 + 4 + 128
        assert_eq!(lte_band_mask(&[]), "0");
        // The band that made a u64 mask overflow: bit 65.
        assert_eq!(lte_band_mask(&[66]), "36893488147419103232");
    }

    #[test]
    fn a_band_list_is_never_mistaken_for_a_mask() {
        // The bug this replaces: "1,3,8" arrived under lte_band_mask and was
        // forwarded as a mask, locking to bands 1, 2, 4, 8, 16 and 32.
        assert_eq!(band_numbers(&json!("1,3,8")), vec![1, 3, 8]);
        assert_eq!(lte_band_mask(&band_numbers(&json!("1,3,8"))), "133");
    }

    #[test]
    fn a_failed_source_is_an_empty_object_not_an_error() {
        // Dashboards poll these every few seconds. One unavailable source must
        // leave a field blank rather than fail the whole screen.
        let (status, body) = passthrough("definitely.not.an.object", "nope");
        assert_eq!(status, 200);
        assert_eq!(body["ok"], true);
        assert!(body["data"].is_object());
    }
}
