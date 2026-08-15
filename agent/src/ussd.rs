//! USSD, over the AT port.
//!
//! This firmware exposes no STK or USSD ubus methods at all, but the modem
//! answers `AT+CUSD=?` with `+CUSD: (0-2)` on `/dev/at_mdm0`, so the network
//! side works even though the toolkit side does not.
//!
//! The catch is that a USSD reply is unsolicited. The modem returns `OK` as
//! soon as it has accepted the request, and the network's actual answer arrives
//! seconds later on its own `+CUSD:` line. Reading it as an ordinary command
//! response gives an empty `OK` and discards the reply, so this waits for the
//! marker instead.

use serde_json::{json, Value};

use crate::at_cmd;
use crate::handlers::AppState;

/// Networks are slow at this, and the reply is the whole point of the request.
const REPLY_TIMEOUT_SECS: u64 = 30;

/// What the `<m>` field of a `+CUSD:` response means.
fn session_state(code: i64) -> &'static str {
    match code {
        0 => "completed",
        // The distinction that matters: the network is waiting for a reply, so
        // a client must keep the session open rather than treat this as done.
        1 => "awaiting_reply",
        2 => "cancelled",
        _ => "unknown",
    }
}

/// A USSD string as accepted from a client.
///
/// These are dialled codes, so the character set is small and well defined.
/// Anything outside it cannot be a USSD request and would be going into a
/// quoted AT argument, so it is refused rather than escaped.
fn validate_code(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("nothing to send".into());
    }
    if value.len() > 182 {
        return Err("that is longer than a USSD request can be".into());
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '*' | '#' | '+'))
    {
        return Err("a USSD code is digits with * and #".into());
    }
    Ok(())
}

/// A reply inside an open session, which may be free text.
fn validate_reply(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("nothing to send".into());
    }
    if value.len() > 182 {
        return Err("that reply is too long".into());
    }
    // Quotes and control characters would break out of the AT argument.
    if value.chars().any(|c| c.is_control() || c == '"' || c == '\\') {
        return Err("that reply contains characters the modem cannot accept".into());
    }
    Ok(())
}

/// Pull the answer out of a `+CUSD:` line.
///
/// The shape is `+CUSD: <m>[,"<text>"[,<dcs>]]`. Both trailing fields are
/// optional — a cancelled session often reports only the code.
pub fn parse_cusd(raw: &str) -> Option<(i64, String, Option<i64>)> {
    let start = raw.find("+CUSD:")?;
    let rest = raw[start + "+CUSD:".len()..].trim_start();

    let mut chars = rest.char_indices();
    let mut code_end = rest.len();
    for (index, ch) in chars.by_ref() {
        if !ch.is_ascii_digit() {
            code_end = index;
            break;
        }
    }
    let code: i64 = rest[..code_end].trim().parse().ok()?;

    let after = rest[code_end..].trim_start();
    let Some(after) = after.strip_prefix(',') else {
        return Some((code, String::new(), None));
    };
    let after = after.trim_start();

    let (text, tail) = if let Some(body) = after.strip_prefix('"') {
        match body.find('"') {
            Some(end) => (body[..end].to_string(), &body[end + 1..]),
            None => (body.to_string(), ""),
        }
    } else {
        let end = after.find(',').unwrap_or(after.len());
        (after[..end].trim().to_string(), &after[end..])
    };

    let dcs = tail
        .trim_start()
        .strip_prefix(',')
        .and_then(|d| d.trim().split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|d| d.parse::<i64>().ok());

    Some((code, text, dcs))
}

/// Turn the network's answer into text.
///
/// With a UCS2 data coding scheme the modem hands back hex rather than
/// characters, which is what makes a balance reply look like `0043006F...`.
/// Decoded when the DCS says UCS2 and the payload really is hex; otherwise the
/// string is passed through, because a wrongly decoded message is worse than an
/// undecoded one.
pub fn decode_message(text: &str, dcs: Option<i64>) -> String {
    let looks_ucs2 = dcs.is_some_and(|d| d & 0x0C == 0x08);
    let is_hex = !text.is_empty()
        && text.len() % 4 == 0
        && text.chars().all(|c| c.is_ascii_hexdigit());
    if !(looks_ucs2 && is_hex) {
        return text.to_string();
    }

    let units: Vec<u16> = text
        .as_bytes()
        .chunks(4)
        .filter_map(|chunk| u16::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok())
        .collect();
    String::from_utf16(&units).unwrap_or_else(|_| text.to_string())
}

/// Pull a `+CME`/`+CMS ERROR` reason out of the modem's output.
///
/// Reported instead of a timeout because the two mean opposite things. On this
/// router the answer is `no network service`: USSD rides the circuit-switched
/// domain, and a data-only device never attaches to it, so no amount of waiting
/// will produce a reply. Saying "the network did not answer in time" would send
/// someone off retrying something that cannot work.
fn modem_error(raw: &str) -> Option<String> {
    let start = raw.find("+CME ERROR:").or_else(|| raw.find("+CMS ERROR:"))?;
    let line = raw[start..].lines().next()?;
    let (_, reason) = line.split_once(':')?;
    Some(reason.trim().to_string())
}

