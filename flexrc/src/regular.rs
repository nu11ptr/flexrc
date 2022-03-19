use core::cell::Cell;
use core::ptr::NonNull;
use core::sync::atomic;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{Algorithm, FlexRc, FlexRcInner};

const MAX_LOCAL_COUNT: usize = usize::MAX;
// Allow some room for overflow
const MAX_SHARED_COUNT: usize = usize::MAX >> 1;

#[repr(C)]
pub struct LocalMeta {
    count: Cell<usize>,
}

pub type LocalRc<T> = FlexRc<LocalMeta, SharedMeta, T>;

impl<T> Algorithm<LocalMeta, SharedMeta, T> for LocalMeta {
    #[inline]
    fn create() -> Self {
        Self {
            count: Cell::new(1),
        }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.count.get() == 1
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

    #[inline]
    fn try_into_other(&self) -> bool {
        // Only if this is the last reference
        <Self as Algorithm<LocalMeta, SharedMeta, T>>::is_unique(self)
    }

    #[inline]
    fn try_to_other(&self) -> bool {
        // This is never safe to do
        false
    }

    fn convert(
        inner: NonNull<FlexRcInner<LocalMeta, SharedMeta, T>>,
    ) -> NonNull<FlexRcInner<SharedMeta, LocalMeta, T>> {
        // TODO: Safety statement
        inner.cast()
    }
}

#[repr(C)]
pub struct SharedMeta {
    count: AtomicUsize,
}

pub type SharedRc<T> = FlexRc<SharedMeta, LocalMeta, T>;

impl<T> Algorithm<SharedMeta, LocalMeta, T> for SharedMeta {
    #[inline]
    fn create() -> Self {
        Self {
            count: AtomicUsize::new(1),
        }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        // Long discussion on why this ordering is required: https://github.com/servo/servo/issues/21186
        self.count.load(Ordering::Acquire) == 1
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

    #[inline]
    fn try_into_other(&self) -> bool {
        // Only if this is the last reference
        <Self as Algorithm<SharedMeta, LocalMeta, T>>::is_unique(self)
    }

    #[inline]
    fn try_to_other(&self) -> bool {
        // This is never safe to do
        false
    }

    fn convert(
        inner: NonNull<FlexRcInner<SharedMeta, LocalMeta, T>>,
    ) -> NonNull<FlexRcInner<LocalMeta, SharedMeta, T>> {
        // TODO: Safety
        inner.cast()
    }
}

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these marked to be safe
unsafe impl Send for SharedMeta {}
unsafe impl Sync for SharedMeta {}
