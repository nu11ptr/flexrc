use flexrc::{HybridArc, HybridMeta, HybridRc, LocalMode, Meta, SharedMode, SmallArc, SmallRc};
#[cfg(feature = "track_threads")]
use flexrc::{ThreadArc, ThreadHybridMeta, ThreadRc};
use std::mem;
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
fn metadata_sizes_match_active_counter_widths() {
    #[cfg(any(not(feature = "small_counters"), feature = "track_threads"))]
    let word = mem::size_of::<usize>();

    #[cfg(not(feature = "small_counters"))]
    {
        assert_eq!(mem::size_of::<Meta<LocalMode>>(), word);
        assert_eq!(mem::size_of::<Meta<SharedMode>>(), word);
        assert_eq!(mem::size_of::<HybridMeta<LocalMode>>(), word * 2);
        assert_eq!(mem::size_of::<HybridMeta<SharedMode>>(), word * 2);

        #[cfg(feature = "track_threads")]
        {
            assert_eq!(mem::size_of::<ThreadHybridMeta<LocalMode>>(), word * 3);
            assert_eq!(mem::size_of::<ThreadHybridMeta<SharedMode>>(), word * 3);
        }
    }

    #[cfg(feature = "small_counters")]
    {
        assert_eq!(mem::size_of::<Meta<LocalMode>>(), 4);
        assert_eq!(mem::size_of::<Meta<SharedMode>>(), 4);
        assert_eq!(mem::size_of::<HybridMeta<LocalMode>>(), 8);
        assert_eq!(mem::size_of::<HybridMeta<SharedMode>>(), 8);

        #[cfg(feature = "track_threads")]
        {
            assert_eq!(mem::size_of::<ThreadHybridMeta<LocalMode>>(), word + 8);
            assert_eq!(mem::size_of::<ThreadHybridMeta<SharedMode>>(), word + 8);
        }
    }
}

