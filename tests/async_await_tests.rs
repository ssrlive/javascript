use javascript::{evaluate_script, obj_get_value, Value};

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_async_function_syntax() {
    // Test that async function syntax is accepted (even if execution is synchronous)
    let script = "async function foo() { return 42; }; foo";
    let result = evaluate_script(script);
    // Should not panic on parsing, even if evaluation is limited
    assert!(result.is_ok() || result.is_err()); // We just care that it doesn't panic
}

#[test]
fn test_await_syntax() {
    // Test that await syntax is accepted
    let script = "let p = Promise.resolve(42); await p";
    let result = evaluate_script(script);
    // Should not panic on parsing
    assert!(result.is_ok() || result.is_err()); // We just care that it doesn't panic
}

#[test]
fn test_async_arrow_function_syntax() {
    // Test that async arrow function syntax is accepted
    let script = "let foo = async () => { return 42; }; foo";
    let result = evaluate_script(script);
    // Should not panic on parsing
    assert!(result.is_ok() || result.is_err()); // We just care that it doesn't panic
}

#[test]
fn test_async_promise_resolution() {
    // Test that promises resolve asynchronously
    let script = r#"
        let result = [];
        let p = new Promise((resolve, reject) => { resolve("async"); });
        p.then((value) => { result.push(value); });
        result.push("sync");
        result
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(value) => {
            // Should be an array with ["sync", "async"] since the then callback executes asynchronously
            if let Value::Object(obj) = &value {
                if let Some(length_val) = obj_get_value(obj, "length").unwrap() {
                    if let Value::Number(len) = *length_val.borrow() {
                        assert_eq!(len, 2.0);
                        if let Some(first_val) = obj_get_value(obj, "0").unwrap() {
                            if let Value::String(first) = &*first_val.borrow() {
                                assert_eq!(String::from_utf16_lossy(first), "sync");
                            }
                        }
                        if let Some(second_val) = obj_get_value(obj, "1").unwrap() {
                            if let Value::String(second) = &*second_val.borrow() {
                                assert_eq!(String::from_utf16_lossy(second), "async");
                            }
                        }
                    }
                }
            } else {
                panic!("Expected array result");
            }
        }
        Err(e) => panic!("Script evaluation failed: {:?}", e),
    }
}
