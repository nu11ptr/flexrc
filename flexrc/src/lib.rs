#![no_std]

extern crate alloc;

mod arc;
mod rc;

pub use arc::*;
pub use rc::*;

use alloc::alloc::{alloc, handle_alloc_error};
use alloc::boxed::Box;
use alloc::str;
use core::alloc::Layout;
use core::marker::PhantomData;
use core::ops::Deref;
use core::ptr::NonNull;
use core::{mem, ptr};

pub trait RefCount {
    fn new() -> Self;

    fn is_unique(&self) -> bool;

    fn get_count(&self) -> usize;

    fn increment(&self) -> usize;

    fn decrement(&self) -> usize;

    fn fence();
}

// MUST ensure both `Rc` and `Arc` have identical memory layout
#[repr(C)]
struct RcBoxInner<RC, T: ?Sized> {
    rc: RC,
    data: T,
}

impl<RC: RefCount, T> RcBoxInner<RC, T> {
    #[inline]
    pub fn new(data: T) -> Self {
        Self {
            rc: RC::new(),
            data,
        }
    }
}

// MUST ensure both `Rc` and `Arc` have identical memory layout
#[repr(C)]
pub struct RcBox<RC: RefCount, T: ?Sized> {
    ptr: NonNull<RcBoxInner<RC, T>>,
    phantom: PhantomData<RcBoxInner<RC, T>>,
}

impl<RC: RefCount, T> RcBox<RC, T> {
    #[inline]
    pub fn new(data: T) -> Self {
        let boxed = Box::new(RcBoxInner::new(data));

        Self {
            // SAFETY: `new_unchecked` is guaranteed to receive a valid pointer
            ptr: unsafe { NonNull::new_unchecked(Box::into_raw(boxed)) },
            phantom: PhantomData,
        }
    }

    #[inline]
    pub fn from_ref(data: &T) -> Self
    where
        T: Clone,
    {
        Self::new(data.clone())
    }

    #[inline]
    pub fn ref_count(&self) -> usize {
        self.as_inner().rc.get_count()
    }

    #[inline]
    pub fn is_unique(&self) -> bool {
        self.as_inner().rc.is_unique()
    }

    /// Try to convert this into a type with a different reference counter (likely `Rc` -> `Arc` or
    /// `Arc` to `Rc`). If the instance is unique (reference count == 1) it will succeed and the
    /// other type is returned. If is not unique (reference count > 1), it will fail and return itself
    /// instead
    #[inline]
    fn try_into_other<RC2>(self) -> Result<RcBox<RC2, T>, Self>
    where
        RC2: RefCount,
    {
        // TODO: Ensure `is_unique` is a strong enough guarantee for Arc -> Rc, if not, it probably
        // isn't possible to do this, but Rc -> Arc is good for sure

        // Safety: If we are the only unique instance then we are safe to do this
        if self.is_unique() {
            // SAFETY:
            // a) both types are same struct and identical other than usage of different RCs
            // b) type is `repr(C)` so we know the layout
            // c) although not required, we will ensure same alignment (TODO)
            // d) we will validate at compile time `AtomicUsize` and `Cell<usize>` are same size (TODO)
            Ok(unsafe { mem::transmute(self) })
        } else {
            Err(self)
        }
    }

    /// Convert this into a type with a different reference counter (likely `Rc` -> `Arc` or
    /// `Arc` to `Rc`). If the instance is unique (reference count == 1) it will succeed and the
    /// other type is returned. If is not unique (reference count > 1), a copy will be made and
    /// returned to ensure the call succeeds
    #[inline]
    fn into_other<RC2>(self) -> RcBox<RC2, T>
    where
        RC2: RefCount,
        T: Clone,
    {
        match self.try_into_other() {
            Ok(other) => other,
            Err(this) => <RcBox<RC2, T>>::from_ref(&*this),
        }
    }
}

impl<RC: RefCount, T: Copy> RcBox<RC, [T]> {
    // This is not safe IF str deref feature is on because there is no guarantee that `str` bytes
    // came from well formed UTF
    #[cfg(not(feature = "str_deref"))]
    #[inline]
    pub fn from_slice(data: &[T]) -> Self {
        Self::from_slice_priv(data)
    }

