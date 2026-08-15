use std::sync::Arc;

use serde_json::{json, Value};

use std::time::Duration;

use crate::at_cmd::AtPort;
use crate::auth::{self, AuthState};
use crate::cache::Cached;
use crate::charge_policy::ChargeLimitEnforcer;
use crate::connection_logger::ConnectionLogger;
use crate::signal_logger::SignalLogger;
use crate::system::{self, CpuTracker, ProcessTracker, SpeedTracker};
use crate::ubus;

// Per-source freshness for the dashboard batch. Every entry below costs a
// fork+exec, so the client's poll rate is deliberately decoupled from the rate
// at which each source is actually re-read — and concurrent clients (phone plus
// laptop) collapse onto one refresh instead of multiplying the load.
const SIGNAL_TTL: Duration = Duration::from_millis(2500);
const THERMAL_TTL: Duration = Duration::from_secs(10);
const WAN_TTL: Duration = Duration::from_secs(30);
const DATA_USAGE_TTL: Duration = Duration::from_secs(30);
/// Billing-cycle dates move once a month.
const CYCLE_DATE_TTL: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct DashboardCache {
    signal: Cached<Value>,
    wan: Cached<Value>,
    wan6: Cached<Value>,
    thermal: Cached<Value>,
    data_usage: Cached<Value>,
    cycle_dates: Cached<(Option<String>, Option<String>)>,
}

pub struct AppState {
    pub auth: AuthState,
    pub cpu: CpuTracker,
    pub speed: SpeedTracker,
    pub proc_tracker: ProcessTracker,
    pub at_port: AtPort,
    pub dash: DashboardCache,
    pub charge_limit: Arc<ChargeLimitEnforcer>,
    pub signal_logger: Arc<SignalLogger>,
    pub connection_logger: Arc<ConnectionLogger>,
    pub scheduler: Arc<crate::scheduler::Scheduler>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            auth: AuthState::new(),
            cpu: CpuTracker::new(),
            speed: SpeedTracker::new(),
            proc_tracker: ProcessTracker::new(),
            at_port: AtPort::new(),
            dash: DashboardCache::default(),
            charge_limit: Arc::new(ChargeLimitEnforcer::new()),
            signal_logger: Arc::new(SignalLogger::new()),
            connection_logger: Arc::new(ConnectionLogger::new()),
            scheduler: Arc::new(crate::scheduler::Scheduler::new()),
        }
    }
}

/// POST /api/auth/login — body: {"password": "..."} or {"pin": "..."}
pub fn login(
    state: &AppState,
    body: &[u8],
    client_ip: &str,
    user_agent: Option<&str>,
) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };

    let password = parsed["password"].as_str();
    let pin = parsed["pin"].as_str();
    if password.is_some() && pin.is_some() {
        return (
            400,
            json!({"ok": false, "error": "provide either password or pin"}),
        );
    }

    let result = if let Some(password) = password {
        state.auth.login_password(password, client_ip)
    } else if let Some(pin) = pin {
        if !is_mobile_user_agent(user_agent.unwrap_or_default()) {
            return (
                403,
                json!({"ok": false, "error": "PIN login is only available from mobile devices"}),
            );
        }
        state.auth.login_pin(pin, client_ip)
    } else {
        return (
            400,
            json!({"ok": false, "error": "missing 'password' or 'pin' field"}),
        );
    };

    match result {
        auth::LoginResult::Ok { token } => (200, json!({"ok": true, "data": {"token": token}})),
        auth::LoginResult::Invalid => (401, json!({"ok": false, "error": "invalid credentials"})),
        auth::LoginResult::Locked { retry_after_secs } => (
            429,
            json!({"ok": false, "error": format!("too many attempts, retry in {retry_after_secs}s")}),
        ),
    }
}

fn is_mobile_user_agent(user_agent: &str) -> bool {
    let ua = user_agent.to_ascii_lowercase();
    ua.contains("mobile")
        || ua.contains("android")
        || ua.contains("iphone")
        || ua.contains("ipad")
        || ua.contains("ipod")
        || ua.contains("blackberry")
        || ua.contains("iemobile")
        || ua.contains("opera mini")
}

