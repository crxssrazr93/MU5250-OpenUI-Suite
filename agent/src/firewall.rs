//! Firewall, port forwarding and domain filtering.
//!
//! All three live on `zwrt_router.api`. `router_get_firewall_para` reports the
//! switches as one object; forwarding and filtering rules are read and written
//! individually.
//!
//! The vendor writes these straight into UCI and then into iptables, so every
//! value a client supplies is checked for shape here rather than escaped. A
//! rule with a malformed address does not fail cleanly — it can leave the
//! firewall reloading in a state nobody asked for.

use serde_json::{json, Value};

use crate::handlers::AppState;
use crate::ubus;

const OBJECT: &str = "zwrt_router.api";

/// The switches, as booleans rather than the strings the vendor returns.
///
/// A client comparing `"0"` to `false` gets it wrong silently, and every one of
/// these controls something a user would notice being wrong.
fn as_bool(value: &Value, key: &str) -> bool {
    matches!(value[key].as_str(), Some("1")) || value[key].as_i64() == Some(1)
}

fn params(value: Value) -> String {
    value.to_string()
}

/// An IPv4 address as accepted from a client.
fn validate_ip(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    value
        .parse::<std::net::Ipv4Addr>()
        .map(|_| ())
        .map_err(|_| format!("{label} must be an IPv4 address"))
}

/// A port or an inclusive range, which is what the vendor field accepts.
fn validate_port_spec(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label} is required"));
    }
    let parts: Vec<&str> = value.split('-').collect();
    if parts.len() > 2 {
        return Err(format!("{label} must be a port or a range like 8000-8010"));
    }
    let mut previous = 0u32;
    for (index, part) in parts.iter().enumerate() {
        let port: u32 = part
            .trim()
            .parse()
            .map_err(|_| format!("{label} must be a port number"))?;
        if port == 0 || port > 65535 {
            return Err(format!("{label} must be between 1 and 65535"));
        }
        // A backwards range is accepted by the vendor script and then forwards
        // nothing, which looks like a rule that exists but does not work.
        if index == 1 && port < previous {
            return Err(format!("{label} range must run low to high"));
        }
        previous = port;
    }
    Ok(())
}

fn validate_proto(value: &str) -> Result<(), String> {
    match value.to_ascii_uppercase().as_str() {
        "TCP" | "UDP" | "TCP+UDP" | "TCPANDUDP" | "ALL" => Ok(()),
        _ => Err("protocol must be TCP, UDP or TCP+UDP".into()),
    }
}

fn parse(body: &[u8]) -> Result<Value, (u16, Value)> {
    serde_json::from_slice(body).map_err(|_| (400, json!({"ok": false, "error": "invalid JSON"})))
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").trim().to_string()
}

/// GET /api/firewall/config
pub fn firewall_config_get(_state: &AppState) -> (u16, Value) {
    let raw = ubus::call(OBJECT, "router_get_firewall_para", Some("{}"))
        .unwrap_or_else(|_| json!({}));
    (
        200,
        json!({"ok": true, "data": {
            "firewall_enabled": as_bool(&raw, "firewall_enable"),
            "nat_enabled": as_bool(&raw, "nat_enable"),
            "port_forward_enabled": as_bool(&raw, "portforward_enable"),
            "port_mapping_enabled": as_bool(&raw, "portmapping_enable"),
            "dmz_enabled": as_bool(&raw, "dmz_enable"),
            "dmz_ip": raw["dmz_ip"],
            "mac_ip_port_filter_enabled": as_bool(&raw, "macipport_filter_enable"),
            "filter_policy": raw["macipport_filter_policy"],
            // Both are ways in from the internet, and both default off. Named
            // plainly because "remote web access" reads harmless and is not.
            "remote_admin_enabled": as_bool(&raw, "remote_web_access_enable"),
            "wan_ping_enabled": as_bool(&raw, "wan_ping_enable"),
            "raw": raw,
        }}),
    )
}

