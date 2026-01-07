#![cfg(loom)]

use flexrc::*;
use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::Arc;
use loom::thread;

// A simple drop tracker to verify that drop happens exactly once
struct DropTracker {
    dropped: Arc<AtomicBool>,
    value: usize,
}

impl DropTracker {
    fn new(dropped: Arc<AtomicBool>, value: usize) -> Self {
        Self { dropped, value }
    }
}

impl Drop for DropTracker {
    fn drop(&mut self) {
        // Verify this hasn't been dropped before
        let was_dropped = self.dropped.swap(true, Ordering::Relaxed);
        assert!(!was_dropped, "DropTracker was dropped more than once!");
    }
}

// Test helper for shared RC types that can be moved across threads
fn loom_shared_rc_test<META1, META2>(
    create_rc: impl Fn(DropTracker) -> FlexRc<META1, META2, DropTracker> + Send + Sync + 'static,
) where
    META1: Algorithm<META1, META2> + 'static,
    META2: Algorithm<META2, META1> + 'static,
    FlexRc<META1, META2, DropTracker>: Send + Sync,
{
    loom::model(move || {
        let dropped = Arc::new(AtomicBool::new(false));

        let tracker = DropTracker::new(dropped.clone(), 42);
        let rc = create_rc(tracker);

        // Verify we can access the value
        assert_eq!(rc.value, 42);

        // Use minimal threads and clones to reduce the number of interleavings loom needs to explore
        // This is still sufficient to test clone/drop semantics with contention
        let num_threads = 2;

        let handles: Vec<_> = (0..num_threads)
            .map(|_| {
                let rc_clone = rc.clone();
                
                thread::spawn(move || {
                    // Verify we can access the value through the clone
                    assert_eq!(rc_clone.value, 42);
                    // Drop the clone - this tests drop semantics with contention
                    drop(rc_clone);
                })
            })
            .collect();

        // Wait for all threads to finish
        for handle in handles {
            handle.join().unwrap();
        }

        // At this point, all clones from threads should be dropped
        // But the original `rc` should still be alive
        assert_eq!(rc.value, 42);
        assert!(
            !dropped.load(Ordering::Relaxed),
            "DropTracker was dropped too early!"
        );

        // Now drop the original - this should trigger the final drop
        drop(rc);

        // Verify the drop happened exactly once
        assert!(
            dropped.load(Ordering::Relaxed),
            "DropTracker was never dropped!"
        );
    });
}

// Test helper for local RC types that cannot be moved across threads
// Tests clone/drop within a single thread but uses loom's model to test different execution orders
fn loom_local_rc_test<META1, META2>(
    create_rc: impl Fn(DropTracker) -> FlexRc<META1, META2, DropTracker> + Send + Sync + 'static,
) where
    META1: Algorithm<META1, META2>,
    META2: Algorithm<META2, META1>,
{
    loom::model(move || {
        let dropped = Arc::new(AtomicBool::new(false));

        let tracker = DropTracker::new(dropped.clone(), 42);
        let rc = create_rc(tracker);

        // Verify we can access the value
        assert_eq!(rc.value, 42);

        // Create multiple clones in the same thread
        let num_clones = 5;
        let mut clones = Vec::new();

        for _ in 0..num_clones {
            let clone = rc.clone();
            // Verify we can access the value through each clone
            assert_eq!(clone.value, 42);
            clones.push(clone);
        }

        // Verify the original still works
        assert_eq!(rc.value, 42);
        assert!(
            !dropped.load(Ordering::Relaxed),
            "DropTracker was dropped too early!"
        );

        // Drop all clones
        drop(clones);

        // Original should still be alive
        assert_eq!(rc.value, 42);
        assert!(
            !dropped.load(Ordering::Relaxed),
            "DropTracker was dropped too early!"
        );

        // Now drop the original - this should trigger the final drop
        drop(rc);

        // Verify the drop happened exactly once
        assert!(
            dropped.load(Ordering::Relaxed),
            "DropTracker was never dropped!"
        );
    });
}

#[test]
fn test_local_rc_clone_drop() {
    loom_local_rc_test(|tracker| LocalRc::new(tracker));
}

#[test]
fn test_shared_rc_clone_drop() {
    loom_shared_rc_test(|tracker| SharedRc::new(tracker));
}

#[test]
fn test_local_hybrid_rc_clone_drop() {
    loom_local_rc_test(|tracker| LocalHybridRc::new(tracker));
}

#[test]
fn test_shared_hybrid_rc_clone_drop() {
    loom_shared_rc_test(|tracker| SharedHybridRc::new(tracker));
}
