//! Saved WireGuard profiles.
//!
//! The vendor firmware has room for exactly one tunnel: a single UCI section
//! that the connect scripts read. Switching providers means overwriting it, and
//! anything you had before is gone — including the private key, which for a
//! commercial provider cannot be regenerated because they only ever gave you
//! the one.
//!
//! So profiles are kept here instead, and activating one copies it into the
//! vendor's section. That keeps the scripts working untouched while letting a
//! router hold as many tunnels as the user has configs for.
//!
//! They live under `/data`, which is writable and survives a firmware update,
//! unlike the read-only rootfs. The file holds private keys, so it is created
//! 0600 and never returned to a client in the clear.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::net::ToSocketAddrs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::tunnel;
use crate::ubus;

const STORE_DIR: &str = "/data/wireguard";
const STORE_PATH: &str = "/data/wireguard/profiles.json";

/// Keep a lid on it: this is a router, and each profile holds a key.
const MAX_PROFILES: usize = 32;

/// A `.conf` beyond this is not a WireGuard config, it is a mistake.
const MAX_CONF_BYTES: usize = 16 * 1024;

/// Read every saved profile.
///
/// A store that cannot be read reads as empty rather than as an error: the
/// common cause is that nothing has been saved yet, and a first-run failure
/// would be indistinguishable from a real fault.
fn load() -> Vec<Value> {
    fs::read_to_string(STORE_PATH)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

/// Write the store back, replacing it atomically.
///
/// Written to a temporary file and renamed so an interrupted write cannot leave
/// a half-written store — which would lose every profile, not just the one
/// being saved.
fn save(profiles: &[Value]) -> Result<(), String> {
    fs::create_dir_all(STORE_DIR).map_err(|e| format!("could not create {STORE_DIR}: {e}"))?;
    let _ = fs::set_permissions(STORE_DIR, fs::Permissions::from_mode(0o700));

    let temp = format!("{STORE_PATH}.tmp");
    let body = serde_json::to_vec_pretty(&profiles).map_err(|e| e.to_string())?;
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            // Private keys. Never group- or world-readable, not even briefly.
            .mode(0o600)
            .open(&temp)
            .map_err(|e| format!("could not write {temp}: {e}"))?;
        file.write_all(&body).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&temp, STORE_PATH).map_err(|e| format!("could not replace the store: {e}"))
}

/// Parse a standard WireGuard `.conf`.
///
/// This is the format every provider hands out, so importing one is the normal
/// way a profile gets created — retyping eight fields from a file you already
/// have is how transcription errors get in.
///
/// Only the keys this router can act on are read. `DNS`, `MTU` and
/// `PersistentKeepalive` are recognised and deliberately ignored: the vendor
/// scripts have nowhere to put them, and silently dropping them is better than
/// pretending they were applied.
pub fn parse_conf(text: &str) -> Result<BTreeMap<String, String>, String> {
    if text.len() > MAX_CONF_BYTES {
        return Err("that file is too large to be a WireGuard config".into());
    }

    let mut section = String::new();
    let mut found: BTreeMap<String, String> = BTreeMap::new();

    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        // Keys are base64 and contain '=' themselves, so only the first splits.
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_string();
        if value.is_empty() {
            continue;
        }
        found.insert(format!("{section}.{key}"), value);
    }

    let mut settings = BTreeMap::new();

    if let Some(key) = found.get("interface.privatekey") {
        settings.insert("private_key".to_string(), key.clone());
    }
    if let Some(address) = found.get("interface.address") {
        // Addresses come as CIDR and may list several; the vendor script wants
        // one bare address and applies its own prefix.
        let first = address.split(',').next().unwrap_or("").trim();
        let bare = first.split('/').next().unwrap_or("").trim();
        if !bare.is_empty() {
            settings.insert("tunnel_ip".to_string(), bare.to_string());
        }
    }
    settings.insert(
        "listen_port".to_string(),
        found
            .get("interface.listenport")
            .cloned()
            // Clients usually leave this out and take an ephemeral port. The
            // vendor script needs a number, so pick the WireGuard default.
            .unwrap_or_else(|| "51820".to_string()),
    );

    if let Some(key) = found.get("peer.publickey") {
        settings.insert("peer_public_key".to_string(), key.clone());
    }
    if let Some(endpoint) = found.get("peer.endpoint") {
        let (host, port) = endpoint
            .rsplit_once(':')
            .ok_or("Endpoint must be host:port")?;
        settings.insert("peer_host".to_string(), host.trim().to_string());
        settings.insert("peer_listen_port".to_string(), port.trim().to_string());
    }

    // AllowedIPs decides what is routed in. Take the widest v4 entry, since a
    // full-tunnel config lists 0.0.0.0/0 alongside a v6 route this cannot use.
    let (mut network, mut prefix) = ("0.0.0.0".to_string(), 32u8);
    let mut saw_v4 = false;
    for entry in found
        .get("peer.allowedips")
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
    {
        let entry = entry.trim();
        if entry.is_empty() || entry.contains(':') {
            continue;
        }
        let (addr, bits) = entry.split_once('/').unwrap_or((entry, "32"));
        let Ok(bits) = bits.trim().parse::<u8>() else {
            continue;
        };
        if !saw_v4 || bits < prefix {
            network = addr.trim().to_string();
            prefix = bits.min(32);
            saw_v4 = true;
        }
    }
    settings.insert("peer_remote_ip".to_string(), network);
    settings.insert("peer_remote_mask".to_string(), prefix.to_string());

    for required in ["private_key", "peer_public_key", "peer_host", "tunnel_ip"] {
        if !settings.contains_key(required) {
            return Err(format!(
                "the config is missing {}",
                match required {
                    "private_key" => "PrivateKey",
                    "peer_public_key" => "the peer's PublicKey",
                    "peer_host" => "Endpoint",
                    _ => "Address",
                }
            ));
        }
    }
    Ok(settings)
}

