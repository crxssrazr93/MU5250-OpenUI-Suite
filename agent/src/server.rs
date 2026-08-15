use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::at_cmd;
use crate::cell;
use crate::connection_logger;
use crate::device_ext;
use crate::euicc;
use crate::handlers::{self, AppState};
use crate::network_ext;
use crate::router;
use crate::compat;
use crate::dns;
use crate::scheduler;
use crate::firewall;
use crate::simlock;
use crate::operator;
use crate::tunnel;
use crate::wgprofiles;
use crate::signal_logger;
use crate::sim;
use crate::sms;
use crate::usb;
use crate::wifi;

/// How long a worker blocks before re-checking whether the listener died.
/// Also bounds how long a rebuild waits for the other workers to drain.
const WORKER_POLL: Duration = Duration::from_secs(30);
const RETRY_MIN: Duration = Duration::from_secs(1);
const RETRY_MAX: Duration = Duration::from_secs(30);

pub fn start(bind: &str, threads: usize, state: Arc<AppState>) {
    // Seed the CPU tracker with a baseline (speed tracker self-seeds)
    state.cpu.seed();

    // tiny_http's accept thread exits permanently on its first accept() error
    // — plausible here via EMFILE, ENOBUFS under memory pressure, or interface
    // churn. That used to leave every worker blocked forever on a listener that
    // would never yield another request, with the process still alive and
    // nothing to restart it. Supervise instead: a worker that sees the failure
    // flags it, all workers drain, and we rebuild the listener.
    let mut retry = RETRY_MIN;
    loop {
        let server = match Server::http(bind) {
            Ok(s) => {
                retry = RETRY_MIN;
                Arc::new(s)
            }
            Err(e) => {
                eprintln!("[server] bind {bind} failed: {e}; retrying in {}s", retry.as_secs());
                std::thread::sleep(retry);
                retry = (retry * 2).min(RETRY_MAX);
                continue;
            }
        };

        let dead = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();

        for _ in 0..threads {
            let server = Arc::clone(&server);
            let state = Arc::clone(&state);
            let dead = Arc::clone(&dead);
            handles.push(std::thread::spawn(move || loop {
                if dead.load(Ordering::Relaxed) {
                    return;
                }
                match server.recv_timeout(WORKER_POLL) {
                    Ok(Some(request)) => handle_request(request, &state),
                    // Idle timeout — loop round and re-check `dead`.
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!("[server] listener failed: {e}");
                        dead.store(true, Ordering::Relaxed);
                        return;
                    }
                }
            }));
        }

        for h in handles {
            let _ = h.join();
        }

        eprintln!("[server] listener down, rebuilding in {}s", retry.as_secs());
        std::thread::sleep(retry);
        retry = (retry * 2).min(RETRY_MAX);
    }
}

/// Routes that require an explicit `X-Confirm: true` header.
///
/// The eUICC writes belong here for the same reason reboot does: enabling or
/// disabling a profile drops the connection, and a delete cannot be undone
/// without a fresh activation code from the carrier, which is usually
/// single-use.
const DESTRUCTIVE_PATHS: &[&str] = &[
    "/api/device/reboot",
    "/api/device/shutdown",
    "/api/euicc/download",
    "/api/euicc/enable",
    "/api/euicc/disable",
    "/api/euicc/delete",
    "/api/doh",
    "/api/scheduler/jobs",
    "/api/scheduler/jobs/delete",
    "/api/device/schedule-reboot",
    "/api/firewall/config",
    "/api/firewall/port-forward",
    "/api/firewall/domain-filter/rule",
    "/api/modem/airplane",
    "/api/modem/bands/lock",
    "/api/modem/bands/lte/lock",
    "/api/modem/bands/nr/lock",
    "/api/modem/cell-lock",
    "/api/modem/scan",
    "/api/modem/register",
    "/api/sim/unlock",
    "/api/modem/online",
    "/api/sim/pin/verify",
    "/api/sim/puk/verify",
    "/api/sim/pin/change",
    "/api/sim/pin/toggle",
    "/api/operator/scan",
    "/api/operator/scan/start",
    "/api/operator/select",
    "/api/euicc/notifications/remove",
    // Bringing a tunnel up reroutes traffic and can drop the caller's own
    // connection; keygen overwrites the private key, which cannot be recovered.
    "/api/tunnel/wireguard/connect",
    "/api/tunnel/wireguard/disconnect",
    "/api/tunnel/wireguard/keygen",
    "/api/tunnel/wireguard/profiles/delete",
    "/api/tunnel/wireguard/profiles/activate",
];

