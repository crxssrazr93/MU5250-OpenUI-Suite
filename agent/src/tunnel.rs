//! WireGuard, driven through the firmware's own tunnel stack.
//!
//! The kernel here is built with `CONFIG_WIREGUARD=y` and the vendor daemon
//! `zte-topsw-tunnel` already implements the whole flow: it shells out to
//! `wg genkey` / `wg pubkey`, and drives `/sbin/wireguard_conf.sh` and
//! `/sbin/wireguard_client.sh`, which handle interface creation, addressing,
//! routing against the LAN device, and the status callback into
//! `zwrt_tunnel.config cb`. None of that is worth reimplementing.
//!
//! One file is missing on a stock unit: the `wg` userspace tool. Without it the
//! daemon's own keygen returns `FAILED`. `zharden.sh` installs it to
//! `/data/bin`.
//!
//! It cannot be placed on the daemon's `PATH` — the rootfs is genuinely
//! read-only, and extending the PATH would mean editing a vendor init script,
//! which is a boot-path change `docs/SAFETY.md` rules out. So this module runs
//! the vendor scripts itself with `PATH` extended, and writes settings through
//! UCI (`zwrt_tunnel.wireguard.*`) — which is also the only way to set the
//! private key, since the ubus `set` method has no field for it.

use std::process::Command;

use serde_json::{json, Value};

use crate::ubus;

/// Where `zharden.sh` puts the tools this needs.
const WG_BIN: &str = "/data/bin/wg";
const EXTRA_PATH: &str = "/data/bin";

const CLIENT_SCRIPT: &str = "/sbin/wireguard_client.sh";
/// Renders the UCI settings into `/etc/wireguard.conf`.
///
/// The client script does `wg setconf wg0 /etc/wireguard.conf` but never
/// generates it — on stock firmware that is the vendor UI's job. Without this
/// step `connect` reports success and brings up a wg0 with no key and no peer,
/// which is exactly what it looked like on hardware: an interface, a random
/// listen port, and no handshake ever.
const CONF_SCRIPT: &str = "/sbin/wireguard_conf.sh";
const UCI_CONFIG: &str = "zwrt_tunnel";
/// Section name on its own. `ubus::uci_show` strips the `<config>.` prefix, so
/// its keys read `wireguard.<option>` — not the full `zwrt_tunnel.wireguard.…`
/// path that `uci_set` and `uci_get` take.
const UCI_SECTION_NAME: &str = "wireguard";
pub const UCI_SECTION: &str = "zwrt_tunnel.wireguard";

/// Settings a client may write. Anything not here is not settable, which keeps
/// a caller from reaching arbitrary UCI keys through this route.
const WRITABLE: &[&str] = &[
    "listen_port",
    "tunnel_ip",
    "peer_public_key",
    "peer_endip",
    "peer_listen_port",
    "peer_tunnel_ip",
    "peer_remote_ip",
    "peer_remote_mask",
    "auto_start",
];

/// Fields that must never leave the device in the clear.
///
/// `zwrt_tunnel.config get` hands back stored credentials as encrypted blobs
/// where `list` returns them empty — encrypted or not, they are credential
/// material and this dashboard gets screenshotted. Masked like EID and ICCID,
/// with the same `?full=true` escape hatch.
pub const SECRET_FIELDS: &[&str] = &["private_key", "password", "tunnel_password", "preshared_key"];

/// Whether the userspace tool is present. Without it nothing here works.
pub fn available() -> bool {
    std::path::Path::new(WG_BIN).is_file()
}

fn unavailable() -> (u16, Value) {
    (
        501,
        json!({"ok": false, "error": format!(
            "{WG_BIN} is not installed — run scripts/zharden.sh to add wireguard-tools"
        )}),
    )
}

/// Mask a secret to first four and last four characters, like `euicc::mask`.
pub fn mask_secret(value: &str) -> String {
    let len = value.chars().count();
    if len <= 8 {
        return "*".repeat(len);
    }
    let head: String = value.chars().take(4).collect();
    let tail: String = value.chars().skip(len - 4).collect();
    format!("{head}{}{tail}", "*".repeat(len - 8))
}

