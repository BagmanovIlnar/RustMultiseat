//! Debounce helper for hot-plug rescans.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Coalesces bursty invalidation signals into one rescan after a quiet period.
///
/// Semantics: each [`signal`] refreshes `last_signal`. [`take_ready`] returns
/// true only when pending and `now - last_signal >= window` (quiet window after
/// the *last* event), then clears pending.
#[derive(Debug)]
pub struct DebounceGate {
    pending: AtomicBool,
    last_signal: Mutex<Option<Instant>>,
    window: Duration,
}

impl DebounceGate {
    /// Creates a gate with the given quiet window.
    pub fn new(window: Duration) -> Self {
        Self {
            pending: AtomicBool::new(false),
            last_signal: Mutex::new(None),
            window,
        }
    }

    /// Default 300 ms quiet window used for device notifications.
    pub fn default_hotplug() -> Self {
        Self::new(Duration::from_millis(300))
    }

    /// Quiet window length.
    pub fn window(&self) -> Duration {
        self.window
    }

    /// Marks that a rescan should happen after the quiet window from *this* signal.
    pub fn signal(&self) {
        self.pending.store(true, Ordering::SeqCst);
        let mut last = self.last_signal.lock().unwrap_or_else(|e| e.into_inner());
        *last = Some(Instant::now());
    }

    /// Returns true once when a pending signal has waited out the quiet window.
    pub fn take_ready(&self) -> bool {
        if !self.pending.load(Ordering::SeqCst) {
            return false;
        }
        let last = self.last_signal.lock().unwrap_or_else(|e| e.into_inner());
        let Some(signaled_at) = *last else {
            return false;
        };
        if Instant::now().duration_since(signaled_at) < self.window {
            return false;
        }
        drop(last);
        self.pending.store(false, Ordering::SeqCst);
        true
    }

    /// True if a signal is waiting (even if still inside the quiet window).
    pub fn is_pending(&self) -> bool {
        self.pending.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn quiet_period_from_last_signal() {
        let gate = DebounceGate::new(Duration::from_millis(25));
        assert!(!gate.take_ready());
        gate.signal();
        assert!(!gate.take_ready(), "must wait quiet window after signal");
        thread::sleep(Duration::from_millis(10));
        gate.signal(); // refresh quiet window
        assert!(!gate.take_ready());
        thread::sleep(Duration::from_millis(30));
        assert!(gate.take_ready());
        assert!(!gate.take_ready());
    }

    #[test]
    fn burst_coalesces_to_single_ready() {
        let gate = DebounceGate::new(Duration::from_millis(20));
        for _ in 0..5 {
            gate.signal();
            thread::sleep(Duration::from_millis(2));
        }
        assert!(gate.is_pending());
        assert!(!gate.take_ready());
        thread::sleep(Duration::from_millis(25));
        assert!(gate.take_ready());
        assert!(!gate.is_pending());
    }
}
