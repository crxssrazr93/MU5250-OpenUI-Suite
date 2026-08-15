//! Scheduled jobs.
//!
//! Jobs live in `/data`, which is writable and survives a firmware update, and
//! the agent re-reads them at startup. That is what makes them survive a
//! reboot: SAFETY.md rules out adding boot hooks, so nothing here installs a
//! cron entry or an init script — the agent is already started from the
//! existing `rc.local` line, and the schedule rides along with it.
//!
//! The consequence is worth stating plainly rather than hiding: if the agent is
//! not running, nothing fires. A job is a promise the agent keeps while it is
//! alive, not a system-level timer.

use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use crate::util::MutexExt;

const STORE_DIR: &str = "/data/scheduler";
const STORE_PATH: &str = "/data/scheduler/jobs.json";

/// How often the loop wakes to look for due jobs.
///
/// Schedules have minute resolution, so half a minute guarantees every minute
/// is observed without spinning.
const TICK: Duration = Duration::from_secs(30);

const MAX_JOBS: usize = 64;

/// What a job is allowed to do.
///
/// A closed list on purpose. The obvious design is a job that runs a command,
/// and that would turn the scheduler into a way to execute arbitrary shell as
/// root on a timer — reachable by anyone who reaches the API. Every action here
/// is something the agent already exposes on its own route, so a job can do
/// nothing a caller could not already do directly.
const ACTIONS: &[&str] = &[
    "reboot",
    "wifi_on",
    "wifi_off",
    "airplane_on",
    "airplane_off",
    "data_on",
    "data_off",
];

pub struct Scheduler {
    /// Guards the store against a tick and an API write colliding.
    lock: Mutex<()>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub fn new() -> Self {
        Self { lock: Mutex::new(()) }
    }

    /// Begin firing jobs.
    pub fn start(self: &Arc<Self>) {
        let scheduler = Arc::clone(self);
        std::thread::spawn(move || loop {
            scheduler.tick();
            std::thread::sleep(TICK);
        });
    }

    /// Run whatever is due now.
    fn tick(&self) {
        let now = match local_now() {
            Some(now) => now,
            // Without a clock there is no safe decision. Skipping is the
            // conservative one: firing a reboot because the time could not be
            // read would be worse than a late job.
            None => return,
        };

        let _guard = self.lock.safe_lock();
        let mut jobs = read_jobs();
        let mut changed = false;

        for job in jobs.iter_mut() {
            if !job["enabled"].as_bool().unwrap_or(true) {
                continue;
            }
            if !is_due(job, &now) {
                continue;
            }
            // Stamped before running, not after: a reboot job never comes back
            // to write it, and an unstamped reboot job fires again on every
            // tick of the same minute — and then again after the reboot.
            job["last_run"] = json!(now.stamp);
            changed = true;

            let action = job["action"].as_str().unwrap_or("").to_string();
            run_action(&action);
        }

        if changed {
            let _ = write_jobs(&jobs);
        }
    }
}

/// The device's local time, as the schedule is written in.
///
/// Read from `date` rather than computed from the epoch because a schedule says
/// "07:30" in whatever timezone the router is set to, and the agent has no
/// timezone database of its own to convert with.
struct Now {
    hour: u32,
    minute: u32,
    /// 0 = Sunday, matching `date +%w`.
    weekday: u32,
    /// Minute-resolution stamp, used to avoid firing twice in one minute.
    stamp: String,
}

fn local_now() -> Option<Now> {
    let output = Command::new("date").arg("+%H %M %w %Y-%m-%dT%H:%M").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.split_whitespace();
    Some(Now {
        hour: parts.next()?.parse().ok()?,
        minute: parts.next()?.parse().ok()?,
        weekday: parts.next()?.parse().ok()?,
        stamp: parts.next()?.to_string(),
    })
}

/// Whether a job should fire at this moment.
pub fn is_due_at(job: &Value, hour: u32, minute: u32, weekday: u32, stamp: &str) -> bool {
    // Already fired this minute. The tick is finer than the schedule, so
    // without this every job runs several times per scheduled minute.
    if job["last_run"].as_str() == Some(stamp) {
        return false;
    }

    let Some(time) = job["time"].as_str() else {
        return false;
    };
    let Some((job_hour, job_minute)) = parse_time(time) else {
        return false;
    };
    if job_hour != hour || job_minute != minute {
        return false;
    }

    match job["days"].as_array() {
        // No days listed means every day, which is what a plain daily job is.
        None => true,
        Some(days) if days.is_empty() => true,
        Some(days) => days.iter().any(|d| d.as_u64() == Some(u64::from(weekday))),
    }
}

