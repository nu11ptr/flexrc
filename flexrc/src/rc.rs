use core::cell::Cell;

use crate::{FlexArc, RcBox, RefCount};

impl RefCount for Cell<usize> {
    #[inline]
    fn new() -> Self {
        Cell::new(1)
    }

    #[inline]
    fn is_unique(&self) -> bool {
        self.get() == 1
    }

    #[inline]
    fn get_count(&self) -> usize {
        self.get()
    }

    #[inline(always)]
    fn increment(&self) {
        let old = self.get();
        // TODO: This check adds 15-16% clone overhead - truly needed?
        if old == usize::MAX {
            // Abort not available on no_std
            panic!("Reference count overflow");
        }
        self.set(old + 1);
    }

    #[inline(always)]
    fn decrement(&self) -> bool {
        self.set(self.get() - 1);
        self.get() == 0
    }

    #[inline]
    fn fence() {}
}

pub type FlexRc<T> = RcBox<Cell<usize>, T>;

impl<T> FlexRc<T> {
    #[inline]
    pub fn try_into_arc(self) -> Result<FlexArc<T>, Self> {
        self.try_into_other()
    }

    #[inline]
    pub fn into_arc(self) -> FlexArc<T>
    where
        T: Clone,
    {
        self.into_other()
    }
}
