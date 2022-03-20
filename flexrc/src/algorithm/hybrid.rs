use core::cell::Cell;
use core::marker::PhantomData;
use core::sync::atomic;
#[cfg(feature = "track_threads")]
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::{AtomicU32, Ordering};

use static_assertions::{assert_eq_align, assert_eq_size, assert_impl_all, assert_not_impl_any};

use crate::algorithm::abort;
#[cfg(feature = "track_threads")]
use crate::algorithm::hybrid_threads::THREAD_ID;
use crate::{Algorithm, FlexRc, FlexRcInner};

#[cfg(not(feature = "track_threads"))]
assert_eq_size!(HybridMeta<LocalMode>, u64);
#[cfg(not(feature = "track_threads"))]
assert_eq_size!(HybridMeta<SharedMode>, u64);

assert_eq_size!(HybridMeta<LocalMode>, HybridMeta<SharedMode>);
assert_eq_align!(HybridMeta<LocalMode>, HybridMeta<SharedMode>);
assert_eq_size!(LocalInner<usize>, SharedInner<usize>);
assert_eq_align!(LocalInner<usize>, SharedInner<usize>);
assert_eq_size!(LocalHybridRc<usize>, SharedHybridRc<usize>);
assert_eq_align!(LocalHybridRc<usize>, SharedHybridRc<usize>);

assert_impl_all!(SharedHybridRc<usize>: Send, Sync);
assert_not_impl_any!(LocalHybridRc<usize>: Send, Sync);

#[cfg(feature = "track_threads")]
const THREAD_ID_LOCKED: usize = (usize::MAX >> 1) + 1;
#[cfg(feature = "track_threads")]
const THREAD_ID_UNLOCKED: usize = usize::MAX >> 1;

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
    #[cfg(feature = "track_threads")]
    thread_id: AtomicUsize,
    local_count: Cell<u32>,
    shared_count: AtomicU32,
    phantom: PhantomData<MODE>,
}

pub type LocalHybridRc<T> = FlexRc<HybridMeta<LocalMode>, HybridMeta<SharedMode>, T>;

type LocalInner<T> = FlexRcInner<HybridMeta<LocalMode>, HybridMeta<SharedMode>, T>;
type SharedInner<T> = FlexRcInner<HybridMeta<SharedMode>, HybridMeta<LocalMode>, T>;

impl Algorithm<HybridMeta<LocalMode>, HybridMeta<SharedMode>> for HybridMeta<LocalMode> {
    #[inline]
    fn create() -> Self {
        Self {
            #[cfg(feature = "track_threads")]
            thread_id: AtomicUsize::new(THREAD_ID.with(|t| t.0)),
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
            abort()
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
    fn try_into_other<T: ?Sized>(
        &self,
        inner: *mut LocalInner<T>,
    ) -> Result<*mut SharedInner<T>, *mut LocalInner<T>> {
        // This is always allowed

        // Safety: These are literally the same type - we invented the `SharedMode` and `LocalMode` tags
        // to FORCE new types where there wouldn't otherwise be so this is safe to cast
        let inner = inner as *mut SharedInner<T>;

        // Since a) creating a new instance, not reusing b) using a diff ref counter field we now
        // need to force a clone
        // SAFETY: See above
        unsafe {
            (*inner).metadata.clone();
        }
        Ok(inner)
    }

    #[inline]
    fn try_to_other<T: ?Sized>(
        &self,
        inner: *mut LocalInner<T>,
    ) -> Result<*mut SharedInner<T>, *mut LocalInner<T>> {
        // Since we can always keep the original, both are the same
        self.try_into_other(inner)
    }
}

pub type SharedHybridRc<T> = FlexRc<HybridMeta<SharedMode>, HybridMeta<LocalMode>, T>;

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these marked to be safe
unsafe impl<T: Send + Sync> Send for SharedHybridRc<T> {}
unsafe impl<T: Send + Sync> Sync for SharedHybridRc<T> {}

impl Algorithm<HybridMeta<SharedMode>, HybridMeta<LocalMode>> for HybridMeta<SharedMode> {
    #[inline]
    fn create() -> Self {
        Self {
            #[cfg(feature = "track_threads")]
            // No thread ID set yet
            thread_id: AtomicUsize::new(0),
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
            abort()
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

    #[cfg(feature = "track_threads")]
    #[inline]
    fn try_into_other<T: ?Sized>(
        &self,
        inner: *mut SharedInner<T>,
    ) -> Result<*mut LocalInner<T>, *mut SharedInner<T>> {
        let thread_id = THREAD_ID.with(|thread_id| thread_id.0);

        // Spinlock to ensure only one thread can access this at a time
        let old_thread_id = loop {
            // FIXME: Verify correct Ordering
            let old_thread_id = self.thread_id.fetch_or(THREAD_ID_LOCKED, Ordering::Acquire);

            // If we obtained lock than old value would have lock bit unset
            if old_thread_id < THREAD_ID_LOCKED {
                break old_thread_id;
            }
            std::hint::spin_loop();
        };

        // Try and make this thread into the local one by setting LOCAL_PRESENT bit.
        // FIXME: Verify correct Ordering
        let old_shared_count = self.shared_count.fetch_or(LOCAL_PRESENT, Ordering::Acquire);

        // If we are the local thread OR there is no local thread
        if thread_id == old_thread_id || old_shared_count < LOCAL_PRESENT {
            // FIXME: Verify correct Ordering
            // Store our thread ID which also acts to release the spinlock
            self.thread_id.store(thread_id, Ordering::Release);

            // Safety: These are literally the same type - we invented the `SharedMode` and `LocalMode` tags
            // to FORCE new types where there wouldn't otherwise be so this is safe to cast
            let inner = inner as *mut LocalInner<T>;

            // Since a) creating a new instance, not reusing b) using a diff ref counter field we now
            // need to force a clone
            // SAFETY: See above
            unsafe {
                (*inner).metadata.clone();
            }
            Ok(inner)
        } else {
            // FIXME: Verify correct Ordering
            // Release spinlock and return error
            self.thread_id
                .fetch_and(THREAD_ID_UNLOCKED, Ordering::Release);
            Err(inner)
        }
    }

    #[cfg(not(feature = "track_threads"))]
    #[inline]
    fn try_into_other<T: ?Sized>(
        &self,
        inner: *mut SharedInner<T>,
    ) -> Result<*mut LocalInner<T>, *mut SharedInner<T>> {
        // Try and make this thread into the local one by setting LOCAL_PRESENT bit. If old value
        // is less than LOCAL_PRESENT we know it wasn't previously set (NOTE: Without tracking and
        // comparing a thread ID field it means we can only call this once and it will fail on
        // successive invocations, even when called from the proper thread)
        // FIXME: Verify correct Ordering
        if self.shared_count.fetch_or(LOCAL_PRESENT, Ordering::Acquire) < LOCAL_PRESENT {
            // Safety: These are literally the same type - we invented the `SharedMode` and `LocalMode` tags
            // to FORCE new types where there wouldn't otherwise be so this is safe to cast
            let inner = inner as *mut LocalInner<T>;

            // Since a) creating a new instance, not reusing b) using a diff ref counter field we now
            // need to force a clone
            // SAFETY: See above
            unsafe {
                (*inner).metadata.clone();
            }
            Ok(inner)
        } else {
            Err(inner)
        }
    }

    #[inline]
    fn try_to_other<T: ?Sized>(
        &self,
        inner: *mut SharedInner<T>,
    ) -> Result<*mut LocalInner<T>, *mut SharedInner<T>> {
        // Since we can always keep the original, both are the same
        self.try_into_other(inner)
    }
}
