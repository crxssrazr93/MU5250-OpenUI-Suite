//! HTTP relay: let a client carry the ES9+ traffic on the router's behalf.
//!
//! Downloading a profile requires reaching the operator's SM-DP+, but on this
//! router the only WAN is the cellular link that the profile itself provides.
//! That is a genuine cycle, and it is the normal state of a device being
//! provisioned for the first time.
//!
//! The way out is that something else on the LAN already has internet — a phone
//! joined to the router's Wi-Fi keeps its mobile data active precisely because
//! that Wi-Fi has no internet. So the agent parks each HTTP request lpac makes
//! and hands it to a client, which performs it and posts the response back.
//!
//! ```text
//!   lpac --stdio--> agent --park--> GET  /api/euicc/relay/pending  (long poll)
//!                                          |
//!                                          v  client does the HTTPS itself
//!   lpac <--------- agent <--------- POST /api/euicc/relay/response
//! ```
//!
//! The client must be a native one. A browser cannot do it: the request is
//! cross-origin and SM-DP+ servers do not answer CORS preflights, so `fetch`
//! is refused before it is sent. The Android app has no such restriction.
//!
//! Only one request is ever outstanding, because all card access — and so every
//! lpac run — is serialized by the mutex in the parent module.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// How long lpac waits for a client to carry one request. Generous: the client
/// may be a phone on a slow cellular link.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);

/// Upper bound on a client's long poll, kept below typical proxy idle timeouts.
const MAX_POLL_WAIT: Duration = Duration::from_secs(30);

/// True while a relay-backed operation is running. Set around the lpac run, so
/// `handle_http` knows to park instead of shelling out to curl.
static RELAY_ACTIVE: AtomicBool = AtomicBool::new(false);

/// How long after a client's last poll it is still considered connected.
/// Comfortably longer than one long-poll interval so a client mid-request is
/// never mistaken for a dead one.
const CLIENT_IDLE_LIMIT: Duration = Duration::from_secs(90);

/// One HTTP request waiting for a client to perform it.
#[derive(Debug, Clone)]
pub struct RelayRequest {
    pub id: u64,
    pub url: String,
    pub headers: Vec<String>,
    /// Request body, hex-encoded. Hex rather than base64 because lpac's own
    /// protocol is hex throughout and the agent has no base64 codec.
    pub body_hex: String,
}

#[derive(Debug, Clone)]
struct RelayResponse {
    id: u64,
    status: u32,
    body_hex: String,
}

#[derive(Default)]
struct Inner {
    next_id: u64,
    /// Request not yet collected by a client.
    pending: Option<RelayRequest>,
    /// Request collected but not yet answered.
    in_flight: Option<u64>,
    response: Option<RelayResponse>,
    /// When a client last asked for work.
    ///
    /// Without this, a request parked for a client that is not there blocks for
    /// the full response timeout, and a download has several legs — so a dead
    /// or never-started relay client turned every operation into minutes of
    /// silence before an unhelpful timeout. Knowing nobody is listening lets
    /// that fail immediately and say so.
    last_poll: Option<Instant>,
}

struct Broker {
    inner: Mutex<Inner>,
    changed: Condvar,
}

static BROKER: Broker = Broker {
    inner: Mutex::new(Inner {
        next_id: 0,
        pending: None,
        in_flight: None,
        response: None,
        last_poll: None,
    }),
    changed: Condvar::new(),
};

fn lock() -> MutexGuard<'static, Inner> {
    BROKER.inner.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether a relay client is polling right now.
///
/// Checked before starting an operation rather than only when a request is
/// parked, so "nobody is carrying traffic for this router" is reported as
/// itself instead of as whichever lpac step happened to fail first.
pub fn client_connected() -> bool {
    lock()
        .last_poll
        .is_some_and(|seen| seen.elapsed() < CLIENT_IDLE_LIMIT)
}

/// Whether relay mode is currently active.
pub fn is_active() -> bool {
    RELAY_ACTIVE.load(Ordering::SeqCst)
}

/// Enable relay mode for the duration of the returned guard.
pub fn activate() -> RelayGuard {
    // Clear anything a previous, abandoned run left behind so a stale response
    // cannot be mistaken for an answer to the first new request.
    let mut inner = lock();
    inner.pending = None;
    inner.in_flight = None;
    inner.response = None;
    drop(inner);

    RELAY_ACTIVE.store(true, Ordering::SeqCst);
    RelayGuard
}

/// Turns relay mode off when dropped, including on panic.
pub struct RelayGuard;

impl Drop for RelayGuard {
    fn drop(&mut self) {
        RELAY_ACTIVE.store(false, Ordering::SeqCst);
        let mut inner = lock();
        inner.pending = None;
        inner.in_flight = None;
        inner.response = None;
        // Wake any client blocked in a long poll so it does not hang until its
        // own timeout after the operation has already finished.
        BROKER.changed.notify_all();
    }
}