fn is_due(job: &Value, now: &Now) -> bool {
    is_due_at(job, now.hour, now.minute, now.weekday, &now.stamp)
}

/// Parse `HH:MM`, rejecting anything that is not a real time.
pub fn parse_time(value: &str) -> Option<(u32, u32)> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour: u32 = hour.parse().ok()?;
    let minute: u32 = minute.parse().ok()?;
    if hour > 23 || minute > 59 {
        return None;
    }
    Some((hour, minute))
}

/// Perform a job's action, by the same means the API route would.
fn run_action(action: &str) {
    let ubus = |object: &str, method: &str, params: &str| {
        let _ = crate::ubus::call(object, method, Some(params));
    };
    match action {
        "reboot" => {
            let _ = Command::new("reboot").output();
        }
        "wifi_on" => ubus("zwrt_wifi.api", "wifi_set_switch", r#"{"wifi_enable":1}"#),
        "wifi_off" => ubus("zwrt_wifi.api", "wifi_set_switch", r#"{"wifi_enable":0}"#),
        "airplane_on" => ubus("zte_nwinfo_api", "nwinfo_set_mode", r#"{"operate_mode":"low_power"}"#),
        "airplane_off" => ubus("zte_nwinfo_api", "nwinfo_set_mode", r#"{"operate_mode":"online"}"#),
        "data_on" => ubus("zwrt_router.api", "router_set_wan_connect_mode", r#"{"connect_action":"connect"}"#),
        "data_off" => ubus("zwrt_router.api", "router_set_wan_connect_mode", r#"{"connect_action":"disconnect"}"#),
        // Unreachable while ACTIONS gates writes, but a store edited by hand
        // must not make the scheduler do something unexpected.
        _ => {}
    }
}

fn read_jobs() -> Vec<Value> {
    std::fs::read_to_string(STORE_PATH)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

fn write_jobs(jobs: &[Value]) -> Result<(), String> {
    std::fs::create_dir_all(STORE_DIR).map_err(|e| e.to_string())?;
    let temp = format!("{STORE_PATH}.tmp");
    let body = serde_json::to_vec_pretty(&jobs).map_err(|e| e.to_string())?;
    std::fs::write(&temp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, STORE_PATH).map_err(|e| e.to_string())
}

// --- Routes ---

use crate::handlers::AppState;

/// GET /api/scheduler/jobs
pub fn jobs_list(_state: &AppState) -> (u16, Value) {
    (
        200,
        json!({"ok": true, "data": {
            "jobs": read_jobs(),
            "actions": ACTIONS,
            // Said in the payload because it is the one thing about this
            // scheduler a user could otherwise get wrong.
            "notice": "Jobs run only while the agent is running.",
        }}),
    )
}

/// POST /api/scheduler/jobs — create or replace a job.
pub fn jobs_set(state: &AppState, body: &[u8]) -> (u16, Value) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };

    let action = parsed["action"].as_str().unwrap_or("").to_string();
    if !ACTIONS.contains(&action.as_str()) {
        return (
            400,
            json!({"ok": false, "error": format!("action must be one of: {}", ACTIONS.join(", "))}),
        );
    }

    let time = parsed["time"].as_str().unwrap_or("").to_string();
    if parse_time(&time).is_none() {
        return (400, json!({"ok": false, "error": "'time' must be HH:MM"}));
    }

    let days: Vec<u64> = parsed["days"]
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();
    if days.iter().any(|d| *d > 6) {
        return (400, json!({"ok": false, "error": "days are 0-6, Sunday first"}));
    }

    let name = parsed["name"].as_str().unwrap_or("").trim().to_string();
    if name.len() > 64 || name.chars().any(char::is_control) {
        return (400, json!({"ok": false, "error": "that name is not usable"}));
    }

    let _guard = state.scheduler.lock.safe_lock();
    let mut jobs = read_jobs();

    let id = parsed["id"].as_u64();
    if id.is_none() && jobs.len() >= MAX_JOBS {
        return (409, json!({"ok": false, "error": format!("no room for more than {MAX_JOBS} jobs")}));
    }
    let id = id.unwrap_or_else(|| jobs.iter().filter_map(|j| j["id"].as_u64()).max().unwrap_or(0) + 1);

    let job = json!({
        "id": id,
        "name": if name.is_empty() { action.clone() } else { name },
        "action": action,
        "time": time,
        "days": days,
        "enabled": parsed["enabled"].as_bool().unwrap_or(true),
        // Preserved across an edit so changing a job's name does not make it
        // fire again in the same minute.
        "last_run": jobs.iter()
            .find(|j| j["id"].as_u64() == Some(id))
            .and_then(|j| j["last_run"].clone().into()),
    });

    match jobs.iter_mut().find(|j| j["id"].as_u64() == Some(id)) {
        Some(existing) => *existing = job.clone(),
        None => jobs.push(job.clone()),
    }

    match write_jobs(&jobs) {
        Ok(()) => (200, json!({"ok": true, "data": {"job": job}})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// POST /api/scheduler/jobs/delete — body `{"id": 1}`
pub fn jobs_delete(state: &AppState, body: &[u8]) -> (u16, Value) {
    let Some(id) = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["id"].as_u64())
    else {
        return (400, json!({"ok": false, "error": "missing 'id'"}));
    };

    let _guard = state.scheduler.lock.safe_lock();
    let mut jobs = read_jobs();
    let before = jobs.len();
    jobs.retain(|j| j["id"].as_u64() != Some(id));
    if jobs.len() == before {
        return (404, json!({"ok": false, "error": "no job with that id"}));
    }
    match write_jobs(&jobs) {
        Ok(()) => (200, json!({"ok": true, "data": {"deleted": id}})),
        Err(e) => (503, json!({"ok": false, "error": e})),
    }
}

/// GET/POST /api/device/schedule-reboot — the common case, as its own route.
pub fn schedule_reboot(state: &AppState, method_is_post: bool, body: &[u8]) -> (u16, Value) {
    if !method_is_post {
        let jobs: Vec<Value> = read_jobs()
            .into_iter()
            .filter(|j| j["action"].as_str() == Some("reboot"))
            .collect();
        return (200, json!({"ok": true, "data": {"jobs": jobs}}));
    }

    let parsed: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (400, json!({"ok": false, "error": "invalid JSON"})),
    };
    let mut request = parsed.clone();
    request["action"] = json!("reboot");
    if request["name"].as_str().unwrap_or("").is_empty() {
        request["name"] = json!("Scheduled reboot");
    }
    jobs_set(state, request.to_string().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_real_times() {
        assert_eq!(parse_time("07:30"), Some((7, 30)));
        assert_eq!(parse_time("00:00"), Some((0, 0)));
        assert_eq!(parse_time("23:59"), Some((23, 59)));
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("07:60"), None);
        assert_eq!(parse_time("0730"), None);
        assert_eq!(parse_time(""), None);
    }

    #[test]
    fn fires_at_the_scheduled_minute() {
        let job = json!({"time": "07:30", "action": "reboot"});
        assert!(is_due_at(&job, 7, 30, 1, "2026-08-14T07:30"));
        assert!(!is_due_at(&job, 7, 31, 1, "2026-08-14T07:31"));
        assert!(!is_due_at(&job, 8, 30, 1, "2026-08-14T08:30"));
    }

    #[test]
    fn does_not_fire_twice_in_one_minute() {
        // The tick is finer than the schedule, so without the stamp check a
        // job runs several times per scheduled minute.
        let job = json!({"time": "07:30", "last_run": "2026-08-14T07:30"});
        assert!(!is_due_at(&job, 7, 30, 1, "2026-08-14T07:30"));
        // The next day is a different stamp, so it fires again.
        assert!(is_due_at(&job, 7, 30, 2, "2026-08-15T07:30"));
    }

    #[test]
    fn honours_selected_days() {
        let weekdays = json!({"time": "07:30", "days": [1, 2, 3, 4, 5]});
        assert!(is_due_at(&weekdays, 7, 30, 1, "a"));
        assert!(is_due_at(&weekdays, 7, 30, 5, "b"));
        // 0 is Sunday, matching `date +%w`.
        assert!(!is_due_at(&weekdays, 7, 30, 0, "c"));
        assert!(!is_due_at(&weekdays, 7, 30, 6, "d"));
    }

    #[test]
    fn an_empty_day_list_means_every_day() {
        let daily = json!({"time": "07:30", "days": []});
        for weekday in 0..7 {
            assert!(is_due_at(&daily, 7, 30, weekday, &format!("day{weekday}")));
        }
        let no_days = json!({"time": "07:30"});
        assert!(is_due_at(&no_days, 7, 30, 3, "x"));
    }

    #[test]
    fn a_job_with_no_time_never_fires() {
        // A hand-edited store must not produce a job that fires constantly.
        assert!(!is_due_at(&json!({"action": "reboot"}), 7, 30, 1, "x"));
        assert!(!is_due_at(&json!({"time": "nonsense"}), 7, 30, 1, "x"));
    }

    #[test]
    fn only_known_actions_are_allowed() {
        // The scheduler must never become a way to run arbitrary shell as root
        // on a timer, which is what a free-form command field would be.
        assert!(ACTIONS.contains(&"reboot"));
        assert!(!ACTIONS.contains(&"exec"));
        assert!(!ACTIONS.contains(&"sh"));
    }
}