/// Hide the private key, keeping enough to tell two profiles apart.
fn present(profile: &Value) -> Value {
    let mut copy = profile.clone();
    if let Some(settings) = copy["settings"].as_object_mut() {
        for field in tunnel::SECRET_FIELDS {
            if let Some(value) = settings.get(*field).and_then(Value::as_str) {
                settings[*field] = Value::String(tunnel::mask_secret(value));
            }
        }
    }
    copy
}

/// Which profile is currently written into the vendor's section.
///
/// Matched on the peer's public key rather than a stored id, so a tunnel
/// configured before profiles existed — or by hand over SSH — is still
/// recognised instead of showing every profile as inactive.
fn active_peer_key() -> Option<String> {
    ubus::uci_get(&format!("{}.peer_public_key", tunnel::UCI_SECTION))
        .ok()
        .filter(|key| !key.is_empty())
}

/// GET /api/tunnel/wireguard/profiles
pub fn list() -> (u16, Value) {
    let active = active_peer_key();
    let profiles: Vec<Value> = load()
        .iter()
        .map(|profile| {
            let mut shown = present(profile);
            let is_active = active.as_deref().is_some_and(|key| {
                profile["settings"]["peer_public_key"].as_str() == Some(key)
            });
            shown["active"] = Value::Bool(is_active);
            shown
        })
        .collect();
    (200, json!({"ok": true, "data": {"profiles": profiles, "masked": true}}))
}

/// POST /api/tunnel/wireguard/profiles
///
/// Body is either `{"name": "...", "conf": "<file contents>"}` or the same
/// fields the settings endpoint takes.
pub fn create(body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };

    let settings = if let Some(conf) = parsed["conf"].as_str() {
        match parse_conf(conf) {
            Ok(s) => s,
            Err(e) => return (400, json!({"ok": false, "error": e})),
        }
    } else {
        let Some(fields) = parsed["settings"].as_object() else {
            return (400, json!({"ok": false, "error": "provide 'conf' or 'settings'"}));
        };
        fields
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect()
    };

    // Validated now rather than at activation, so a bad config is rejected
    // while the user is still looking at where it came from.
    for (key, value) in &settings {
        if key == "peer_host" {
            continue;
        }
        if let Err(e) = tunnel::check_settable(key).and_then(|()| tunnel::validate_field(key, value))
        {
            return (400, json!({"ok": false, "error": e}));
        }
    }

    let name = parsed["name"]
        .as_str()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("Untitled tunnel");
    if name.len() > 64 || name.chars().any(char::is_control) {
        return (400, json!({"ok": false, "error": "that name is not usable"}));
    }

    let mut profiles = load();
    if profiles.len() >= MAX_PROFILES {
        return (
            409,
            json!({"ok": false, "error": format!("no room for more than {MAX_PROFILES} profiles")}),
        );
    }

    // Ids are only unique within this store, and a peer key already identifies
    // a tunnel, so the next free number is enough and stays readable in a URL.
    let id = profiles
        .iter()
        .filter_map(|p| p["id"].as_u64())
        .max()
        .unwrap_or(0)
        + 1;

    let mut stored = Map::new();
    for (key, value) in settings {
        stored.insert(key, Value::String(value));
    }
    profiles.push(json!({"id": id, "name": name, "settings": stored}));

    if let Err(e) = save(&profiles) {
        return (503, json!({"ok": false, "error": e}));
    }
    (200, json!({"ok": true, "data": {"id": id, "name": name}}))
}

