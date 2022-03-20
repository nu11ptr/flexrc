use core::cell::Cell;
use core::sync::atomic;
use core::sync::atomic::{AtomicUsize, Ordering};

use static_assertions::{assert_eq_align, assert_eq_size, assert_impl_all, assert_not_impl_any};

use crate::algorithm::abort;
use crate::{Algorithm, FlexRc, FlexRcInner};

assert_eq_size!(LocalMeta, SharedMeta);
assert_eq_align!(LocalMeta, SharedMeta);
assert_eq_size!(LocalInner<usize>, SharedInner<usize>);
assert_eq_align!(LocalInner<usize>, SharedInner<usize>);
assert_eq_size!(LocalRc<usize>, SharedRc<usize>);
assert_eq_align!(LocalRc<usize>, SharedRc<usize>);

assert_impl_all!(SharedRc<usize>: Send, Sync);
assert_not_impl_any!(LocalRc<usize>: Send, Sync);

const MAX_LOCAL_COUNT: usize = usize::MAX;
// Allow some room for overflow
const MAX_SHARED_COUNT: usize = usize::MAX >> 1;

#[repr(C)]
pub struct LocalMeta {
    count: Cell<usize>,
}

pub type LocalRc<T> = FlexRc<LocalMeta, SharedMeta, T>;

type LocalInner<T> = FlexRcInner<LocalMeta, SharedMeta, T>;
type SharedInner<T> = FlexRcInner<SharedMeta, LocalMeta, T>;

impl Algorithm<LocalMeta, SharedMeta> for LocalMeta {
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
            abort()
        }

        self.count.set(old + 1);
    }

    #[inline(always)]
    fn drop(&self) -> bool {
        self.count.set(self.count.get() - 1);
        self.count.get() == 0
    }

    #[inline]
    fn try_into_other<T: ?Sized>(
        &self,
        inner: *mut LocalInner<T>,
    ) -> Result<*mut SharedInner<T>, *mut LocalInner<T>> {
        if self.is_unique() {
            // Safety:
            // a) both types are the same struct and identical other than usage of different META types
            // b) type is `repr(C)` so we know the layout
            // c) although not required, we will ensure same alignment
            // d) we will validate at compile time `LocalMeta` and `SharedMeta` are same size
            // e) Cell<usize> and AtomicUsize are same size and layout
            // f) only the two pre-defined metadata pairs are allowed
            Ok(inner as *mut SharedInner<T>)
        } else {
            Err(inner)
        }
    }

    #[inline]
    fn try_to_other<T: ?Sized>(
        &self,
        inner: *mut LocalInner<T>,
    ) -> Result<*mut SharedInner<T>, *mut LocalInner<T>> {
        // This is never safe to do
        Err(inner)
    }
}

#[repr(C)]
pub struct SharedMeta {
    count: AtomicUsize,
}

pub type SharedRc<T> = FlexRc<SharedMeta, LocalMeta, T>;

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these marked to be safe
unsafe impl<T: Send + Sync> Send for SharedRc<T> {}
unsafe impl<T: Send + Sync> Sync for SharedRc<T> {}

impl Algorithm<SharedMeta, LocalMeta> for SharedMeta {
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
            abort()
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
    fn try_into_other<T: ?Sized>(
        &self,
        inner: *mut SharedInner<T>,
    ) -> Result<*mut LocalInner<T>, *mut SharedInner<T>> {
        if self.is_unique() {
            // Safety:
            // a) both types are the same struct and identical other than usage of different META types
            // b) type is `repr(C)` so we know the layout
            // c) although not required, we will ensure same alignment (TODO)
            // d) we will validate at compile time `LocalMeta` and `SharedMeta` are same size (TODO)
            // e) Cell<usize> and AtomicUsize are same size and layout
            // f) only the two pre-defined metadata pairs are allowed
            Ok(inner as *mut LocalInner<T>)
        } else {
            Err(inner)
        }
    }

    #[inline]
    fn try_to_other<T: ?Sized>(
        &self,
        inner: *mut SharedInner<T>,
    ) -> Result<*mut LocalInner<T>, *mut SharedInner<T>> {
        // This is never safe to do
        Err(inner)
    }
}