fn cors_headers(origin: Option<&str>) -> Vec<Header> {
    let allowed = origin
        .and_then(|o| is_lan_origin(o).then_some(o))
        .unwrap_or("");
    vec![
        Header::from_bytes("Access-Control-Allow-Origin", allowed).unwrap(),
        Header::from_bytes(
            "Access-Control-Allow-Methods",
            "GET, POST, PUT, DELETE, OPTIONS",
        )
        .unwrap(),
        Header::from_bytes(
            "Access-Control-Allow-Headers",
            "Authorization, Content-Type, X-Confirm",
        )
        .unwrap(),
        Header::from_bytes("Access-Control-Max-Age", "86400").unwrap(),
    ]
}

fn is_lan_origin(origin: &str) -> bool {
    if !origin.starts_with("http://") {
        return false;
    }
    let host = &origin[7..];
    let host = host.split(':').next().unwrap_or(host);
    if host == "localhost" || host == "127.0.0.1" || host == "::1" {
        return true;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let octets: Vec<u8> = parts.iter().filter_map(|p| p.parse().ok()).collect();
    if octets.len() != 4 {
        return false;
    }
    octets[0] == 10
        || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
        || (octets[0] == 192 && octets[1] == 168)
}

fn handle_request(mut request: Request, state: &AppState) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or(&url).to_string();
    let origin = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Origin"))
        .map(|h| h.value.as_str().to_string());
    let origin_ref = origin.as_deref();
    let client_ip = request
        .remote_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_default();
    let user_agent = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("User-Agent"))
        .map(|h| h.value.as_str().to_string());
    let user_agent_ref = user_agent.as_deref();

    if method == Method::Options {
        let mut response = Response::empty(200);
        for h in cors_headers(origin_ref) {
            response = response.with_header(h);
        }
        let _ = request.respond(response);
        return;
    }

    let needs_auth = path != "/api/auth/login";
    if needs_auth {
        let authorized = request
            .headers()
            .iter()
            .find(|h| h.field.as_str().to_ascii_lowercase() == "authorization")
            .and_then(|h| h.value.as_str().strip_prefix("Bearer "))
            .map(|token| state.auth.validate(token))
            .unwrap_or(false);

        if !state.auth.has_password() {
            respond(
                request,
                403,
                json!({"ok": false, "error": "no password configured. Set ZTE_AGENT_PASSWORD environment variable."}),
                origin_ref,
            );
            return;
        } else if !authorized {
            respond(
                request,
                401,
                json!({"ok": false, "error": "unauthorized"}),
                origin_ref,
            );
            return;
        }
    }

    // Gated on method as well as path. Several routes read on GET and act on
    // POST at the same path, and matching on path alone made the read demand a
    // confirmation header and fail with a 400 — twice, on operator scan and
    // again on the app's /api/modem/scan. A GET never changes anything, so it
    // is never what the confirmation is protecting.
    let mutating = *request.method() != Method::Get;
    if mutating && DESTRUCTIVE_PATHS.contains(&path.as_str()) {
        let confirmed = request
            .headers()
            .iter()
            .any(|h| h.field.equiv("X-Confirm") && h.value.as_str() == "true");
        if !confirmed {
            respond(
                request,
                400,
                json!({"ok": false, "error": "destructive action requires X-Confirm: true header"}),
                origin_ref,
            );
            return;
        }
    }

    let mut body = Vec::new();
    let mut reader = request.as_reader();
    let mut limited = std::io::Read::take(&mut reader, 1024 * 1024);
    if let Err(e) = std::io::Read::read_to_end(&mut limited, &mut body) {
        respond(
            request,
            400,
            json!({"ok": false, "error": format!("failed to read body: {e}")}),
            origin_ref,
        );
        return;
    }

    let query = url.split_once('?').map(|(_, q)| q.to_string());
    let (status, body_json) = route(
        &method,
        &path,
        state,
        &body,
        &client_ip,
        user_agent_ref,
        query.as_deref(),
    );
    respond(request, status, body_json, origin_ref);
}

