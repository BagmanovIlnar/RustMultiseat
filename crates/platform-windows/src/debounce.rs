//! Debounce helper for hot-plug rescans.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Coalesces bursty invalidation signals into spaced rescan requests.
#[derive(Debug)]
pub struct DebounceGate {
    pending: AtomicBool,
    last_ready: Mutex<Option<Instant>>,
    window: Duration,
}

impl DebounceGate {
    /// Creates a gate with the given quiet window.
    pub fn new(window: Duration) -> Self {
        Self {
            pending: AtomicBool::new(false),
            last_ready: Mutex::new(None),
            window,
        }
    }

    /// Default 300 ms debounce used for device notifications.
    pub fn default_hotplug() -> Self {
        Self::new(Duration::from_millis(300))
    }

    /// Marks that a rescan should happen after the quiet window.
    pub fn signal(&self) {
        self.pending.store(true, Ordering::SeqCst);
    }

    /// Returns true once when a pending signal has waited out the window.
    pub fn take_ready(&self) -> bool {
        if !self.pending.load(Ordering::SeqCst) {
            return false;
        }
        let mut last = self.last_ready.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if let Some(prev) = *last {
            if now.duration_since(prev) < self.window {
                return false;
            }
        }
        self.pending.store(false, Ordering::SeqCst);
        *last = Some(now);
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
    fn signals_become_ready_after_window() {
        let gate = DebounceGate::new(Duration::from_millis(20));
        assert!(!gate.take_ready());
        gate.signal();
        // First take may succeed immediately when last_ready is None.
        assert!(gate.take_ready());
        gate.signal();
        assert!(!gate.take_ready());
        thread::sleep(Duration::from_millis(30));
        assert!(gate.take_ready());
    }
}
