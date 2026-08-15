use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::util::MutexExt;

/// A single value refreshed on demand, at most once per TTL.
///
/// Every `ubus`/`uci` read in this agent is a fork+exec, which is the dominant
/// cost of serving `/api/dashboard`. Wrapping each source in its own TTL means
/// the poll rate is decoupled from the refresh rate: a 3 s dashboard poll no
/// longer re-reads WAN state that changes hourly, and several clients polling
/// at once collapse into one refresh instead of multiplying the load.
pub struct Cached<T> {
    slot: Mutex<Option<(Instant, T)>>,
}

impl<T: Clone> Cached<T> {
    /// Return the cached value if it is younger than `ttl`, otherwise call
    /// `refresh` and store the result.
    ///
    /// The lock is held across `refresh`, so concurrent callers wait for one
    /// refresh rather than each spawning their own subprocess — which is the
    /// point on a device where the subprocess is the expensive part.
    pub fn get_or_refresh(&self, ttl: Duration, refresh: impl FnOnce() -> T) -> T {
        let mut slot = self.slot.safe_lock();
        if let Some((at, value)) = slot.as_ref() {
            if at.elapsed() < ttl {
                return value.clone();
            }
        }
        let value = refresh();
        *slot = Some((Instant::now(), value.clone()));
        value
    }

    /// Force the next `get_or_refresh` to re-read. Call after a mutation that
    /// makes the cached copy wrong by definition.
    pub fn invalidate(&self) {
        *self.slot.safe_lock() = None;
    }
}

impl<T> Default for Cached<T> {
    fn default() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn serves_from_cache_within_ttl() {
        let calls = AtomicU32::new(0);
        let cached: Cached<u32> = Cached::default();
        let refresh = || {
            calls.fetch_add(1, Ordering::Relaxed);
            7
        };

        assert_eq!(cached.get_or_refresh(Duration::from_secs(60), refresh), 7);
        assert_eq!(cached.get_or_refresh(Duration::from_secs(60), refresh), 7);
        assert_eq!(calls.load(Ordering::Relaxed), 1, "second call should hit cache");
    }

    #[test]
    fn refreshes_once_ttl_elapses() {
        let calls = AtomicU32::new(0);
        let cached: Cached<u32> = Cached::default();
        let refresh = || calls.fetch_add(1, Ordering::Relaxed);

        cached.get_or_refresh(Duration::from_secs(60), refresh);
        // A zero TTL always counts as stale.
        cached.get_or_refresh(Duration::ZERO, refresh);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
}
