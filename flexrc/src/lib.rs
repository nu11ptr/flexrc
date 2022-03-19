#![no_std]

extern crate alloc;

mod algorithm;
#[cfg(test)]
mod tests;

pub use algorithm::*;

use alloc::alloc::{alloc, handle_alloc_error};
use alloc::boxed::Box;
use alloc::str;
use core::alloc::Layout;
use core::marker::PhantomData;
use core::ops::Deref;
use core::ptr::NonNull;
use core::{mem, ptr};

// *** FlexRcInner ***

// MUST ensure both `Rc` and `Arc` have identical memory layout
#[doc(hidden)]
#[repr(C)]
pub struct FlexRcInner<META, META2, T: ?Sized> {
    metadata: META,
    phantom: PhantomData<META2>,
    data: T,
}

impl<META, META2, T> FlexRcInner<META, META2, T>
where
    META: Algorithm<META, META2>,
{
    #[inline]
    fn new(data: T) -> Self {
        Self {
            metadata: META::create(),
            phantom: PhantomData,
            data,
        }
    }
}

impl<META, META2, T> FlexRcInner<META, META2, [mem::MaybeUninit<T>]> {
    #[inline]
    unsafe fn assume_init(&mut self) -> &mut FlexRcInner<META, META2, [T]> {
        // SAFETY: We hold an exclusive borrow and we just cast away `MaybeUninit<T>` which is
        // guaranteed to be layout/alignment identical to `T`
        &mut *(self as *mut Self as *mut FlexRcInner<META, META2, [T]>)
    }
}

// *** FlexRc ***

// MUST ensure both `Rc` and `Arc` have identical memory layout
#[repr(C)]
pub struct FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: ?Sized,
{
    ptr: NonNull<FlexRcInner<META, META2, T>>,
    phantom: PhantomData<FlexRcInner<META, META2, T>>,
}

impl<META, META2, T> FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
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
}

impl<META, META2, T> FlexRc<META, META2, [T]>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: Copy,
{
    #[inline]
    fn new_slice_uninit_inner<'a>(
        len: usize,
    ) -> &'a mut FlexRcInner<META, META2, [mem::MaybeUninit<T>]> {
        // Unwrap safety: All good as long as array length doesn't overflow in which case we panic
        let array_layout = Layout::array::<mem::MaybeUninit<T>>(len).expect("valid array length");

        // Unwrap safety: All good as long as same sort of overflow like above doesn't occur
        // Use () (size 0) because we will get the whole size from above when extending
        let layout = Layout::new::<FlexRcInner<META, META2, ()>>()
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
        let inner = ptr::slice_from_raw_parts(ptr, len)
            as *mut FlexRcInner<META, META2, [mem::MaybeUninit<T>]>;

        // Create our inner
        // SAFETY: We made sure T is `Copy` and we carefully write out each field
        unsafe {
            ptr::write(
                &mut (*inner).metadata,
                <META as Algorithm<META, META2>>::create(),
            );
            &mut (*inner)
        }
    }

    #[inline]
    pub fn new_slice_uninit(len: usize) -> FlexRc<META, META2, [mem::MaybeUninit<T>]> {
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

impl<META, META2, T> FlexRc<META, META2, [mem::MaybeUninit<T>]>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    /// # Safety
    /// We have unique ownership. We are trusting the user that this memory has been initialized
    /// (thus why it is an unsafe function)
    #[inline]
    pub unsafe fn assume_init(self) -> FlexRc<META, META2, [T]> {
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

impl<META, META2> FlexRc<META, META2, [u8]>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    // There isn't an agreed upon way at a low level to go from [u8] -> str DST.
    // While casting may very well work forever, I decided to go the ultra safe
    // route and store as [U8] and just convert via deref to str
    #[inline]
    pub fn from_str_ref(s: impl AsRef<str>) -> FlexRc<META, META2, [u8]> {
        FlexRc::from_slice_priv(s.as_ref().as_bytes())
    }
}

impl<META, META2, T> FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: ?Sized,
{
    #[inline(always)]
    fn from_inner(inner: NonNull<FlexRcInner<META, META2, T>>) -> Self {
        Self {
            ptr: inner,
            phantom: PhantomData,
        }
    }

    #[inline(always)]
    fn as_inner(&self) -> &FlexRcInner<META, META2, T> {
        // SAFETY: As long as we have an instance, our pointer is guaranteed valid
        unsafe { self.ptr.as_ref() }
    }

    /// Try to convert this into a type with the other type of metadata for the pair (local -> shared,
    /// or shared -> local). If it is possible it will return the new type, else it will fail and
    /// return itself instead
    #[inline]
    pub fn try_into_other(self) -> Result<FlexRc<META2, META, T>, Self> {
        let meta = &self.as_inner().metadata;

        match meta.try_into_other(self.ptr.as_ptr()) {
            Ok(inner) => {
                // SAFETY: We are guaranteed to have a non-null pointer here
                let inner = unsafe { NonNull::new_unchecked(inner) };
                Ok(<FlexRc<META2, META, T>>::from_inner(inner))
            }
            Err(_) => Err(self),
        }
    }

    /// Try to convert this into a type with the other type of metadata for the pair (local -> shared,
    /// or shared -> local). If it is possible it will return the new type without additional copy
    /// or allocation, but if not possible, it will clone the underlying data and return a new Rc
    #[inline]
    pub fn into_other(self) -> FlexRc<META2, META, T>
    where
        T: Clone,
    {
        match self.try_into_other() {
            Ok(other) => other,
            Err(this) => <FlexRc<META2, META, T>>::from_ref(&*this),
        }
    }
}

impl<META, META2, T> Deref for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.as_inner().data
    }
}

#[cfg(feature = "str_deref")]
impl<META, META2> Deref for FlexRc<META, META2, [u8]>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    type Target = str;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        // SAFETY: When the `str_deref` feature is on to enable this method, we disable the `from_slice`
        // method to ensure the data came from a `str`
        unsafe { str::from_utf8_unchecked(&self.as_inner().data) }
    }
}

impl<META, META2, T> Clone for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: ?Sized,
{
    #[inline(always)]
    fn clone(&self) -> Self {
        self.as_inner().metadata.clone();
        Self::from_inner(self.ptr)
    }
}

impl<META, META2, T> Drop for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: ?Sized,
{
    #[inline(always)]
    fn drop(&mut self) {
        let meta = &self.as_inner().metadata;

        // If true, then ref count is zero
        if meta.drop() {
            // SAFETY: We own this memory, so guaranteed to exist while we have instance
            unsafe {
                // Once back into a box, it will drop and deallocate normally
                Box::from_raw(self.ptr.as_ptr());
            }
        }
    }
}