/// POST /api/tunnel/wireguard/profiles/delete — body `{"id": 1}`
pub fn delete(body: &[u8]) -> (u16, Value) {
    let Some(id) = parse_id(body) else {
        return (400, json!({"ok": false, "error": "missing 'id'"}));
    };
    let mut profiles = load();
    let before = profiles.len();
    profiles.retain(|p| p["id"].as_u64() != Some(id));
    if profiles.len() == before {
        return (404, json!({"ok": false, "error": "no profile with that id"}));
    }
    match save(&profiles) {
        Ok(()) => (200, json!({"ok": true, "data": {"deleted": id}})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// POST /api/tunnel/wireguard/profiles/activate — body `{"id": 1}`
///
/// Copies the profile into the vendor's UCI section. Does not connect: bringing
/// the tunnel up is a separate, visible action, and activating is also what you
/// do before editing one.
pub fn activate(body: &[u8]) -> (u16, Value) {
    let Some(id) = parse_id(body) else {
        return (400, json!({"ok": false, "error": "missing 'id'"}));
    };
    let profiles = load();
    let Some(profile) = profiles.iter().find(|p| p["id"].as_u64() == Some(id)) else {
        return (404, json!({"ok": false, "error": "no profile with that id"}));
    };
    let Some(settings) = profile["settings"].as_object() else {
        return (500, json!({"ok": false, "error": "that profile is malformed"}));
    };

    if let Err(e) = tunnel::ensure_section() {
        return (503, json!({"ok": false, "error": e}));
    }

    // Resolved at activation, not at import. Providers move endpoints, and the
    // vendor script writes peer_endip straight into the config with no resolver
    // of its own — so a stored IP silently rots while a stored hostname does
    // not. Failing here says so plainly instead of producing a tunnel that
    // never handshakes.
    let host = settings.get("peer_host").and_then(Value::as_str);
    let port = settings
        .get("peer_listen_port")
        .and_then(Value::as_str)
        .unwrap_or("51820");
    let endpoint_ip = match host {
        Some(host) => match resolve(host, port) {
            Ok(ip) => Some(ip),
            Err(e) => return (503, json!({"ok": false, "error": e})),
        },
        None => None,
    };

    let mut written = Vec::new();
    for (key, value) in settings {
        if key == "peer_host" {
            continue;
        }
        let Some(text) = value.as_str() else { continue };
        if let Err(e) = ubus::uci_set(&format!("{}.{key}", tunnel::UCI_SECTION), text) {
            return (503, json!({"ok": false, "error": e}));
        }
        written.push(key.clone());
    }
    if let Some(ip) = &endpoint_ip {
        if let Err(e) = ubus::uci_set(&format!("{}.peer_endip", tunnel::UCI_SECTION), ip) {
            return (503, json!({"ok": false, "error": e}));
        }
        written.push("peer_endip".into());
    }

    (
        200,
        json!({"ok": true, "data": {
            "activated": id,
            "name": profile["name"],
            "endpoint": endpoint_ip,
            "written": written,
        }}),
    )
}

/// Turn a hostname into an address the vendor script can use.
fn resolve(host: &str, port: &str) -> Result<String, String> {
    // Already an address: hand it straight back rather than asking a resolver.
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        return Ok(host.to_string());
    }
    let port: u16 = port.parse().unwrap_or(51820);
    (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("could not look up {host}: {e}"))?
        .find_map(|addr| match addr.ip() {
            std::net::IpAddr::V4(v4) => Some(v4.to_string()),
            // The vendor config has one endpoint field and the scripts assume
            // v4 throughout, so a v6-only answer is unusable here.
            std::net::IpAddr::V6(_) => None,
        })
        .ok_or_else(|| format!("{host} has no IPv4 address"))
}

fn parse_id(body: &[u8]) -> Option<u64> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["id"].as_u64())
}

