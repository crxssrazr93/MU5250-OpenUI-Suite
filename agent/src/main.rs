mod at_cmd;
mod auth;
mod cache;
mod cell;
mod charge_policy;
mod compat;
mod connection_logger;
mod csv_utils;
mod device_ext;
mod euicc;
mod event_bus;
mod dns;
mod firewall;
mod handlers;
mod network_ext;
mod qmi;
mod router;
mod scheduler;
mod server;
mod signal_logger;
mod sim;
mod simlock;
mod sms;
mod system;
mod operator;
mod tunnel;
mod wgprofiles;
mod ubus;
mod usb;
mod util;
mod validate;
mod wifi;

use std::sync::Arc;

use event_bus::EventBus;
use handlers::AppState;

const DEFAULT_BIND: &str = "192.168.0.1:9090";
const DEFAULT_THREADS: usize = 4;
const STARTUP_SCRIPT: &str = "/data/local/tmp/start_zte_agent.sh";

fn main() {
    // Read-only eUICC probe. Runs the QMI/QRTR path once and exits without
    // starting the server or touching any boot state, so the transport can be
    // verified on the device before the agent is ever deployed.
    if std::env::args().nth(1).as_deref() == Some("euicc-probe") {
        std::process::exit(euicc_probe());
    }

    // Power-cycle the card over QMI UIM (POWER_OFF_SIM then POWER_ON_SIM).
    //
    // Development probe only. It tests whether a modem-driven card re-init makes
    // a freshly enabled eSIM profile visible without a full router reboot — the
    // REFRESH substitute this modem's missing CAT path denies. Not wired into
    // the enable flow; run by hand to measure the outcome.
    //   zte-agent uim-power-cycle
    if std::env::args().nth(1).as_deref() == Some("uim-power-cycle") {
        std::process::exit(uim_power_cycle());
    }

    // Run lpac with this agent as its APDU/HTTP backend and print the raw
    // result. Development aid for validating the bridge on-device.
    //   zte-agent lpac chip info
    if std::env::args().nth(1).as_deref() == Some("lpac") {
        let args: Vec<String> = std::env::args().skip(2).collect();
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        std::process::exit(match euicc::lpac::run(&refs) {
            Ok(result) => {
                for line in &result.progress {
                    eprintln!("[progress] {line}");
                }
                println!("{}", serde_json::to_string_pretty(&result.payload).unwrap_or_default());
                0
            }
            Err(e) => {
                eprintln!("lpac failed: {e}");
                1
            }
        });
    }

    let bind = std::env::var("ZTE_AGENT_BIND").unwrap_or_else(|_| DEFAULT_BIND.to_string());
    let threads: usize = std::env::var("ZTE_AGENT_THREADS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_THREADS);

    migrate_drop_removed_features();

    let state = Arc::new(AppState::new());

    // Set password from environment if provided
    if let Ok(pw) = std::env::var("ZTE_AGENT_PASSWORD") {
        state.auth.set_password(&pw);
    } else if let Some(pw) = read_startup_export("ZTE_AGENT_PASSWORD") {
        // Fallback: try reading from the startup script if executed manually
        state.auth.set_password(&pw);
    }

    let pin = std::env::var("ZTE_AGENT_PIN")
        .ok()
        .or_else(|| read_startup_export("ZTE_AGENT_PIN"));
    if let Some(pin) = pin {
        if let Err(e) = state.auth.set_pin(&pin) {
            eprintln!("[WARN] ignoring invalid ZTE_AGENT_PIN: {e}");
        }
    }

    // Event bus: single `ubus listen` process dispatches to subscribers
    let event_bus = EventBus::new();
    let charger_rx = event_bus.subscribe("BSP_CHARGER_EVENT");
    event_bus.start();

    state.charge_limit.start(charger_rx);
    state.scheduler.start();

    // Apply persisted TTL settings if they exist
    let _ = std::process::Command::new("sh")
        .arg("/data/local/tmp/start_ttl.sh")
        .output();

    usb::enforce_usb_mode_on_boot();

    server::start(&bind, threads, state);
}

/// Run the read-only eUICC probe and print a redacted report.
///
/// Identifiers are masked here as well as in the API: probe output is the thing
/// most likely to end up pasted into an issue.
fn euicc_probe() -> i32 {
    println!("== eUICC read-only probe ==");

    match euicc::status() {
        Ok(status) => {
            println!("card present   : {}", status.card_present);
            println!("eUICC (ISD-R)  : {}", status.isdr_available);
            println!("detail         : {}", status.detail);
            if !status.isdr_available {
                return 1;
            }
        }
        Err(e) => {
            eprintln!("status failed  : {e}");
            return 1;
        }
    }

    match euicc::eid() {
        Ok(value) => println!("EID            : {}", euicc::mask(&value)),
        Err(e) => eprintln!("EID failed     : {e}"),
    }

    match euicc::profiles() {
        Ok(profiles) => {
            println!("profiles       : {}", profiles.len());
            for (i, p) in profiles.iter().enumerate() {
                println!(
                    "  [{i}] {} / {} / {} / {}",
                    p.iccid.as_deref().map(euicc::mask).unwrap_or_default(),
                    p.state_label(),
                    p.class_label(),
                    p.service_provider.as_deref().unwrap_or("-"),
                );
            }
        }
        Err(e) => eprintln!("profiles failed: {e}"),
    }
    0
}

/// Power-cycle the card over QMI UIM and report card status either side.
///
/// This exercises the one primitive the enable flow never calls. If a modem
/// card re-init picks up a profile the eUICC already switched locally, this is
/// a reboot-free path where ES10 REFRESH fails. If the card comes back on the
/// same profile, the reboot really is load-bearing. Read the modem's ICCID
/// before and after (via `/api/sim/info`) to tell which happened.
fn uim_power_cycle() -> i32 {
    use qmi::uim::UimClient;

    let mut client = match UimClient::connect() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("uim connect    : {e}");
            return 1;
        }
    };

    match client.power_off_card() {
        Ok(()) => println!("power_off_card : ok"),
        Err(e) => {
            // Do not leave the card down: try to bring it back before giving up.
            eprintln!("power_off_card : {e}");
            let _ = client.power_on_card();
            return 1;
        }
    }

    // Give the card a moment down before re-powering, the way a slot reset would.
    std::thread::sleep(std::time::Duration::from_millis(1500));

    match client.power_on_card() {
        Ok(()) => println!("power_on_card  : ok"),
        Err(e) => {
            eprintln!("power_on_card  : {e}");
            return 1;
        }
    }

    // Card init runs asynchronously after POWER_ON; wait before it is read back.
    std::thread::sleep(std::time::Duration::from_secs(4));
    println!("done           : card re-init requested; re-read /api/sim/info");
    0
}

