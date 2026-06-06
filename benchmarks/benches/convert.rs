use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use flexrc::{
    LocalHybridRc, LocalRc, LocalThreadRc, SharedHybridRc, SharedRc, SharedThreadRc,
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
    "Regular LocalRc -> SharedRc / into_other",
    || LocalRc::new(black_box(1usize)),
    |local: LocalRc<usize>| {
        let shared: SharedRc<usize> = local.into_other();
        black_box(shared);
    },
    "Regular SharedRc -> LocalRc / into_other",
    || SharedRc::new(black_box(1usize)),
    |shared: SharedRc<usize>| {
        let local: LocalRc<usize> = shared.into_other();
        black_box(local);
    },
    "Regular LocalRc -> SharedRc / to_other",
    || LocalRc::new(black_box(1usize)),
    |local: LocalRc<usize>| {
        let shared: SharedRc<usize> = local.to_other();
        black_box(&local);
        black_box(shared);
    },
    "Regular SharedRc -> LocalRc / to_other",
    || SharedRc::new(black_box(1usize)),
    |shared: SharedRc<usize>| {
        let local: LocalRc<usize> = shared.to_other();
        black_box(&shared);
        black_box(local);
    },
    "Hybrid LocalHybridRc -> SharedHybridRc / into_other",
    || LocalHybridRc::new(black_box(1usize)),
    |local: LocalHybridRc<usize>| {
        let shared: SharedHybridRc<usize> = local.into_other();
        black_box(shared);
    },
    "Hybrid SharedHybridRc -> LocalHybridRc / into_other",
    || SharedHybridRc::new(black_box(1usize)),
    |shared: SharedHybridRc<usize>| {
        let local: LocalHybridRc<usize> = shared.into_other();
        black_box(local);
    },
    "Hybrid LocalHybridRc -> SharedHybridRc / to_other",
    || LocalHybridRc::new(black_box(1usize)),
    |local: LocalHybridRc<usize>| {
        let shared: SharedHybridRc<usize> = local.to_other();
        black_box(&local);
        black_box(shared);
    },
    "Hybrid SharedHybridRc -> LocalHybridRc / to_other",
    || SharedHybridRc::new(black_box(1usize)),
    |shared: SharedHybridRc<usize>| {
        let local: LocalHybridRc<usize> = shared.to_other();
        black_box(&shared);
        black_box(local);
    },
    "Thread LocalThreadRc -> SharedThreadRc / into_other",
    || LocalThreadRc::new(black_box(1usize)),
    |local: LocalThreadRc<usize>| {
        let shared: SharedThreadRc<usize> = local.into_other();
        black_box(shared);
    },
    "Thread SharedThreadRc -> LocalThreadRc / into_other",
    || SharedThreadRc::new(black_box(1usize)),
    |shared: SharedThreadRc<usize>| {
        let local: LocalThreadRc<usize> = shared.into_other();
        black_box(local);
    },
    "Thread LocalThreadRc -> SharedThreadRc / to_other",
    || LocalThreadRc::new(black_box(1usize)),
    |local: LocalThreadRc<usize>| {
        let shared: SharedThreadRc<usize> = local.to_other();
        black_box(&local);
        black_box(shared);
    },
    "Thread SharedThreadRc -> LocalThreadRc / to_other",
    || SharedThreadRc::new(black_box(1usize)),
    |shared: SharedThreadRc<usize>| {
        let local: LocalThreadRc<usize> = shared.to_other();
        black_box(&shared);
        black_box(local);
    },
    "Thread SharedThreadRc -> LocalThreadRc / to_other local present",
    || {
        let local = LocalThreadRc::new(black_box(1usize));
        let shared: SharedThreadRc<usize> = local.to_other();
        (local, shared)
    },
    |(local, shared): (LocalThreadRc<usize>, SharedThreadRc<usize>)| {
        let recovered: LocalThreadRc<usize> = shared.to_other();
        black_box(&local);
        black_box(&shared);
        black_box(recovered);
    },
    "Thread SharedThreadRc -> LocalThreadRc / into_other local present",
    || {
        let local = LocalThreadRc::new(black_box(1usize));
        let shared: SharedThreadRc<usize> = local.to_other();
        (local, shared)
    },
    |(local, shared): (LocalThreadRc<usize>, SharedThreadRc<usize>)| {
        let recovered: LocalThreadRc<usize> = shared.into_other();
        black_box(&local);
        black_box(recovered);
    }
);

criterion_group!(benches, convert);
criterion_main!(benches);
