#![cfg(feature = "track_threads")]

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::algorithm::abort;

const MAX_THREADS: usize = usize::MAX >> 1;

static NEXT_THREAD_ID: AtomicUsize = AtomicUsize::new(1);

thread_local! { pub(crate) static THREAD_ID: ThreadId = ThreadId::new() }

// *** Thread Id ***

pub(crate) struct ThreadId(pub usize);

impl ThreadId {
    fn new() -> Self {
        let id = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed);

        if id >= MAX_THREADS {
            abort()
        }

        Self(id)
    }
}