/// Apply masking to every secret field in a settings map.
fn mask_settings(mut settings: serde_json::Map<String, Value>, full: bool) -> Value {
    if !full {
        for field in SECRET_FIELDS {
            if let Some(Value::String(secret)) = settings.get(*field) {
                if !secret.is_empty() {
                    let masked = mask_secret(secret);
                    settings.insert((*field).to_string(), Value::String(masked));
                }
            }
        }
    }
    Value::Object(settings)
}

/// Create the UCI section if it is not there yet.
///
/// A stock unit ships sections for pptp, l2tp, gre, ipsec and vxlan but not
/// wireguard, and UCI refuses to set an option on a section that does not
/// exist ("uci: Invalid argument"). The other tunnels are all of type
/// `client`, so this matches them.
pub fn ensure_section() -> Result<(), String> {
    if ubus::uci_show(UCI_CONFIG).contains_key(UCI_SECTION_NAME) {
        return Ok(());
    }
    ubus::uci_set(UCI_SECTION, "client")
}

/// Read the WireGuard section out of UCI.
fn read_settings() -> serde_json::Map<String, Value> {
    let prefix = format!("{UCI_SECTION_NAME}.");
    let mut out = serde_json::Map::new();
    for (key, value) in ubus::uci_show(UCI_CONFIG) {
        if let Some(field) = key.strip_prefix(&prefix) {
            // uci_show also yields the section type as a bare `wireguard`
            // entry; only `<section>.<option>` pairs are settings.
            if !field.is_empty() && !field.contains('.') {
                out.insert(field.to_string(), Value::String(value));
            }
        }
    }
    out
}

/// GET /api/tunnel/wireguard[?full=true]
pub fn wireguard_get(query: Option<&str>) -> (u16, Value) {
    let full = query
        .map(|q| {
            q.split('&')
                .filter_map(|pair| pair.split_once('='))
                .any(|(k, v)| k == "full" && matches!(v, "true" | "1" | "yes"))
        })
        .unwrap_or(false);

    let settings = read_settings();
    let configured = settings
        .get("private_key")
        .and_then(Value::as_str)
        .is_some_and(|k| !k.is_empty());

    let active = ubus::uci_get("zwrt_tunnel.cur_type.type")
        .map(|t| t == "wireguard")
        .unwrap_or(false);
    let status = ubus::uci_get("zwrt_tunnel.tunnel.connect_status").unwrap_or_default();

    (
        200,
        json!({"ok": true, "data": {
            "available": available(),
            "configured": configured,
            "is_active_tunnel": active,
            "connect_status": status,
            "settings": mask_settings(settings, full),
            "masked": !full,
        }}),
    )
}

/// POST /api/tunnel/wireguard/keygen
///
/// Generates a fresh keypair and stores the private half. Returns only the
/// public key — that is the half meant to be shared with a peer, and the
/// private half never needs to leave the device.
pub fn wireguard_keygen() -> (u16, Value) {
    if !available() {
        return unavailable();
    }

    if let Err(e) = ensure_section() {
        return (503, json!({"ok": false, "error": e}));
    }

    let private = match run_wg(&["genkey"], None) {
        Ok(key) => key,
        Err(e) => return (503, json!({"ok": false, "error": e})),
    };
    let public = match run_wg(&["pubkey"], Some(&private)) {
        Ok(key) => key,
        Err(e) => return (503, json!({"ok": false, "error": e})),
    };

    if let Err(e) = ubus::uci_set(&format!("{UCI_SECTION}.private_key"), &private) {
        return (503, json!({"ok": false, "error": e}));
    }

    (200, json!({"ok": true, "data": {"public_key": public}}))
}

