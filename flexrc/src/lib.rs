#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod algorithm;

pub use algorithm::*;

use alloc::alloc::{alloc, handle_alloc_error};
use alloc::boxed::Box;
use alloc::string::String;
use core::alloc::Layout;
use core::borrow::Borrow;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use core::ops::Deref;
use core::pin::Pin;
use core::ptr::NonNull;
use core::{mem, ptr};

// *** FlexRcInner ***

// MUST ensure both `Rc` and `Arc` have identical memory layout
#[doc(hidden)]
#[repr(C)]
pub struct FlexRcInner<META, META2, T: ?Sized> {
    metadata: META,
    marker: PhantomData<META2>,
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
            marker: PhantomData,
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
    marker: PhantomData<FlexRcInner<META, META2, T>>,
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
    pub fn new_uninit() -> FlexRc<META, META2, mem::MaybeUninit<T>> {
        FlexRc::new(mem::MaybeUninit::uninit())
    }

    #[inline]
    pub fn new_zeroed() -> FlexRc<META, META2, mem::MaybeUninit<T>> {
        FlexRc::new(mem::MaybeUninit::zeroed())
    }

    #[inline]
    pub fn pin(data: T) -> Pin<Self> {
        // SAFETY: The data is stored in a heap allocation and will not move when the handle moves.
        unsafe { Pin::new_unchecked(Self::new(data)) }
    }

    #[inline]
    pub fn from_ref(data: &T) -> Self
    where
        T: Clone,
    {
        Self::new(data.clone())
    }
}

impl<META, META2, T> FlexRc<META, META2, [T]>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
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

        // Create our inner.
        // SAFETY: The allocation has the layout for this DST tail and `MaybeUninit<T>`
        // permits uninitialized elements.
        unsafe {
            ptr::write(ptr::addr_of_mut!((*inner).metadata), META::create());
            &mut (*inner)
        }
    }

    #[inline]
    pub fn new_uninit_slice(len: usize) -> FlexRc<META, META2, [mem::MaybeUninit<T>]> {
        let inner = Self::new_slice_uninit_inner(len);
        FlexRc::from_inner(inner.into())
    }

    #[inline]
    pub fn new_zeroed_slice(len: usize) -> FlexRc<META, META2, [mem::MaybeUninit<T>]> {
        let inner = Self::new_slice_uninit_inner(len);

        // SAFETY: `MaybeUninit<T>` may hold any bit pattern, including all zero bytes.
        unsafe {
            ptr::write_bytes(inner.data.as_mut_ptr(), 0, len);
        }

        FlexRc::from_inner(inner.into())
    }

    #[inline]
    pub fn new_slice_uninit(len: usize) -> FlexRc<META, META2, [mem::MaybeUninit<T>]> {
        Self::new_uninit_slice(len)
    }
}

impl<META, META2, T> FlexRc<META, META2, [T]>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: Copy,
{
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
                &mut inner.data as *mut [mem::MaybeUninit<T>] as *mut [T] as *mut T,
                data.len(),
            );
        }

        // Now that we are initialized, dump the MaybeUninit wrapper
        unsafe { Self::from_inner(inner.assume_init().into()) }
    }
}

