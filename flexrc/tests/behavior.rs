use flexrc::{LocalHybridRc, LocalRc, SharedHybridRc, SharedRc};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

#[derive(Debug, Default)]
struct Counters {
    clones: AtomicUsize,
    drops: AtomicUsize,
}

#[derive(Debug)]
struct Tracked {
    counters: Arc<Counters>,
    value: usize,
}

impl Tracked {
    fn new(counters: Arc<Counters>, value: usize) -> Self {
        Self { counters, value }
    }
}

impl Clone for Tracked {
    fn clone(&self) -> Self {
        self.counters.clones.fetch_add(1, Ordering::SeqCst);
        Self {
            counters: self.counters.clone(),
            value: self.value,
        }
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.counters.drops.fetch_add(1, Ordering::SeqCst);
    }
}

fn tracked(value: usize) -> (Tracked, Arc<Counters>) {
    let counters = Arc::new(Counters::default());
    (Tracked::new(counters.clone(), value), counters)
}

fn expect_ok<T, E>(result: Result<T, E>, message: &str) -> T {
    match result {
        Ok(value) => value,
        Err(_) => panic!("{message}"),
    }
}

fn expect_err<T, E>(result: Result<T, E>, message: &str) -> E {
    match result {
        Ok(_) => panic!("{message}"),
        Err(value) => value,
    }
}

fn assert_counts(counters: &Counters, clones: usize, drops: usize) {
    assert_eq!(counters.clones.load(Ordering::SeqCst), clones);
    assert_eq!(counters.drops.load(Ordering::SeqCst), drops);
}

#[test]
fn regular_local_into_shared_transfers_unique_allocation() {
    let (value, counters) = tracked(7);
    let local: LocalRc<Tracked> = LocalRc::new(value);
    let original_data = &*local as *const Tracked;

    let shared: SharedRc<Tracked> = expect_ok(
        local.try_into_other(),
        "unique LocalRc should promote to SharedRc",
    );

    assert_eq!(shared.value, 7);
    assert!(ptr::addr_eq(&*shared as *const Tracked, original_data));
    assert_counts(&counters, 0, 0);

    drop(shared);
    assert_counts(&counters, 0, 1);
}

#[test]
fn regular_shared_into_local_transfers_unique_allocation() {
    let (value, counters) = tracked(11);
    let shared: SharedRc<Tracked> = SharedRc::new(value);
    let original_data = &*shared as *const Tracked;

    let local: LocalRc<Tracked> = expect_ok(
        shared.try_into_other(),
        "unique SharedRc should demote to LocalRc",
    );

    assert_eq!(local.value, 11);
    assert!(ptr::addr_eq(&*local as *const Tracked, original_data));
    assert_counts(&counters, 0, 0);

    drop(local);
    assert_counts(&counters, 0, 1);
}

#[test]
fn regular_consuming_conversion_fails_when_not_unique() {
    let (value, counters) = tracked(13);
    let local: LocalRc<Tracked> = LocalRc::new(value);
    let local_clone = local.clone();

    let local = expect_err(
        local.try_into_other(),
        "cloned LocalRc should not promote in place",
    );

    drop(local);
    assert_counts(&counters, 0, 0);
    drop(local_clone);
    assert_counts(&counters, 0, 1);

    let (value, counters) = tracked(17);
    let shared: SharedRc<Tracked> = SharedRc::new(value);
    let shared_clone = shared.clone();

    let shared = expect_err(
        shared.try_into_other(),
        "cloned SharedRc should not demote in place",
    );

    drop(shared);
    assert_counts(&counters, 0, 0);
    drop(shared_clone);
    assert_counts(&counters, 0, 1);
}

#[test]
fn regular_into_other_clones_data_when_in_place_conversion_fails() {
    let (value, counters) = tracked(19);
    let local: LocalRc<Tracked> = LocalRc::new(value);
    let original_data = &*local as *const Tracked;
    let local_clone = local.clone();

    let shared: SharedRc<Tracked> = local.into_other();

    assert_eq!(shared.value, 19);
    assert!(!ptr::addr_eq(&*shared as *const Tracked, original_data));
    assert_counts(&counters, 1, 0);

    drop(shared);
    assert_counts(&counters, 1, 1);
    drop(local_clone);
    assert_counts(&counters, 1, 2);
}

#[test]
fn hybrid_local_to_shared_retains_and_transfers_counts() {
    let (value, counters) = tracked(23);
    let local: LocalHybridRc<Tracked> = LocalHybridRc::new(value);
    let original_data = &*local as *const Tracked;

    let shared: SharedHybridRc<Tracked> = expect_ok(
        local.try_to_other(),
        "LocalHybridRc should retain a SharedHybridRc",
    );

    assert!(ptr::addr_eq(&*shared as *const Tracked, original_data));

    let shared_clone = shared.clone();
    drop(shared_clone);
    assert_counts(&counters, 0, 0);

    drop(local);
    assert_counts(&counters, 0, 0);
    drop(shared);
    assert_counts(&counters, 0, 1);

    let (value, counters) = tracked(29);
    let local: LocalHybridRc<Tracked> = LocalHybridRc::new(value);
    let local_clone = local.clone();
    let shared: SharedHybridRc<Tracked> = expect_ok(
        local.try_into_other(),
        "LocalHybridRc should transfer into SharedHybridRc",
    );

    drop(shared);
    assert_counts(&counters, 0, 0);
    drop(local_clone);
    assert_counts(&counters, 0, 1);
}

