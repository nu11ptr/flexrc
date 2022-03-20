#![cfg(feature = "track_threads")]

use std::collections::HashSet;
use std::sync::{Mutex, Once};

const MAX_THREADS: usize = usize::MAX >> 1;

static mut THREAD_TRACKER: Option<ThreadTracker> = None;
static ONCE: Once = Once::new();

thread_local! { pub(crate) static THREAD_ID: ThreadId = thread_tracker().unwrap().get_new_id() }

fn thread_tracker() -> Option<&'static ThreadTracker> {
    // SAFETY: This works because THREAD_TRACKER init is synchronized via Once
    unsafe {
        ONCE.call_once(|| {
            THREAD_TRACKER = Some(ThreadTracker::default());
        });
        THREAD_TRACKER.as_ref()
    }
}

// *** Thread Id ***

pub(crate) struct ThreadId(pub usize);

impl Drop for ThreadId {
    fn drop(&mut self) {
        thread_tracker().unwrap().return_id(self.0);
    }
}

// *** Thread Tracker ***

#[derive(Default)]
struct ThreadTrackerInner {
    counter: usize,
    used_counters: HashSet<usize>,
}

#[derive(Default)]
struct ThreadTracker(Mutex<ThreadTrackerInner>);

impl ThreadTracker {
    pub fn get_new_id(&self) -> ThreadId {
        let mut inner = self.0.lock().expect("poisoned lock");

        loop {
            inner.counter += 1;
            if inner.counter == MAX_THREADS {
                // We could reset back to zero, but why? Now we have zero reserved
                // for some possible future use
                inner.counter = 1;
            }

            let counter = inner.counter;
            if inner.used_counters.insert(counter) {
                return ThreadId(counter);
            }
        }
    }

    pub fn return_id(&self, id: usize) {
        let mut inner = self.0.lock().expect("poisoned lock");
        inner.used_counters.remove(&id);
    }
}