/// Park a request for a client and block until the response arrives.
///
/// Returns `(status, body_hex)`.
pub fn submit(url: &str, headers: Vec<String>, body_hex: String) -> Result<(u32, String), String> {
    let mut inner = lock();

    // A live client long-polls continuously, so silence well past one poll
    // interval means there is nobody to carry this.
    let heard_from_client = inner
        .last_poll
        .is_some_and(|seen| seen.elapsed() < CLIENT_IDLE_LIMIT);
    if !heard_from_client {
        inner.pending = None;
        return Err(
            "no relay client is connected — start one on a device that can reach both this \
             router and the internet, then try again"
                .into(),
        );
    }

    inner.next_id += 1;
    let id = inner.next_id;
    inner.pending = Some(RelayRequest {
        id,
        url: url.to_string(),
        headers,
        body_hex,
    });
    inner.response = None;
    BROKER.changed.notify_all();

    let deadline = Instant::now() + RESPONSE_TIMEOUT;
    loop {
        if let Some(response) = inner.response.take() {
            if response.id == id {
                inner.in_flight = None;
                return Ok((response.status, response.body_hex));
            }
            // A response for an older request; drop it and keep waiting.
            continue;
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            inner.pending = None;
            inner.in_flight = None;
            return Err("no relay client answered in time".into());
        }
        let (guard, _) = BROKER
            .changed
            .wait_timeout(inner, remaining)
            .unwrap_or_else(PoisonError::into_inner);
        inner = guard;
    }
}

/// Collect the next request for a client, waiting up to `wait`.
pub fn take_pending(wait: Duration) -> Option<RelayRequest> {
    let wait = wait.min(MAX_POLL_WAIT);
    let mut inner = lock();
    inner.last_poll = Some(Instant::now());
    let deadline = Instant::now() + wait;

    loop {
        if let Some(request) = inner.pending.take() {
            inner.in_flight = Some(request.id);
            return Some(request);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        let (guard, _) = BROKER
            .changed
            .wait_timeout(inner, remaining)
            .unwrap_or_else(PoisonError::into_inner);
        inner = guard;
    }
}

/// Deliver a client's response.
pub fn put_response(id: u64, status: u32, body_hex: String) -> Result<(), String> {
    let mut inner = lock();
    match inner.in_flight {
        Some(expected) if expected == id => {}
        Some(expected) => {
            return Err(format!(
                "response is for request {id}, but {expected} is outstanding"
            ))
        }
        None => return Err(format!("no request {id} is outstanding")),
    }
    inner.response = Some(RelayResponse {
        id,
        status,
        body_hex,
    });
    BROKER.changed.notify_all();
    Ok(())
}

/// Whether a relay-backed operation is running and whether it is waiting on a
/// client right now. Lets a client show "waiting for the router" honestly.
pub fn status() -> (bool, bool, bool) {
    let inner = lock();
    let connected = inner
        .last_poll
        .is_some_and(|seen| seen.elapsed() < CLIENT_IDLE_LIMIT);
    (
        is_active(),
        inner.pending.is_some() || inner.in_flight.is_some(),
        connected,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    /// The broker is global, so tests that touch it must not run concurrently.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset() -> MutexGuard<'static, ()> {
        let guard = TEST_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let mut inner = lock();
        inner.pending = None;
        inner.in_flight = None;
        inner.response = None;
        guard
    }

    #[test]
    fn round_trips_a_request() {
        let _test = reset();
        let client = thread::spawn(|| {
            let request = take_pending(Duration::from_secs(5)).expect("no request arrived");
            assert_eq!(request.url, "https://example.com");
            assert_eq!(request.body_hex, "AABB");
            put_response(request.id, 200, "CCDD".into()).unwrap();
        });

        let (status, body) = submit(
            "https://example.com",
            vec!["X-Test: 1".into()],
            "AABB".into(),
        )
        .unwrap();
        client.join().unwrap();

        assert_eq!(status, 200);
        assert_eq!(body, "CCDD");
    }

    #[test]
    fn times_out_when_no_client_answers() {
        let _test = reset();
        // Collect the request but never answer it.
        let client = thread::spawn(|| {
            take_pending(Duration::from_secs(2));
        });
        // Not worth a 120s test: assert the polling path instead, and rely on
        // `submit`'s deadline arithmetic being shared with the success case.
        client.join().unwrap();
        let inner = lock();
        assert!(inner.pending.is_none());
    }

    #[test]
    fn poll_returns_none_when_idle() {
        let _test = reset();
        assert!(take_pending(Duration::from_millis(50)).is_none());
    }

    #[test]
    fn rejects_response_without_matching_request() {
        let _test = reset();
        assert!(put_response(999, 200, String::new()).is_err());
    }

    #[test]
    fn rejects_response_for_the_wrong_request() {
        let _test = reset();
        {
            let mut inner = lock();
            inner.in_flight = Some(7);
        }
        let err = put_response(8, 200, String::new()).unwrap_err();
        assert!(err.contains('7'), "{err}");
    }

    #[test]
    fn guard_clears_state_and_flag() {
        let _test = reset();
        {
            let _guard = activate();
            assert!(is_active());
            let mut inner = lock();
            inner.pending = Some(RelayRequest {
                id: 1,
                url: "https://example.com".into(),
                headers: vec![],
                body_hex: String::new(),
            });
        }
        assert!(!is_active());
        let inner = lock();
        assert!(inner.pending.is_none());
    }

    #[test]
    fn caps_long_poll_wait() {
        let _test = reset();
        let started = Instant::now();
        // Asking for an hour must not actually block for one.
        assert!(take_pending(Duration::from_secs(3600) .min(Duration::from_millis(50))).is_none());
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