/// Whether the profile store is reachable. Used for capability reporting.
pub fn available() -> bool {
    Path::new(STORE_DIR).exists() || Path::new("/data").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped exactly like a provider's downloaded .conf, with fabricated
    /// keys — base64 of readable text rather than real key material, since a
    /// genuine provider config lived here until it was caught on the way to a
    /// public remote.
    const REAL_CONF: &str = "\
[Interface]
Address = 192.168.6.190/32
DNS = 1.1.1.1,8.8.8.8
PrivateKey = T3BlblU2MCBmYWJyaWNhdGVkIHdnIHByaXZrZXkhISE=

[Peer]
PublicKey=T3BlblU2MCBmYWJyaWNhdGVkIHdnIHB1YmtleSEhISE=
AllowedIPs = 0.0.0.0/0, ::/0
Endpoint = vpn.example.com:1024
";

    #[test]
    fn parses_a_real_provider_config() {
        let parsed = parse_conf(REAL_CONF).unwrap();
        assert_eq!(parsed["tunnel_ip"], "192.168.6.190");
        assert_eq!(parsed["peer_host"], "vpn.example.com");
        assert_eq!(parsed["peer_listen_port"], "1024");
        assert_eq!(parsed["peer_public_key"], "T3BlblU2MCBmYWJyaWNhdGVkIHdnIHB1YmtleSEhISE=");
        // Full tunnel: the widest v4 route wins and the v6 one is dropped,
        // because the vendor config has nowhere to put it.
        assert_eq!(parsed["peer_remote_ip"], "0.0.0.0");
        assert_eq!(parsed["peer_remote_mask"], "0");
        // Absent from the file, so the WireGuard default rather than nothing.
        assert_eq!(parsed["listen_port"], "51820");
    }

    #[test]
    fn keeps_base64_padding_in_keys() {
        // Keys contain '=' themselves; splitting on every '=' truncates them.
        // Asserting the whole key survives says that better than a suffix does.
        let parsed = parse_conf(REAL_CONF).unwrap();
        assert_eq!(parsed["private_key"], "T3BlblU2MCBmYWJyaWNhdGVkIHdnIHByaXZrZXkhISE=");
        assert_eq!(parsed["private_key"].len(), 44);
    }

    #[test]
    fn takes_the_widest_route_when_several_are_listed() {
        let conf = REAL_CONF.replace("AllowedIPs = 0.0.0.0/0, ::/0", "AllowedIPs = 10.0.0.0/24, 10.0.0.5/32");
        let parsed = parse_conf(&conf).unwrap();
        assert_eq!(parsed["peer_remote_ip"], "10.0.0.0");
        assert_eq!(parsed["peer_remote_mask"], "24");
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let conf = format!("# a comment\n\n{REAL_CONF}\n# trailing\n");
        assert!(parse_conf(&conf).is_ok());
    }

    #[test]
    fn says_which_field_is_missing() {
        let without_peer = REAL_CONF.replace("PublicKey=T3BlblU2MCBmYWJyaWNhdGVkIHdnIHB1YmtleSEhISE=", "");
        let error = parse_conf(&without_peer).unwrap_err();
        assert!(error.contains("PublicKey"), "unhelpful error: {error}");

        let without_endpoint = REAL_CONF.replace("Endpoint = vpn.example.com:1024", "");
        assert!(parse_conf(&without_endpoint).unwrap_err().contains("Endpoint"));
    }

    #[test]
    fn refuses_something_that_is_not_a_config() {
        assert!(parse_conf("this is not a config at all").is_err());
        assert!(parse_conf(&"x".repeat(MAX_CONF_BYTES + 1)).is_err());
    }

    #[test]
    fn an_address_needs_no_resolver() {
        assert_eq!(resolve("65.20.85.14", "1024").unwrap(), "65.20.85.14");
    }
}
