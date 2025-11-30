use criterion::{Criterion, criterion_group, criterion_main};
use javascript::evaluate_script;
use std::hint::black_box;

// cargo bench --profile dev

// Initialize logger for benchmark so `RUST_LOG` is honored.
#[ctor::ctor]
fn __init_bench_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).try_init();
}

fn benchmark_promise_operations(c: &mut Criterion) {
    // Benchmark basic promise creation and resolution
    c.bench_function("promise_basic_resolution", |b| {
        b.iter(|| {
            let script = r#"
                let p = new Promise((resolve, reject) => {
                    resolve(42);
                });
                p.then(result => result * 2);
            "#;
            let _ = black_box(evaluate_script(script));
        })
    });

    // Benchmark promise chaining
    c.bench_function("promise_chaining", |b| {
        b.iter(|| {
            let script = r#"
                let p = new Promise((resolve, reject) => {
                    resolve(1);
                });
                p.then(x => x + 1)
                 .then(x => x * 2)
                 .then(x => x - 3)
                 .then(x => x / 2);
            "#;
            let _ = black_box(evaluate_script(script));
        })
    });

    // Benchmark promise rejection and catch
    c.bench_function("promise_rejection_catch", |b| {
        b.iter(|| {
            let script = r#"
                let p = new Promise((resolve, reject) => {
                    reject("error");
                });
                p.catch(err => "caught: " + err);
            "#;
            let _ = black_box(evaluate_script(script));
        })
    });

    // Benchmark Promise.all with multiple promises
    c.bench_function("promise_all_multiple", |b| {
        b.iter(|| {
            let script = r#"
                let p1 = new Promise(function(resolve, reject) {
                    resolve(1);
                });
                let p2 = new Promise(function(resolve, reject) {
                    resolve(2);
                });
                Promise.all([p1, p2]);
            "#;
            let _ = black_box(evaluate_script(script));
        })
    });

    // Benchmark Promise.race
    // c.bench_function("promise_race", |b| {
    //     b.iter(|| {
    //         let script = r#"
    //             let p1 = new Promise(resolve => resolve(1));
    //             let p2 = new Promise(resolve => resolve(2));
    //             Promise.race([p1, p2]);
    //         "#;
    //         black_box(evaluate_script(script).unwrap());
    //     })
    // });

    // Test arrow function syntax
    // c.bench_function("arrow_function_test", |b| {
    //     b.iter(|| {
    //         let script = r#"
    //             let f = x => x * 2;
    //             f(5);
    //         "#;
    //         black_box(evaluate_script(script).unwrap());
    //     })
    // });

    // Test Promise constructor syntax
    c.bench_function("promise_constructor_test", |b| {
        b.iter(|| {
            let script = r#"
                function test() { return 42; }
                test();
            "#;
            let _ = black_box(evaluate_script(script));
        })
    });

    // Test array syntax
    c.bench_function("array_test", |b| {
        b.iter(|| {
            let script = r#"
                let arr = [1, 2, 3];
                arr.length;
            "#;
            let _ = black_box(evaluate_script(script));
        })
    });

    // Benchmark async/await syntax
    c.bench_function("async_await_basic", |b| {
        b.iter(|| {
            let script = r#"
                async function test() {
                    let result = await Promise.resolve(42);
                    return result * 2;
                }
                test();
            "#;
            let _ = black_box(evaluate_script(script));
        })
    });

    // Benchmark complex promise chains with error handling
    c.bench_function("promise_complex_chain", |b| {
        b.iter(|| {
            let script = r#"
                function asyncOperation(value) {
                    return new Promise((resolve, reject) => {
                        if (value > 0) {
                            resolve(value * 2);
                        } else {
                            reject("negative value");
                        }
                    });
                }

                Promise.resolve(5)
                    .then(asyncOperation)
                    .then(result => result + 10)
                    .then(asyncOperation)
                    .catch(err => "error: " + err)
                    .finally(() => "cleanup");
            "#;
            black_box(evaluate_script(script)).unwrap();
        })
    });
}

criterion_group!(benches, benchmark_promise_operations);
criterion_main!(benches);
