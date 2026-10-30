//! Shared mutex for tests that mutate process environment variables.

use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Locks process-wide env mutation for the duration of a test.
pub fn lock() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}
