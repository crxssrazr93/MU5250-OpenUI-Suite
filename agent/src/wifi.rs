use std::collections::HashMap;
use std::process::Command;

use serde_json::{json, Value};

use crate::handlers::AppState;
use crate::ubus;

const WIFI_ONOFF_KEY: &str = "wifi_onoff";
const WIFI6_SWITCH_KEY: &str = "wifi6_switch";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The whole `wireless` and `zte_mbb` configs, dumped once per request.
struct WifiConfig {
    wireless: HashMap<String, String>,
    mbb: HashMap<String, String>,
}

impl WifiConfig {
    fn load() -> Self {
        Self {
            wireless: ubus::uci_show("wireless"),
            mbb: ubus::uci_show("zte_mbb"),
        }
    }

    fn get(&self, key: &str) -> String {
        self.wireless.get(key).cloned().unwrap_or_default()
    }

    /// Newer firmware (CN B27+) keeps the global Wi-Fi switches in a separate
    /// `zte_mbb` UCI config instead of `wireless.zte_mbb`. Read both namespaces.
    fn feature(&self, key: &str) -> String {
        if let Some(v) = self.mbb.get(&format!("wifi.{key}")) {
            if !v.is_empty() {
                return v.clone();
            }
        }
        self.get(&format!("zte_mbb.{key}"))
    }
}

