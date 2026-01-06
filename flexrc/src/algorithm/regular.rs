use core::cell::Cell;
use core::marker::PhantomData;
use core::mem::ManuallyDrop;
use core::ptr;
use core::sync::atomic;
#[cfg(feature = "small_counters")]
use core::sync::atomic::AtomicU32;
#[cfg(not(feature = "small_counters"))]
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use static_assertions::{assert_eq_align, assert_eq_size, assert_impl_all, assert_not_impl_any};

use crate::algorithm::abort;
use crate::{Algorithm, FlexRc, FlexRcInner, LocalMode, SharedMode};

assert_eq_size!(Meta<LocalMode>, Meta<SharedMode>);
assert_eq_align!(Meta<LocalMode>, Meta<SharedMode>);
assert_eq_size!(LocalInner<usize>, SharedInner<usize>);
assert_eq_align!(LocalInner<usize>, SharedInner<usize>);
assert_eq_size!(LocalRc<usize>, SharedRc<usize>);
assert_eq_align!(LocalRc<usize>, SharedRc<usize>);

assert_impl_all!(SharedRc<usize>: Send, Sync);
assert_not_impl_any!(LocalRc<usize>: Send, Sync);

#[cfg(not(feature = "small_counters"))]
const MAX_LOCAL_COUNT: usize = usize::MAX;
#[cfg(not(feature = "small_counters"))]
const MAX_SHARED_COUNT: usize = usize::MAX >> 1;
#[cfg(feature = "small_counters")]
const MAX_LOCAL_COUNT: u32 = u32::MAX;
#[cfg(feature = "small_counters")]
const MAX_SHARED_COUNT: u32 = u32::MAX >> 1;

pub type LocalRc<T> = FlexRc<Meta<LocalMode>, Meta<SharedMode>, T>;
pub type SharedRc<T> = FlexRc<Meta<SharedMode>, Meta<LocalMode>, T>;

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these marked to be safe
unsafe impl<T: Send + Sync> Send for SharedRc<T> {}
unsafe impl<T: Send + Sync> Sync for SharedRc<T> {}

type LocalInner<T> = FlexRcInner<Meta<LocalMode>, Meta<SharedMode>, T>;
type SharedInner<T> = FlexRcInner<Meta<SharedMode>, Meta<LocalMode>, T>;

// *** Meta ***

#[cfg(not(feature = "small_counters"))]
#[repr(C)]
pub union Meta<MODE> {
    local: ManuallyDrop<Cell<usize>>,
    shared: ManuallyDrop<AtomicUsize>,
    _marker: PhantomData<MODE>,
}

#[cfg(feature = "small_counters")]
#[repr(C)]
pub union Meta<MODE> {
    local: ManuallyDrop<Cell<u32>>,
    shared: ManuallyDrop<AtomicU32>,
    _marker: PhantomData<MODE>,
}

// *** Meta<LocalMode> ***

impl Meta<LocalMode> {
    #[cfg(not(feature = "small_counters"))]
    #[inline]
    fn get_count(&self) -> usize {
        // SAFETY: We are accessing the correct variant for this type and we know the layout
        unsafe { self.local.get() }
    }

    #[cfg(feature = "small_counters")]
    #[inline]
    fn get_count(&self) -> u32 {
        // SAFETY: We are accessing the correct variant for this type and we know the layout
        unsafe { self.local.get() }
    }

    #[cfg(not(feature = "small_counters"))]
    #[inline]
    fn set_count(&self, count: usize) {
        // SAFETY: We are accessing the correct variant for this type and we know the layout
        unsafe { self.local.set(count) }
    }

    #[cfg(feature = "small_counters")]
    #[inline]
    fn set_count(&self, count: u32) {
        // SAFETY: We are accessing the correct variant for this type and we know the layout
        unsafe { self.local.set(count) }
    }
}

impl Algorithm<Meta<LocalMode>, Meta<SharedMode>> for Meta<LocalMode> {
    #[inline]
    fn create() -> Self {
        Self {
            local: ManuallyDrop::new(Cell::new(1)),
        }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.get_count() == 1
    }

    #[inline(always)]
    fn clone(&self) {
        let old = self.get_count();

        // TODO: This check adds 15-16% clone overhead - truly needed?
        if old == MAX_LOCAL_COUNT {
            abort();
        }

        self.set_count(old + 1);
    }

    #[inline(always)]
    fn drop(&self) -> bool {
        self.set_count(self.get_count() - 1);
        self.get_count() == 0
    }

    #[inline]
    fn try_into_other<T: ?Sized>(
        &self,
        inner: *mut LocalInner<T>,
    ) -> Result<*mut SharedInner<T>, *mut LocalInner<T>> {
        if self.is_unique() {
            // SAFETY: We are accessing the correct variant for this type and we know the layout.
            // We also know we have unique access to the inner so we can safely write to the shared variant.
            unsafe {
                let shared_ptr = ptr::addr_of_mut!((*inner).metadata.shared);
                ptr::write(
                    shared_ptr,
                    ManuallyDrop::new(Meta::<SharedMode>::make_atomic()),
                );
            }

            // Safety:
            // a) both types are the same struct and identical other than usage of different MODE types
            // b) type is `repr(C)` so we know the layout
            // c) although not required, we will ensure same alignment
            // d) we will validate at compile time `Meta<LocalMode>` and `Meta<SharedMode>` are same size
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

// *** Meta<SharedMode> ***

impl Meta<SharedMode> {
    #[cfg(not(feature = "small_counters"))]
    #[inline]
    fn make_atomic() -> AtomicUsize {
        AtomicUsize::new(1)
    }

    #[cfg(feature = "small_counters")]
    #[inline]
    fn make_atomic() -> AtomicU32 {
        AtomicU32::new(1)
    }
}
impl Algorithm<Meta<SharedMode>, Meta<LocalMode>> for Meta<SharedMode> {
    #[inline]
    fn create() -> Self {
        Self {
            shared: ManuallyDrop::new(Self::make_atomic()),
        }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        // Long discussion on why this ordering is required: https://github.com/servo/servo/issues/21186
        // SAFETY: We are accessing the correct variant for this type and we know the layout
        unsafe { self.shared.load(Ordering::Acquire) == 1 }
    }

    #[inline(always)]
    fn clone(&self) {
        // SAFETY: We are accessing the correct variant for this type and we know the layout.
        let old = unsafe { self.shared.fetch_add(1, Ordering::Relaxed) };

        if old > MAX_SHARED_COUNT {
            abort()
        }
    }

    #[inline(always)]
    fn drop(&self) -> bool {
        // SAFETY: We are accessing the correct variant for this type and we know the layout.
        if unsafe { self.shared.fetch_sub(1, Ordering::Release) } == 1 {
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
            // SAFETY: We are accessing the correct variant for this type and we know the layout.
            // We also know we have unique access to the inner so we can safely write to the shared variant.
            unsafe {
                let local_ptr = ptr::addr_of_mut!((*inner).metadata.local);
                ptr::write(local_ptr, ManuallyDrop::new(Cell::new(1)));
            }

            // Safety:
            // a) both types are the same struct and identical other than usage of different MODE types
            // b) type is `repr(C)` so we know the layout
            // c) although not required, we will ensure same alignment
            // d) we will validate at compile time `Meta<LocalMode>` and `Meta<SharedMode>` are same size
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
