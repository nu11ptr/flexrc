use core::cell::Cell;
use core::marker::PhantomData;
#[cfg(all(not(loom), feature = "track_threads"))]
use core::sync::atomic::AtomicUsize;
#[cfg(not(loom))]
use core::sync::atomic::{fence, AtomicU32, Ordering};
#[cfg(all(loom, feature = "track_threads"))]
use loom::sync::atomic::AtomicUsize;
#[cfg(loom)]
use loom::sync::atomic::{fence, AtomicU32, Ordering};

use static_assertions::{assert_eq_align, assert_eq_size, assert_impl_all, assert_not_impl_any};

use crate::algorithm::abort;
#[cfg(feature = "track_threads")]
use crate::algorithm::hybrid_threads::THREAD_ID;
use crate::{Algorithm, FlexRc, FlexRcInner, LocalMode, SharedMode};

// NOTE: It is not clear to me why, but with cfg(loom) the size jumps to 128-bits for both.
#[cfg(all(not(loom), not(feature = "track_threads")))]
assert_eq_size!(HybridMeta<LocalMode>, u64);
#[cfg(all(not(loom), not(feature = "track_threads")))]
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
const SHARED_COUNT_MASK: u32 = u32::MAX >> 2;
const MAX_SHARED_COUNT: u32 = SHARED_COUNT_MASK;
const SHARED_OVERFLOW: u32 = SHARED_COUNT_MASK + 1;
// Top bit of shared counter signifies local present (or not)
const LOCAL_PRESENT: u32 = (u32::MAX >> 1) + 1;
// All bits set except top
const CLEAR_LOCAL: u32 = u32::MAX >> 1;

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

#[inline(always)]
fn abort_on_shared_overflow(old: u32) {
    if old & SHARED_OVERFLOW != 0 || old & SHARED_COUNT_MASK == MAX_SHARED_COUNT {
        abort()
    }
}

impl HybridMeta<LocalMode> {
    #[inline(always)]
    fn retain_shared(&self) {
        let old = self.shared_count.fetch_add(1, Ordering::Relaxed);
        abort_on_shared_overflow(old);
    }

    #[inline(always)]
    fn release_local(&self) -> bool {
        let old = self.local_count.get();

        if old == 0 {
            abort()
        }

        self.local_count.set(old - 1);

        if old == 1 {
            let old_shared = self.shared_count.fetch_and(CLEAR_LOCAL, Ordering::Release);

            if old_shared == LOCAL_PRESENT {
                fence(Ordering::Acquire);
                true
            } else {
                false
            }
        } else {
            false
        }
    }
}

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
        self.release_local()
    }

    #[inline]
    unsafe fn try_into_other<T: ?Sized>(
        inner: *mut LocalInner<T>,
    ) -> Result<*mut SharedInner<T>, *mut LocalInner<T>> {
        // SAFETY: We are accessing the correct variant for this type and we know the layout.
        let metadata = unsafe { &(*inner).metadata };

        // Safety: These are literally the same type - we invented the `SharedMode` and `LocalMode` tags
        // to FORCE new types where there wouldn't otherwise be so this is safe to cast
        let inner = inner as *mut SharedInner<T>;

        metadata.retain_shared();
        debug_assert!(!metadata.release_local());

        Ok(inner)
    }

    #[inline]
    unsafe fn try_to_other<T: ?Sized>(
        inner: *mut LocalInner<T>,
    ) -> Result<*mut SharedInner<T>, *mut LocalInner<T>> {
        // SAFETY: We are accessing the correct variant for this type and we know the layout.
        let metadata = unsafe { &(*inner).metadata };

        // Safety: These are literally the same type - we invented the `SharedMode` and `LocalMode` tags
        // to FORCE new types where there wouldn't otherwise be so this is safe to cast
        let inner = inner as *mut SharedInner<T>;

        metadata.retain_shared();

        Ok(inner)
    }
}

pub type SharedHybridRc<T> = FlexRc<HybridMeta<SharedMode>, HybridMeta<LocalMode>, T>;

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these marked to be safe
unsafe impl<T: ?Sized + Send + Sync> Send for SharedHybridRc<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for SharedHybridRc<T> {}

impl HybridMeta<SharedMode> {
    #[inline(always)]
    fn retain_shared(&self) {
        let old = self.shared_count.fetch_add(1, Ordering::Relaxed);
        abort_on_shared_overflow(old);
    }

    #[inline(always)]
    fn release_shared(&self) -> bool {
        // If the value was 1 previously, that means LOCAL_PRESENT wasn't set which means this
        // is the last remaining counter
        if self.shared_count.fetch_sub(1, Ordering::Release) == 1 {
            fence(Ordering::Acquire);
            true
        } else {
            false
        }
    }