fn report_value(report: Option<&Value>, key: &str) -> String {
    report
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn iw_info(iface: &str) -> (String, String) {
    let output = Command::new("iw").args([iface, "info"]).output().ok();
    let out = output
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let channel = out
        .lines()
        .find_map(|l| {
            let l = l.trim();
            if l.starts_with("channel ") {
                l.split_whitespace().nth(1).map(|s| s.to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let bw = out
        .lines()
        .find_map(|l| {
            let l = l.trim();
            if let Some(pos) = l.find("width:") {
                let rest = l[pos + 6..].trim();
                let end = rest.find("MHz").map(|i| i + 3).unwrap_or(rest.len());
                Some(rest[..end].trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    (channel, bw)
}

/// Count associated stations. Calls `iw` directly and counts in Rust — the
/// old `sh -c "iw ... | grep -c Station"` spawned three processes per band.
fn station_count(iface: &str) -> u64 {
    let Ok(output) = Command::new("iw").args([iface, "station", "dump"]).output() else {
        return 0;
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| l.trim_start().starts_with("Station "))
        .count() as u64
}

fn sanitize_uci_value(v: &str) -> String {
    v.chars()
        .filter(|c| {
            !matches!(
                c,
                '\'' | '"' | ';' | '$' | '`' | '\\' | '|' | '<' | '>' | '&'
            )
        })
        .collect()
}

fn reload_wireless() -> Result<(), String> {
    ubus::call("zwrt_wlan", "reload", Some("{}")).map(|_| ())
}

fn sanitize_wifi_key_value(v: &str) -> String {
    // Keep Wi-Fi passphrases intact except for control chars that can corrupt UCI entries.
    v.chars()
        .filter(|c| !matches!(c, '\u{0000}' | '\n' | '\r'))
        .collect()
}

fn sanitize_wifi_input_value(key: &str, v: &str) -> String {
    match key {
        "key_2g" | "key_5g" | "guest_key" => sanitize_wifi_key_value(v),
        _ => sanitize_uci_value(v),
    }
}

// ---------------------------------------------------------------------------
// GET /api/wifi/status
// ---------------------------------------------------------------------------

pub fn wifi_status(_state: &AppState) -> (u16, Value) {
    let mut result = serde_json::Map::new();
    let report = ubus::call("zwrt_wlan", "report", Some("{}")).ok();
    let cfg = WifiConfig::load();

    // Global switches from wireless feature config (both UCI namespaces),
    // with report fallback when exposed there.
    let mut wifi_onoff = cfg.feature(WIFI_ONOFF_KEY);
    if wifi_onoff.is_empty() {
        wifi_onoff = report_value(report.as_ref(), WIFI_ONOFF_KEY);
    }
    let wifi_onoff_supported = !wifi_onoff.is_empty();
    if wifi_onoff_supported {
        result.insert("wifi_onoff".into(), json!(wifi_onoff));
    }
    result.insert("wifi_onoff_supported".into(), json!(wifi_onoff_supported));

    let mut wifi6_switch = cfg.feature(WIFI6_SWITCH_KEY);
    if wifi6_switch.is_empty() {
        wifi6_switch = report_value(report.as_ref(), WIFI6_SWITCH_KEY);
    }
    let wifi6_supported = !wifi6_switch.is_empty();
    if wifi6_supported {
        result.insert("wifi6_switch".into(), json!(wifi6_switch));
    }
    result.insert("wifi6_supported".into(), json!(wifi6_supported));

    // Radio config
    result.insert(
        "radio2_disabled".into(),
        json!(cfg.get("wifi0.disabled")),
    );
    result.insert(
        "radio5_disabled".into(),
        json!(cfg.get("wifi1.disabled")),
    );
    result.insert(
        "channel_2g".into(),
        json!(cfg.get("wifi0.channel")),
    );
    result.insert(
        "channel_5g".into(),
        json!(cfg.get("wifi1.channel")),
    );
    result.insert(
        "txpower_2g".into(),
        json!(cfg.get("wifi0.txpowerpercent")),
    );
    result.insert(
        "txpower_5g".into(),
        json!(cfg.get("wifi1.txpowerpercent")),
    );
    result.insert("htmode_2g".into(), json!(cfg.get("wifi0.htmode")));
    result.insert("htmode_5g".into(), json!(cfg.get("wifi1.htmode")));
    result.insert(
        "country_code".into(),
        json!(cfg.get("wifi0.country")),
    );

    // Interface config
    result.insert("ssid_2g".into(), json!(cfg.get("main_2g.ssid")));
    result.insert("ssid_5g".into(), json!(cfg.get("main_5g.ssid")));
    result.insert("key_2g".into(), json!(cfg.get("main_2g.key")));
    result.insert("key_5g".into(), json!(cfg.get("main_5g.key")));
    result.insert(
        "has_key_2g".into(),
        json!(!cfg.get("main_2g.key").is_empty()),
    );
    result.insert(
        "has_key_5g".into(),
        json!(!cfg.get("main_5g.key").is_empty()),
    );
    result.insert(
        "encryption_2g".into(),
        json!(cfg.get("main_2g.encryption")),
    );
    result.insert(
        "encryption_5g".into(),
        json!(cfg.get("main_5g.encryption")),
    );
    result.insert(
        "hidden_2g".into(),
        json!(cfg.get("main_2g.hidden")),
    );
    result.insert(
        "hidden_5g".into(),
        json!(cfg.get("main_5g.hidden")),
    );

    // Runtime info from iw
    let (ch2, bw2) = iw_info("wlan0");
    let (ch5, bw5) = iw_info("wlan2");
    result.insert("actual_channel_2g".into(), json!(ch2));
    result.insert("actual_bw_2g".into(), json!(bw2));
    result.insert("actual_channel_5g".into(), json!(ch5));
    result.insert("actual_bw_5g".into(), json!(bw5));

    // Client counts
    let c2g = station_count("wlan0");
    let c5g = station_count("wlan2");
    result.insert("clients_2g".into(), json!(c2g));
    result.insert("clients_5g".into(), json!(c5g));
    result.insert("clients_total".into(), json!(c2g + c5g));

    // Guest WiFi summary
    result.insert(
        "guest_disabled_2g".into(),
        json!(cfg.get("guest_2g.disabled")),
    );
    result.insert(
        "guest_disabled_5g".into(),
        json!(cfg.get("guest_5g.disabled")),
    );
    result.insert(
        "guest_ssid".into(),
        json!(cfg.get("guest_2g.ssid")),
    );

    (200, json!({"ok": true, "data": result}))
}

// ---------------------------------------------------------------------------
// PUT /api/wifi/settings
// ---------------------------------------------------------------------------

pub fn wifi_set(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let obj = match parsed.as_object() {
        Some(o) => o,
        None => return (400, json!({"ok": false, "error": "expected JSON object"})),
    };

    let uci_map: &[(&str, &str)] = &[
        ("ssid_2g", "wireless.main_2g.ssid"),
        ("ssid_5g", "wireless.main_5g.ssid"),
        ("key_2g", "wireless.main_2g.key"),
        ("key_5g", "wireless.main_5g.key"),
        ("encryption_2g", "wireless.main_2g.encryption"),
        ("encryption_5g", "wireless.main_5g.encryption"),
        ("hidden_2g", "wireless.main_2g.hidden"),
        ("hidden_5g", "wireless.main_5g.hidden"),
        ("channel_2g", "wireless.wifi0.channel"),
        ("channel_5g", "wireless.wifi1.channel"),
        ("txpower_2g", "wireless.wifi0.txpowerpercent"),
        ("txpower_5g", "wireless.wifi1.txpowerpercent"),
        ("htmode_2g", "wireless.wifi0.htmode"),
        ("htmode_5g", "wireless.wifi1.htmode"),
        ("radio2_disabled", "wireless.wifi0.disabled"),
        ("radio5_disabled", "wireless.wifi1.disabled"),
    ];
    let txpower_keys: &[&str] = &["txpower_2g", "txpower_5g"];

    // Global switches live in `wireless.zte_mbb.*` on older firmware and in
    // `zte_mbb.wifi.*` on newer (CN B27+). Write whichever namespaces exist.
    let mbb_map: &[(&str, &[&str])] = &[
        (
            WIFI_ONOFF_KEY,
            &["wireless.zte_mbb.wifi_onoff", "zte_mbb.wifi.wifi_onoff"],
        ),
        (
            WIFI6_SWITCH_KEY,
            &["wireless.zte_mbb.wifi6_switch", "zte_mbb.wifi.wifi6_switch"],
        ),
    ];

    let mut wireless_changed = false;
    let mut mbb_changed = false;
    let mut only_txpower = true;
    let mut txpower_2g_val: Option<u32> = None;
    let mut txpower_5g_val: Option<u32> = None;

    for (key, value) in obj {
        let val_str = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
            _ => continue,
        };
        let val_str = sanitize_wifi_input_value(key, &val_str);

        // Prevent overwriting with masked placeholder
        if (key == "key_2g" || key == "key_5g") && val_str == "••••••••" {
            continue;
        }

        if key == WIFI_ONOFF_KEY {
            let mut changed_any = false;
            if let Some(&(_, paths)) = mbb_map.iter().find(|&&(k, _)| k == WIFI_ONOFF_KEY) {
                for &path in paths {
                    let current = ubus::uci_get(path).unwrap_or_default();
                    if current.is_empty() {
                        continue; // namespace absent on this firmware
                    }
                    if current != val_str {
                        if let Err(e) = ubus::uci_set_no_commit(path, &val_str) {
                            return (500, json!({"ok": false, "error": e}));
                        }
                        changed_any = true;
                        if path.starts_with("zte_mbb.") {
                            mbb_changed = true;
                        }
                    }
                }
            }

            let current_user =
                ubus::uci_get("wireless.zte_mbb.wifi_onoff_by_user").unwrap_or_default();
            if !current_user.is_empty() && current_user != val_str {
                if let Err(e) =
                    ubus::uci_set_no_commit("wireless.zte_mbb.wifi_onoff_by_user", &val_str)
                {
                    return (500, json!({"ok": false, "error": e}));
                }
                changed_any = true;
            }

            if changed_any {
                wireless_changed = true;
                only_txpower = false;
            }
            continue;
        }

        if key == WIFI6_SWITCH_KEY {
            if let Some(&(_, paths)) = mbb_map.iter().find(|&&(k, _)| k == WIFI6_SWITCH_KEY) {
                for &path in paths {
                    let current = ubus::uci_get(path).unwrap_or_default();
                    if current.is_empty() {
                        continue; // namespace absent on this firmware
                    }
                    if current != val_str {
                        if let Err(e) = ubus::uci_set_no_commit(path, &val_str) {
                            return (500, json!({"ok": false, "error": e}));
                        }
                        wireless_changed = true;
                        only_txpower = false;
                        if path.starts_with("zte_mbb.") {
                            mbb_changed = true;
                        }
                    }
                }
            }
            continue;
        }

        // Check wireless UCI map
        if let Some(&(_, path)) = uci_map.iter().find(|&&(k, _)| k == key) {
            let current = ubus::uci_get(path).unwrap_or_default();
            if current != val_str {
                if let Err(e) = ubus::uci_set_no_commit(path, &val_str) {
                    return (500, json!({"ok": false, "error": e}));
                }
                wireless_changed = true;
                if !txpower_keys.contains(&key.as_str()) {
                    only_txpower = false;
                } else if key == "txpower_2g" {
                    txpower_2g_val = val_str.parse().ok();
                } else if key == "txpower_5g" {
                    txpower_5g_val = val_str.parse().ok();
                }
            }
            continue;
        }
    }

    // Commit batched changes
    if wireless_changed {
        if let Err(e) = ubus::uci_commit("wireless") {
            return (500, json!({"ok": false, "error": e}));
        }
    }
    if mbb_changed {
        if let Err(e) = ubus::uci_commit("zte_mbb") {
            return (500, json!({"ok": false, "error": e}));
        }
    }

    if !wireless_changed && !mbb_changed {
        return (
            200,
            json!({"ok": true, "data": {"status": "ok", "note": "no changes"}}),
        );
    }

    // Hot-apply txpower if that's the only change
    if only_txpower {
        if let Some(val) = txpower_2g_val {
            let _ = Command::new("iw")
                .args([
                    "dev",
                    "wlan0",
                    "set",
                    "txpower",
                    "limit",
                    &(val * 30).to_string(),
                ])
                .output();
        }
        if let Some(val) = txpower_5g_val {
            let _ = Command::new("iw")
                .args([
                    "dev",
                    "wlan2",
                    "set",
                    "txpower",
                    "limit",
                    &(val * 30).to_string(),
                ])
                .output();
        }
        return (
            200,
            json!({"ok": true, "data": {"status": "ok", "hot": true}}),
        );
    }

    // Full reload needed. Run synchronously so a reload failure surfaces to
    // the caller instead of leaving UCI committed but running config stale.
    if let Err(e) = reload_wireless() {
        return (
            500,
            json!({"ok": false, "error": format!("wireless reload failed: {e}")}),
        );
    }

    (200, json!({"ok": true, "data": {"status": "ok"}}))
}

#[cfg(test)]
mod tests {
    use super::sanitize_wifi_input_value;

    #[test]
    fn wifi_keys_keep_special_characters() {
        let input = r#"Pass$word'";\|<>&`!"#;
        assert_eq!(sanitize_wifi_input_value("key_5g", input), input);
        assert_eq!(sanitize_wifi_input_value("guest_key", input), input);
    }

    #[test]
    fn wifi_keys_strip_control_characters_only() {
        let input = "line1\nline2\rline3\u{0000}";
        assert_eq!(
            sanitize_wifi_input_value("key_2g", input),
            "line1line2line3"
        );
    }

    #[test]
    fn non_key_values_remain_sanitized() {
        let input = r#"wifi$';`"\name|<&"#;
        assert_eq!(sanitize_wifi_input_value("ssid_5g", input), "wifiname");
    }
}
