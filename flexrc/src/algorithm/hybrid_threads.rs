#![cfg(feature = "track_threads")]

use core::cell::Cell;
use core::marker::PhantomData;
#[cfg(not(loom))]
use core::sync::atomic::{AtomicUsize, Ordering};
use core::sync::atomic::{AtomicUsize as GlobalAtomicUsize, Ordering as GlobalOrdering};
#[cfg(loom)]
use loom::sync::atomic::{AtomicUsize, Ordering};

use static_assertions::{assert_eq_align, assert_eq_size, assert_impl_all, assert_not_impl_any};

use crate::algorithm::abort;
use crate::algorithm::hybrid::{
    release_local, release_shared, retain_local, retain_shared, AtomicCount, Count, LOCAL_PRESENT,
};
use crate::{Algorithm, FlexRc, FlexRcInner, LocalMode, SharedMode};

const MAX_THREADS: usize = usize::MAX >> 1;
const THREAD_ID_LOCKED: usize = (usize::MAX >> 1) + 1;
const THREAD_ID_UNLOCKED: usize = usize::MAX >> 1;

static NEXT_THREAD_ID: GlobalAtomicUsize = GlobalAtomicUsize::new(1);

#[cfg(not(loom))]
thread_local! { static THREAD_ID: ThreadId = ThreadId::new() }

#[cfg(loom)]
static THREAD_ID: loom::thread::LocalKey<ThreadId> = loom::thread::LocalKey {
    init: ThreadId::new,
    _p: PhantomData,
};

// *** Thread Id ***

struct ThreadId(usize);

impl ThreadId {
    fn new() -> Self {
        let id = NEXT_THREAD_ID.fetch_add(1, GlobalOrdering::Relaxed);

        if id >= MAX_THREADS {
            abort()
        }

        Self(id)
    }
}

#[inline]
fn current_thread_id() -> usize {
    THREAD_ID.with(|thread_id| thread_id.0)
}

// *** ThreadHybridMeta ***

#[repr(C)]
pub struct ThreadHybridMeta<MODE> {
    thread_id: AtomicUsize,
    local_count: Cell<Count>,
    shared_count: AtomicCount,
    phantom: PhantomData<MODE>,
}

pub type ThreadRc<T> = FlexRc<ThreadHybridMeta<LocalMode>, ThreadHybridMeta<SharedMode>, T>;
pub type ThreadArc<T> = FlexRc<ThreadHybridMeta<SharedMode>, ThreadHybridMeta<LocalMode>, T>;

type LocalInner<T> = FlexRcInner<ThreadHybridMeta<LocalMode>, ThreadHybridMeta<SharedMode>, T>;
type SharedInner<T> = FlexRcInner<ThreadHybridMeta<SharedMode>, ThreadHybridMeta<LocalMode>, T>;

assert_eq_size!(ThreadHybridMeta<LocalMode>, ThreadHybridMeta<SharedMode>);
assert_eq_align!(ThreadHybridMeta<LocalMode>, ThreadHybridMeta<SharedMode>);
assert_eq_size!(LocalInner<usize>, SharedInner<usize>);
assert_eq_align!(LocalInner<usize>, SharedInner<usize>);
assert_eq_size!(ThreadRc<usize>, ThreadArc<usize>);
assert_eq_align!(ThreadRc<usize>, ThreadArc<usize>);
#[cfg(all(not(loom), not(feature = "small_counters")))]
assert_eq_size!(ThreadHybridMeta<LocalMode>, [usize; 3]);
#[cfg(all(not(loom), not(feature = "small_counters")))]
assert_eq_size!(ThreadHybridMeta<SharedMode>, [usize; 3]);
#[cfg(all(not(loom), feature = "small_counters", target_pointer_width = "64"))]
assert_eq_size!(ThreadHybridMeta<LocalMode>, [usize; 2]);
#[cfg(all(not(loom), feature = "small_counters", target_pointer_width = "64"))]
assert_eq_size!(ThreadHybridMeta<SharedMode>, [usize; 2]);
#[cfg(all(not(loom), feature = "small_counters", target_pointer_width = "32"))]
assert_eq_size!(ThreadHybridMeta<LocalMode>, [usize; 3]);
#[cfg(all(not(loom), feature = "small_counters", target_pointer_width = "32"))]
assert_eq_size!(ThreadHybridMeta<SharedMode>, [usize; 3]);