pub fn route(
    method: &Method,
    path: &str,
    state: &AppState,
    body: &[u8],
    client_ip: &str,
    user_agent: Option<&str>,
    query: Option<&str>,
) -> (u16, Value) {
    match (method, path) {
        // Auth
        (&Method::Post, "/api/auth/login") => handlers::login(state, body, client_ip, user_agent),
        // Batch — the dashboard's heartbeat; feeds Home, Signal and Modem/Data
        (&Method::Get, "/api/dashboard") => handlers::dashboard(state),
        // Device / system
        (&Method::Get, "/api/device") => handlers::device(state),
        (&Method::Get, "/api/cpu") => handlers::cpu(state),
        (&Method::Get, "/api/memory") => handlers::memory(state),
        (&Method::Get, "/api/system/top") => handlers::system_top(state),
        (&Method::Post, "/api/system/kill-bloat") => handlers::system_kill_bloat(state, body),
        (&Method::Post, "/api/system/restart-agent") => device_ext::agent_restart(state),
        (&Method::Post, "/api/device/reboot") => device_ext::device_reboot(state),
        (&Method::Post, "/api/device/shutdown") => device_ext::device_shutdown(state),
        (&Method::Get, "/api/device/battery-info") => network_ext::network_battery_ubus(state),
        (&Method::Get, "/api/device/thermal/all") => device_ext::device_thermal_all(state),
        (&Method::Get, "/api/device/battery/detail") => device_ext::device_battery_detail(state),
        (&Method::Get, "/api/device/charger") => device_ext::device_charger(state),
        (&Method::Get, "/api/device/charge-control") => device_ext::charge_control_get(state),
        (&Method::Put, "/api/device/charge-control") => device_ext::charge_control_set(state, body),
        // Network
        (&Method::Get, "/api/network/clients") => network_ext::network_clients(state),

        // Firewall, port forwarding and domain filtering.
        (&Method::Get, "/api/scheduler/jobs") => scheduler::jobs_list(state),
        (&Method::Post, "/api/scheduler/jobs") => scheduler::jobs_set(state, body),
        (&Method::Post, "/api/scheduler/jobs/delete") => scheduler::jobs_delete(state, body),
        (&Method::Get, "/api/device/schedule-reboot") => scheduler::schedule_reboot(state, false, body),
        (&Method::Post, "/api/device/schedule-reboot") => scheduler::schedule_reboot(state, true, body),

        (&Method::Get, "/api/doh") => dns::doh_get(state),
        (&Method::Post, "/api/doh") => dns::doh_set(state, body),
        (&Method::Get, "/api/doh/status") => dns::doh_get(state),
        (&Method::Get, "/api/doh/cache") => dns::dns_cache_get(state),
        (&Method::Post, "/api/doh/cache") => dns::dns_cache_clear(state, body),

        (&Method::Get, "/api/firewall/config") => firewall::firewall_config_get(state),
        (&Method::Post, "/api/firewall/config") => firewall::firewall_config_set(state, body),
        (&Method::Get, "/api/firewall/port-forward") => firewall::port_forward_list(state),
        (&Method::Post, "/api/firewall/port-forward") => firewall::port_forward_set(state, body),
        (&Method::Get, "/api/firewall/domain-filter") => firewall::domain_filter_list(state),
        (&Method::Post, "/api/firewall/domain-filter/rule") => firewall::domain_filter_rule(state, body),

        // Paths the mobile apps use. See compat.rs and docs/MOBILE-API-GAP.md.
        (&Method::Get, "/api/network/signal") => compat::network_signal(state),
        (&Method::Get, "/api/network/wan") => compat::network_wan(state),
        (&Method::Get, "/api/network/wan6") => compat::network_wan6(state),
        (&Method::Get, "/api/network/rmnet") => compat::network_rmnet(state),
        (&Method::Get, "/api/network/dhcp-leases") => compat::network_dhcp_leases(state),
        (&Method::Get, "/api/network/lan") => compat::network_lan(state, false, body),
        (&Method::Post, "/api/network/lan") => compat::network_lan(state, true, body),
        (&Method::Get, "/api/network/dns") => compat::network_dns(state, false, body),
        (&Method::Post, "/api/network/dns") => compat::network_dns(state, true, body),
        (&Method::Get, "/api/modem/status") => compat::modem_status(state),
        (&Method::Get, "/api/modem/data") => compat::modem_data(state),
        (&Method::Put, "/api/modem/data") => compat::modem_data_set(state, body),
        (&Method::Get, "/api/modem/apn") => compat::modem_apn(state, false, body),
        (&Method::Post, "/api/modem/apn") => compat::modem_apn(state, true, body),
        (&Method::Post, "/api/modem/apn/profile") => compat::modem_apn_profile(state, body),
        (&Method::Post, "/api/modem/apn/activate") => compat::modem_apn_activate(state, body),
        (&Method::Get, "/api/modem/apn/mode") => compat::modem_apn_mode(state, false, body),
        (&Method::Post, "/api/modem/apn/mode") => compat::modem_apn_mode(state, true, body),
        (&Method::Post, "/api/modem/bands/lock") => compat::modem_bands_lock(state, body),
        (&Method::Post, "/api/modem/bands/lte/lock") => compat::modem_bands_lte(state, body),
        (&Method::Post, "/api/modem/bands/nr/lock") => compat::modem_bands_nr(state, body),
        (&Method::Get, "/api/modem/cell-lock") => compat::modem_cell_lock(state, false, body),
        (&Method::Post, "/api/modem/cell-lock") => compat::modem_cell_lock(state, true, body),
        (&Method::Get, "/api/modem/neighbors") => compat::modem_neighbors(state),
        (&Method::Get, "/api/modem/scan") => compat::modem_scan(state, false),
        (&Method::Post, "/api/modem/scan") => compat::modem_scan(state, true),
        (&Method::Post, "/api/modem/register") => compat::modem_register(state, body),
        (&Method::Get, "/api/network/speed") => compat::network_speed(state),
        (&Method::Get, "/api/network/speeds") => compat::network_speed(state),
        (&Method::Get, "/api/network/traffic") => compat::network_speed(state),
        (&Method::Get, "/api/sms/capacity") => compat::sms_capacity(state),
        (&Method::Post, "/api/sim/unlock") => compat::sim_unlock(state, body),
        (&Method::Get, "/api/modem/airplane") => compat::modem_airplane(state, false, body),
        (&Method::Post, "/api/modem/airplane") => compat::modem_airplane(state, true, body),
        (&Method::Post, "/api/modem/online") => compat::modem_online(state, body),
        (&Method::Get, "/api/battery") => compat::battery(state),
        (&Method::Get, "/api/device/thermal") => compat::device_thermal(state),
        (&Method::Get, "/api/device/system") => compat::device_system(state),
        (&Method::Get, "/api/device/imei") => compat::device_imei(state),
        (&Method::Get, "/api/device/usb") => compat::device_usb(state),
        (&Method::Post, "/api/device/usb/mode") => compat::device_usb_mode(state, body),
        (&Method::Post, "/api/device/powerbank") => compat::device_powerbank(state, body),
        // WiFi
        (&Method::Get, "/api/wifi/status") => wifi::wifi_status(state),
        (&Method::Put, "/api/wifi/settings") => wifi::wifi_set(state, body),
        // Modem
        (&Method::Put, "/api/data-usage/reset-day") => {
            handlers::data_usage_reset_day_set(state, body)
        }
        (&Method::Get, "/api/modem/stc/params") => cell::modem_stc_params(state),
        (&Method::Get, "/api/modem/stc/status") => cell::modem_stc_status(state),
        (&Method::Put, "/api/modem/stc") => cell::modem_stc_set(state, body),
        (&Method::Post, "/api/modem/stc/reset") => cell::modem_stc_reset(state),
        (&Method::Get, "/api/modem/network-mode") => cell::modem_network_mode(state),
        (&Method::Put, "/api/modem/network-mode") => cell::modem_network_mode_set(state, body),
        // SMS
        (&Method::Post, "/api/sms/list") => sms::sms_list(state, body),
        (&Method::Post, "/api/sms/send") => sms::sms_send(state, body),
        (&Method::Post, "/api/sms/delete") => sms::sms_delete(state, body),
        (&Method::Post, "/api/sms/read") => sms::sms_mark_read(state, body),
        // SIM
        (&Method::Get, "/api/sim/lock") => simlock::sim_lock_status(state),
        (&Method::Post, "/api/sim/pin/verify") => simlock::sim_pin_verify(state, body),
        (&Method::Post, "/api/sim/puk/verify") => simlock::sim_puk_verify(state, body),
        (&Method::Post, "/api/sim/pin/change") => simlock::sim_pin_change(state, body),
        (&Method::Post, "/api/sim/pin/toggle") => simlock::sim_pin_toggle(state, body),
        (&Method::Get, "/api/sim/info") => sim::sim_info(state),
        (&Method::Get, "/api/sim/imei") => sim::sim_imei(state),
        // eUICC / eSIM — read-only. See docs/EUICC.md for the safety boundary.
        (&Method::Get, "/api/euicc/status") => euicc::api::euicc_status(),
        (&Method::Get, "/api/euicc/eid") => euicc::api::euicc_eid(query),
        (&Method::Get, "/api/euicc/profiles") => euicc::api::euicc_profiles(query),
        (&Method::Get, "/api/euicc/chip") => euicc::api::euicc_chip(),
        (&Method::Get, "/api/euicc/notifications") => euicc::api::euicc_notifications(query),
        // eUICC writes — all gated on X-Confirm, see DESTRUCTIVE_PATHS
        (&Method::Post, "/api/euicc/download") => euicc::api::euicc_download(body),
        (&Method::Post, "/api/euicc/enable") => euicc::api::euicc_enable(body),
        (&Method::Post, "/api/euicc/disable") => euicc::api::euicc_disable(body),
        (&Method::Post, "/api/euicc/delete") => euicc::api::euicc_delete(body),
        (&Method::Post, "/api/euicc/nickname") => euicc::api::euicc_nickname(body),
        (&Method::Post, "/api/euicc/notifications/process") => {
            euicc::api::euicc_notifications_process(body)
        }
        (&Method::Post, "/api/euicc/notifications/remove") => {
            euicc::api::euicc_notifications_remove(body)
        }
        // Relay — a LAN client carries ES9+ traffic for a router with no WAN
        (&Method::Get, "/api/euicc/relay/pending") => euicc::api::relay_pending(query),
        (&Method::Post, "/api/euicc/relay/response") => euicc::api::relay_response(body),
        (&Method::Get, "/api/euicc/relay/status") => euicc::api::relay_status(),
        // Capability discovery — lets clients hide what this build does not serve
        (&Method::Get, "/api/capabilities") => capabilities(),
        // Operator scan
        (&Method::Get, "/api/operator/scan") => operator::operator_scan_status(),
        (&Method::Post, "/api/operator/scan") => operator::operator_scan_start(),
        (&Method::Post, "/api/operator/scan/start") => operator::operator_scan_start(),
        (&Method::Post, "/api/operator/select") => operator::operator_select(body),

        // Cell / band lock
        (&Method::Post, "/api/cell/lock/nr") => cell::cell_lock_nr(state, body),

        // WireGuard, through the firmware's own tunnel stack. See tunnel.rs for
        // why the agent drives the vendor scripts rather than reimplementing.
        (&Method::Get, "/api/tunnel/wireguard") => tunnel::wireguard_get(query),
        (&Method::Post, "/api/tunnel/wireguard") => tunnel::wireguard_set(body),
        (&Method::Get, "/api/tunnel/wireguard/profiles") => wgprofiles::list(),
        (&Method::Post, "/api/tunnel/wireguard/profiles") => wgprofiles::create(body),
        (&Method::Post, "/api/tunnel/wireguard/profiles/delete") => wgprofiles::delete(body),
        (&Method::Post, "/api/tunnel/wireguard/profiles/activate") => wgprofiles::activate(body),
        (&Method::Post, "/api/tunnel/wireguard/keygen") => tunnel::wireguard_keygen(),
        (&Method::Post, "/api/tunnel/wireguard/connect") => tunnel::wireguard_handle("connect"),
        (&Method::Post, "/api/tunnel/wireguard/disconnect") => tunnel::wireguard_handle("disconnect"),
        (&Method::Post, "/api/cell/lock/lte") => cell::cell_lock_lte(state, body),
        (&Method::Post, "/api/cell/lock/reset") => cell::cell_lock_reset(state),
        (&Method::Post, "/api/cell/band/nr") => cell::cell_band_nr(state, body),
        (&Method::Post, "/api/cell/band/lte") => cell::cell_band_lte(state, body),
        (&Method::Post, "/api/cell/band/reset") => cell::cell_band_reset(state),
        // Router
        (&Method::Get, "/api/router/dns") => router::router_dns_get(state),
        (&Method::Put, "/api/router/dns") => router::router_dns_set(state, body),
        (&Method::Get, "/api/router/lan") => router::router_lan_get(state),
        (&Method::Put, "/api/router/lan") => router::router_lan_set(state, body),
        (&Method::Get, "/api/router/apn/mode") => router::router_apn_mode_get(state),
        (&Method::Put, "/api/router/apn/mode") => router::router_apn_mode_set(state, body),
        (&Method::Get, "/api/router/apn/profiles") => router::router_apn_profiles_get(state),
        (&Method::Post, "/api/router/apn/profiles") => router::router_apn_profiles_add(state, body),
        (&Method::Post, "/api/router/apn/profiles/delete") => {
            router::router_apn_profiles_delete(state, body)
        }
        (&Method::Post, "/api/router/apn/profiles/activate") => {
            router::router_apn_profiles_activate(state, body)
        }
        // USB
        (&Method::Get, "/api/usb/status") => usb::usb_status(state),
        (&Method::Put, "/api/usb/mode") => usb::usb_mode_set(state, body),
        (&Method::Put, "/api/usb/default") => usb::usb_default_set(state, body),
        (&Method::Put, "/api/usb/powerbank") => usb::usb_powerbank_set(state, body),
        // TTL override
        (&Method::Get, "/api/ttl/status") => ttl_status(),
        (&Method::Put, "/api/ttl/set") => ttl_set(body),
        (&Method::Delete, "/api/ttl/clear") => ttl_clear(),
        // AT console
        (&Method::Post, "/api/at/send") => at_console(state, body),
        (&Method::Get, "/api/at/port") => at_port(state),
        // Signal logger
        (&Method::Post, "/api/logger/signal/start") => signal_logger::start_logging(state, body),
        (&Method::Post, "/api/logger/signal/stop") => signal_logger::stop_logging(state),
        (&Method::Get, "/api/logger/signal/status") => signal_logger::status(state),
        (&Method::Get, "/api/logger/signal/download") => signal_logger::download(state),
        // Connection logger
        (&Method::Post, "/api/logger/connection/start") => {
            connection_logger::start_logging(state, body)
        }
        (&Method::Post, "/api/logger/connection/stop") => connection_logger::stop_logging(state),
        (&Method::Get, "/api/logger/connection/status") => connection_logger::status(state),
        (&Method::Get, "/api/logger/connection/download") => connection_logger::download(state),
        // Fallback
        _ => (404, json!({"ok": false, "error": "not found"})),
    }
}

