use flexrc::{HybridArc, HybridRc, SmallArc};
#[cfg(feature = "track_threads")]
use flexrc::{ThreadArc, ThreadRc};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

const DEFAULT_ITERS: usize = 32;
const DEFAULT_THREADS: usize = 4;
const DEFAULT_OPS: usize = 64;

#[derive(Debug)]
struct Tracked {
    drops: Arc<AtomicUsize>,
    value: usize,
}

impl Tracked {
    fn new(drops: Arc<AtomicUsize>, value: usize) -> Self {
        Self { drops, value }
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn stress_iters() -> usize {
    env_usize("FLEXRC_STRESS_ITERS", DEFAULT_ITERS)
}

fn stress_threads() -> usize {
    env_usize("FLEXRC_STRESS_THREADS", DEFAULT_THREADS)
}

fn stress_ops() -> usize {
    env_usize("FLEXRC_STRESS_OPS", DEFAULT_OPS)
}

fn maybe_yield(seed: usize) {
    if seed & 3 == 0 {
        thread::yield_now();
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

#[test]
fn small_arc_clone_drop_stress() {
    let iterations = stress_iters();
    let threads = stress_threads();
    let ops = stress_ops();

    for iteration in 0..iterations {
        let drops = Arc::new(AtomicUsize::new(0));
        let shared = SmallArc::new(Tracked::new(drops.clone(), iteration));
        let barrier = Arc::new(Barrier::new(threads + 1));

        let handles: Vec<_> = (0..threads)
            .map(|thread_id| {
                let shared = shared.clone();
                let barrier = barrier.clone();

                thread::spawn(move || {
                    barrier.wait();

                    for op in 0..ops {
                        let clone = shared.clone();
                        assert_eq!(clone.value, iteration);
                        maybe_yield(iteration ^ thread_id ^ op);
                        drop(clone);
                    }

                    maybe_yield(iteration ^ thread_id);
                    drop(shared);
                })
            })
            .collect();

        barrier.wait();
        maybe_yield(iteration);
        drop(shared);

        for handle in handles {
            handle.join().expect("worker should finish");
        }

        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn hybrid_local_shared_coexistence_stress() {
    let iterations = stress_iters();
    let threads = stress_threads();
    let ops = stress_ops();

    for iteration in 0..iterations {
        let drops = Arc::new(AtomicUsize::new(0));
        let local = HybridRc::new(Tracked::new(drops.clone(), iteration));
        let shared: HybridArc<Tracked> =
            expect_ok(local.try_to_other(), "hybrid local should retain shared");
        let barrier = Arc::new(Barrier::new(threads + 1));

        let handles: Vec<_> = (0..threads)
            .map(|thread_id| {
                let shared = shared.clone();
                let barrier = barrier.clone();

                thread::spawn(move || {
                    barrier.wait();

                    for op in 0..ops {
                        let clone = shared.clone();
                        assert_eq!(clone.value, iteration);
                        maybe_yield(iteration ^ thread_id ^ op);
                        drop(clone);
                    }

                    maybe_yield(iteration ^ thread_id);
                    drop(shared);
                })
            })
            .collect();

        barrier.wait();

        for op in 0..ops {
            let clone = local.clone();
            assert_eq!(clone.value, iteration);
            maybe_yield(iteration ^ op);
            drop(clone);
        }

        drop(local);
        assert_eq!(drops.load(Ordering::SeqCst), 0);

        for handle in handles {
            handle.join().expect("worker should finish");
        }

        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(shared);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn hybrid_shared_local_recovery_stress() {
    let iterations = stress_iters();
    let ops = stress_ops();

    for iteration in 0..iterations {
        let drops = Arc::new(AtomicUsize::new(0));
        let shared = HybridArc::new(Tracked::new(drops.clone(), iteration));

        for op in 0..ops {
            let local: HybridRc<Tracked> = expect_ok(
                shared.try_to_other(),
                "unique hybrid shared should recover local",
            );
            assert_eq!(local.value, iteration);
            maybe_yield(iteration ^ op);
            drop(local);

            let clone = shared.clone();
            assert_eq!(clone.value, iteration);
            maybe_yield(iteration ^ op ^ 0x55);
            drop(clone);
        }

        assert_eq!(shared.value, iteration);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(shared);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

#[cfg(feature = "track_threads")]
#[test]
fn thread_hybrid_same_thread_recovery_stress() {
    let iterations = stress_iters();
    let ops = stress_ops();

    for iteration in 0..iterations {
        let drops = Arc::new(AtomicUsize::new(0));
        let local = ThreadRc::new(Tracked::new(drops.clone(), iteration));
        let shared: ThreadArc<Tracked> =
            expect_ok(local.try_to_other(), "thread local should retain shared");

        for op in 0..ops {
            let recovered: ThreadRc<Tracked> = expect_ok(
                shared.try_to_other(),
                "same thread should recover local while local exists",
            );
            assert_eq!(recovered.value, iteration);
            maybe_yield(iteration ^ op);
            drop(recovered);
        }

        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(local);
        drop(shared);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

#[cfg(feature = "track_threads")]
#[test]
fn thread_hybrid_other_thread_recovery_rejected_stress() {
    let iterations = stress_iters();
    let threads = stress_threads();
    let ops = stress_ops();

    for iteration in 0..iterations {
        let drops = Arc::new(AtomicUsize::new(0));
        let local = ThreadRc::new(Tracked::new(drops.clone(), iteration));
        let shared: ThreadArc<Tracked> =
            expect_ok(local.try_to_other(), "thread local should retain shared");
        let barrier = Arc::new(Barrier::new(threads + 1));

        let handles: Vec<_> = (0..threads)
            .map(|thread_id| {
                let shared = shared.clone();
                let barrier = barrier.clone();

                thread::spawn(move || {
                    barrier.wait();

                    for op in 0..ops {
                        let rejected = expect_err(
                            shared.try_to_other(),
                            "other thread must not recover local",
                        );
                        assert_eq!(rejected.value, iteration);
                        maybe_yield(iteration ^ thread_id ^ op);

                        let clone = rejected.clone();
                        assert_eq!(clone.value, iteration);
                        drop(clone);
                    }

                    drop(shared);
                })
            })
            .collect();

        barrier.wait();

        for op in 0..ops {
            let recovered: ThreadRc<Tracked> = expect_ok(
                shared.try_to_other(),
                "owner thread should recover local while local exists",
            );
            assert_eq!(recovered.value, iteration);
            maybe_yield(iteration ^ op);
            drop(recovered);
        }

        for handle in handles {
            handle.join().expect("worker should finish");
        }

        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(local);
        drop(shared);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}
