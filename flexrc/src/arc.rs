use core::sync::atomic;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{FastRc, FlexRc, RefCount};

const MAX_REFCOUNT: usize = isize::MAX as usize;

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

    #[inline(always)]
    fn increment(&self) {
        let old = self.fetch_add(1, Ordering::Relaxed);

        if old > MAX_REFCOUNT {
            // Abort not available on no_std
            panic!("Reference count overflow");
        }
    }

    #[inline(always)]
    fn decrement(&self) -> bool {
        self.fetch_sub(1, Ordering::Release) == 1
    }

    #[inline]
    fn fence() {
        atomic::fence(Ordering::Acquire);
    }
}

pub type AtomicRc<T> = FlexRc<AtomicUsize, T>;

// SAFETY: We ensure what we are holding is Sync/Send and we have been careful to ensure invariants
// that allow these markets to be safe
unsafe impl<T: Sync + Send> Send for AtomicRc<T> {}
unsafe impl<T: Sync + Send> Sync for AtomicRc<T> {}

impl<T> AtomicRc<T> {
    #[inline]
    pub fn try_into_rc(self) -> Result<FastRc<T>, Self> {
        self.try_into_other()
    }

    #[inline]
    pub fn into_rc(self) -> FastRc<T>
    where
        T: Clone,
    {
        self.into_other()
    }
}
