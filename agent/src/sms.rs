use std::process::Command;

use serde_json::{json, Value};

use crate::handlers::AppState;
use crate::ubus;
use crate::validate::validate_ubus_input;

const SMS_DB_PATH: &str = "/etc_rw/ztembb/ztesms/sms_db/sms.db";

pub fn sms_list(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    if let Err(e) = validate_ubus_input(&parsed) {
        return (400, json!({"ok": false, "error": e}));
    }
    match ubus::call(
        "zwrt_wms",
        "zte_libwms_get_sms_data",
        Some(&normalize_sms_query(&parsed).to_string()),
    ) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// Build the argument set `zte_libwms_get_sms_data` will actually accept.
///
/// The daemon rejects the whole call with "Invalid argument" if any field is
/// missing or malformed, and `order_by` is not a sort spec — it is pasted into
/// a SQL statement, so it has to read like `order by id desc`. Passing the
/// obvious `"date,desc"` fails, which is what broke every SMS listing: the
/// route forwarded the client's body verbatim, so the client had to already
/// know the daemon's SQL dialect.
///
/// Callers now send whatever they like and this fills in the rest. Only a
/// fixed set of sort spellings is accepted, because the value reaches SQL.
fn normalize_sms_query(input: &Value) -> Value {
    const DEFAULT_ORDER: &str = "order by id desc";

    let order_by = match input["order_by"].as_str().map(str::trim) {
        // Already a SQL fragment: keep it only if it is one we recognise.
        Some(v) if is_known_sms_order(v) => v.to_string(),
        // The natural "<field>,<direction>" spelling, mapped rather than passed.
        Some(v) => match v.to_ascii_lowercase().replace(' ', "").as_str() {
            "date,desc" | "id,desc" | "" => DEFAULT_ORDER.to_string(),
            "date,asc" | "id,asc" => "order by id asc".to_string(),
            _ => DEFAULT_ORDER.to_string(),
        },
        None => DEFAULT_ORDER.to_string(),
    };

    // `per_page` is the name every client reaches for; the daemon calls it
    // `data_per_page`.
    let per_page = input["data_per_page"]
        .as_u64()
        .or_else(|| input["per_page"].as_u64())
        .unwrap_or(500);

    json!({
        "page": input["page"].as_u64().unwrap_or(0),
        "data_per_page": per_page,
        "mem_store": input["mem_store"].as_u64().unwrap_or(1),
        "tags": input["tags"].as_u64().unwrap_or(10),
        "order_by": order_by,
    })
}

/// The sort fragments this agent is willing to hand to the daemon's SQL.
fn is_known_sms_order(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "order by id desc" | "order by id asc" | "order by date desc" | "order by date asc"
    )
}

