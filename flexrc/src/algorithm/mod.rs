mod hybrid;
#[cfg(feature = "track_threads")]
mod hybrid_threads;
mod regular;

use crate::FlexRcInner;

pub use hybrid::*;
pub use regular::*;

pub struct LocalMode;
pub struct SharedMode;

pub trait Algorithm<META, META2> {
    /// Create and return new metadata    
    fn create() -> Self;

    /// Returns true if this instance is the last one before final release of resources
    fn is_unique(&self) -> bool;

    /// Increment reference counters
    fn clone(&self);

    /// Decrement reference counters and return true if storage should be deallocated
    fn drop(&self) -> bool;

    /// Attempts to converts one inner type into another while consuming the other
    ///
    /// # Safety
    /// It is up to the recipient to ensure the pointer is used correctly
    unsafe fn try_into_other<T: ?Sized>(
        &self,
        inner: *mut FlexRcInner<META, META2, T>,
    ) -> Result<*mut FlexRcInner<META2, META, T>, *mut FlexRcInner<META, META2, T>>;

    /// Attempts to converts one inner type into another but NOT consuming the other
    ///
    /// # Safety
    /// It is up to the recipient to ensure the pointer is used correctly
    unsafe fn try_to_other<T: ?Sized>(
        &self,
        inner: *mut FlexRcInner<META, META2, T>,
    ) -> Result<*mut FlexRcInner<META2, META, T>, *mut FlexRcInner<META, META2, T>>;
}

#[cfg(feature = "std")]
#[inline]
fn abort() -> ! {
    std::process::abort()
}

#[cfg(not(feature = "std"))]
#[inline]
fn abort() -> ! {
    // Abort not available on no_std
    panic!("Reference count overflow");
}
