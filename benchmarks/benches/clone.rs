use std::rc::Rc;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use flexrc::{LocalRc, SharedRc};

const ITERATIONS: usize = 10_000;

macro_rules! clone {
    ($($name:expr, $setup:expr),+) => {
        fn clone(c: &mut Criterion) {
            let mut group = c.benchmark_group("Clone - Computed");
            let lengths = vec![0usize, 10, 20, 100, 1000, 16384];

            for len in lengths {
                $(let id = BenchmarkId::new($name, len);
                group.bench_function(id, |b| {
                    b.iter_batched(|| $setup(len), |s| {
                        for _ in 0..ITERATIONS{
                            let s2 = s.clone();
                            black_box(&s);
                            black_box(&s2);
                        }
                    }, BatchSize::SmallInput)
                });)+
            }

            group.finish();
        }
    };
}

clone!(
    "String",
    |len| "x".repeat(len),
    "Rc<str>",
    |len| -> Rc<str> { "x".repeat(len).into() },
    "Arc<str>",
    |len| -> Arc<str> { "x".repeat(len).into() },
    "LocalRc",
    |len| LocalRc::from_str_ref(&*"x".repeat(len)),
    "SharedRc",
    |len| SharedRc::from_str_ref(&*"x".repeat(len))
);

criterion_group!(benches, clone);
criterion_main!(benches);