pub fn sms_send(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    if let Err(e) = validate_ubus_input(&parsed) {
        return (400, json!({"ok": false, "error": e}));
    }
    let call = match normalize_sms_send(&parsed) {
        Ok(v) => v,
        Err(e) => return (400, json!({"ok": false, "error": e})),
    };
    match ubus::call("zwrt_wms", "zte_libwms_send_sms", Some(&call.to_string())) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// Build the argument set `zte_libwms_send_sms` will accept.
///
/// The same problem `normalize_sms_query` solves for listing: the route used to
/// forward the client's body verbatim, so a client had to already know the
/// daemon's field names and its encoding rules. The dashboard sent
/// `{to, text}`, which reached the daemon carrying neither `number` nor
/// `message_body`, so sending from the web UI could never have worked.
///
/// Two things are not obvious and were established on the device rather than
/// assumed:
///
/// - `sms_time` is separated by **semicolons**, even though the listing returns
///   stored dates comma-separated and `zte_topsw_wms` carries a
///   `%s,%s,%s,%s,%s,%s,%s` format string. That format string belongs to the
///   reader, not this argument: sending a comma-separated stamp is refused with
///   `Invalid argument`, and the separator is the only field that decides it.
///   Every other combination of number and body encoding was tried against the
///   live daemon and the split was clean — semicolons accepted, commas refused.
/// - `number` is sent **plain**. The daemon UCS-2 encodes it for storage
///   itself, so pre-encoding it stores a double-encoded address.
/// - the trailing field is the UTC offset in **quarter hours**, which is how
///   the listing reports `+22` for a UTC+5:30 network.
fn normalize_sms_send(input: &Value) -> Result<Value, String> {
    let first_str = |keys: &[&str]| -> String {
        keys.iter()
            .find_map(|k| input[*k].as_str().map(str::trim).filter(|v| !v.is_empty()))
            .unwrap_or("")
            .to_string()
    };

    let number = first_str(&["number", "to", "msisdn", "phone"]);
    if number.is_empty() {
        return Err("'number' is required (or 'to')".into());
    }
    // Reaches a shell-quoted ubus argument, so keep it to what a destination
    // can actually be.
    if !number.chars().all(|c| c.is_ascii_digit() || c == '+' || c == '*' || c == '#') {
        return Err("that destination is not a number the modem can dial".into());
    }

    // A caller that already encoded gets left alone; one that sent plain text
    // gets encoded here, so neither has to know which is expected.
    let (message_body, encode_type) = match input["message_body"].as_str() {
        Some(hex) if !hex.is_empty() => (
            hex.to_string(),
            input["encode_type"].as_str().unwrap_or("UNICODE").to_string(),
        ),
        _ => {
            let text = first_str(&["text", "message", "body"]);
            if text.is_empty() {
                return Err("'text' is required (or 'message_body' already encoded)".into());
            }
            // Always hex, never the plain text. The daemon reads
            // `message_body` as hex whatever `encode_type` says, so sending
            // "TEST" as GSM7_default stored a single "@" — the letters were
            // read as hex digits. UNICODE costs 70 characters a segment
            // instead of 160, which is the price of a message that arrives.
            (to_ucs2_hex(&text), "UNICODE".to_string())
        }
    };

    Ok(json!({
        "number": number,
        "message_body": message_body,
        "encode_type": encode_type,
        "sms_time": input["sms_time"].as_str().map(str::to_string).unwrap_or_else(sms_timestamp),
        // -1 is "a new message" rather than an edit of a stored one.
        "id": input["id"].as_str().unwrap_or("-1"),
    }))
}

fn to_ucs2_hex(text: &str) -> String {
    text.encode_utf16().map(|u| format!("{u:04X}")).collect()
}

/// `YY;MM;DD;HH;MM;SS;+Q`, local time, offset in quarter hours.
fn sms_timestamp() -> String {
    let stamp = Command::new("date")
        .arg("+%y;%m;%d;%H;%M;%S;%z")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let stamp = stamp.trim();

    // `%z` is +0530; the daemon wants the offset counted in quarter hours.
    match stamp.rsplit_once(';') {
        Some((head, offset)) if offset.len() == 5 => {
            let sign = if offset.starts_with('-') { -1 } else { 1 };
            let hours: i32 = offset[1..3].parse().unwrap_or(0);
            let minutes: i32 = offset[3..5].parse().unwrap_or(0);
            let quarters = sign * (hours * 4 + minutes / 15);
            format!("{head};{quarters:+}")
        }
        // Better a message with no timestamp than no message: the daemon
        // stamps it itself when the field is empty.
        _ => String::new(),
    }
}

/// Delete one or more SMS by id.
///
/// Body shape (legacy ZTE format): `{"id": "3681;3682;"}` — semicolon-joined ids with trailing `;`.
///
/// Firmware bug: `zwrt_wms_delete_sms` works for NV-stored messages but silently returns
/// `{"result": 3}` without deleting SIM-stored rows. The daemon's listing reads from
/// `/etc_rw/ztembb/ztesms/sms_db/sms.db`, so we fall back to a direct SQLite DELETE for any
/// id that survived the ubus call.
pub fn sms_delete(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    if let Err(e) = validate_ubus_input(&parsed) {
        return (400, json!({"ok": false, "error": e}));
    }

    let ids = match parse_ids(parsed.get("id")) {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => return (400, json!({"ok": false, "error": "no ids in 'id' field"})),
        Err(e) => return (400, json!({"ok": false, "error": e})),
    };

    let ubus_result = ubus::call("zwrt_wms", "zwrt_wms_delete_sms", Some(&parsed.to_string()));

    let survivors = match db_filter_existing(&ids) {
        Ok(v) => v,
        Err(e) => {
            return match ubus_result {
                Ok(data) => (
                    200,
                    json!({"ok": true, "data": data, "warning": format!("db check skipped: {e}")}),
                ),
                Err(ubus_err) => (
                    503,
                    json!({"ok": false, "error": format!("ubus: {ubus_err}; db: {e}")}),
                ),
            };
        }
    };

    if survivors.is_empty() {
        return (
            200,
            json!({"ok": true, "data": ubus_result.unwrap_or(Value::Null), "deleted_via": "ubus"}),
        );
    }

    match db_delete_ids(&survivors) {
        Ok(()) => (
            200,
            json!({"ok": true, "deleted_via": "sqlite", "ids": survivors}),
        ),
        Err(e) => (
            503,
            json!({"ok": false, "error": format!("sqlite delete failed: {e}")}),
        ),
    }
}

/// Parse the legacy ZTE id format (semicolon-joined with trailing `;`).
fn parse_ids(field: Option<&Value>) -> Result<Vec<i64>, String> {
    let raw = field.ok_or_else(|| "missing 'id' field".to_string())?;
    let s = raw.as_str().ok_or_else(|| "'id' must be a string".to_string())?;
    let mut out = Vec::new();
    for part in s.split(';') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let n: i64 = p
            .parse()
            .map_err(|_| format!("invalid id '{p}' (must be integer)"))?;
        out.push(n);
    }
    Ok(out)
}