assert_impl_all!(ThreadArc<usize>: Send, Sync);
assert_not_impl_any!(ThreadRc<usize>: Send, Sync);

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these marked to be safe
unsafe impl<T: ?Sized + Send + Sync> Send for ThreadArc<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for ThreadArc<T> {}

impl ThreadHybridMeta<LocalMode> {
    #[inline(always)]
    fn retain_shared(&self) {
        retain_shared(&self.shared_count);
    }

    #[inline(always)]
    fn release_local(&self) -> bool {
        release_local(&self.local_count, &self.shared_count)
    }

    #[inline(always)]
    fn retain_local(&self) {
        retain_local(&self.local_count);
    }
}

impl Algorithm<ThreadHybridMeta<LocalMode>, ThreadHybridMeta<SharedMode>>
    for ThreadHybridMeta<LocalMode>
{
    #[inline]
    fn create() -> Self {
        Self {
            thread_id: AtomicUsize::new(current_thread_id()),
            local_count: Cell::new(1),
            shared_count: AtomicCount::new(LOCAL_PRESENT),
            phantom: PhantomData,
        }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.local_count.get() == 1 && self.shared_count.load(Ordering::Acquire) == LOCAL_PRESENT
    }

    #[inline(always)]
    fn clone(&self) {
        self.retain_local();
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

impl ThreadHybridMeta<SharedMode> {
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

    #[inline]
    fn lock_thread_id(&self) -> usize {
        loop {
            let old_thread_id = self.thread_id.fetch_or(THREAD_ID_LOCKED, Ordering::Acquire);

            if old_thread_id < THREAD_ID_LOCKED {
                return old_thread_id;
            }

            std::hint::spin_loop();
        }
    }

    #[inline(always)]
    fn unlock_thread_id(&self) {
        self.thread_id
            .fetch_and(THREAD_ID_UNLOCKED, Ordering::Release);
    }

    #[inline(always)]
    fn unlock_with_thread_id(&self, thread_id: usize) {
        self.thread_id.store(thread_id, Ordering::Release);
    }
}

impl Algorithm<ThreadHybridMeta<SharedMode>, ThreadHybridMeta<LocalMode>>
    for ThreadHybridMeta<SharedMode>
{
    #[inline]
    fn create() -> Self {
        Self {
            // No thread ID set yet.
            thread_id: AtomicUsize::new(0),
            local_count: Cell::new(0),
            shared_count: AtomicCount::new(1),
            phantom: PhantomData,
        }
    }

    #[inline]
    fn is_unique(&self) -> bool {
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
        let thread_id = current_thread_id();
        let old_thread_id = metadata.lock_thread_id();

        let old_shared_count = metadata
            .shared_count
            .fetch_or(LOCAL_PRESENT, Ordering::AcqRel);

        if thread_id == old_thread_id || old_shared_count < LOCAL_PRESENT {
            metadata.retain_local();
            metadata.release_shared();
            metadata.unlock_with_thread_id(thread_id);

            Ok(inner as *mut LocalInner<T>)
        } else {
            metadata.unlock_thread_id();
            Err(inner)
        }
    }

    #[inline]
    unsafe fn try_to_other<T: ?Sized>(
        inner: *mut SharedInner<T>,
    ) -> Result<*mut LocalInner<T>, *mut SharedInner<T>> {
        // SAFETY: We are accessing the correct variant for this type and we know the layout.
        let metadata = unsafe { &(*inner).metadata };
        let thread_id = current_thread_id();
        let old_thread_id = metadata.lock_thread_id();

        let old_shared_count = metadata
            .shared_count
            .fetch_or(LOCAL_PRESENT, Ordering::AcqRel);

        if thread_id == old_thread_id || old_shared_count < LOCAL_PRESENT {
            metadata.retain_local();
            metadata.unlock_with_thread_id(thread_id);

            Ok(inner as *mut LocalInner<T>)
        } else {
            metadata.unlock_thread_id();
            Err(inner)
        }
    }
}
