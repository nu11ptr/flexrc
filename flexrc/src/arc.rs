use core::sync::atomic;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{FlexRc, RcBox, RefCount};

impl RefCount for AtomicUsize {
    #[inline]
    fn new() -> Self {
        AtomicUsize::new(1)
    }

    #[inline]
    fn is_unique(&self) -> bool {
        // Long discussion on why this ordering is required
        // https://github.com/servo/servo/issues/21186
        self.load(Ordering::Acquire) == 1
    }

    #[inline]
    fn get_count(&self) -> usize {
        // This is what stdlib does
        self.load(Ordering::SeqCst)
    }

    #[inline]
    fn increment(&self) -> usize {
        self.fetch_add(1, Ordering::Relaxed)
    }

    #[inline]
    fn decrement(&self) -> usize {
        self.fetch_sub(1, Ordering::Release)
    }

    #[inline]
    fn fence() {
        atomic::fence(Ordering::Acquire);
    }
}

pub type FlexArc<T> = RcBox<AtomicUsize, T>;

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these markets to be safe
unsafe impl<T: Sync + Send> Send for FlexArc<T> {}
unsafe impl<T: Sync + Send> Sync for FlexArc<T> {}

impl<T> FlexArc<T> {
    #[inline]
    pub fn try_into_rc(self) -> Result<FlexRc<T>, Self> {
        self.try_into_other()
    }

    #[inline]
    pub fn into_rc(self) -> FlexRc<T>
    where
        T: Clone,
    {
        self.into_other()
    }
}
