mod hybrid;
mod regular;

use crate::FlexRcInner;

pub use hybrid::*;
pub use regular::*;

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
    fn try_into_other<T: ?Sized>(
        &self,
        inner: *mut FlexRcInner<META, META2, T>,
    ) -> Result<*mut FlexRcInner<META2, META, T>, *mut FlexRcInner<META, META2, T>>;

    /// Attempts to converts one inner type into another but NOT consuming the other
    fn try_to_other<T: ?Sized>(
        &self,
        inner: *mut FlexRcInner<META, META2, T>,
    ) -> Result<*mut FlexRcInner<META2, META, T>, *mut FlexRcInner<META, META2, T>>;
}
