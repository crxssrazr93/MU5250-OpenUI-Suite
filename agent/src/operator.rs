//! Manual operator scan.
//!
//! The modem can be asked to sweep for visible networks and report them, which
//! is the only way to see who is actually reachable before locking to one. The
//! vendor exposes it as three calls that have to be driven in order: start the
//! scan, poll a status string until it settles, then read a packed result.
//!
//! Scans take around 40 seconds on this modem and disrupt service while they
//! run, so this never starts one on its own.

use serde_json::{json, Value};

use crate::ubus;

const OBJECT: &str = "zte_nwinfo_api";

/// Radio access technologies as the scan reports them.
///
/// The vendor's own web UI maps these the same way. Anything unrecognised is
/// passed through as a number rather than guessed at, because a wrong label is
/// worse than an honest unknown.
fn rat_label(code: &str) -> String {
    match code {
        "0" => "GSM".into(),
        "2" => "UMTS".into(),
        "7" => "LTE".into(),
        "11" | "12" => "5G NR".into(),
        other => format!("RAT {other}"),
    }
}

/// Availability, as 3GPP `+COPS` reports it.
fn status_label(code: &str) -> &'static str {
    match code {
        "1" => "available",
        "2" => "current",
        "3" => "forbidden",
        _ => "unknown",
    }
}

/// Split an operator's PLMN into MCC and MNC.
///
/// The scan concatenates them, and MNC is two or three digits depending on the
/// country, so the split is by length: everything past the first three digits
/// is the MNC.
fn split_plmn(plmn: &str) -> (String, String) {
    if plmn.len() > 3 {
        let (mcc, mnc) = plmn.split_at(3);
        (mcc.to_string(), mnc.to_string())
    } else {
        (plmn.to_string(), String::new())
    }
}

/// Parse the packed scan result.
///
/// Live format, captured from this modem:
/// `"1,Mobitel,41301,7;1,DBN,41311,7;3,Hutch,41308,7;"` — semicolon-separated
/// records of `status,name,plmn,rat`, with a trailing semicolon.
///
/// Records that do not have four fields are skipped rather than partially
/// decoded: a malformed entry means the field order is not what is assumed
/// here, and inventing values for a network the user might then lock onto is
/// the wrong failure.
pub fn parse_scan_contents(raw: &str) -> Vec<Value> {
    raw.split(';')
        .map(str::trim)
        .filter(|record| !record.is_empty())
        .filter_map(|record| {
            let fields: Vec<&str> = record.split(',').collect();
            if fields.len() != 4 {
                return None;
            }
            let (mcc, mnc) = split_plmn(fields[2]);
            Some(json!({
                "status": status_label(fields[0]),
                "name": fields[1].trim(),
                "plmn": fields[2],
                "mcc": mcc,
                "mnc": mnc,
                "rat": rat_label(fields[3]),
                // Kept so a caller can select this network without having to
                // reassemble the string the modem wants back.
                "select": format!("{},{}", fields[2], fields[3]),
            }))
        })
        .collect()
}

/// Whether a scan is still running.
///
/// The modem reports progress through a small set of strings and settles on
/// `manual_selected` when it worked or `manual_search_fail` when it did not —
/// a state seen on hardware while the router was in limited service. Anything
/// unrecognised counts as finished, so an unexpected string cannot leave a
/// caller polling forever.
fn is_scanning(state: &str) -> bool {
    matches!(state, "manual_selecting" | "manual_searching" | "manual_scanning")
}