// --- Capability discovery ---

/// GET /api/capabilities
///
/// Clients (dashboard, Android app) are built against a superset of what any
/// one agent build serves: this fork deliberately dropped several upstream
/// features, and the eUICC surface is new. Without this endpoint a client can
/// only discover a missing feature by calling it and taking a 404, so features
/// removed for safety show up to the user as errors rather than as absent.
///
/// `supported` says what this *build* can do. Whether the *hardware* has an
/// eUICC fitted is a separate question answered by `/api/euicc/status`.
fn capabilities() -> (u16, Value) {
    (
        200,
        json!({"ok": true, "data": {
            "agent_version": env!("CARGO_PKG_VERSION"),
            "api_version": 1,
            "supported": {
                "dashboard_batch": true,
                "wifi": true,
                "sms": true,
                "sim": true,
                "cell_lock": true,
                "band_lock": true,
                "apn": true,
                "router_dns": true,
                "router_lan": true,
                "usb": true,
                "ttl": true,
                "charge_control": true,
                "signal_logger": true,
                "connection_logger": true,
                "at_console_readonly": true,
                "euicc_read": true,
                "euicc_write": euicc::lpac::available(),
                "euicc_http_relay": true,
            "wireguard": tunnel::available(),
            },
            // Present in the upstream client but not served here. Listed
            // explicitly so a client can hide them instead of failing.
            "unsupported": {
                "at_console_write": "removed: unrestricted AT access can brick the modem",
                "doh_proxy": "removed: could leave DNS pointing at a dead local port",
                "sms_forward": "removed: background service without maintained UI",
                "scheduler": "removed: background service without maintained UI",
                "speedtest": "removed",
                "telephony": "removed",
                "tailscale": "removed: outside the conservative deployment boundary",
            },
        }}),
    )
}

