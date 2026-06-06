mod hybrid;
#[cfg(feature = "track_threads")]
mod hybrid_threads;
mod regular;

use crate::FlexRcInner;

pub use hybrid::*;
#[cfg(feature = "track_threads")]
pub use hybrid_threads::*;
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

    /// Attempts to convert one inner type into another while consuming the original handle.
    ///
    /// On success, the caller will not run the original handle's destructor. Implementations must
    /// fully transfer that one handle's count into the returned representation. On failure, the
    /// original handle must remain valid and unchanged from the caller's perspective.
    ///
    /// # Safety
    /// It is up to the recipient to ensure the pointer is used correctly
    unsafe fn try_into_other<T: ?Sized>(
        inner: *mut FlexRcInner<META, META2, T>,
    ) -> Result<*mut FlexRcInner<META2, META, T>, *mut FlexRcInner<META, META2, T>>;

    /// Attempts to convert one inner type into another without consuming the original handle.
    ///
    /// On success, implementations must retain an additional handle for the returned
    /// representation.
    ///
    /// # Safety
    /// It is up to the recipient to ensure the pointer is used correctly
    unsafe fn try_to_other<T: ?Sized>(
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
