use core::cell::Cell;
use core::marker::PhantomData;
use core::ptr::NonNull;
use core::sync::atomic;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::{Algorithm, FlexRc, FlexRcInner};

// Entire counter is usable for local
const MAX_LOCAL_COUNT: u32 = u32::MAX;
// Save top bit for "local present" bit and second to top for overflow
const MAX_SHARED_COUNT: u32 = u32::MAX >> 2;
// Top bit of shared counter signifies local present (or not)
const LOCAL_PRESENT: u32 = (u32::MAX >> 1) + 1;
// All bits set except top
const CLEAR_LOCAL: u32 = u32::MAX >> 1;

pub struct LocalMode;
pub struct SharedMode;

#[repr(C)]
pub struct HybridMeta<MODE> {
    local_count: Cell<u32>,
    shared_count: AtomicU32,
    phantom: PhantomData<MODE>,
}

pub type HybridLocalRc<T> = FlexRc<HybridMeta<LocalMode>, HybridMeta<SharedMode>, T>;

type LocalInner<T> = FlexRcInner<HybridMeta<LocalMode>, HybridMeta<SharedMode>, T>;
type SharedInner<T> = FlexRcInner<HybridMeta<SharedMode>, HybridMeta<LocalMode>, T>;

impl<T> Algorithm<HybridMeta<LocalMode>, HybridMeta<SharedMode>, T> for HybridMeta<LocalMode> {
    #[inline]
    fn create() -> Self {
        Self {
            local_count: Cell::new(1),
            shared_count: AtomicU32::new(LOCAL_PRESENT),
            phantom: PhantomData,
        }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        // if LOCAL_PRESENT is shared counter value that means only high bit is set and shared count == 0
        // Long discussion on why this ordering is required: https://github.com/servo/servo/issues/21186
        self.local_count.get() == 1 && self.shared_count.load(Ordering::Acquire) == LOCAL_PRESENT
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
            // FIXME: Verify correct Ordering
            let old = self.shared_count.fetch_and(CLEAR_LOCAL, Ordering::Release);

            // If the value is just `LOCAL_PRESENT` that means only the top bit was set and the
            // shared counter was zero
            old == LOCAL_PRESENT
        } else {
            false
        }
    }

    #[inline]
    fn try_into_other(&self) -> bool {
        // This is always allowed
        true
    }

    #[inline]
    fn try_to_other(&self) -> bool {
        // This is always allowed
        true
    }

    #[inline]
    fn convert(inner: NonNull<LocalInner<T>>) -> NonNull<SharedInner<T>> {
        // Safety: These are literally the same types - we just use the `LocalMode` / `SharedMode`
        // as a dummy type to force different types - totally safe
        inner.cast()
    }
}

pub type HybridSharedRc<T> = FlexRc<HybridMeta<SharedMode>, HybridMeta<LocalMode>, T>;

impl<T> Algorithm<HybridMeta<SharedMode>, HybridMeta<LocalMode>, T> for HybridMeta<SharedMode> {
    #[inline]
    fn create() -> Self {
        Self {
            local_count: Cell::new(0),
            shared_count: AtomicU32::new(1),
            phantom: PhantomData,
        }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        // If set to 1, that means there are no local mode type left and this is last shared
        // Long discussion on why this ordering is required: https://github.com/servo/servo/issues/21186
        self.shared_count.load(Ordering::Acquire) == 1
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

    #[inline]
    fn try_into_other(&self) -> bool {
        // Try and make this thread into the local one by setting LOCAL_PRESENT bit. If old value
        // is less than LOCAL_PRESENT we know it wasn't previously set
        // FIXME: Verify correct Ordering
        self.shared_count.fetch_or(LOCAL_PRESENT, Ordering::Acquire) < LOCAL_PRESENT
    }

    #[inline]
    fn try_to_other(&self) -> bool {
        <Self as Algorithm<HybridMeta<SharedMode>, HybridMeta<LocalMode>, T>>::try_into_other(self)
    }

    #[inline]
    fn convert(inner: NonNull<SharedInner<T>>) -> NonNull<LocalInner<T>> {
        // Safety: These are literally the same types - we just use the `LocalMode` / `SharedMode`
        // as a dummy type to force different types - totally safe
        inner.cast()
    }
}

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these marked to be safe
unsafe impl Send for HybridMeta<SharedMode> {}
unsafe impl Sync for HybridMeta<SharedMode> {}
