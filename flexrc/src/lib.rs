#![no_std]

extern crate alloc;

mod arc;
mod hybrid;
mod rc;
mod regular;

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

pub trait Algorithm {
    fn create() -> Self;

    // Increment reference counters
    fn clone(&self);

    // Decrement reference counters and return true if storage should be deallocated
    fn drop(&self) -> bool;
}

pub trait RefCount {
    /// Initializes a new `RefCount` implementation with a count of 1
    fn new() -> Self;

    /// Tests (atomically if necessary) whether or not this is the only instance (count == 1)
    fn is_unique(&self) -> bool;

    /// Returns the reference count
    fn get_count(&self) -> usize;

    /// Increments the reference count
    fn increment(&self);

    /// Decrements the reference count and returns true ref count is now 0
    fn decrement(&self) -> bool;

    /// Initiate a memory fence before taking further action
    fn fence();
}

// *** FlexRcInner ***

// MUST ensure both `Rc` and `Arc` have identical memory layout
#[repr(C)]
struct FlexRcInner<META, T: ?Sized> {
    metadata: META,
    data: T,
}

impl<META: RefCount, T> FlexRcInner<META, T> {
    #[inline]
    fn new(data: T) -> Self {
        Self {
            metadata: META::new(),
            data,
        }
    }
}

impl<META: RefCount, T> FlexRcInner<META, [mem::MaybeUninit<T>]> {
    #[inline]
    unsafe fn assume_init(&mut self) -> &mut FlexRcInner<META, [T]> {
        // SAFETY: We hold an exclusive borrow and we just cast away `MaybeUninit<T>` which is
        // guaranteed to be layout/alignment identical to `T`
        &mut *(self as *mut Self as *mut FlexRcInner<META, [T]>)
    }
}

// *** FlexRc ***

// MUST ensure both `Rc` and `Arc` have identical memory layout
#[repr(C)]
pub struct FlexRc<META: RefCount, T: ?Sized> {
    ptr: NonNull<FlexRcInner<META, T>>,
    phantom: PhantomData<FlexRcInner<META, T>>,
}

impl<META: RefCount, T> FlexRc<META, T> {
    #[inline]
    pub fn new(data: T) -> Self {
        let boxed = Box::new(FlexRcInner::new(data));

        // SAFETY: `new_unchecked` is guaranteed to receive a valid pointer
        Self::from_inner(unsafe { NonNull::new_unchecked(Box::into_raw(boxed)) })
    }

    #[inline]
    pub fn from_ref(data: &T) -> Self
    where
        T: Clone,
    {
        Self::new(data.clone())
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.as_inner().metadata.is_unique()
    }

    #[inline]
    pub fn get_mut(&mut self) -> Option<&mut T> {
        if self.is_unique() {
            // SAFETY: Since this is the unique owner, we can be assured we are only giving out one `&mut`
            unsafe { Some(self.get_mut_unchecked()) }
        } else {
            None
        }
    }

    /// # Safety
    /// The user is trusted they are to be the sole owner before calling this (typically at init time)
    #[inline]
    pub unsafe fn get_mut_unchecked(&mut self) -> &mut T {
        &mut (*self.ptr.as_ptr()).data
    }