/// Read `export KEY='value'` out of the startup script.
fn read_startup_export(key: &str) -> Option<String> {
    let script = std::fs::read_to_string(STARTUP_SCRIPT).ok()?;
    let prefix = format!("export {key}=");
    script.lines().find_map(|line| {
        line.strip_prefix(&prefix)
            .map(|v| v.trim_matches(|c| c == '\'' || c == '"').to_string())
    })
}

/// One-shot cleanup for features removed from the agent (DoH proxy, SMS
/// forwarder, job scheduler).
///
/// The DoH part matters: enabling DoH pointed dnsmasq at the agent's own
/// resolver on 127.0.0.1:5353, and the module that undid that is gone. Without
/// this, a device that had DoH enabled would come back up forwarding DNS to a
/// port nothing listens on. Safe to run when DoH was never enabled — the config
/// file only exists if it was configured at least once.
fn migrate_drop_removed_features() {
    const DOH_CONFIG: &str = "/data/local/tmp/doh_config.json";

    if std::path::Path::new(DOH_CONFIG).exists() {
        eprintln!("[migrate] DoH was configured on this device — restoring dnsmasq defaults");
        let _ = std::process::Command::new("sh")
            .args([
                "-c",
                "rm -f /tmp/dnsmasq.d/doh.conf; \
                 uci delete dhcp.lan_dns.server 2>/dev/null; \
                 uci delete dhcp.lan_dns.noresolv 2>/dev/null; \
                 uci commit dhcp; \
                 /etc/init.d/dnsmasq restart",
            ])
            .output();
        let _ = std::fs::remove_file(DOH_CONFIG);
    }

    for orphan in [
        "/data/local/tmp/sms_forward.json",
        "/data/local/tmp/sms_forward_state.json",
        "/data/local/tmp/scheduler.json",
    ] {
        let _ = std::fs::remove_file(orphan);
    }
}
