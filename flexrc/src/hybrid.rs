use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::Algorithm;

// Entire counter is usable for local
const MAX_LOCAL_COUNT: u32 = u32::MAX;
// Save top bit for "local present" bit and second to top for overflow
const MAX_SHARED_COUNT: u32 = u32::MAX >> 2;
// Top bit of shared counter signifies local present (or not)
const LOCAL_PRESENT: u32 = (u32::MAX >> 1) + 1;
// All bits set except top
const CLEAR_LOCAL: u32 = u32::MAX >> 1;

struct LocalMode;
struct SharedMode;

#[repr(C)]
struct HybridMeta<MODE> {
    local_count: Cell<u32>,
    shared_count: AtomicU32,
    phantom: PhantomData<MODE>,
}

impl Algorithm for HybridMeta<LocalMode> {
    #[inline]
    fn create() -> Self {
        Self {
            local_count: Cell::new(1),
            shared_count: AtomicU32::new(LOCAL_PRESENT),
            phantom: PhantomData,
        }
    }

    #[inline(always)]
    fn clone(&self) {
        let old = self.local_count.get();

        // TODO: This check adds 15-16% clone overhead - truly needed?
        if old == MAX_LOCAL_COUNT {
            // Abort not available on no_std
            panic!("Reference count overflow");
        }
        self.local_count.set(old + 1);
    }

    #[inline(always)]
    fn drop(&self) -> bool {
        self.local_count.set(self.local_count.get() - 1);

        if self.local_count.get() == 0 {
            // TODO: Is ordering 'Release' what we need?
            let old = self.shared_count.fetch_and(CLEAR_LOCAL, Ordering::Release);

            // If the value is just `LOCAL_PRESENT` that means only the top bit was set and the
            // shared counter was zero
            old == LOCAL_PRESENT
        } else {
            false
        }
    }
}

impl Algorithm for HybridMeta<SharedMode> {
    #[inline]
    fn create() -> Self {
        Self {
            local_count: Cell::new(0),
            shared_count: AtomicU32::new(1),
            phantom: PhantomData,
        }
    }

    #[inline(always)]
    fn clone(&self) {
        let old = self.shared_count.fetch_add(1, Ordering::Relaxed);

        if old > MAX_SHARED_COUNT {
            // Abort not available on no_std
            panic!("Reference count overflow");
        }
    }

    #[inline(always)]
    fn drop(&self) -> bool {
        // If the value was 1 previously, that means LOCAL_PRESENT wasn't set which means this
        // is the last remaining counter
        if self.shared_count.fetch_sub(1, Ordering::Release) == 1 {
            atomic::fence(Ordering::Acquire);
            true
        } else {
            false
        }
    }
}

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these marked to be safe
unsafe impl Send for HybridMeta<SharedMode> {}
unsafe impl Sync for HybridMeta<SharedMode> {}
