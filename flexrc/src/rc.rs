use core::cell::Cell;

use crate::{RcBox, RefCount};

impl RefCount for Cell<usize> {
    #[inline]
    fn new() -> Self {
        Cell::new(1)
    }

    #[inline]
    fn increment(&self) -> usize {
        let old = self.get();
        self.set(old + 1);
        old
    }

    #[inline]
    fn decrement(&self) -> usize {
        let old = self.get();
        self.set(old - 1);
        old
    }

    #[inline]
    fn fence() {}
}

pub type FlexRc<T> = RcBox<Cell<usize>, T>;