fn answer(state: &AppState, command: &str) -> (u16, Value) {
    match at_cmd::send_awaiting(&state.at_port, command, REPLY_TIMEOUT_SECS, "+CUSD:") {
        Ok(raw) if modem_error(&raw).is_some() => {
            let reason = modem_error(&raw).unwrap_or_default();
            let unavailable = reason.to_ascii_lowercase().contains("no network service");
            (
                502,
                json!({"ok": false, "error": if unavailable {
                    format!("{reason} — USSD needs a circuit-switched connection, which a \
                             data-only router does not make. This modem supports USSD but \
                             cannot reach it on this network.")
                } else {
                    reason
                }}),
            )
        }
        Ok(raw) => match parse_cusd(&raw) {
            Some((code, text, dcs)) => (
                200,
                json!({"ok": true, "data": {
                    "state": session_state(code),
                    "message": decode_message(&text, dcs),
                    "dcs": dcs,
                }}),
            ),
            // The modem accepted it but the network said nothing in time. Not
            // an error the user can act on, so it is reported as its own state
            // rather than a failure.
            None => (
                200,
                json!({"ok": true, "data": {
                    "state": "no_reply",
                    "message": "",
                    "notice": "The request was sent but the network did not answer in time.",
                }}),
            ),
        },
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

fn parse(body: &[u8]) -> Result<Value, (u16, Value)> {
    serde_json::from_slice(body).map_err(|_| (400, json!({"ok": false, "error": "invalid JSON"})))
}

/// POST /api/ussd/send — body `{"code": "*100#"}`
pub fn ussd_send(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let code = parsed["code"].as_str().unwrap_or("").trim().to_string();
    if let Err(e) = validate_code(&code) {
        return (400, json!({"ok": false, "error": e}));
    }
    // DCS 15 is the unpacked 7-bit default every network accepts for a dialled
    // code; the reply comes back in whatever the network chooses.
    answer(state, &format!("AT+CUSD=1,\"{code}\",15"))
}

/// POST /api/ussd/respond — body `{"reply": "1"}`
pub fn ussd_respond(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let reply = parsed["reply"].as_str().unwrap_or("").trim().to_string();
    if let Err(e) = validate_reply(&reply) {
        return (400, json!({"ok": false, "error": e}));
    }
    answer(state, &format!("AT+CUSD=1,\"{reply}\",15"))
}

/// POST /api/ussd/cancel — end an open session.
pub fn ussd_cancel(state: &AppState, _body: &[u8]) -> (u16, Value) {
    match at_cmd::send(&state.at_port, "AT+CUSD=2", 10) {
        Ok(_) => (200, json!({"ok": true, "data": {"state": "cancelled"}})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_plain_reply() {
        let raw = "AT+CUSD=1,\"*100#\",15\r\r\nOK\r\n\r\n+CUSD: 0,\"Your balance is Rs 250.00\",15\r\n";
        let (code, text, dcs) = parse_cusd(raw).unwrap();
        assert_eq!(code, 0);
        assert_eq!(text, "Your balance is Rs 250.00");
        assert_eq!(dcs, Some(15));
        assert_eq!(session_state(code), "completed");
    }

    #[test]
    fn recognises_a_session_waiting_on_the_user() {
        // The distinction that matters: treating this as completed closes a
        // session the network is still holding open.
        let (code, text, _) = parse_cusd("+CUSD: 1,\"1. Balance\n2. Data\",15").unwrap();
        assert_eq!(session_state(code), "awaiting_reply");
        assert!(text.contains("Balance"));
    }

    #[test]
    fn handles_a_response_with_no_text() {
        let (code, text, dcs) = parse_cusd("\r\n+CUSD: 2\r\n").unwrap();
        assert_eq!(code, 2);
        assert_eq!(session_state(code), "cancelled");
        assert!(text.is_empty());
        assert_eq!(dcs, None);
    }

    #[test]
    fn ignores_output_with_no_cusd_line() {
        assert!(parse_cusd("AT+CUSD=1,\"*100#\",15\r\r\nOK\r\n").is_none());
        assert!(parse_cusd("ERROR").is_none());
    }

    #[test]
    fn decodes_a_ucs2_reply() {
        // "Hi" as UTF-16BE hex, which is how a balance reply arrives on many
        // networks and why it otherwise reads as digits.
        assert_eq!(decode_message("00480069", Some(72)), "Hi");
    }

    #[test]
    fn leaves_a_message_alone_when_it_is_not_ucs2() {
        // Plain text under a UCS2 DCS, and hex under a 7-bit DCS: decoding
        // either would produce something the network never sent.
        assert_eq!(decode_message("Balance: 250", Some(72)), "Balance: 250");
        assert_eq!(decode_message("00480069", Some(15)), "00480069");
        assert_eq!(decode_message("ABC", None), "ABC");
    }

    #[test]
    fn reports_the_modems_reason_rather_than_a_timeout() {
        // What this router actually answers: USSD rides the circuit-switched
        // domain and a data-only device never attaches to it. Reporting a
        // timeout would send someone off retrying something that cannot work.
        let raw = "AT+CUSD=1,\"#100#\",15\r\r\n+CME ERROR: no network service\r\n";
        assert_eq!(modem_error(raw).as_deref(), Some("no network service"));
        assert!(modem_error("+CUSD: 0,\"fine\",15").is_none());
        assert!(modem_error("OK").is_none());
    }

    #[test]
    fn refuses_things_that_are_not_ussd_codes() {
        assert!(validate_code("*100#").is_ok());
        assert!(validate_code("*123*1#").is_ok());
        assert!(validate_code("").is_err());
        // Would go straight into a quoted AT argument.
        assert!(validate_code("*100#\"; ATZ").is_err());
        assert!(validate_reply("1").is_ok());
        assert!(validate_reply("some text").is_ok());
        assert!(validate_reply("bad\"quote").is_err());
    }
}
