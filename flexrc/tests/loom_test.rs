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

fn expect_ok<T, E>(result: Result<T, E>, message: &str) -> T {
    match result {
        Ok(value) => value,
        Err(_) => panic!("{message}"),
    }
}

#[cfg(feature = "track_threads")]
fn expect_err<T, E>(result: Result<T, E>, message: &str) -> E {
    match result {
        Ok(_) => panic!("{message}"),
        Err(value) => value,
    }
}

// Test helper for shared RC types that can be moved across threads
fn loom_small_arc_test<META1, META2>(
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
fn loom_small_rc_test<META1, META2>(
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
fn test_small_rc_clone_drop() {
    loom_small_rc_test(|tracker| SmallRc::new(tracker));
}

#[test]
fn test_small_arc_clone_drop() {
    loom_small_arc_test(|tracker| SmallArc::new(tracker));
}

#[test]
fn test_hybrid_rc_clone_drop() {
    loom_small_rc_test(|tracker| HybridRc::new(tracker));
}

#[test]
fn test_hybrid_arc_clone_drop() {
    loom_small_arc_test(|tracker| HybridArc::new(tracker));
}

#[cfg(feature = "track_threads")]
#[test]
fn test_thread_rc_clone_drop() {
    loom_small_rc_test(|tracker| ThreadRc::new(tracker));
}

#[cfg(feature = "track_threads")]
#[test]
fn test_thread_arc_clone_drop() {
    loom_small_arc_test(|tracker| ThreadArc::new(tracker));
}

#[test]
fn test_regular_local_into_shared_conversion() {
    loom::model(|| {
        let dropped = Arc::new(AtomicBool::new(false));
        let local: SmallRc<DropTracker> = SmallRc::new(DropTracker::new(dropped.clone(), 42));

        let shared: SmallArc<DropTracker> =
            expect_ok(local.try_into_other(), "unique SmallRc should promote");

        assert_eq!(shared.value, 42);
        assert!(!dropped.load(Ordering::Relaxed));

        drop(shared);
        assert!(dropped.load(Ordering::Relaxed));
    });
}

#[test]
fn test_regular_shared_into_local_conversion() {
    loom::model(|| {
        let dropped = Arc::new(AtomicBool::new(false));
        let shared: SmallArc<DropTracker> = SmallArc::new(DropTracker::new(dropped.clone(), 42));

        let local: SmallRc<DropTracker> =
            expect_ok(shared.try_into_other(), "unique SmallArc should demote");

        assert_eq!(local.value, 42);
        assert!(!dropped.load(Ordering::Relaxed));

        drop(local);
        assert!(dropped.load(Ordering::Relaxed));
    });
}

#[test]
fn test_hybrid_local_shared_drop_race() {
    loom::model(|| {
        let dropped = Arc::new(AtomicBool::new(false));
        let local: HybridRc<DropTracker> = HybridRc::new(DropTracker::new(dropped.clone(), 42));
        let shared: HybridArc<DropTracker> =
            expect_ok(local.try_to_other(), "hybrid local should retain shared");

        let shared_owner = Arc::new(shared);
        let thread_owner = shared_owner.clone();

        let handle = thread::spawn(move || {
            let shared_clone = (*thread_owner).clone();
            assert_eq!(shared_clone.value, 42);
            drop(shared_clone);
        });

        drop(local);
        assert!(!dropped.load(Ordering::Relaxed));

        handle.join().unwrap();
        assert!(!dropped.load(Ordering::Relaxed));

        drop(shared_owner);
        assert!(dropped.load(Ordering::Relaxed));
    });
}

#[test]
fn test_hybrid_shared_into_local_races_with_shared_drop() {
    loom::model(|| {
        let dropped = Arc::new(AtomicBool::new(false));
        let shared: HybridArc<DropTracker> = HybridArc::new(DropTracker::new(dropped.clone(), 42));
        let thread_shared = shared.clone();

        let handle = thread::spawn(move || {
            assert_eq!(thread_shared.value, 42);
            drop(thread_shared);
        });

        let local: HybridRc<DropTracker> = expect_ok(
            shared.try_into_other(),
            "hybrid shared should transfer to local",
        );

        handle.join().unwrap();
        assert!(!dropped.load(Ordering::Relaxed));

        drop(local);
        assert!(dropped.load(Ordering::Relaxed));
    });
}

#[test]
fn test_hybrid_shared_clone_while_local_present() {
    loom::model(|| {
        let dropped = Arc::new(AtomicBool::new(false));
        let shared: HybridArc<DropTracker> = HybridArc::new(DropTracker::new(dropped.clone(), 42));
        let local: HybridRc<DropTracker> =
            expect_ok(shared.try_to_other(), "hybrid shared should retain local");

        let shared_owner = Arc::new(shared);
        let first_owner = shared_owner.clone();
        let second_owner = shared_owner.clone();

        let first = thread::spawn(move || {
            let clone = (*first_owner).clone();
            assert_eq!(clone.value, 42);
            drop(clone);
        });

        let second = thread::spawn(move || {
            let clone = (*second_owner).clone();
            assert_eq!(clone.value, 42);
            drop(clone);
        });

        first.join().unwrap();
        second.join().unwrap();

        drop(shared_owner);
        assert!(!dropped.load(Ordering::Relaxed));

        drop(local);
        assert!(dropped.load(Ordering::Relaxed));
    });
}

#[cfg(feature = "track_threads")]
#[test]
fn test_thread_hybrid_local_shared_drop_race() {
    loom::model(|| {
        let dropped = Arc::new(AtomicBool::new(false));
        let local: ThreadRc<DropTracker> = ThreadRc::new(DropTracker::new(dropped.clone(), 42));
        let shared: ThreadArc<DropTracker> =
            expect_ok(local.try_to_other(), "thread local should retain shared");

        let shared_owner = Arc::new(shared);
        let thread_owner = shared_owner.clone();

        let handle = thread::spawn(move || {
            let shared_clone = (*thread_owner).clone();
            assert_eq!(shared_clone.value, 42);
            drop(shared_clone);
        });

        drop(local);
        assert!(!dropped.load(Ordering::Relaxed));

        handle.join().unwrap();
        assert!(!dropped.load(Ordering::Relaxed));

        drop(shared_owner);
        assert!(dropped.load(Ordering::Relaxed));
    });
}

#[cfg(feature = "track_threads")]
#[test]
fn test_thread_hybrid_recovers_local_on_same_thread() {
    loom::model(|| {
        let dropped = Arc::new(AtomicBool::new(false));
        let local: ThreadRc<DropTracker> = ThreadRc::new(DropTracker::new(dropped.clone(), 42));
        let shared: ThreadArc<DropTracker> =
            expect_ok(local.try_to_other(), "thread local should retain shared");

        let recovered: ThreadRc<DropTracker> =
            expect_ok(shared.try_to_other(), "thread shared should recover local");

        drop(local);
        assert!(!dropped.load(Ordering::Relaxed));
        drop(shared);
        assert!(!dropped.load(Ordering::Relaxed));
        drop(recovered);
        assert!(dropped.load(Ordering::Relaxed));
    });
}

#[cfg(feature = "track_threads")]
#[test]
fn test_thread_hybrid_rejects_local_recovery_on_other_thread() {
    loom::model(|| {
        let dropped = Arc::new(AtomicBool::new(false));
        let local: ThreadRc<DropTracker> = ThreadRc::new(DropTracker::new(dropped.clone(), 42));
        let shared: ThreadArc<DropTracker> =
            expect_ok(local.try_to_other(), "thread local should retain shared");

        let handle = thread::spawn(move || {
            expect_err(
                shared.try_into_other(),
                "thread shared should not recover local on another thread",
            )
        });

        let shared = handle.join().unwrap();
        assert!(!dropped.load(Ordering::Relaxed));

        drop(local);
        assert!(!dropped.load(Ordering::Relaxed));

        drop(shared);
        assert!(dropped.load(Ordering::Relaxed));
    });
}

#[cfg(feature = "track_threads")]
#[test]
fn test_thread_hybrid_shared_clone_while_local_present() {
    loom::model(|| {
        let dropped = Arc::new(AtomicBool::new(false));
        let shared: ThreadArc<DropTracker> = ThreadArc::new(DropTracker::new(dropped.clone(), 42));
        let local: ThreadRc<DropTracker> =
            expect_ok(shared.try_to_other(), "thread shared should retain local");

        let shared_owner = Arc::new(shared);
        let first_owner = shared_owner.clone();
        let second_owner = shared_owner.clone();

        let first = thread::spawn(move || {
            let clone = (*first_owner).clone();
            assert_eq!(clone.value, 42);
            drop(clone);
        });

        let second = thread::spawn(move || {
            let clone = (*second_owner).clone();
            assert_eq!(clone.value, 42);
            drop(clone);
        });

        first.join().unwrap();
        second.join().unwrap();

        drop(shared_owner);
        assert!(!dropped.load(Ordering::Relaxed));

        drop(local);
        assert!(dropped.load(Ordering::Relaxed));
    });
}