/// POST /api/firewall/config — body `{"firewall_enabled": true}` and friends.
///
/// Only the switches this maps are writable. The vendor object has more, and
/// several of them (DDNS credentials, MWAN routing) are not firewall settings
/// at all and have no business behind this route.
pub fn firewall_config_set(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse(body) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let mut applied = Vec::new();

    if let Some(enabled) = parsed["firewall_enabled"].as_bool() {
        let call = params(json!({"enable": i32::from(enabled)}));
        if let Err(e) = ubus::call(OBJECT, "router_set_firewall_switch", Some(&call)) {
            return (503, json!({"ok": false, "error": e}));
        }
        applied.push("firewall_enabled");
    }

    if let Some(enabled) = parsed["port_forward_enabled"].as_bool() {
        let call = params(json!({"portforward_enable": i32::from(enabled)}));
        if let Err(e) = ubus::call(OBJECT, "router_set_portforward_switch", Some(&call)) {
            return (503, json!({"ok": false, "error": e}));
        }
        applied.push("port_forward_enabled");
    }

    if let Some(enabled) = parsed["port_mapping_enabled"].as_bool() {
        let call = params(json!({"portmapping_enable": i32::from(enabled)}));
        if let Err(e) = ubus::call(OBJECT, "router_set_portmapping_switch", Some(&call)) {
            return (503, json!({"ok": false, "error": e}));
        }
        applied.push("port_mapping_enabled");
    }

    if applied.is_empty() {
        return (
            400,
            json!({"ok": false, "error": "nothing to change — no known setting was given"}),
        );
    }
    (200, json!({"ok": true, "data": {"applied": applied}}))
}

/// GET /api/firewall/port-forward
pub fn port_forward_list(_state: &AppState) -> (u16, Value) {
    let rules = ubus::call(OBJECT, "router_get_portforward_rule", Some("{}"))
        .unwrap_or_else(|_| json!({}));
    (200, json!({"ok": true, "data": {"rules": rules}}))
}

