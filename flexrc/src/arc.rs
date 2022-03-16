use core::sync::atomic;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{RcBox, RefCount};

impl RefCount for AtomicUsize {
    #[inline]
    fn new() -> Self {
        AtomicUsize::new(1)
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
