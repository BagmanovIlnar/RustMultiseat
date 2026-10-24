//! Cached [`HardwareTree`] invalidated by debounced hot-plug signals.
//!
//! Per agent.md: device events only invalidate; a full DXGI/SetupAPI/CM rescan
//! runs after the debounce quiet window (daemon poller or next forced rebuild).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use multiseat_core::{CoreResult, HardwareTree};

use crate::debounce::DebounceGate;

/// Shared gate + optional last successful snapshot.
#[derive(Debug)]
pub struct SnapshotCache {
    gate: Arc<DebounceGate>,
    cache: Mutex<Option<HardwareTree>>,
    /// Set when a debounced signal cleared the cache and a rescan is owed.
    rescan_owed: AtomicBool,
}

impl SnapshotCache {
    /// Creates an empty cache with the given debounce gate.
    pub fn new(gate: Arc<DebounceGate>) -> Self {
        Self {
            gate,
            cache: Mutex::new(None),
            rescan_owed: AtomicBool::new(false),
        }
    }

    /// Shared debounce gate (hot-plug watcher signals this).
    pub fn gate(&self) -> Arc<DebounceGate> {
        Arc::clone(&self.gate)
    }

    /// Consumes a ready debounce signal, drops the cache, and marks a rescan owed.
    ///
    /// Returns `true` when the daemon should rebuild and publish `HardwareUpdated`.
    pub fn poll_invalidated(&self) -> bool {
        if !self.gate.take_ready() {
            return false;
        }
        if let Ok(mut guard) = self.cache.lock() {
            *guard = None;
        }
        self.rescan_owed.store(true, Ordering::SeqCst);
        true
    }

    /// Returns the cached tree when still valid; otherwise runs `build` and stores it.
    pub fn get_or_build<F>(&self, build: F) -> CoreResult<HardwareTree>
    where
        F: FnOnce() -> CoreResult<HardwareTree>,
    {
        if let Ok(guard) = self.cache.lock() {
            if let Some(tree) = guard.as_ref() {
                return Ok(tree.clone());
            }
        }

        let tree = build()?;
        if let Ok(mut guard) = self.cache.lock() {
            *guard = Some(tree.clone());
        }
        self.rescan_owed.store(false, Ordering::SeqCst);
        Ok(tree)
    }

    /// True when a hot-plug cycle cleared the cache and no rebuild has completed yet.
    pub fn rescan_owed(&self) -> bool {
        self.rescan_owed.load(Ordering::SeqCst)
    }

    /// Drops any cached snapshot (tests / forced refresh).
    pub fn invalidate(&self) {
        if let Ok(mut guard) = self.cache.lock() {
            *guard = None;
        }
        self.rescan_owed.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debounce::DebounceGate;
    use multiseat_core::HardwareTree;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::time::Duration;

    fn empty_tree(revision: u64) -> HardwareTree {
        HardwareTree {
            revision,
            gpus: vec![],
            displays: vec![],
            usb_roots: vec![],
            input_devices: vec![],
            capabilities: Default::default(),
        }
    }

    #[test]
    fn cache_hits_until_invalidated() {
        let builds = AtomicUsize::new(0);
        let cache = SnapshotCache::new(Arc::new(DebounceGate::new(Duration::from_millis(5))));
        let tree = cache
            .get_or_build(|| {
                builds.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(empty_tree(1))
            })
            .unwrap();
        assert_eq!(tree.revision, 1);
        assert_eq!(builds.load(AtomicOrdering::SeqCst), 1);

        let tree = cache
            .get_or_build(|| {
                builds.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(empty_tree(2))
            })
            .unwrap();
        assert_eq!(tree.revision, 1);
        assert_eq!(builds.load(AtomicOrdering::SeqCst), 1);

        cache.invalidate();
        let tree = cache
            .get_or_build(|| {
                builds.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(empty_tree(3))
            })
            .unwrap();
        assert_eq!(tree.revision, 3);
        assert_eq!(builds.load(AtomicOrdering::SeqCst), 2);
    }

    #[test]
    fn poll_invalidated_clears_cache() {
        let cache = SnapshotCache::new(Arc::new(DebounceGate::new(Duration::from_millis(1))));
        cache.get_or_build(|| Ok(empty_tree(1))).unwrap();
        cache.gate().signal();
        // Quiet-period debounce: first ready waits for window from last signal.
        std::thread::sleep(Duration::from_millis(5));
        assert!(cache.poll_invalidated());
        assert!(cache.rescan_owed());
        let builds = AtomicUsize::new(0);
        let tree = cache
            .get_or_build(|| {
                builds.fetch_add(1, AtomicOrdering::SeqCst);
                Ok(empty_tree(9))
            })
            .unwrap();
        assert_eq!(tree.revision, 9);
        assert_eq!(builds.load(AtomicOrdering::SeqCst), 1);
        assert!(!cache.rescan_owed());
    }
}
