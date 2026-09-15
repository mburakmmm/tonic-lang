use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};
use tonic_core::diagnostic::{Diagnostic, Result};

static NEXT_RUNTIME_ID: AtomicU64 = AtomicU64::new(1);

/// Stable native-side ownership token. It never exposes or retains a `Vm`
/// address; delayed work is drained by the owning VM at safe boundaries.
pub(crate) struct RuntimeOwner {
    id: u64,
    alive: AtomicBool,
    deferred_persistent_releases: Mutex<Vec<u64>>,
    deferred_foreign_reference_releases: Mutex<Vec<u64>>,
}

impl RuntimeOwner {
    pub(crate) fn new() -> Result<Self> {
        let id = NEXT_RUNTIME_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| Diagnostic::new("RuntimeError", "runtime identity space exhausted"))?;
        Ok(Self {
            id,
            alive: AtomicBool::new(true),
            deferred_persistent_releases: Mutex::new(Vec::new()),
            deferred_foreign_reference_releases: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn queue_persistent_release(&self, handle: u64) -> bool {
        if !self.alive.load(Ordering::Acquire) {
            return false;
        }
        self.deferred_persistent_releases
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(handle);
        true
    }

    pub(crate) fn take_persistent_releases(&self) -> Vec<u64> {
        std::mem::take(
            &mut *self
                .deferred_persistent_releases
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    pub(crate) fn queue_foreign_reference_release(&self, handle: u64) -> bool {
        if !self.alive.load(Ordering::Acquire) {
            return false;
        }
        self.deferred_foreign_reference_releases
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(handle);
        true
    }

    pub(crate) fn take_foreign_reference_releases(&self) -> Vec<u64> {
        std::mem::take(
            &mut *self
                .deferred_foreign_reference_releases
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    pub(crate) fn mark_dead(&self) {
        self.alive.store(false, Ordering::Release);
    }
}