#[test]
fn regular_local_into_shared_transfers_unique_allocation() {
    let (value, counters) = tracked(7);
    let local: SmallRc<Tracked> = SmallRc::new(value);
    let original_data = &*local as *const Tracked;

    let shared: SmallArc<Tracked> = expect_ok(
        local.try_into_other(),
        "unique SmallRc should promote to SmallArc",
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
    let shared: SmallArc<Tracked> = SmallArc::new(value);
    let original_data = &*shared as *const Tracked;

    let local: SmallRc<Tracked> = expect_ok(
        shared.try_into_other(),
        "unique SmallArc should demote to SmallRc",
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
    let local: SmallRc<Tracked> = SmallRc::new(value);
    let local_clone = local.clone();

    let local = expect_err(
        local.try_into_other(),
        "cloned SmallRc should not promote in place",
    );

    drop(local);
    assert_counts(&counters, 0, 0);
    drop(local_clone);
    assert_counts(&counters, 0, 1);

    let (value, counters) = tracked(17);
    let shared: SmallArc<Tracked> = SmallArc::new(value);
    let shared_clone = shared.clone();

    let shared = expect_err(
        shared.try_into_other(),
        "cloned SmallArc should not demote in place",
    );

    drop(shared);
    assert_counts(&counters, 0, 0);
    drop(shared_clone);
    assert_counts(&counters, 0, 1);
}

#[test]
fn regular_into_other_clones_data_when_in_place_conversion_fails() {
    let (value, counters) = tracked(19);
    let local: SmallRc<Tracked> = SmallRc::new(value);
    let original_data = &*local as *const Tracked;
    let local_clone = local.clone();

    let shared: SmallArc<Tracked> = local.into_other();

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
    let local: HybridRc<Tracked> = HybridRc::new(value);
    let original_data = &*local as *const Tracked;

    let shared: HybridArc<Tracked> =
        expect_ok(local.try_to_other(), "HybridRc should retain a HybridArc");

    assert!(ptr::addr_eq(&*shared as *const Tracked, original_data));

    let shared_clone = shared.clone();
    drop(shared_clone);
    assert_counts(&counters, 0, 0);

    drop(local);
    assert_counts(&counters, 0, 0);
    drop(shared);
    assert_counts(&counters, 0, 1);

    let (value, counters) = tracked(29);
    let local: HybridRc<Tracked> = HybridRc::new(value);
    let local_clone = local.clone();
    let shared: HybridArc<Tracked> = expect_ok(
        local.try_into_other(),
        "HybridRc should transfer into HybridArc",
    );

    drop(shared);
    assert_counts(&counters, 0, 0);
    drop(local_clone);
    assert_counts(&counters, 0, 1);
}

#[test]
fn hybrid_shared_to_local_retains_and_transfers_counts() {
    let (value, counters) = tracked(31);
    let shared: HybridArc<Tracked> = HybridArc::new(value);
    let original_data = &*shared as *const Tracked;

    let local: HybridRc<Tracked> = expect_ok(
        shared.try_to_other(),
        "HybridArc should retain a HybridRc when no local exists",
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
    let shared: HybridArc<Tracked> = HybridArc::new(value);
    let shared_clone = shared.clone();
    let local: HybridRc<Tracked> = expect_ok(
        shared.try_into_other(),
        "HybridArc should transfer into HybridRc when no local exists",
    );

    drop(local);
    assert_counts(&counters, 0, 0);
    drop(shared_clone);
    assert_counts(&counters, 0, 1);
}

#[test]
fn hybrid_shared_to_local_fails_when_local_is_already_present() {
    let (value, counters) = tracked(39);
    let local: HybridRc<Tracked> = HybridRc::new(value);
    let shared: HybridArc<Tracked> =
        expect_ok(local.try_to_other(), "HybridRc should retain a HybridArc");

    assert!(shared.try_to_other().is_err());

    drop(local);
    assert_counts(&counters, 0, 0);
    drop(shared);
    assert_counts(&counters, 0, 1);
}

#[cfg(feature = "track_threads")]
#[test]
fn thread_hybrid_shared_to_local_recovers_on_same_thread() {
    let (value, counters) = tracked(40);
    let local: ThreadRc<Tracked> = ThreadRc::new(value);
    let original_data = &*local as *const Tracked;
    let shared: ThreadArc<Tracked> =
        expect_ok(local.try_to_other(), "ThreadRc should retain a ThreadArc");

    let recovered: ThreadRc<Tracked> = expect_ok(
        shared.try_to_other(),
        "ThreadArc should recover a local handle on the tracked thread",
    );

    assert!(ptr::addr_eq(&*recovered as *const Tracked, original_data));

    drop(local);
    assert_counts(&counters, 0, 0);
    drop(shared);
    assert_counts(&counters, 0, 0);
    drop(recovered);
    assert_counts(&counters, 0, 1);

    let (value, counters) = tracked(42);
    let local: ThreadRc<Tracked> = ThreadRc::new(value);
    let shared: ThreadArc<Tracked> =
        expect_ok(local.try_to_other(), "ThreadRc should retain a ThreadArc");
    let shared_clone = shared.clone();
    let recovered: ThreadRc<Tracked> = expect_ok(
        shared.try_into_other(),
        "ThreadArc should transfer to local on the tracked thread",
    );

    drop(local);
    assert_counts(&counters, 0, 0);
    drop(recovered);
    assert_counts(&counters, 0, 0);
    drop(shared_clone);
    assert_counts(&counters, 0, 1);
}

#[cfg(feature = "track_threads")]
#[test]
fn thread_hybrid_shared_to_local_rejects_other_threads() {
    let (value, counters) = tracked(44);
    let local: ThreadRc<Tracked> = ThreadRc::new(value);
    let shared: ThreadArc<Tracked> =
        expect_ok(local.try_to_other(), "ThreadRc should retain a ThreadArc");

    let shared = thread::spawn(move || {
        let shared = expect_err(
            shared.try_into_other(),
            "ThreadArc should not transfer to local on another thread",
        );
        assert!(shared.try_to_other().is_err());
        shared
    })
    .join()
    .expect("thread should finish");

    drop(local);
    assert_counts(&counters, 0, 0);
    drop(shared);
    assert_counts(&counters, 0, 1);
}

#[test]
fn get_mut_only_succeeds_for_unique_owners() {
    let (value, _counters) = tracked(41);
    let mut local: SmallRc<Tracked> = SmallRc::new(value);
    assert_eq!(local.get_mut().expect("unique local").value, 41);
    let local_clone = local.clone();
    assert!(local.get_mut().is_none());
    drop(local_clone);
    local.get_mut().expect("local unique again").value = 43;
    assert_eq!(local.value, 43);

    let (value, _counters) = tracked(47);
    let mut shared: SmallArc<Tracked> = SmallArc::new(value);
    assert_eq!(shared.get_mut().expect("unique shared").value, 47);
    let shared_clone = shared.clone();
    assert!(shared.get_mut().is_none());
    drop(shared_clone);
    shared.get_mut().expect("shared unique again").value = 53;
    assert_eq!(shared.value, 53);

    let (value, _counters) = tracked(59);
    let mut hybrid: HybridRc<Tracked> = HybridRc::new(value);
    assert_eq!(hybrid.get_mut().expect("unique hybrid local").value, 59);
    let shared: HybridArc<Tracked> =
        expect_ok(hybrid.try_to_other(), "HybridRc should retain a HybridArc");
    assert!(hybrid.get_mut().is_none());
    drop(shared);
    hybrid.get_mut().expect("hybrid local unique again").value = 61;
    assert_eq!(hybrid.value, 61);

    #[cfg(feature = "track_threads")]
    {
        let (value, _counters) = tracked(63);
        let mut tracked_hybrid: ThreadRc<Tracked> = ThreadRc::new(value);
        assert_eq!(
            tracked_hybrid
                .get_mut()
                .expect("unique thread hybrid local")
                .value,
            63
        );
        let shared: ThreadArc<Tracked> = expect_ok(
            tracked_hybrid.try_to_other(),
            "ThreadRc should retain a ThreadArc",
        );
        assert!(tracked_hybrid.get_mut().is_none());
        drop(shared);
        tracked_hybrid
            .get_mut()
            .expect("thread hybrid local unique again")
            .value = 65;
        assert_eq!(tracked_hybrid.value, 65);
    }
}

#[test]
fn shared_handles_clone_and_drop_across_threads() {
    let (value, counters) = tracked(67);
    let shared: SmallArc<Tracked> = SmallArc::new(value);
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
    let shared: HybridArc<Tracked> = HybridArc::new(value);
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

    #[cfg(feature = "track_threads")]
    {
        let (value, counters) = tracked(73);
        let shared: ThreadArc<Tracked> = ThreadArc::new(value);
        let thread_shared = shared.clone();

        thread::spawn(move || {
            assert_eq!(thread_shared.value, 73);
            let clone = thread_shared.clone();
            drop(clone);
        })
        .join()
        .expect("thread should finish");

        assert_counts(&counters, 0, 0);
        drop(shared);
        assert_counts(&counters, 0, 1);
    }
}

#[cfg(not(feature = "str_deref"))]
#[test]
fn byte_slice_round_trips_through_regular_promotion() {
    let local: SmallRc<[u8]> = SmallRc::from_slice(b"flex");
    assert_eq!(&*local, b"flex");

    let shared: SmallArc<[u8]> = expect_ok(
        local.try_into_other(),
        "unique byte slice should promote without copying",
    );
    assert_eq!(&*shared, b"flex");
}

#[cfg(feature = "str_deref")]
#[test]
fn str_deref_round_trips_through_regular_promotion() {
    let local: SmallRc<[u8]> = SmallRc::from_str_ref("flex");
    assert_eq!(&*local, "flex");

    let shared: SmallArc<[u8]> = expect_ok(
        local.try_into_other(),
        "unique string bytes should promote without copying",
    );
    assert_eq!(&*shared, "flex");
}