// --- AT console ---

fn at_console(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let command = match parsed["command"].as_str() {
        Some(c) if !c.is_empty() => c,
        _ => return (400, json!({"ok": false, "error": "missing 'command'"})),
    };
    let timeout = parsed["timeout"].as_u64().unwrap_or(2).min(30);

    if !is_at_command_allowed(command) {
        return (
            403,
            json!({"ok": false, "error": "command not allowed. Only read-only AT commands are permitted."}),
        );
    }

    match at_cmd::send(&state.at_port, command, timeout) {
        Ok(resp) => (200, json!({"ok": true, "data": {"response": resp.trim()}})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

const AT_BLOCKED_PREFIXES: &[&str] = &[
    "AT+CFUN",
    "AT^",
    "AT$QCRMCALL",
    "AT+CLCK",
    "AT+CMGD",
    "AT+CMGF=1;+CMGS",
    "AT+CGDCONT=",
    "AT+CGACT=",
];

const AT_ALLOWED_PREFIXES: &[&str] = &[
    "ATI",
    "AT+CSQ",
    "AT+COPS",
    "AT+CGDCONT?",
    "AT+CREG",
    "AT+CGREG",
    "AT+CEREG",
    "AT+CGPADDR",
    "AT+CGACT?",
    "AT+CLAC",
    "AT+CGSN",
    "AT+CGMI",
    "AT+CGMM",
    "AT+CGMR",
    "AT+QENG",
    "AT+QNWINFO",
    "AT+QRSRP",
    "AT+QRSRQ",
    "AT+QINISTAT",
    "AT+QSPN",
    "AT+QCIDINCOMING",
    "AT+CGDCONT?",
    "AT+CGCONTRDP",
    "AT+CGPADDR",
    "AT",
];

fn is_at_command_allowed(cmd: &str) -> bool {
    let upper = cmd.trim().to_uppercase();
    if upper.is_empty() {
        return false;
    }
    for prefix in AT_BLOCKED_PREFIXES {
        if upper.starts_with(prefix) {
            return false;
        }
    }
    for prefix in AT_ALLOWED_PREFIXES {
        if upper.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// GET /api/at/port — report the detected AT serial port (if any)
fn at_port(state: &AppState) -> (u16, Value) {
    match state.at_port.detect_serialized() {
        Some(port) => (
            200,
            json!({"ok": true, "data": {"port": port, "available": true}}),
        ),
        None => (
            200,
            json!({"ok": true, "data": {"port": null, "available": false}}),
        ),
    }
}

// --- TTL handlers ---

fn ttl_status() -> (u16, Value) {
    let ipv4 = std::process::Command::new("iptables")
        .args(["-t", "mangle", "-L", "PREROUTING", "-n"])
        .output();
    let ipv6 = std::process::Command::new("ip6tables")
        .args(["-t", "mangle", "-L", "PREROUTING", "-n"])
        .output();
    let mut active = false;
    let mut ttl_value: u32 = 0;
    if let Ok(out) = &ipv4 {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            if line.contains("TTL set to") {
                active = true;
                if let Some(v) = line.rsplit("TTL set to ").next() {
                    ttl_value = v.trim().parse().unwrap_or(0);
                }
                break;
            }
        }
    }
    let mut hl_active = false;
    if let Ok(out) = &ipv6 {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            if line.contains("HL set to") {
                hl_active = true;
                if ttl_value == 0 {
                    if let Some(v) = line.rsplit("HL set to ").next() {
                        ttl_value = v.trim().parse().unwrap_or(0);
                    }
                }
                break;
            }
        }
    }
    (
        200,
        json!({"ok": true, "data": {
            "active": active,
            "ipv6_active": hl_active,
            "ttl_value": ttl_value,
        }}),
    )
}

fn ttl_set(body: &[u8]) -> (u16, Value) {
    let val: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let ttl = match val.get("ttl").and_then(|v| v.as_u64()) {
        Some(v) if v >= 1 && v <= 255 => v as u32,
        _ => return (400, json!({"ok": false, "error": "ttl must be 1-255"})),
    };
    // Clear existing rules first
    let _ = std::process::Command::new("sh").args(["-c",
        "iptables -t mangle -S PREROUTING 2>/dev/null | grep 'TTL --ttl-set' | while read -r rule; do iptables -t mangle $(echo \"$rule\" | sed 's/-A/-D/'); done"
    ]).output();
    let _ = std::process::Command::new("sh").args(["-c",
        "ip6tables -t mangle -S PREROUTING 2>/dev/null | grep 'HL --hl-set' | while read -r rule; do ip6tables -t mangle $(echo \"$rule\" | sed 's/-A/-D/'); done"
    ]).output();
    // Add new rules
    let r4 = std::process::Command::new("iptables")
        .args([
            "-t",
            "mangle",
            "-A",
            "PREROUTING",
            "-i",
            "br-lan",
            "-j",
            "TTL",
            "--ttl-set",
            &ttl.to_string(),
        ])
        .output();
    let r6 = std::process::Command::new("ip6tables")
        .args([
            "-t",
            "mangle",
            "-A",
            "PREROUTING",
            "-i",
            "br-lan",
            "-j",
            "HL",
            "--hl-set",
            &ttl.to_string(),
        ])
        .output();
    let ok4 = r4.map(|o| o.status.success()).unwrap_or(false);
    let ok6 = r6.map(|o| o.status.success()).unwrap_or(false);
    // Persist to start_ttl.sh
    let script = format!(
        "#!/bin/sh\niptables  -t mangle -C PREROUTING -i br-lan -j TTL --ttl-set {ttl} 2>/dev/null ||   iptables  -t mangle -A PREROUTING -i br-lan -j TTL --ttl-set {ttl}\nip6tables -t mangle -C PREROUTING -i br-lan -j HL  --hl-set  {ttl} 2>/dev/null ||   ip6tables -t mangle -A PREROUTING -i br-lan -j HL  --hl-set  {ttl}\n"
    );
    let _ = std::fs::write("/data/local/tmp/start_ttl.sh", script);
    if ok4 || ok6 {
        (
            200,
            json!({"ok": true, "data": {"ttl": ttl, "ipv4": ok4, "ipv6": ok6}}),
        )
    } else {
        (
            500,
            json!({"ok": false, "error": format!("ipv4={ok4} ipv6={ok6}")}),
        )
    }
}

fn ttl_clear() -> (u16, Value) {
    let _ = std::process::Command::new("sh").args(["-c",
        "iptables -t mangle -S PREROUTING 2>/dev/null | grep 'TTL --ttl-set' | while read -r rule; do iptables -t mangle $(echo \"$rule\" | sed 's/-A/-D/'); done"
    ]).output();
    let _ = std::process::Command::new("sh").args(["-c",
        "ip6tables -t mangle -S PREROUTING 2>/dev/null | grep 'HL --hl-set' | while read -r rule; do ip6tables -t mangle $(echo \"$rule\" | sed 's/-A/-D/'); done"
    ]).output();
    // Remove persistence script content (keep file but make it a no-op)
    let _ = std::fs::write(
        "/data/local/tmp/start_ttl.sh",
        "#!/bin/sh\n# TTL disabled\n",
    );
    (200, json!({"ok": true}))
}

fn respond(request: Request, status: u16, body: Value, origin: Option<&str>) {
    let body_str = serde_json::to_string(&body).unwrap_or_default();
    let content_type = Header::from_bytes("Content-Type", "application/json").unwrap();
    let mut response = Response::from_string(body_str)
        .with_status_code(status)
        .with_header(content_type);
    for h in cors_headers(origin) {
        response = response.with_header(h);
    }
    let _ = request.respond(response);
}