/// POST /api/tunnel/wireguard — body is any subset of the writable settings.
pub fn wireguard_set(body: &[u8]) -> (u16, Value) {
    if !available() {
        return unavailable();
    }
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let Some(fields) = parsed.as_object() else {
        return (400, json!({"ok": false, "error": "body must be an object"}));
    };

    if let Err(e) = ensure_section() {
        return (503, json!({"ok": false, "error": e}));
    }

    let mut written = Vec::new();
    for (key, value) in fields {
        if let Err(e) = check_settable(key) {
            return (400, json!({"ok": false, "error": e}));
        }
        let text = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
            _ => {
                return (
                    400,
                    json!({"ok": false, "error": format!("'{key}' must be a string, number or bool")}),
                )
            }
        };
        if let Err(e) = validate_field(key, &text) {
            return (400, json!({"ok": false, "error": e}));
        }
        if let Err(e) = ubus::uci_set(&format!("{UCI_SECTION}.{key}"), &text) {
            return (503, json!({"ok": false, "error": e}));
        }
        written.push(key.clone());
    }

    (200, json!({"ok": true, "data": {"written": written}}))
}

/// Fields a client may write but never read back.
///
/// `private_key` was originally refused on the grounds that only keygen should
/// produce one. That made the feature unusable: every commercial WireGuard
/// provider hands you a config file with a key already in it, and there is no
/// way to give a provider a key you generated. Refusing the import did not stop
/// anyone planting a key either — it only pushed them to SSH in and set it by
/// hand, unvalidated.
///
/// So it is settable, and write-only: it is in `SECRET_FIELDS`, so a read never
/// returns it in the clear.
const WRITE_ONLY: &[&str] = &["private_key"];

/// Whether a client may write this field.
pub fn check_settable(key: &str) -> Result<(), String> {
    (WRITABLE.contains(&key) || WRITE_ONLY.contains(&key))
        .then_some(())
        .ok_or_else(|| format!("'{key}' is not a settable field"))
}

/// Reject values that would be nonsense, or that would escape into the shell.
///
/// These reach `uci set` and then a shell script, so anything with quoting or
/// command characters is refused outright rather than escaped.
pub fn validate_field(key: &str, value: &str) -> Result<(), String> {
    if value.len() > 128 {
        return Err(format!("'{key}' is too long"));
    }
    if value.chars().any(|c| {
        c.is_control() || matches!(c, '\'' | '"' | '`' | '$' | ';' | '&' | '|' | '<' | '>' | '\\')
    }) {
        return Err(format!("'{key}' contains characters that are not allowed"));
    }
    match key {
        "listen_port" | "peer_listen_port" => value
            .parse::<u16>()
            .map(|_| ())
            .map_err(|_| format!("'{key}' must be a port number")),
        // Both halves of a WireGuard keypair have the same shape.
        // The vendor script writes this straight after a slash in AllowedIPs,
        // so it must be a prefix length. Given a dotted mask, wg rejects the
        // whole config and the tunnel comes up with no peer at all.
        "peer_remote_mask" => {
            if value.is_empty() {
                return Ok(());
            }
            value
                .parse::<u8>()
                .ok()
                .filter(|bits| *bits <= 32)
                .map(|_| ())
                .ok_or_else(|| "'peer_remote_mask' must be a prefix length, 0-32".to_string())
        }
        "peer_public_key" | "private_key" => {
            // A WireGuard key is 32 bytes, base64 — 44 characters ending in '='.
            if value.is_empty() {
                return Ok(());
            }
            let valid = value.len() == 44
                && value.ends_with('=')
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='));
            valid
                .then_some(())
                .ok_or_else(|| format!("'{key}' is not a valid WireGuard key"))
        }
        _ => Ok(()),
    }
}