/// POST /api/firewall/port-forward
///
/// Body `{"action": "add"|"edit"|"delete", "src_dport": "8080", "dest_ip":
/// "192.168.0.50", "proto": "TCP", "enabled": true, "comment": "..."}`.
pub fn port_forward_set(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse(body) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let action = text(&parsed, "action").to_ascii_lowercase();
    if !matches!(action.as_str(), "add" | "edit" | "delete") {
        return (
            400,
            json!({"ok": false, "error": "action must be add, edit or delete"}),
        );
    }

    let section_id: Vec<Value> = parsed["section_id"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if action != "add" && section_id.is_empty() {
        return (
            400,
            json!({"ok": false, "error": "'section_id' identifies which rule to change"}),
        );
    }

    let mut call = json!({"action": action, "section_id": section_id});

    if action != "delete" {
        let src_dport = text(&parsed, "src_dport");
        let dest_ip = text(&parsed, "dest_ip");
        let proto = text(&parsed, "proto");
        if let Err(e) = validate_port_spec("The external port", &src_dport) {
            return (400, json!({"ok": false, "error": e}));
        }
        if let Err(e) = validate_ip("The destination", &dest_ip) {
            return (400, json!({"ok": false, "error": e}));
        }
        if dest_ip.is_empty() {
            return (400, json!({"ok": false, "error": "'dest_ip' is required"}));
        }
        if let Err(e) = validate_proto(&proto) {
            return (400, json!({"ok": false, "error": e}));
        }
        let comment = text(&parsed, "comment");
        if comment.len() > 64 || comment.chars().any(char::is_control) {
            return (400, json!({"ok": false, "error": "that comment is not usable"}));
        }
        call["src_dport"] = json!(src_dport);
        call["dest_ip"] = json!(dest_ip);
        call["proto"] = json!(proto.to_ascii_uppercase());
        call["enabled"] = json!(i32::from(parsed["enabled"].as_bool().unwrap_or(true)));
        call["comment"] = json!(comment);
    }

    match ubus::call(OBJECT, "router_set_portforward", Some(&params(call))) {
        Ok(result) => (200, json!({"ok": true, "data": {"result": result}})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// GET /api/firewall/domain-filter
pub fn domain_filter_list(_state: &AppState) -> (u16, Value) {
    let rules = ubus::call(OBJECT, "router_get_domainfilter_rule", Some("{}"))
        .unwrap_or_else(|_| json!({}));
    (200, json!({"ok": true, "data": {"rules": rules}}))
}

/// POST /api/firewall/domain-filter/rule
///
/// Body `{"action": "add"|"delete", "fqdn": "example.com", "mac": "...",
/// "enable": true, "target": "DROP"}`.
pub fn domain_filter_rule(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed = match parse(body) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let action = text(&parsed, "action").to_ascii_lowercase();
    if !matches!(action.as_str(), "add" | "edit" | "delete") {
        return (
            400,
            json!({"ok": false, "error": "action must be add, edit or delete"}),
        );
    }

    let fqdn = text(&parsed, "fqdn");
    if action != "delete" {
        if let Err(e) = validate_fqdn(&fqdn) {
            return (400, json!({"ok": false, "error": e}));
        }
    }

    let mac = text(&parsed, "mac");
    if !mac.is_empty() && !is_mac(&mac) {
        return (400, json!({"ok": false, "error": "'mac' is not a MAC address"}));
    }

    // DROP by default: this is a blocklist, and a rule that silently defaults
    // to ACCEPT would read as blocking while allowing.
    let target = match text(&parsed, "target").to_ascii_uppercase().as_str() {
        "" | "DROP" => "DROP".to_string(),
        "ACCEPT" => "ACCEPT".to_string(),
        _ => return (400, json!({"ok": false, "error": "target must be DROP or ACCEPT"})),
    };

    let call = json!({
        "action": action,
        "section_id": parsed["section_id"].as_array().cloned().unwrap_or_default(),
        "mac": mac,
        "fqdn": fqdn,
        "enable": i32::from(parsed["enable"].as_bool().unwrap_or(true)),
        "target": target,
        "dev": text(&parsed, "dev"),
        "policy": text(&parsed, "policy"),
    });

    match ubus::call(OBJECT, "router_set_domain_filter", Some(&params(call))) {
        Ok(result) => (200, json!({"ok": true, "data": {"result": result}})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// A hostname the filter can match on.
fn validate_fqdn(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("'fqdn' is required".into());
    }
    if value.len() > 253 {
        return Err("that domain is too long".into());
    }
    let usable = value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '*'));
    if !usable {
        return Err("that domain contains characters a filter rule cannot hold".into());
    }
    Ok(())
}

fn is_mac(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_vendor_switch_strings_as_booleans() {
        let raw = json!({"firewall_enable": "1", "nat_enable": "0", "dmz_enable": 1});
        assert!(as_bool(&raw, "firewall_enable"));
        assert!(!as_bool(&raw, "nat_enable"));
        assert!(as_bool(&raw, "dmz_enable"));
        // Absent is off, not on: these open ways in from the internet.
        assert!(!as_bool(&raw, "remote_web_access_enable"));
    }

    #[test]
    fn accepts_ports_and_ranges() {
        assert!(validate_port_spec("port", "8080").is_ok());
        assert!(validate_port_spec("port", "8000-8010").is_ok());
        assert!(validate_port_spec("port", "0").is_err());
        assert!(validate_port_spec("port", "65536").is_err());
        assert!(validate_port_spec("port", "").is_err());
        assert!(validate_port_spec("port", "80-90-100").is_err());
        // Accepted by the vendor script and then forwards nothing, which looks
        // like a rule that exists but does not work.
        assert!(validate_port_spec("port", "9000-8000").is_err());
    }

    #[test]
    fn checks_addresses_and_protocols() {
        assert!(validate_ip("dest", "192.168.0.50").is_ok());
        assert!(validate_ip("dest", "192.168.0.999").is_err());
        assert!(validate_ip("dest", "; reboot").is_err());
        assert!(validate_proto("TCP").is_ok());
        assert!(validate_proto("tcp").is_ok());
        assert!(validate_proto("SCTP").is_err());
    }

    #[test]
    fn checks_domains_and_macs() {
        assert!(validate_fqdn("example.com").is_ok());
        assert!(validate_fqdn("*.ads.example.com").is_ok());
        assert!(validate_fqdn("").is_err());
        assert!(validate_fqdn("example.com; reboot").is_err());
        assert!(is_mac("aa:bb:cc:dd:ee:ff"));
        assert!(!is_mac("aa:bb:cc:dd:ee"));
        assert!(!is_mac("not a mac"));
    }
}
