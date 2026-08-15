//! SIM PIN and PUK.
//!
//! Backed by `zwrt_zte_mdm.api`, which exposes `sim_verify_pin_puk`,
//! `sim_change_pin` and `sim_change_pin_mode`. The state to read them against
//! comes from `get_sim_info`: `pin_status`, plus `pinnumber` and `puknumber`
//! as the remaining attempt counts.
//!
//! Everything here is guarded harder than the rest of the agent, because the
//! failure mode is not "the request did not work". Three wrong PINs lock the
//! SIM and ten wrong PUKs destroy it permanently — there is no recovery, and
//! the user buys a new SIM. So attempts are refused when the remaining count is
//! unknown, and the remaining count is always reported back.

use serde_json::{json, Value};

use crate::handlers::AppState;
use crate::ubus;

const OBJECT: &str = "zwrt_zte_mdm.api";

/// Below this many PUK attempts, a wrong answer is close to destroying the SIM.
///
/// Not a refusal — the user may genuinely know the PUK and need to use it — but
/// the response says how many are left so a UI can make the risk visible rather
/// than presenting another ordinary text box.
const PUK_DANGER_THRESHOLD: u32 = 3;

/// What `pin_status` means.
fn status_label(code: &str) -> &'static str {
    match code {
        "0" => "ready",
        "1" => "pin_required",
        "2" => "puk_required",
        "3" => "disabled",
        _ => "unknown",
    }
}

fn sim_info() -> Value {
    ubus::call(OBJECT, "get_sim_info", Some("{}")).unwrap_or_else(|_| json!({}))
}

fn field(info: &Value, key: &str) -> String {
    info[key].as_str().unwrap_or("").to_string()
}

/// A PIN or PUK as accepted from a client.
///
/// Digits only, and length-checked to the GSM range. These reach a shell-quoted
/// ubus argument, and a value that cannot be a PIN is a client bug worth
/// refusing before it costs an attempt.
fn validate_code(label: &str, value: &str, min: usize, max: usize) -> Result<(), String> {
    if value.len() < min || value.len() > max {
        return Err(format!("{label} must be {min}-{max} digits"));
    }
    if !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("{label} must be digits only"));
    }
    Ok(())
}

fn code_field(body: &Value, key: &str) -> String {
    body[key].as_str().unwrap_or("").trim().to_string()
}

fn parse(body: &[u8]) -> Result<Value, (u16, Value)> {
    serde_json::from_slice(body).map_err(|_| (400, json!({"ok": false, "error": "invalid JSON"})))
}

/// GET /api/sim/lock — PIN state and how many attempts remain.
pub fn sim_lock_status(_state: &AppState) -> (u16, Value) {
    let info = sim_info();
    let status = field(&info, "pin_status");
    let pin_left = field(&info, "pinnumber").parse::<u32>().ok();
    let puk_left = field(&info, "puknumber").parse::<u32>().ok();

    // Network (carrier) lock, which is a separate counter from PIN and PUK and
    // comes from its own method. Reported as null rather than 0 when it cannot
    // be read, because 0 here means "no attempts left" — the same rule the rest
    // of this module follows about never guessing a count downwards.
    let nck_left = ubus::call("zwrt_zte_mdm.api", "get_simlock_available_trials", Some("{}"))
        .ok()
        .and_then(|data| match &data["available_trials"] {
            Value::String(s) => s.parse::<u32>().ok(),
            Value::Number(n) => n.as_u64().map(|v| v as u32),
            _ => None,
        });

    (
        200,
        json!({"ok": true, "data": {
            "status": status_label(&status),
            "raw_status": status,
            "sim_state": field(&info, "sim_states"),
            "pin_attempts_left": pin_left,
            "puk_attempts_left": puk_left,
            "available_trials": nck_left,
            // Said explicitly so a client does not have to know the threshold,
            // and so the warning cannot drift between clients.
            "puk_nearly_exhausted": puk_left.is_some_and(|left| left <= PUK_DANGER_THRESHOLD),
        }}),
    )
}

/// POST /api/sim/pin/verify — body `{"pin": "1234"}`
pub fn sim_pin_verify(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let pin = code_field(&parsed, "pin");
    if let Err(e) = validate_code("PIN", &pin, 4, 8) {
        return (400, json!({"ok": false, "error": e}));
    }
    if let Some(refusal) = refuse_when_attempts_unknown("pinnumber") {
        return refusal;
    }

    let params = json!({
        "pin_num": pin,
        "puk_num": "",
        "pin_save_flag": "0",
        "pin_encode_flag": "0",
    })
    .to_string();
    respond(ubus::call(OBJECT, "sim_verify_pin_puk", Some(&params)))
}

/// POST /api/sim/puk/verify — body `{"puk": "12345678", "pin": "1234"}`
///
/// The new PIN is required: unblocking with a PUK always sets one, and a client
/// that omits it would leave the SIM in a state the user did not choose.
pub fn sim_puk_verify(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let puk = code_field(&parsed, "puk");
    let pin = code_field(&parsed, "pin");
    if let Err(e) = validate_code("PUK", &puk, 8, 8) {
        return (400, json!({"ok": false, "error": e}));
    }
    if let Err(e) = validate_code("The new PIN", &pin, 4, 8) {
        return (400, json!({"ok": false, "error": e}));
    }
    if let Some(refusal) = refuse_when_attempts_unknown("puknumber") {
        return refusal;
    }

    let params = json!({
        "puk_num": puk,
        "pin_num": pin,
        "pin_save_flag": "0",
        "pin_encode_flag": "0",
    })
    .to_string();
    respond(ubus::call(OBJECT, "sim_verify_pin_puk", Some(&params)))
}

