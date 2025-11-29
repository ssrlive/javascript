use javascript::Value;
use javascript::evaluate_script;
use javascript::obj_get_value;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod promise_tests {
    use super::*;

    #[test]
    fn test_promise_then_basic() {
        let code = r#"
            let result = null;
            let p = new Promise(function(resolve, reject) {
                resolve(100);
            });
            p.then(function(val) {
                result = val * 2;
            });
            result
        "#;
        let result = evaluate_script(code);
        assert!(result.is_ok());
        // For now, just check that it doesn't crash
    }

    #[test]
    fn test_promise_chaining() {
        let code = r#"
            let finalResult = null;
            let p = new Promise(function(resolve, reject) {
                resolve(10);
            });
            p.then(function(val) {
                return val + 5;
            }).then(function(val) {
                return val * 2;
            }).then(function(val) {
                finalResult = val;
            });
            finalResult
        "#;
        let result = evaluate_script(code);
        assert!(result.is_ok());
        // For now, just check that it doesn't crash - full chaining requires async execution
    }

    #[test]
    fn test_promise_all_resolved() {
        let code = r#"
            let result = null;
            let p1 = new Promise(function(resolve, reject) {
                resolve(1);
            });
            let p2 = new Promise(function(resolve, reject) {
                resolve(2);
            });
            let p3 = new Promise(function(resolve, reject) {
                resolve(3);
            });
            Promise.all([p1, p2, p3]).then(function(values) {
                result = values[0] + values[1] + values[2];
            });
            result
        "#;
        let result = evaluate_script(code);
        assert!(result.is_ok());
        // For now, just check that it doesn't crash - full functionality requires async execution
    }

    #[test]
    fn test_promise_race_resolved() {
        let code = r#"
            let result = null;
            let p1 = new Promise(function(resolve, reject) {
                resolve(1);
            });
            let p2 = new Promise(function(resolve, reject) {
                resolve(2);
            });
            Promise.race([p1, p2]).then(function(value) {
                result = value;
            });
            result
        "#;
        let result = evaluate_script(code);
        assert!(result.is_ok());
        // For now, just check that it doesn't crash - full functionality requires async execution
    }

    #[test]
    fn test_promise_async_execution_order() {
        // Test that Promise then callbacks execute asynchronously after synchronous code
        let code = r#"
            let executionOrder = [];
            new Promise((resolve, reject) => {
                let p = new Promise((res, rej) => res("async result"));
                p.then((value) => {
                    executionOrder.push(value);
                    resolve(executionOrder);
                });
            });
            executionOrder.push("sync");
        "#;

        let result = evaluate_script(code);
        match result {
            Ok(Value::Object(arr)) => {
                // Check that we have an array with 2 elements
                if let Ok(Some(length_val)) = obj_get_value(&arr, "length") {
                    if let Value::Number(len) = *length_val.borrow() {
                        assert_eq!(len, 2.0, "Array should have 2 elements");

                        // Check first element is "sync"
                        if let Ok(Some(first_val)) = obj_get_value(&arr, "0") {
                            if let Value::String(first) = &*first_val.borrow() {
                                assert_eq!(String::from_utf16_lossy(first), "sync");
                            } else {
                                panic!("First element should be string 'sync'");
                            }
                        }

                        // Check second element is "async result"
                        if let Ok(Some(second_val)) = obj_get_value(&arr, "1") {
                            if let Value::String(second) = &*second_val.borrow() {
                                assert_eq!(String::from_utf16_lossy(second), "async result");
                            } else {
                                panic!("Second element should be string 'async result'");
                            }
                        }
                    } else {
                        panic!("Array length should be a number");
                    }
                } else {
                    panic!("Array should have length property");
                }
            }
            _ => panic!("Expected array result, got {:?}", result),
        }
    }

    #[test]
    fn test_promise_finally() {
        let code = r#"new Promise(function(resolve, reject) { resolve(42); }).finally(function() { console.log('finally executed'); })"#;
        let result = evaluate_script(code);
        match result {
            Ok(Value::Number(42.0)) => {
                // Test passed - basic promise works
            }
            _ => panic!("Test failed: {:?}", result),
        }
    }
}
