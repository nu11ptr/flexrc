use core::cell::Cell;
use core::marker::PhantomData;
#[cfg(all(not(loom), feature = "small_counters"))]
use core::sync::atomic::AtomicU32;
#[cfg(all(not(loom), not(feature = "small_counters")))]
use core::sync::atomic::AtomicUsize;
#[cfg(not(loom))]
use core::sync::atomic::Ordering;
#[cfg(all(loom, feature = "small_counters"))]
use loom::sync::atomic::AtomicU32;
#[cfg(all(loom, not(feature = "small_counters")))]
use loom::sync::atomic::AtomicUsize;
#[cfg(loom)]
use loom::sync::atomic::Ordering;

use static_assertions::{assert_eq_align, assert_eq_size, assert_impl_all, assert_not_impl_any};

use crate::algorithm::abort;
use crate::{Algorithm, FlexRc, FlexRcInner, LocalMode, SharedMode};

// NOTE: It is not clear to me why, but with cfg(loom) the size jumps to 128-bits for both.
#[cfg(all(not(loom), feature = "small_counters"))]
assert_eq_size!(HybridMeta<LocalMode>, u64);
#[cfg(all(not(loom), feature = "small_counters"))]
assert_eq_size!(HybridMeta<SharedMode>, u64);
#[cfg(all(not(loom), not(feature = "small_counters")))]
assert_eq_size!(HybridMeta<LocalMode>, [usize; 2]);
#[cfg(all(not(loom), not(feature = "small_counters")))]
assert_eq_size!(HybridMeta<SharedMode>, [usize; 2]);

assert_eq_size!(HybridMeta<LocalMode>, HybridMeta<SharedMode>);
assert_eq_align!(HybridMeta<LocalMode>, HybridMeta<SharedMode>);
assert_eq_size!(LocalInner<usize>, SharedInner<usize>);
assert_eq_align!(LocalInner<usize>, SharedInner<usize>);
assert_eq_size!(HybridRc<usize>, HybridArc<usize>);
assert_eq_align!(HybridRc<usize>, HybridArc<usize>);

assert_impl_all!(HybridArc<usize>: Send, Sync);
assert_not_impl_any!(HybridRc<usize>: Send, Sync);

#[cfg(feature = "small_counters")]
pub(in crate::algorithm) type Count = u32;
#[cfg(not(feature = "small_counters"))]
pub(in crate::algorithm) type Count = usize;

#[cfg(feature = "small_counters")]
pub(in crate::algorithm) type AtomicCount = AtomicU32;
#[cfg(not(feature = "small_counters"))]
pub(in crate::algorithm) type AtomicCount = AtomicUsize;

// Entire counter is usable for local.
pub(in crate::algorithm) const MAX_LOCAL_COUNT: Count = Count::MAX;
// Save top bit for "local present" bit and second to top for overflow
pub(in crate::algorithm) const SHARED_COUNT_MASK: Count = Count::MAX >> 2;
pub(in crate::algorithm) const MAX_SHARED_COUNT: Count = SHARED_COUNT_MASK;
pub(in crate::algorithm) const SHARED_OVERFLOW: Count = SHARED_COUNT_MASK + 1;
// Top bit of shared counter signifies local present (or not)
pub(in crate::algorithm) const LOCAL_PRESENT: Count = (Count::MAX >> 1) + 1;
// All bits set except top
pub(in crate::algorithm) const CLEAR_LOCAL: Count = Count::MAX >> 1;

#[repr(C)]
pub struct HybridMeta<MODE> {
    local_count: Cell<Count>,
    shared_count: AtomicCount,
    phantom: PhantomData<MODE>,
}

pub type HybridRc<T> = FlexRc<HybridMeta<LocalMode>, HybridMeta<SharedMode>, T>;

type LocalInner<T> = FlexRcInner<HybridMeta<LocalMode>, HybridMeta<SharedMode>, T>;
type SharedInner<T> = FlexRcInner<HybridMeta<SharedMode>, HybridMeta<LocalMode>, T>;

#[inline(always)]
pub(in crate::algorithm) fn abort_on_shared_overflow(old: Count) {
    if old & SHARED_OVERFLOW != 0 || old & SHARED_COUNT_MASK == MAX_SHARED_COUNT {
        abort()
    }
}

#[inline(always)]
pub(in crate::algorithm) fn retain_shared(shared_count: &AtomicCount) {
    let old = shared_count.fetch_add(1, Ordering::Relaxed);
    abort_on_shared_overflow(old);
}

#[inline(always)]
pub(in crate::algorithm) fn release_shared(shared_count: &AtomicCount) -> bool {
    // If the value was 1 previously, that means LOCAL_PRESENT wasn't set which means this
    // is the last remaining counter
    if shared_count.fetch_sub(1, Ordering::Release) == 1 {
        crate::algorithm::acquire_after_release!(shared_count);
        true
    } else {
        false
    }
}

#[inline(always)]
pub(in crate::algorithm) fn retain_local(local_count: &Cell<Count>) {
    let old = local_count.get();

    if old == MAX_LOCAL_COUNT {
        abort()
    }

    local_count.set(old + 1);
}

#[inline(always)]
pub(in crate::algorithm) fn release_local(
    local_count: &Cell<Count>,
    shared_count: &AtomicCount,
) -> bool {
    let old = local_count.get();

    if old == 0 {
        abort()
    }

    local_count.set(old - 1);

    if old == 1 {
        let old_shared = shared_count.fetch_and(CLEAR_LOCAL, Ordering::Release);

        if old_shared == LOCAL_PRESENT {
            crate::algorithm::acquire_after_release!(shared_count);
            true
        } else {
            false
        }
    } else {
        false
    }
}

impl HybridMeta<LocalMode> {
    #[inline(always)]
    fn retain_shared(&self) {
        retain_shared(&self.shared_count);
    }

    #[inline(always)]
    fn release_local(&self) -> bool {
        release_local(&self.local_count, &self.shared_count)
    }
}

impl Algorithm<HybridMeta<LocalMode>, HybridMeta<SharedMode>> for HybridMeta<LocalMode> {
    #[inline]
    fn create() -> Self {
        Self {
            local_count: Cell::new(1),
            shared_count: AtomicCount::new(LOCAL_PRESENT),
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

pub type HybridArc<T> = FlexRc<HybridMeta<SharedMode>, HybridMeta<LocalMode>, T>;

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these marked to be safe
unsafe impl<T: ?Sized + Send + Sync> Send for HybridArc<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for HybridArc<T> {}

impl HybridMeta<SharedMode> {
    #[inline(always)]
    fn retain_shared(&self) {
        retain_shared(&self.shared_count);
    }

    #[inline(always)]
    fn release_shared(&self) -> bool {
        release_shared(&self.shared_count)
    }

    #[inline(always)]
    fn retain_local(&self) {
        retain_local(&self.local_count);
    }
}

impl Algorithm<HybridMeta<SharedMode>, HybridMeta<LocalMode>> for HybridMeta<SharedMode> {
    #[inline]
    fn create() -> Self {
        Self {
            local_count: Cell::new(0),
            shared_count: AtomicCount::new(1),
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