/// POST /api/tunnel/wireguard/connect and /disconnect.
pub fn wireguard_handle(action: &str) -> (u16, Value) {
    if !available() {
        return unavailable();
    }
    if !matches!(action, "connect" | "disconnect") {
        return (400, json!({"ok": false, "error": "action must be connect or disconnect"}));
    }

    if action == "connect" {
        let settings = read_settings();
        for required in ["private_key", "peer_public_key", "tunnel_ip", "peer_endip"] {
            let missing = settings
                .get(required)
                .and_then(Value::as_str)
                .is_none_or(str::is_empty);
            if missing {
                return (
                    400,
                    json!({"ok": false, "error": format!("'{required}' must be set before connecting")}),
                );
            }
        }
        // The vendor scripts read the active type from UCI, so point it at
        // WireGuard before asking them to bring anything up.
        if let Err(e) = ubus::uci_set("zwrt_tunnel.cur_type.type", "wireguard") {
            return (503, json!({"ok": false, "error": e}));
        }
    }

    match run_vendor_script(action) {
        Ok(output) => (
            200,
            json!({"ok": true, "data": {
                "action": action,
                "output": output.lines().take(20).collect::<Vec<_>>(),
                "connect_status": ubus::uci_get("zwrt_tunnel.tunnel.connect_status").unwrap_or_default(),
            }}),
        ),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// Run `wg`, optionally piping `stdin` in.
fn run_wg(args: &[&str], stdin_data: Option<&str>) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = Command::new(WG_BIN)
        .args(args)
        .stdin(if stdin_data.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run {WG_BIN}: {e}"))?;

    if let Some(data) = stdin_data {
        child
            .stdin
            .as_mut()
            .ok_or("no stdin")?
            .write_all(data.as_bytes())
            .map_err(|e| format!("writing to wg failed: {e}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("wg did not complete: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "wg {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run the vendor's own connect/disconnect script with `wg` made reachable.
///
/// The script resolves `wg` through `PATH`, and the daemon's PATH cannot
/// include `/data/bin` without editing a vendor init script. Supplying it here
/// keeps the change to this process.
fn run_vendor_script(action: &str) -> Result<String, String> {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/sbin:/usr/bin:/sbin:/bin".into());

    // Tear down any existing interface first. The vendor script opens with
    // `ifconfig | grep wg0` and `exit 1` if it finds one, so a second connect —
    // after a settings change, or after a failed attempt left wg0 behind —
    // always failed while reporting nothing about why. Ignored on the way in
    // because "there was nothing to remove" is the normal case.
    if action == "connect" {
        let _ = Command::new("sh")
            .arg(CLIENT_SCRIPT)
            .arg("disconnect")
            .env("PATH", format!("{EXTRA_PATH}:{path}"))
            .output();
    }

    // Regenerate the config first, so the interface is brought up from the
    // settings that were just written rather than whatever was there before.
    if action == "connect" {
        let generated = Command::new("sh")
            .arg(CONF_SCRIPT)
            .env("PATH", format!("{EXTRA_PATH}:{path}"))
            .output()
            .map_err(|e| format!("could not run {CONF_SCRIPT}: {e}"))?;
        if !generated.status.success() {
            return Err(format!(
                "{CONF_SCRIPT} failed: {}",
                String::from_utf8_lossy(&generated.stderr).trim()
            ));
        }
    }

    let output = Command::new("sh")
        .arg(CLIENT_SCRIPT)
        .arg(action)
        .env("PATH", format!("{EXTRA_PATH}:{path}"))
        .output()
        .map_err(|e| format!("could not run {CLIENT_SCRIPT}: {e}"))?;

    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.status.success() {
        return Err(format!(
            "{CLIENT_SCRIPT} {action} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_secrets_to_head_and_tail() {
        assert_eq!(
            mask_secret("QK1JZ3fXvxLDgHkKGZ2LQaPYbCzXwqxE0IuWkDmZLnc="),
            "QK1J************************************Lnc="
        );
        // Short enough that a prefix and suffix would reveal most of it.
        assert_eq!(mask_secret("abcd1234"), "********");
        assert_eq!(mask_secret(""), "");
    }

    #[test]
    fn masks_every_secret_field_but_leaves_the_rest() {
        let mut settings = serde_json::Map::new();
        settings.insert("private_key".into(), json!("QK1JZ3fXvxLDgHkKGZ2LQaPYbCzXwqxE0IuWkDmZLnc="));
        settings.insert("peer_public_key".into(), json!("T3BlblU2MCBmYWJyaWNhdGVkIHdnIHByaXZrZXkhISE="));
        settings.insert("tunnel_ip".into(), json!("10.0.0.2"));

        let masked = mask_settings(settings.clone(), false);
        assert!(masked["private_key"].as_str().unwrap().contains('*'));
        // The peer's *public* key is not a secret — it is meant to be shared,
        // and hiding it makes the config impossible to check by eye.
        assert_eq!(masked["peer_public_key"], settings["peer_public_key"]);
        assert_eq!(masked["tunnel_ip"], "10.0.0.2");

        let full = mask_settings(settings.clone(), true);
        assert_eq!(full["private_key"], settings["private_key"]);
    }

    #[test]
    fn leaves_empty_secrets_alone() {
        let mut settings = serde_json::Map::new();
        settings.insert("private_key".into(), json!(""));
        // Masking "" to "" is right, but it must not become "****" and imply
        // a key exists when none does.
        assert_eq!(mask_settings(settings, false)["private_key"], "");
    }

    #[test]
    fn refuses_fields_that_are_not_settable() {
        // Importable, because every provider ships a config with a key already
        // in it. Write-only: it is masked on the way back out.
        assert!(check_settable("private_key").is_ok());
        assert!(SECRET_FIELDS.contains(&"private_key"));
        // Vendor state, not settings — writing these would desynchronise the
        // agent's view from the tunnel's.
        assert!(check_settable("connect_status").is_err());
        assert!(check_settable("../../etc/passwd").is_err());
        assert!(check_settable("tunnel_ip").is_ok());
        assert!(check_settable("peer_public_key").is_ok());
    }

    #[test]
    fn requires_a_prefix_length_not_a_dotted_mask() {
        assert!(validate_field("peer_remote_mask", "0").is_ok());
        assert!(validate_field("peer_remote_mask", "24").is_ok());
        // What a config file's AllowedIPs would tempt you to paste, and what
        // silently produced a peerless tunnel on hardware.
        assert!(validate_field("peer_remote_mask", "0.0.0.0").is_err());
        assert!(validate_field("peer_remote_mask", "33").is_err());
    }

    #[test]
    fn validates_an_imported_private_key_like_a_public_one() {
        assert!(validate_field("private_key", "T3BlblU2MCBmYWJyaWNhdGVkIHdnIHByaXZrZXkhISE=").is_ok());
        assert!(validate_field("private_key", "too-short").is_err());
    }

    #[test]
    fn refuses_shell_metacharacters() {
        // These values reach `uci set` and then a shell script.
        for hostile in ["10.0.0.1; reboot", "a`id`b", "x$(id)", "a'b", "a\"b", "a|b"] {
            assert!(validate_field("tunnel_ip", hostile).is_err(), "{hostile} was allowed");
        }
        assert!(validate_field("tunnel_ip", "10.0.0.2").is_ok());
    }

    #[test]
    fn validates_ports_and_keys() {
        assert!(validate_field("listen_port", "51820").is_ok());
        assert!(validate_field("listen_port", "70000").is_err());
        assert!(validate_field("listen_port", "abc").is_err());

        assert!(validate_field("peer_public_key", "T3BlblU2MCBmYWJyaWNhdGVkIHdnIHByaXZrZXkhISE=").is_ok());
        assert!(validate_field("peer_public_key", "too-short").is_err());
        // Empty is allowed: it means "not configured yet".
        assert!(validate_field("peer_public_key", "").is_ok());
    }

    #[test]
    fn rejects_an_unknown_action() {
        // Guards against a caller reaching some other verb in the vendor script.
        let (status, _) = wireguard_handle("destroy");
        assert!(status == 400 || status == 501);
    }
}
