use criterion::{Criterion, criterion_group, criterion_main};
use num_bigint::BigInt;
use std::hint::black_box;

// Simple micro-benchmarks to measure BigInt parsing and arithmetic cost.
// Compares: parsing per-op (current engine approach) vs parsed/cached reuse.

// Initialize logger for benchmark so `RUST_LOG` is honored.
#[ctor::ctor]
fn __init_bench_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).try_init();
}

fn bench_bigint_parse_only(c: &mut Criterion) {
    let s = "1234567890123456789012345678901234567890n";
    c.bench_function("bigint_parse_bytes", |b| {
        b.iter(|| {
            // simulate the current parse path which strips trailing 'n' and parses base-10
            let s_trim = if s.ends_with('n') { &s[..s.len() - 1] } else { s };
            let _ = BigInt::parse_bytes(s_trim.as_bytes(), 10).unwrap();
        })
    });
}

fn bench_bigint_parse_and_add(c: &mut Criterion) {
    let a = "1234567890123456789012345678901234567890n";
    let bstr = "9876543210987654321098765432109876543210n";
    c.bench_function("bigint_parse_and_add", |ben| {
        ben.iter(|| {
            let a_trim = if a.ends_with('n') { &a[..a.len() - 1] } else { a };
            let b_trim = if bstr.ends_with('n') { &bstr[..bstr.len() - 1] } else { bstr };
            let aa = BigInt::parse_bytes(a_trim.as_bytes(), 10).unwrap();
            let bb = BigInt::parse_bytes(b_trim.as_bytes(), 10).unwrap();
            let _ = black_box((aa + bb).to_string());
        })
    });
}

fn bench_bigint_cached_add(c: &mut Criterion) {
    let a = BigInt::parse_bytes(b"1234567890123456789012345678901234567890", 10).unwrap();
    let b = BigInt::parse_bytes(b"9876543210987654321098765432109876543210", 10).unwrap();
    c.bench_function("bigint_cached_add", |ben| {
        ben.iter(|| {
            let _ = black_box((a.clone() + b.clone()).to_string());
        })
    });
}

criterion_group!(
    benches,
    bench_bigint_parse_only,
    bench_bigint_parse_and_add,
    bench_bigint_cached_add
);
criterion_main!(benches);