/// Return the subset of `ids` still present in the WMS sms table.
fn db_filter_existing(ids: &[i64]) -> Result<Vec<i64>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let in_clause = ids
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT id FROM sms WHERE id IN ({in_clause});");
    let output = Command::new("/usr/bin/sqlite3")
        .args(["-cmd", ".timeout 2000", "-readonly", SMS_DB_PATH, &sql])
        .output()
        .map_err(|e| format!("spawn sqlite3: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let mut out = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if let Ok(n) = l.parse::<i64>() {
            out.push(n);
        }
    }
    Ok(out)
}

/// Direct DELETE bypassing the broken ubus path for SIM-stored rows.
fn db_delete_ids(ids: &[i64]) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let in_clause = ids
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("DELETE FROM sms WHERE id IN ({in_clause});");
    let output = Command::new("/usr/bin/sqlite3")
        .args(["-cmd", ".timeout 2000", SMS_DB_PATH, &sql])
        .output()
        .map_err(|e| format!("spawn sqlite3: {e}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

pub fn sms_mark_read(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    if let Err(e) = validate_ubus_input(&parsed) {
        return (400, json!({"ok": false, "error": e}));
    }
    match ubus::call("zwrt_wms", "zwrt_wms_modify_tag", Some(&parsed.to_string())) {
        Ok(data) => (200, json!({"ok": true, "data": data})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

#[cfg(test)]
mod sms_query_tests {
    use super::*;

    #[test]
    fn maps_the_natural_sort_spelling_to_the_daemons_sql() {
        // What the dashboard used to send, which the daemon rejected outright.
        let out = normalize_sms_query(&json!({"page": 0, "mem_store": 1, "order_by": "date,desc"}));
        assert_eq!(out["order_by"], "order by id desc");
    }

    #[test]
    fn fills_in_every_field_the_daemon_demands() {
        // An empty body must still produce a complete, valid argument set:
        // the daemon fails the whole call if anything is missing.
        let out = normalize_sms_query(&json!({}));
        for key in ["page", "data_per_page", "mem_store", "tags", "order_by"] {
            assert!(out.get(key).is_some(), "missing {key}");
        }
        assert_eq!(out["tags"], 10);
        assert_eq!(out["data_per_page"], 500);
    }

    #[test]
    fn accepts_per_page_as_well_as_data_per_page() {
        assert_eq!(normalize_sms_query(&json!({"per_page": 25}))["data_per_page"], 25);
        assert_eq!(normalize_sms_query(&json!({"data_per_page": 7}))["data_per_page"], 7);
    }

    #[test]
    fn refuses_to_pass_arbitrary_text_into_sql() {
        // order_by is interpolated into a SQL statement by the daemon, so an
        // unrecognised value must fall back rather than travel.
        let hostile = json!({"order_by": "id desc; drop table sms"});
        assert_eq!(normalize_sms_query(&hostile)["order_by"], "order by id desc");
    }

    #[test]
    fn keeps_a_recognised_sql_fragment() {
        let out = normalize_sms_query(&json!({"order_by": "order by id asc"}));
        assert_eq!(out["order_by"], "order by id asc");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_dashboard_spelling() {
        // {to, text} is what the web UI sends. It used to reach the daemon
        // unchanged, carrying neither of the fields the daemon reads.
        let call = normalize_sms_send(&json!({"to": "12345", "text": "hello"})).unwrap();
        assert_eq!(call["number"], "12345");
        assert_eq!(call["message_body"], "00680065006C006C006F");
        assert_eq!(call["encode_type"], "UNICODE");
        assert_eq!(call["id"], "-1");
    }

    #[test]
    fn accepts_the_vendor_spelling_unchanged() {
        let call = normalize_sms_send(&json!({
            "number": "12345",
            "message_body": "0054004500530054",
            "encode_type": "UNICODE",
            "sms_time": "26;08;15;07;30;00;+22",
        }))
        .unwrap();
        assert_eq!(call["message_body"], "0054004500530054");
        assert_eq!(call["sms_time"], "26;08;15;07;30;00;+22");
    }

    #[test]
    fn encodes_every_body_as_hex() {
        // Plain ASCII is encoded too. Passing it through reached the daemon as
        // hex digits and stored one wrong character in place of the message.
        let plain = normalize_sms_send(&json!({"to": "1", "text": "Top-up Rs.385"})).unwrap();
        assert_eq!(plain["encode_type"], "UNICODE");
        assert_eq!(plain["message_body"], to_ucs2_hex("Top-up Rs.385"));
        assert_eq!(plain["message_body"].as_str().unwrap().len(), 13 * 4);

        let unicode = normalize_sms_send(&json!({"to": "1", "text": "ආයුබෝවන්"})).unwrap();
        assert_eq!(unicode["encode_type"], "UNICODE");
        // Encoded, not passed through, and reversible.
        assert_eq!(unicode["message_body"].as_str().unwrap().len() % 4, 0);
    }

    #[test]
    fn ucs2_matches_the_encoding_the_firmware_uses() {
        // "DialogPROMO" as it arrives in a listing from this firmware.
        assert_eq!(
            to_ucs2_hex("DialogPROMO"),
            "004400690061006C006F006700500052004F004D004F"
        );
    }

    #[test]
    fn a_destination_reaches_a_shell_so_it_is_checked() {
        assert!(normalize_sms_send(&json!({"to": "12345; reboot", "text": "x"})).is_err());
        assert!(normalize_sms_send(&json!({"to": "$(id)", "text": "x"})).is_err());
        // The characters a real destination does use.
        assert!(normalize_sms_send(&json!({"to": "+94771234567", "text": "x"})).is_ok());
        assert!(normalize_sms_send(&json!({"to": "*123#", "text": "x"})).is_ok());
    }

    #[test]
    fn a_message_with_no_destination_is_refused() {
        assert!(normalize_sms_send(&json!({"text": "hello"})).is_err());
        assert!(normalize_sms_send(&json!({"to": "12345"})).is_err());
    }

    #[test]
    fn the_timestamp_is_semicolons_and_quarter_hours() {
        // The daemon refuses a comma-separated stamp with `Invalid argument`
        // and accepts the identical call with semicolons. It is the only field
        // that decides acceptance, so the separator is asserted here.
        let stamp = sms_timestamp();
        if stamp.is_empty() {
            return; // no `date` in the test environment; nothing to assert
        }
        assert!(!stamp.contains(','), "{stamp} must not use commas");
        let fields: Vec<&str> = stamp.split(';').collect();
        assert_eq!(fields.len(), 7, "{stamp} should be YY;MM;DD;HH;MM;SS;+Q");
        let quarters: i32 = fields[6].parse().expect("offset is a number of quarter hours");
        assert!((-48..=56).contains(&quarters), "{quarters} is not a real UTC offset");
    }
}