    fn from_slice_priv(data: &[T]) -> Self {
        // Unwrap safety: All good as long as array length doesn't overflow in which case we panic
        let array_layout = Layout::array::<T>(data.len()).expect("valid array length");

        // Unwrap safety: All good as long as same sort of overflow like above doesn't occur
        // Use () (size 0) because we will get the whole size from above when extending
        let layout = Layout::new::<RcBoxInner<RC, ()>>()
            .extend(array_layout)
            .expect("valid inner layout")
            .0
            .pad_to_align();

        // SAFETY: We carefully crafted our layout to correct specifications above - but we check
        // for null below just in case we run out of memory
        let ptr = unsafe { alloc(layout) } as *mut T;

        // Ensure allocator didn't return NULL (docs say some allocators will)
        let ptr = match ptr::NonNull::new(ptr) {
            Some(ptr) => ptr.as_ptr(),
            None => handle_alloc_error(layout),
        };

        // This just makes a "fat pointer" setting the correct # of `T` entries in the metadata
        let inner = ptr::slice_from_raw_parts(ptr, data.len()) as *mut RcBoxInner<RC, [T]>;

        // Create our inner
        // SAFETY: We made sure T is `Copy` and we carefully write out each field
        unsafe {
            ptr::write(&mut (*inner).rc, RC::new());
            ptr::copy_nonoverlapping(
                data.as_ptr(),
                &mut (*inner).data as *mut [T] as *mut T,
                data.len(),
            );
        }

        Self::from_inner(inner)
    }
}

impl<RC: RefCount> RcBox<RC, [u8]> {
    // There isn't an agreed upon way at a low level to go from [u8] -> str DST.
    // While casting may very well work forever, I decided to go the ultra safe
    // route and store as [U8] and just convert via deref to str
    #[inline]
    pub fn from_str_ref(s: impl AsRef<str>) -> RcBox<RC, [u8]> {
        RcBox::from_slice_priv(s.as_ref().as_bytes())
    }
}

impl<RC: RefCount, T: ?Sized> RcBox<RC, T> {
    #[inline]
    fn from_inner(inner: *mut RcBoxInner<RC, T>) -> Self {
        Self {
            // SAFETY: `inner` is checked before this is called to ensure it is valid
            ptr: unsafe { NonNull::new_unchecked(inner) },
            phantom: PhantomData,
        }
    }

    #[inline]
    fn as_inner(&self) -> &RcBoxInner<RC, T> {
        // SAFETY: As long as we have an instance, our pointer is guaranteed valid
        unsafe { self.ptr.as_ref() }
    }
}

impl<RC: RefCount, T> Deref for RcBox<RC, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.as_inner().data
    }
}

#[cfg(feature = "str_deref")]
impl<RC: RefCount> Deref for RcBox<RC, [u8]> {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        // SAFETY: When the `str_deref` feature is on to enable this method, we disable the `from_slice`
        // method to ensure the data came from a `str`
        unsafe { str::from_utf8_unchecked(&self.as_inner().data) }
    }
}

impl<RC: RefCount, T> Clone for RcBox<RC, T> {
    #[inline]
    fn clone(&self) -> Self {
        let rc = &self.as_inner().rc;
        let old_rc = rc.increment();

        if old_rc >= isize::MAX as usize {
            // Abort not available without std
            panic!("Ref count limit exceeded!");
        }

        Self {
            ptr: self.ptr,
            phantom: PhantomData,
        }
    }
}

impl<RC: RefCount, T: ?Sized> Drop for RcBox<RC, T> {
    #[inline]
    fn drop(&mut self) {
        let rc = &self.as_inner().rc;

        // If old val is 1, then it is really 0 now
        if rc.decrement() == 1 {
            RC::fence();
            // SAFETY: We own this memory, so guaranteed to exist while we have instance
            unsafe {
                // Once back into a box, it will drop and deallocate normally
                Box::from_raw(self.ptr.as_ptr());
            }
        }
    }
}