#[test]
fn hybrid_shared_to_local_retains_and_transfers_counts() {
    let (value, counters) = tracked(31);
    let shared: SharedHybridRc<Tracked> = SharedHybridRc::new(value);
    let original_data = &*shared as *const Tracked;

    let local: LocalHybridRc<Tracked> = expect_ok(
        shared.try_to_other(),
        "SharedHybridRc should retain a LocalHybridRc when no local exists",
    );

    assert!(ptr::addr_eq(&*local as *const Tracked, original_data));

    let shared_clone = shared.clone();
    drop(shared);
    assert_counts(&counters, 0, 0);
    drop(shared_clone);
    assert_counts(&counters, 0, 0);
    drop(local);
    assert_counts(&counters, 0, 1);

    let (value, counters) = tracked(37);
    let shared: SharedHybridRc<Tracked> = SharedHybridRc::new(value);
    let shared_clone = shared.clone();
    let local: LocalHybridRc<Tracked> = expect_ok(
        shared.try_into_other(),
        "SharedHybridRc should transfer into LocalHybridRc when no local exists",
    );

    drop(local);
    assert_counts(&counters, 0, 0);
    drop(shared_clone);
    assert_counts(&counters, 0, 1);
}

#[test]
fn get_mut_only_succeeds_for_unique_owners() {
    let (value, _counters) = tracked(41);
    let mut local: LocalRc<Tracked> = LocalRc::new(value);
    assert_eq!(local.get_mut().expect("unique local").value, 41);
    let local_clone = local.clone();
    assert!(local.get_mut().is_none());
    drop(local_clone);
    local.get_mut().expect("local unique again").value = 43;
    assert_eq!(local.value, 43);

    let (value, _counters) = tracked(47);
    let mut shared: SharedRc<Tracked> = SharedRc::new(value);
    assert_eq!(shared.get_mut().expect("unique shared").value, 47);
    let shared_clone = shared.clone();
    assert!(shared.get_mut().is_none());
    drop(shared_clone);
    shared.get_mut().expect("shared unique again").value = 53;
    assert_eq!(shared.value, 53);

    let (value, _counters) = tracked(59);
    let mut hybrid: LocalHybridRc<Tracked> = LocalHybridRc::new(value);
    assert_eq!(hybrid.get_mut().expect("unique hybrid local").value, 59);
    let shared: SharedHybridRc<Tracked> = expect_ok(
        hybrid.try_to_other(),
        "LocalHybridRc should retain a SharedHybridRc",
    );
    assert!(hybrid.get_mut().is_none());
    drop(shared);
    hybrid.get_mut().expect("hybrid local unique again").value = 61;
    assert_eq!(hybrid.value, 61);
}

#[test]
fn shared_handles_clone_and_drop_across_threads() {
    let (value, counters) = tracked(67);
    let shared: SharedRc<Tracked> = SharedRc::new(value);
    let thread_shared = shared.clone();

    thread::spawn(move || {
        assert_eq!(thread_shared.value, 67);
        let clone = thread_shared.clone();
        drop(clone);
    })
    .join()
    .expect("thread should finish");

    assert_counts(&counters, 0, 0);
    drop(shared);
    assert_counts(&counters, 0, 1);

    let (value, counters) = tracked(71);
    let shared: SharedHybridRc<Tracked> = SharedHybridRc::new(value);
    let thread_shared = shared.clone();

    thread::spawn(move || {
        assert_eq!(thread_shared.value, 71);
        let clone = thread_shared.clone();
        drop(clone);
    })
    .join()
    .expect("thread should finish");

    assert_counts(&counters, 0, 0);
    drop(shared);
    assert_counts(&counters, 0, 1);
}

#[cfg(not(feature = "str_deref"))]
#[test]
fn byte_slice_round_trips_through_regular_promotion() {
    let local: LocalRc<[u8]> = LocalRc::from_slice(b"flex");
    assert_eq!(&*local, b"flex");

    let shared: SharedRc<[u8]> = expect_ok(
        local.try_into_other(),
        "unique byte slice should promote without copying",
    );
    assert_eq!(&*shared, b"flex");
}

#[cfg(feature = "str_deref")]
#[test]
fn str_deref_round_trips_through_regular_promotion() {
    let local: LocalRc<[u8]> = LocalRc::from_str_ref("flex");
    assert_eq!(&*local, "flex");

    let shared: SharedRc<[u8]> = expect_ok(
        local.try_into_other(),
        "unique string bytes should promote without copying",
    );
    assert_eq!(&*shared, "flex");
}
