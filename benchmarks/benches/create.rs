use std::hint::black_box;
use std::rc::Rc;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use flexrc::{HybridArc, HybridRc, SmallArc, SmallRc, ThreadArc, ThreadRc};

const ITERATIONS: usize = 10_000;

macro_rules! create {
    ($($name:expr, $op:expr),+) => {
        fn create(c: &mut Criterion) {
            let mut group = c.benchmark_group("Create and Destroy - Computed");

            let strings: Vec<String> = vec![0usize, 100, 16384]
                .into_iter()
                .map(|n| "x".repeat(n))
                .collect();

            for string in strings {
                $(let id = BenchmarkId::new($name, string.len());
                group.bench_with_input(id, string.as_str(), |b, s| b.iter(|| {
                    for _ in 0..ITERATIONS {
                        let s = $op(s);
                        black_box(&s);
                    }
                } ));)+
            }

            group.finish();
        }
    };
}

create!(
    "String",
    |s: &str| String::from(s),
    "Rc<str>",
    |s: &str| <Rc<str>>::from(s),
    "Arc<str>",
    |s: &str| <Arc<str>>::from(s),
    "SmallRc",
    |s: &str| -> SmallRc<str> { SmallRc::from(s) },
    "SmallArc",
    |s: &str| -> SmallArc<str> { SmallArc::from(s) },
    "HybridRc",
    |s: &str| -> HybridRc<str> { HybridRc::from(s) },
    "HybridArc",
    |s: &str| -> HybridArc<str> { HybridArc::from(s) },
    "ThreadRc",
    |s: &str| -> ThreadRc<str> { ThreadRc::from(s) },
    "ThreadArc",
    |s: &str| -> ThreadArc<str> { ThreadArc::from(s) }
);

criterion_group!(benches, create);
criterion_main!(benches);
