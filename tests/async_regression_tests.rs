use javascript::*;
use serde_json::Value;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_async_n_throw_async_tests_regression() {
    // Increase stack size to handle recursion in event loop during async tests
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let path = std::path::Path::new("js-scripts/async_n_throw_async_tests_regression.js");
            let script = read_script_file(path).expect("failed to read regression script");

            // Append extraction of the exposed global summary as a JSON string so evaluate_script
            // returns it as the final result for easy assertion.
            let wrapped = format!("{}\nJSON.stringify(globalThis.__async_regression_summary);", script);

            let result = evaluate_script(&wrapped, Some(path)).expect("evaluate_script failed");

            // evaluate_script wraps returned JS strings as JSON quoted strings, so parse twice
            let json_str: String = serde_json::from_str(&result).expect("expected JSON string");
            let v: Value = serde_json::from_str(&json_str).expect("expected JSON object");

            assert_eq!(v["passed"].as_i64().expect("passed is number"), 3, "expected 3 passed tests");
            assert_eq!(v["failed"].as_i64().expect("failed is number"), 0, "expected 0 failed tests");
        })
        .expect("failed to spawn thread")
        .join()
        .expect("thread panicked");
}