    #[inline(always)]
    fn retain_local(&self) {
        let old = self.local_count.get();

        if old == MAX_LOCAL_COUNT {
            abort()
        }

        self.local_count.set(old + 1);
    }
}

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
        self.retain_shared();
    }

    #[inline(always)]
    fn drop(&self) -> bool {
        self.release_shared()
    }

    #[cfg(feature = "track_threads")]
    #[inline]
    unsafe fn try_into_other<T: ?Sized>(
        inner: *mut SharedInner<T>,
    ) -> Result<*mut LocalInner<T>, *mut SharedInner<T>> {
        // SAFETY: We are accessing the correct variant for this type and we know the layout.
        let metadata = unsafe { &(*inner).metadata };
        let thread_id = THREAD_ID.with(|thread_id| thread_id.0);

        // Spinlock to ensure only one thread can access this at a time
        let old_thread_id = loop {
            let old_thread_id = metadata
                .thread_id
                .fetch_or(THREAD_ID_LOCKED, Ordering::Acquire);

            // If we obtained lock than old value would have lock bit unset
            if old_thread_id < THREAD_ID_LOCKED {
                break old_thread_id;
            }
            std::hint::spin_loop();
        };

        // Try and make this thread into the local one by setting LOCAL_PRESENT bit.
        let old_shared_count = metadata
            .shared_count
            .fetch_or(LOCAL_PRESENT, Ordering::AcqRel);

        // If we are the local thread OR there is no local thread
        if thread_id == old_thread_id || old_shared_count < LOCAL_PRESENT {
            metadata.retain_local();
            metadata.release_shared();

            // Store our thread ID which also acts to release the spinlock
            metadata.thread_id.store(thread_id, Ordering::Release);

            // Safety: These are literally the same type - we invented the `SharedMode` and `LocalMode` tags
            // to FORCE new types where there wouldn't otherwise be so this is safe to cast
            let inner = inner as *mut LocalInner<T>;

            Ok(inner)
        } else {
            // Release spinlock and return error
            metadata
                .thread_id
                .fetch_and(THREAD_ID_UNLOCKED, Ordering::Release);
            Err(inner)
        }
    }

    #[cfg(not(feature = "track_threads"))]
    #[inline]
    unsafe fn try_into_other<T: ?Sized>(
        inner: *mut SharedInner<T>,
    ) -> Result<*mut LocalInner<T>, *mut SharedInner<T>> {
        // SAFETY: We are accessing the correct variant for this type and we know the layout.
        let metadata = unsafe { &(*inner).metadata };

        // Try and make this thread into the local one by setting LOCAL_PRESENT bit. If old value
        // is less than LOCAL_PRESENT we know it wasn't previously set (NOTE: Without tracking and
        // comparing a thread ID field it means we can only call this once and it will fail on
        // successive invocations, even when called from the proper thread)
        if metadata
            .shared_count
            .fetch_or(LOCAL_PRESENT, Ordering::AcqRel)
            < LOCAL_PRESENT
        {
            metadata.retain_local();
            metadata.release_shared();

            // Safety: These are literally the same type - we invented the `SharedMode` and `LocalMode` tags
            // to FORCE new types where there wouldn't otherwise be so this is safe to cast
            let inner = inner as *mut LocalInner<T>;

            Ok(inner)
        } else {
            Err(inner)
        }
    }

    #[cfg(feature = "track_threads")]
    #[inline]
    unsafe fn try_to_other<T: ?Sized>(
        inner: *mut SharedInner<T>,
    ) -> Result<*mut LocalInner<T>, *mut SharedInner<T>> {
        // SAFETY: We are accessing the correct variant for this type and we know the layout.
        let metadata = unsafe { &(*inner).metadata };

        let thread_id = THREAD_ID.with(|thread_id| thread_id.0);

        let old_thread_id = loop {
            let old_thread_id = metadata
                .thread_id
                .fetch_or(THREAD_ID_LOCKED, Ordering::Acquire);

            if old_thread_id < THREAD_ID_LOCKED {
                break old_thread_id;
            }
            std::hint::spin_loop();
        };

        let old_shared_count = metadata
            .shared_count
            .fetch_or(LOCAL_PRESENT, Ordering::AcqRel);

        if thread_id == old_thread_id || old_shared_count < LOCAL_PRESENT {
            metadata.retain_local();
            metadata.thread_id.store(thread_id, Ordering::Release);

            Ok(inner as *mut LocalInner<T>)
        } else {
            metadata
                .thread_id
                .fetch_and(THREAD_ID_UNLOCKED, Ordering::Release);
            Err(inner)
        }
    }

    #[cfg(not(feature = "track_threads"))]
    #[inline]
    unsafe fn try_to_other<T: ?Sized>(
        inner: *mut SharedInner<T>,
    ) -> Result<*mut LocalInner<T>, *mut SharedInner<T>> {
        // SAFETY: We are accessing the correct variant for this type and we know the layout.
        let metadata = unsafe { &(*inner).metadata };

        if metadata
            .shared_count
            .fetch_or(LOCAL_PRESENT, Ordering::AcqRel)
            < LOCAL_PRESENT
        {
            metadata.retain_local();

            Ok(inner as *mut LocalInner<T>)
        } else {
            Err(inner)
        }
    }
}