/// Pull a string out of a ubus reply, whatever key the vendor used.
fn first_string(value: &Value) -> String {
    value
        .as_object()
        .and_then(|map| map.values().find_map(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

/// POST /api/operator/scan — start a sweep.
pub fn operator_scan_start() -> (u16, Value) {
    match ubus::call(OBJECT, "nwinfo_manual_scan", Some("{}")) {
        Ok(_) => (
            200,
            json!({"ok": true, "data": {
                "started": true,
                "notice": "Scanning takes about 40 seconds and mobile data is interrupted while it runs.",
            }}),
        ),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// GET /api/operator/scan — how the sweep is going, and its result once done.
///
/// One endpoint for both because the caller polls the same thing either way,
/// and the results are only meaningful alongside the state that produced them.
pub fn operator_scan_status() -> (u16, Value) {
    let state = match ubus::call(OBJECT, "nwinfo_m_netselect_status", Some("{}")) {
        Ok(v) => first_string(&v),
        Err(e) => return (503, json!({"ok": false, "error": e})),
    };

    let scanning = is_scanning(&state);
    let failed = state == "manual_search_fail";
    let contents = if scanning {
        String::new()
    } else {
        ubus::call(OBJECT, "nwinfo_m_netselect_contents", Some("{}"))
            .map(|v| first_string(&v))
            .unwrap_or_default()
    };

    (
        200,
        json!({"ok": true, "data": {
            "state": state,
            "scanning": scanning,
            // Distinguished from "no networks found" because they need
            // different things from the user: a failed sweep is worth retrying,
            // an empty one is not.
            "failed": failed,
            "operators": parse_scan_contents(&contents),
        }}),
    )
}

/// POST /api/operator/select — body: `{"select": "41301,7"}` or `{"auto": true}`
pub fn operator_select(body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };

    // Automatic is its own value rather than an empty selection, so a client
    // that forgets the field cannot silently unregister the router.
    let choice = if parsed["auto"].as_bool().unwrap_or(false) {
        "auto".to_string()
    } else {
        match parsed["select"].as_str().map(str::trim) {
            Some(v) if !v.is_empty() => v.to_string(),
            _ => return (400, json!({"ok": false, "error": "missing 'select' or 'auto'"})),
        }
    };

    // Straight into a shell-quoted ubus argument, so keep it to what the modem
    // can mean: digits and one comma.
    if choice != "auto" && !choice.chars().all(|c| c.is_ascii_digit() || c == ',') {
        return (
            400,
            json!({"ok": false, "error": "selection must be digits and a comma, e.g. 41301,7"}),
        );
    }

    let params = json!({"net_select": choice}).to_string();
    match ubus::call(OBJECT, "nwinfo_set_netselect", Some(&params)) {
        Ok(_) => (
            200,
            json!({"ok": true, "data": {
                "selected": choice,
                "notice": "The modem re-registers, which takes a few seconds.",
            }}),
        ),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_states_seen_on_hardware() {
        assert!(is_scanning("manual_selecting"));
        assert!(!is_scanning("manual_selected"));
        // Observed live when the router was in limited service. Must not be
        // treated as "still going", or the UI polls forever.
        assert!(!is_scanning("manual_search_fail"));
        assert!(!is_scanning(""));
        assert!(!is_scanning("something_new"));
    }

    #[test]
    fn parses_a_live_scan_result() {
        // Captured verbatim from this modem.
        let raw = "1,Mobitel,41301,7;1,DBN,41311,7;3,Hutch,41308,7;1,DIALOG,41302,7;";
        let parsed = parse_scan_contents(raw);
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0]["name"], "Mobitel");
        assert_eq!(parsed[0]["status"], "available");
        assert_eq!(parsed[0]["mcc"], "413");
        assert_eq!(parsed[0]["mnc"], "01");
        assert_eq!(parsed[0]["rat"], "LTE");
        assert_eq!(parsed[0]["select"], "41301,7");
        // Forbidden networks are reported, not hidden: seeing that a network is
        // barred is the point of scanning.
        assert_eq!(parsed[2]["status"], "forbidden");
    }

    #[test]
    fn keeps_operator_names_that_contain_spaces() {
        // This modem reports an unnamed network as its own PLMN with a space.
        let parsed = parse_scan_contents("1,413 12,41312,7;");
        assert_eq!(parsed[0]["name"], "413 12");
        assert_eq!(parsed[0]["plmn"], "41312");
    }

    #[test]
    fn skips_malformed_records_rather_than_guessing() {
        let parsed = parse_scan_contents("1,Mobitel,41301,7;garbage;1,DBN,41311;");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["name"], "Mobitel");
    }

    #[test]
    fn empty_and_trailing_separators_yield_nothing() {
        assert!(parse_scan_contents("").is_empty());
        assert!(parse_scan_contents(";;;").is_empty());
    }

    #[test]
    fn three_digit_mnc_splits_after_the_country_code() {
        let parsed = parse_scan_contents("1,Verizon,311480,7;");
        assert_eq!(parsed[0]["mcc"], "311");
        assert_eq!(parsed[0]["mnc"], "480");
    }
}
