#![no_std]

extern crate alloc;

mod arc;
mod rc;

pub use arc::*;
pub use rc::*;

use alloc::boxed::Box;
use core::marker::PhantomData;
use core::mem;
use core::ops::Deref;
use core::ptr::NonNull;

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
struct RcBoxInner<RC, T> {
    rc: RC,
    data: T,
}

impl<RC, T> RcBoxInner<RC, T>
where
    RC: RefCount,
{
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
pub struct RcBox<RC, T>
where
    RC: RefCount,
{
    ptr: NonNull<RcBoxInner<RC, T>>,
    phantom: PhantomData<RcBoxInner<RC, T>>,
}

impl<RC, T> RcBox<RC, T>
where
    RC: RefCount,
{
    #[inline]
    pub fn new(data: T) -> Self {
        let boxed = Box::new(RcBoxInner::new(data));

        Self {
            // SAFETY: `new` is guaranteed to have a pointer so can't ever be `None`
            ptr: unsafe { NonNull::new(Box::into_raw(boxed)).unwrap_unchecked() },
            phantom: PhantomData::default(),
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

    #[inline]
    fn as_inner(&self) -> &RcBoxInner<RC, T> {
        // SAFETY: As long as we have an instance, our pointer is guaranteed valid
        unsafe { self.ptr.as_ref() }
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
        // isn't possible to do this

        // Safety: If we are the only unique instance then we are safe to do this
        if self.is_unique() {
            // SAFETY: Since FlexRc and FlexArc have the exact same memory layout and their inners
            // (Cell<usize> and AtomicUsize>) are memory identical we can do this safely
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

impl<RC, T> Deref for RcBox<RC, T>
where
    RC: RefCount,
{
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.as_inner().data
    }
}

impl<RC, T> Clone for RcBox<RC, T>
where
    RC: RefCount,
{
    #[inline]
    fn clone(&self) -> Self {
        let rc = &self.as_inner().rc;
        let old_rc = rc.increment();

        if old_rc >= isize::MAX as usize {
            panic!("Ref count limit exceeded!");
        }

        Self {
            ptr: self.ptr,
            phantom: PhantomData,
        }
    }
}

impl<RC, T> Drop for RcBox<RC, T>
where
    RC: RefCount,
{
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
