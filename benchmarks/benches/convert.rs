use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use flexrc::{
    HybridRc, SmallRc, ThreadRc, HybridArc, SmallArc, ThreadArc,
};

macro_rules! convert {
    ($($name:expr, $setup:expr, $body:expr),+) => {
        fn convert(c: &mut Criterion) {
            let mut group = c.benchmark_group("Convert - Computed");

            $(group.bench_function($name, |b| {
                b.iter_batched($setup, $body, BatchSize::SmallInput);
            });)+

            group.finish();
        }
    };
}

convert!(
    "SmallRc -> SmallArc / into_other",
    || SmallRc::new(black_box(1usize)),
    |local: SmallRc<usize>| {
        let shared: SmallArc<usize> = local.into_other();
        black_box(shared);
    },
    "SmallArc -> SmallRc / into_other",
    || SmallArc::new(black_box(1usize)),
    |shared: SmallArc<usize>| {
        let local: SmallRc<usize> = shared.into_other();
        black_box(local);
    },
    "SmallRc -> SmallArc / to_other",
    || SmallRc::new(black_box(1usize)),
    |local: SmallRc<usize>| {
        let shared: SmallArc<usize> = local.to_other();
        black_box(&local);
        black_box(shared);
    },
    "SmallArc -> SmallRc / to_other",
    || SmallArc::new(black_box(1usize)),
    |shared: SmallArc<usize>| {
        let local: SmallRc<usize> = shared.to_other();
        black_box(&shared);
        black_box(local);
    },
    "HybridRc -> HybridArc / into_other",
    || HybridRc::new(black_box(1usize)),
    |local: HybridRc<usize>| {
        let shared: HybridArc<usize> = local.into_other();
        black_box(shared);
    },
    "HybridArc -> HybridRc / into_other",
    || HybridArc::new(black_box(1usize)),
    |shared: HybridArc<usize>| {
        let local: HybridRc<usize> = shared.into_other();
        black_box(local);
    },
    "HybridRc -> HybridArc / to_other",
    || HybridRc::new(black_box(1usize)),
    |local: HybridRc<usize>| {
        let shared: HybridArc<usize> = local.to_other();
        black_box(&local);
        black_box(shared);
    },
    "HybridArc -> HybridRc / to_other",
    || HybridArc::new(black_box(1usize)),
    |shared: HybridArc<usize>| {
        let local: HybridRc<usize> = shared.to_other();
        black_box(&shared);
        black_box(local);
    },
    "ThreadRc -> ThreadArc / into_other",
    || ThreadRc::new(black_box(1usize)),
    |local: ThreadRc<usize>| {
        let shared: ThreadArc<usize> = local.into_other();
        black_box(shared);
    },
    "ThreadArc -> ThreadRc / into_other",
    || ThreadArc::new(black_box(1usize)),
    |shared: ThreadArc<usize>| {
        let local: ThreadRc<usize> = shared.into_other();
        black_box(local);
    },
    "ThreadRc -> ThreadArc / to_other",
    || ThreadRc::new(black_box(1usize)),
    |local: ThreadRc<usize>| {
        let shared: ThreadArc<usize> = local.to_other();
        black_box(&local);
        black_box(shared);
    },
    "ThreadArc -> ThreadRc / to_other",
    || ThreadArc::new(black_box(1usize)),
    |shared: ThreadArc<usize>| {
        let local: ThreadRc<usize> = shared.to_other();
        black_box(&shared);
        black_box(local);
    },
    "ThreadArc -> ThreadRc / to_other local present",
    || {
        let local = ThreadRc::new(black_box(1usize));
        let shared: ThreadArc<usize> = local.to_other();
        (local, shared)
    },
    |(local, shared): (ThreadRc<usize>, ThreadArc<usize>)| {
        let recovered: ThreadRc<usize> = shared.to_other();
        black_box(&local);
        black_box(&shared);
        black_box(recovered);
    },
    "ThreadArc -> ThreadRc / into_other local present",
    || {
        let local = ThreadRc::new(black_box(1usize));
        let shared: ThreadArc<usize> = local.to_other();
        (local, shared)
    },
    |(local, shared): (ThreadRc<usize>, ThreadArc<usize>)| {
        let recovered: ThreadRc<usize> = shared.into_other();
        black_box(&local);
        black_box(recovered);
    }
);

criterion_group!(benches, convert);
criterion_main!(benches);