/// GET /api/device
pub fn device(_state: &AppState) -> (u16, Value) {
    let info = system::read_device_info();
    (
        200,
        json!({"ok": true, "data": {
            "hostname": info.hostname,
            "uptime_secs": info.uptime_secs,
            "load_avg": info.load_avg,
            "kernel": info.kernel,
        }}),
    )
}

/// GET /api/cpu
pub fn cpu(state: &AppState) -> (u16, Value) {
    let usage = state.cpu.sample();
    (200, json!({"ok": true, "data": usage}))
}

/// GET /api/memory
pub fn memory(_state: &AppState) -> (u16, Value) {
    match system::read_meminfo() {
        Some(m) => (200, json!({"ok": true, "data": m})),
        None => (
            503,
            json!({"ok": false, "error": "memory info not available"}),
        ),
    }
}

/// PUT /api/data-usage/reset-day
pub fn data_usage_reset_day_set(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let day = parsed["reset_day"]
        .as_u64()
        .or_else(|| parsed["clearday"].as_u64())
        .unwrap_or(0);
    if !(1..=31).contains(&day) {
        return (
            400,
            json!({"ok": false, "error": "reset_day must be between 1 and 31"}),
        );
    }

    let params = json!({
        "source_module": "web",
        "cid": 1,
        "type": 4,
        "enable": 1,
        "clearday": day,
    });

    match ubus::call(
        "zwrt_data",
        "set_wwandst_clearday",
        Some(&params.to_string()),
    ) {
        // We just changed the cycle, so the cached copies are wrong by definition.
        Ok(_) => {
            state.dash.data_usage.invalidate();
            state.dash.cycle_dates.invalidate();
            match read_data_usage_live(&state.dash) {
                Ok(data) => (200, json!({"ok": true, "data": data})),
                Err(e) => (503, json!({"ok": false, "error": e})),
            }
        }
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

fn read_data_usage_live(cache: &DashboardCache) -> Result<Value, String> {
    let stats = ubus::call(
        "zwrt_data",
        "get_wwandst",
        Some(r#"{"source_module":"web","cid":1,"type":4}"#),
    )?;
    let clear = ubus::call(
        "zwrt_data",
        "get_wwandst_clearday",
        Some(r#"{"source_module":"web","cid":1,"type":4}"#),
    )
    .unwrap_or_else(|_| json!({}));

    let section = "zwrt_data_commit.wwancid1dst";

    let read_stat_period = |prefix: &str| -> Value {
        let get = |suffix: &str| -> Value {
            let key = format!("{prefix}_{suffix}");
            number_value(stats.get(&key)).unwrap_or(Value::Null)
        };
        json!({
            "tx_bytes": get("tx_bytes"),
            "rx_bytes": get("rx_bytes"),
            "time_secs": get("time"),
            "tx_packets": get("tx_packets"),
            "rx_packets": get("rx_packets"),
        })
    };

    let reset_day = number_value(clear.get("clearday")).unwrap_or_else(|| {
        ubus::uci_get(&format!("{section}.clearday"))
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Value::from)
            .unwrap_or(Value::from(1))
    });
    let reset_enabled = number_value(clear.get("enable")).unwrap_or_else(|| {
        ubus::uci_get(&format!("{section}.clearday_enable"))
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Value::from)
            .unwrap_or(Value::from(0))
    });

    // These two are billing-cycle dates: they change once a month, but used to
    // cost two `uci get` forks on every dashboard poll.
    let (clear_date_record, next_clear_date) =
        cache.cycle_dates.get_or_refresh(CYCLE_DATE_TTL, || {
            (
                ubus::uci_get(&format!("{section}.clear_date_record")).ok(),
                ubus::uci_get(&format!("{section}.clearday_date")).ok(),
            )
        });

    Ok(json!({
        "day": read_stat_period("day"),
        "month": read_stat_period("month"),
        "cycle": read_stat_period("month"),
        "since_power_on": read_stat_period("real"),
        "total": read_stat_period("total"),
        "reset_day": reset_day,
        "reset_enabled": reset_enabled,
        "clear_date_record": clear_date_record,
        "next_clear_date": next_clear_date,
    }))
}

fn number_value(value: Option<&Value>) -> Option<Value> {
    match value? {
        Value::Number(n) => Some(Value::Number(n.clone())),
        Value::String(s) => s.parse::<u64>().ok().map(Value::from),
        _ => None,
    }
}

/// GET /api/system/top
pub fn system_top(state: &AppState) -> (u16, Value) {
    let result = state.proc_tracker.sample();
    (200, json!({"ok": true, "data": result}))
}

/// POST /api/system/kill-bloat — body: {"all": true} or {"pids": [1, 2, 3]}
pub fn system_kill_bloat(_state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };

    let pids: Option<Vec<u32>> = if parsed["all"].as_bool() == Some(true) {
        None
    } else if let Some(arr) = parsed["pids"].as_array() {
        let ids: Vec<u32> = arr
            .iter()
            .filter_map(|v| v.as_u64().map(|n| n as u32))
            .collect();
        if ids.is_empty() {
            return (400, json!({"ok": false, "error": "pids array is empty"}));
        }
        Some(ids)
    } else {
        return (
            400,
            json!({"ok": false, "error": "expected 'all' or 'pids'"}),
        );
    };

    let result = system::kill_bloat(pids.as_deref());
    (200, json!({"ok": true, "data": result}))
}

/// GET /api/dashboard — batch endpoint aggregating all dashboard data.
///
/// This is the app's heartbeat: Home, Signal and Modem/Data all read it rather
/// than polling their own endpoints. Free sources (procfs/sysfs) are read every
/// time; each ubus/uci source sits behind its own TTL.
pub fn dashboard(state: &AppState) -> (u16, Value) {
    let cache = &state.dash;

    // procfs / sysfs — no subprocess, read fresh every time.
    let device_info = system::read_device_info();
    let battery = system::read_battery();
    let cpu_usage = state.cpu.sample();
    let meminfo = system::read_meminfo();
    let speed = state.speed.sample();

    // Raw ubus passthroughs — the dashboard client maps these itself.
    let signal = cache.signal.get_or_refresh(SIGNAL_TTL, || {
        ubus::call("zte_nwinfo_api", "nwinfo_get_netinfo", Some("{}")).unwrap_or(Value::Null)
    });
    let wan = cache.wan.get_or_refresh(WAN_TTL, || {
        ubus::call("network.interface.zte_wan", "status", Some("{}")).unwrap_or(Value::Null)
    });
    let wan6 = cache.wan6.get_or_refresh(WAN_TTL, || {
        ubus::call("network.interface.zte_wan6", "status", Some("{}")).unwrap_or(Value::Null)
    });
    let thermal = cache.thermal.get_or_refresh(THERMAL_TTL, || {
        ubus::call("zwrt_bsp.thermal", "get_cpu_temp", Some("{}")).unwrap_or(Value::Null)
    });
    let data_usage = cache
        .data_usage
        .get_or_refresh(DATA_USAGE_TTL, || read_data_usage(cache));

    let mut result = serde_json::Map::new();
    result.insert(
        "device".into(),
        json!({
            "hostname": device_info.hostname,
            "uptime_secs": device_info.uptime_secs,
            "load_avg": device_info.load_avg,
            "kernel": device_info.kernel,
        }),
    );
    result.insert("battery".into(), json!(battery));
    result.insert("cpu".into(), json!(cpu_usage));
    result.insert("memory".into(), json!(meminfo));
    result.insert("speed".into(), json!(speed));
    result.insert("data_usage".into(), data_usage);
    result.insert("signal".into(), signal);
    result.insert("wan".into(), wan);
    result.insert("wan6".into(), wan6);
    result.insert("thermal".into(), thermal);
    (200, json!({"ok": true, "data": result}))
}

fn read_data_usage(cache: &DashboardCache) -> Value {
    read_data_usage_live(cache).unwrap_or_else(|e| json!({"error": e}))
}
