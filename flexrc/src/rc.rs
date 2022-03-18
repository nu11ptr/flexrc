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
    fn increment(&self) -> usize {
        let old = self.get();
        self.set(old + 1);
        old
    }

    #[inline(always)]
    fn decrement(&self) -> usize {
        let old = self.get();
        self.set(old - 1);
        old
    }

    #[inline(always)]
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