impl<META, META2, T> FlexRc<META, META2, mem::MaybeUninit<T>>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    /// # Safety
    /// We are trusting the user that this memory has been initialized
    /// (thus why it is an unsafe function)
    #[inline]
    pub unsafe fn assume_init(self) -> FlexRc<META, META2, T> {
        let inner = mem::ManuallyDrop::new(self).ptr.as_ptr() as *mut FlexRcInner<META, META2, T>;

        // SAFETY: `MaybeUninit<T>` and `T` have the same layout, and the caller guarantees init.
        FlexRc::from_inner(unsafe { NonNull::new_unchecked(inner) })
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

impl<META, META2> FlexRc<META, META2, str>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    #[inline]
    fn from_str_inner(data: &str) -> Self {
        let bytes = data.as_bytes();
        let array_layout = Layout::array::<u8>(bytes.len()).expect("valid str length");

        let layout = Layout::new::<FlexRcInner<META, META2, ()>>()
            .extend(array_layout)
            .expect("valid inner layout")
            .0
            .pad_to_align();

        // SAFETY: We carefully crafted our layout to correct specifications above, and
        // we check for null below in case the allocator returns one.
        let ptr = unsafe { alloc(layout) };

        let ptr = match ptr::NonNull::new(ptr) {
            Some(ptr) => ptr.as_ptr(),
            None => handle_alloc_error(layout),
        };

        let inner =
            ptr::slice_from_raw_parts_mut(ptr, bytes.len()) as *mut FlexRcInner<META, META2, str>;

        // SAFETY: `data` is valid UTF-8, and we copy those bytes into the tail before any
        // reference to the `str` is created.
        unsafe {
            ptr::write(ptr::addr_of_mut!((*inner).metadata), META::create());
            ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                ptr::addr_of_mut!((*inner).data) as *mut u8,
                bytes.len(),
            );
            Self::from_inner(NonNull::new_unchecked(inner))
        }
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
            marker: PhantomData,
        }
    }

    #[inline(always)]
    fn as_inner(&self) -> &FlexRcInner<META, META2, T> {
        // SAFETY: As long as we have an instance, our pointer is guaranteed valid
        unsafe { self.ptr.as_ref() }
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.as_inner().metadata.is_unique()
    }

    #[inline]
    pub fn as_ptr(this: &Self) -> *const T {
        ptr::addr_of!(this.as_inner().data)
    }

    #[inline]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        ptr::addr_eq(this.ptr.as_ptr(), other.ptr.as_ptr())
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

    /// Try to convert this into a type with the other type of metadata for the pair (local -> shared,
    /// or shared -> local). If it is possible it will return the new type, else it will fail and
    /// return itself instead
    #[inline]
    pub fn try_into_other(self) -> Result<FlexRc<META2, META, T>, Self> {
        let this = mem::ManuallyDrop::new(self);

        // SAFETY: It is up to the recipient to ensure the pointer is valid
        match unsafe { META::try_into_other(this.ptr.as_ptr()) } {
            Ok(inner) => {
                // SAFETY: We are guaranteed to have a non-null pointer here
                let inner = unsafe { NonNull::new_unchecked(inner) };
                Ok(<FlexRc<META2, META, T>>::from_inner(inner))
            }
            Err(_) => Err(mem::ManuallyDrop::into_inner(this)),
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

    /// Try to create another instance of this Rc with the other type of metadata for the pair
    /// (local -> shared, or shared -> local). If it is possible to create this new instance without
    /// allocation or copying it will return it, else it will fail and return a ref to the same instance
    #[inline]
    pub fn try_to_other(&self) -> Result<FlexRc<META2, META, T>, &Self> {
        // SAFETY: It is up to the recipient to ensure the pointer is valid
        match unsafe { META::try_to_other(self.ptr.as_ptr()) } {
            Ok(inner) => {
                // SAFETY: We are guaranteed to have a non-null pointer here
                let inner = unsafe { NonNull::new_unchecked(inner) };
                Ok(<FlexRc<META2, META, T>>::from_inner(inner))
            }
            Err(_) => Err(self),
        }
    }

    /// Try to create another instance of this Rc with the other type of metadata for the pair
    /// (local -> shared, or shared -> local). If it is possible to create this new instance without
    /// allocation or copying it will return it, else it clone the underlying data and return a new Rc
    #[inline]
    pub fn to_other(&self) -> FlexRc<META2, META, T>
    where
        T: Clone,
    {
        match self.try_to_other() {
            Ok(other) => other,
            Err(this) => <FlexRc<META2, META, T>>::from_ref(this),
        }
    }
}

impl<META, META2, T> AsRef<T> for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: ?Sized,
{
    #[inline(always)]
    fn as_ref(&self) -> &T {
        &self.as_inner().data
    }
}

impl<META, META2, T> Borrow<T> for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: ?Sized,
{
    #[inline(always)]
    fn borrow(&self) -> &T {
        &self.as_inner().data
    }
}

impl<META, META2, T> Deref for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: ?Sized,
{
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.as_inner().data
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

impl<META, META2, T> Default for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: Default,
{
    #[inline]
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<META, META2, T> From<T> for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    #[inline]
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<META, META2> Default for FlexRc<META, META2, str>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    #[inline]
    fn default() -> Self {
        Self::from_str_inner("")
    }
}

impl<META, META2> From<&str> for FlexRc<META, META2, str>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    #[inline]
    fn from(value: &str) -> Self {
        Self::from_str_inner(value)
    }
}

impl<META, META2> From<String> for FlexRc<META, META2, str>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    #[inline]
    fn from(value: String) -> Self {
        Self::from_str_inner(&value)
    }
}

impl<META, META2> From<Box<str>> for FlexRc<META, META2, str>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
{
    #[inline]
    fn from(value: Box<str>) -> Self {
        Self::from_str_inner(&value)
    }
}

impl<META, META2, T> fmt::Debug for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: fmt::Debug + ?Sized,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_inner().data, f)
    }
}

impl<META, META2, T> fmt::Display for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: fmt::Display + ?Sized,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.as_inner().data, f)
    }
}

impl<META, META2, T> fmt::Pointer for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: ?Sized,
{
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&Self::as_ptr(self), f)
    }
}

impl<META, META2, T> PartialEq for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: PartialEq + ?Sized,
{
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_inner().data.eq(&other.as_inner().data)
    }
}

impl<META, META2, T> Eq for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: Eq + ?Sized,
{
}

impl<META, META2, T> PartialOrd for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: PartialOrd + ?Sized,
{
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        self.as_inner().data.partial_cmp(&other.as_inner().data)
    }
}

impl<META, META2, T> Ord for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: Ord + ?Sized,
{
    #[inline]
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_inner().data.cmp(&other.as_inner().data)
    }
}

impl<META, META2, T> Hash for FlexRc<META, META2, T>
where
    META: Algorithm<META, META2>,
    META2: Algorithm<META2, META>,
    T: Hash + ?Sized,
{
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_inner().data.hash(state);
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
                let _ = Box::from_raw(self.ptr.as_ptr());
            }
        }
    }
}