    /// Try to convert this into a type with a different reference counter (likely `Rc` -> `Arc` or
    /// `Arc` to `Rc`). If the instance is unique (reference count == 1) it will succeed and the
    /// other type is returned. If is not unique (reference count > 1), it will fail and return itself
    /// instead
    #[inline]
    fn try_into_other<META2>(self) -> Result<FlexRc<META2, T>, Self>
    where
        META2: RefCount,
    {
        // TODO: Ensure `is_unique` is a strong enough guarantee for Arc -> Rc, if not, it probably
        // isn't possible to do this, but Rc -> Arc is good for sure

        // Safety: If we are the only unique instance then we are safe to do this
        if self.is_unique() {
            // SAFETY:
            // a) both types are the same struct and identical other than usage of different RC types
            // b) type is `repr(C)` so we know the layout
            // c) although not required, we will ensure same alignment (TODO)
            // d) we will validate at compile time `AtomicUsize` and `Cell<usize>` are same size (TODO)
            // e) only the two pre-defined RCs (`AtomicUsize` and `Cell<usize>` are allowed)
            // f) this will not be made `pub` with arbitrary META
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
    fn into_other<META2>(self) -> FlexRc<META2, T>
    where
        META2: RefCount,
        T: Clone,
    {
        match self.try_into_other() {
            Ok(other) => other,
            Err(this) => <FlexRc<META2, T>>::from_ref(&*this),
        }
    }
}

impl<META: RefCount, T: Copy> FlexRc<META, [T]> {
    #[inline]
    fn new_slice_uninit_inner<'a>(len: usize) -> &'a mut FlexRcInner<META, [mem::MaybeUninit<T>]> {
        // Unwrap safety: All good as long as array length doesn't overflow in which case we panic
        let array_layout = Layout::array::<mem::MaybeUninit<T>>(len).expect("valid array length");

        // Unwrap safety: All good as long as same sort of overflow like above doesn't occur
        // Use () (size 0) because we will get the whole size from above when extending
        let layout = Layout::new::<FlexRcInner<META, ()>>()
            .extend(array_layout)
            .expect("valid inner layout")
            .0
            .pad_to_align();

        // SAFETY: We carefully crafted our layout to correct specifications above - but we check
        // for null below just in case we run out of memory
        let ptr = unsafe { alloc(layout) } as *mut mem::MaybeUninit<T>;

        // Ensure allocator didn't return NULL (docs say some allocators will)
        let ptr = match ptr::NonNull::new(ptr) {
            Some(ptr) => ptr.as_ptr(),
            None => handle_alloc_error(layout),
        };

        // This just makes a "fat pointer" setting the correct # of `T` entries in the metadata
        let inner =
            ptr::slice_from_raw_parts(ptr, len) as *mut FlexRcInner<META, [mem::MaybeUninit<T>]>;

        // Create our inner
        // SAFETY: We made sure T is `Copy` and we carefully write out each field
        unsafe {
            ptr::write(&mut (*inner).metadata, META::new());
            &mut (*inner)
        }
    }

    #[inline]
    pub fn new_slice_uninit(len: usize) -> FlexRc<META, [mem::MaybeUninit<T>]> {
        let inner = Self::new_slice_uninit_inner(len);
        FlexRc::from_inner(inner.into())
    }

    // This is not safe IF str deref feature is on because there is no guarantee that `str` bytes
    // came from well formed UTF
    #[cfg(not(feature = "str_deref"))]
    #[inline]
    pub fn from_slice(data: &[T]) -> Self {
        Self::from_slice_priv(data)
    }

    #[inline]
    fn from_slice_priv(data: &[T]) -> Self {
        let inner = Self::new_slice_uninit_inner(data.len());

        // SAFETY: We made sure T is `Copy` and we only copy the correct length
        unsafe {
            ptr::copy_nonoverlapping(
                data.as_ptr(),
                &mut (*inner).data as *mut [mem::MaybeUninit<T>] as *mut [T] as *mut T,
                data.len(),
            );
        }

        // Now that we are initialized, dump the MaybeUninit wrapper
        unsafe { Self::from_inner(inner.assume_init().into()) }
    }
}

impl<META: RefCount, T> FlexRc<META, [mem::MaybeUninit<T>]> {
    /// # Safety
    /// We have unique ownership. We are trusting the user that this memory has been initialized
    /// (thus why it is an unsafe function)
    #[inline]
    pub unsafe fn assume_init(self) -> FlexRc<META, [T]> {
        FlexRc::from_inner(
            // Avoid drop to ensure no ref count decrement
            mem::ManuallyDrop::new(self)
                .ptr
                .as_mut()
                .assume_init()
                .into(),
        )
    }
}

impl<META: RefCount> FlexRc<META, [u8]> {
    // There isn't an agreed upon way at a low level to go from [u8] -> str DST.
    // While casting may very well work forever, I decided to go the ultra safe
    // route and store as [U8] and just convert via deref to str
    #[inline]
    pub fn from_str_ref(s: impl AsRef<str>) -> FlexRc<META, [u8]> {
        FlexRc::from_slice_priv(s.as_ref().as_bytes())
    }
}

impl<META: RefCount, T: ?Sized> FlexRc<META, T> {
    #[inline(always)]
    fn from_inner(inner: NonNull<FlexRcInner<META, T>>) -> Self {
        Self {
            ptr: inner,
            phantom: PhantomData,
        }
    }

    #[inline(always)]
    fn as_inner(&self) -> &FlexRcInner<META, T> {
        // SAFETY: As long as we have an instance, our pointer is guaranteed valid
        unsafe { self.ptr.as_ref() }
    }
}

impl<META: RefCount, T> Deref for FlexRc<META, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.as_inner().data
    }
}

#[cfg(feature = "str_deref")]
impl<META: RefCount> Deref for FlexRc<META, [u8]> {
    type Target = str;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        // SAFETY: When the `str_deref` feature is on to enable this method, we disable the `from_slice`
        // method to ensure the data came from a `str`
        unsafe { str::from_utf8_unchecked(&self.as_inner().data) }
    }
}

impl<META: RefCount, T: ?Sized> Clone for FlexRc<META, T> {
    #[inline(always)]
    fn clone(&self) -> Self {
        self.as_inner().metadata.increment();
        Self::from_inner(self.ptr)
    }
}

impl<META: RefCount, T: ?Sized> Drop for FlexRc<META, T> {
    #[inline(always)]
    fn drop(&mut self) {
        let rc = &self.as_inner().metadata;

        // If true, then ref count is zero
        if rc.decrement() {
            META::fence();

            // SAFETY: We own this memory, so guaranteed to exist while we have instance
            unsafe {
                // Once back into a box, it will drop and deallocate normally
                Box::from_raw(self.ptr.as_ptr());
            }
        }
    }
}