/// POST /api/sim/pin/change — body `{"pin": "1234", "new_pin": "4321"}`
pub fn sim_pin_change(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let pin = code_field(&parsed, "pin");
    let new_pin = code_field(&parsed, "new_pin");
    if let Err(e) = validate_code("PIN", &pin, 4, 8) {
        return (400, json!({"ok": false, "error": e}));
    }
    if let Err(e) = validate_code("The new PIN", &new_pin, 4, 8) {
        return (400, json!({"ok": false, "error": e}));
    }
    if let Some(refusal) = refuse_when_attempts_unknown("pinnumber") {
        return refusal;
    }

    let params = json!({
        "pin_num": pin,
        "new_pin_num": new_pin,
        "pin_save_flag": "0",
        "pin_encode_flag": "0",
    })
    .to_string();
    respond(ubus::call(OBJECT, "sim_change_pin", Some(&params)))
}

/// POST /api/sim/pin/toggle — body `{"pin": "1234", "enabled": true}`
pub fn sim_pin_toggle(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let pin = code_field(&parsed, "pin");
    if let Err(e) = validate_code("PIN", &pin, 4, 8) {
        return (400, json!({"ok": false, "error": e}));
    }
    let Some(enabled) = parsed["enabled"].as_bool() else {
        return (400, json!({"ok": false, "error": "missing 'enabled'"}));
    };
    if let Some(refusal) = refuse_when_attempts_unknown("pinnumber") {
        return refusal;
    }

    let params = json!({
        "pin_num_m": pin,
        "pin_mode": if enabled { 1 } else { 0 },
        "pin_save_flag": "0",
        "pin_encode_flag": "0",
    })
    .to_string();
    respond(ubus::call(OBJECT, "sim_change_pin_mode", Some(&params)))
}

/// Refuse to spend an attempt when the remaining count cannot be read.
///
/// A wrong PIN costs one of three tries and a wrong PUK is one of ten before
/// the SIM is scrap. If the modem will not say how many are left, the honest
/// answer is to not try — the caller can read the state and decide, rather than
/// the agent gambling with something that cannot be undone.
fn refuse_when_attempts_unknown(key: &str) -> Option<(u16, Value)> {
    let info = sim_info();
    let readable = info[key].as_str().is_some_and(|v| v.parse::<u32>().is_ok());
    if readable {
        return None;
    }
    Some((
        503,
        json!({"ok": false, "error":
            "the modem will not report how many attempts remain, so this was not sent. \
             A wrong PIN or PUK cannot be taken back."}),
    ))
}

/// Answer with the vendor result and the attempt counts that follow it.
///
/// The counts matter more than the result: "wrong PIN" and "wrong PIN, one try
/// left" call for very different behaviour from whoever is holding the phone.
fn respond(result: Result<Value, String>) -> (u16, Value) {
    match result {
        Ok(value) => {
            let after = sim_lock_status_data();
            let ok = value["ret"].as_i64().unwrap_or(0) == 0
                || value["result"].as_str() == Some("SUCCESS");
            (
                if ok { 200 } else { 502 },
                json!({"ok": ok, "data": {"result": value, "state": after},
                       "error": if ok { Value::Null } else { json!("the modem rejected that code") }}),
            )
        }
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

fn sim_lock_status_data() -> Value {
    let info = sim_info();
    json!({
        "status": status_label(&field(&info, "pin_status")),
        "pin_attempts_left": field(&info, "pinnumber").parse::<u32>().ok(),
        "puk_attempts_left": field(&info, "puknumber").parse::<u32>().ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_pin_states_the_modem_reports() {
        assert_eq!(status_label("0"), "ready");
        assert_eq!(status_label("1"), "pin_required");
        assert_eq!(status_label("2"), "puk_required");
        // Never guessed at: an unknown code must not be presented as "ready",
        // which is the one label that would make a UI hide the unlock screen.
        assert_eq!(status_label("9"), "unknown");
        assert_eq!(status_label(""), "unknown");
    }

    #[test]
    fn rejects_codes_that_cannot_be_pins() {
        assert!(validate_code("PIN", "1234", 4, 8).is_ok());
        assert!(validate_code("PIN", "12345678", 4, 8).is_ok());
        assert!(validate_code("PIN", "123", 4, 8).is_err());
        assert!(validate_code("PIN", "123456789", 4, 8).is_err());
        // Letters, spaces and shell characters never reach a ubus argument,
        // and refusing here costs no attempt.
        assert!(validate_code("PIN", "12a4", 4, 8).is_err());
        assert!(validate_code("PIN", "12 4", 4, 8).is_err());
        assert!(validate_code("PIN", "'; reboot", 4, 8).is_err());
        assert!(validate_code("PIN", "", 4, 8).is_err());
    }

    #[test]
    fn a_puk_is_exactly_eight_digits() {
        assert!(validate_code("PUK", "12345678", 8, 8).is_ok());
        assert!(validate_code("PUK", "1234567", 8, 8).is_err());
        assert!(validate_code("PUK", "123456789", 8, 8).is_err());
    }
}
