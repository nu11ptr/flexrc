#![no_std]

extern crate alloc;

mod arc;
mod rc;

pub use arc::*;
pub use rc::*;

use alloc::boxed::Box;
use core::marker::PhantomData;
use core::ops::Deref;
use core::ptr::NonNull;

pub trait RefCount {
    fn new() -> Self;

    fn increment(&self) -> usize;

    fn decrement(&self) -> usize;

    fn fence();
}

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
    fn as_inner(&self) -> &RcBoxInner<RC, T> {
        // SAFETY: As long as we have an instance, our pointer is guaranteed valid
        unsafe { self.ptr.as_ref() }
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
