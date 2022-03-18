use core::cell::Cell;
use core::sync::atomic;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::Algorithm;

const MAX_LOCAL_COUNT: usize = usize::MAX;
// Allow some room for overflow
const MAX_SHARED_COUNT: usize = usize::MAX >> 1;

#[repr(C)]
struct LocalMeta {
    count: Cell<usize>,
}

impl Algorithm for LocalMeta {
    #[inline]
    fn create() -> Self {
        Self {
            count: Cell::new(1),
        }
    }

    #[inline(always)]
    fn clone(&self) {
        let old = self.count.get();

        // TODO: This check adds 15-16% clone overhead - truly needed?
        if old == MAX_LOCAL_COUNT {
            // Abort not available on no_std
            panic!("Reference count overflow");
        }

        self.count.set(old + 1);
    }

    #[inline(always)]
    fn drop(&self) -> bool {
        self.count.set(self.count.get() - 1);
        self.count.get() == 0
    }
}

#[repr(C)]
struct SharedMeta {
    count: AtomicUsize,
}

impl Algorithm for SharedMeta {
    #[inline]
    fn create() -> Self {
        Self {
            count: AtomicUsize::new(1),
        }
    }

    #[inline(always)]
    fn clone(&self) {
        let old = self.count.fetch_add(1, Ordering::Relaxed);

        if old > MAX_SHARED_COUNT {
            // Abort not available on no_std
            panic!("Reference count overflow");
        }
    }

    #[inline(always)]
    fn drop(&self) -> bool {
        if self.count.fetch_sub(1, Ordering::Release) == 1 {
            atomic::fence(Ordering::Acquire);
            true
        } else {
            false
        }
    }
}

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these marked to be safe
unsafe impl Send for SharedMeta {}
unsafe impl Sync for SharedMeta {}
