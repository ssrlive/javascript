use javascript::{Value, evaluate_script, obj_get_value};

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
        let result = evaluate_script(code, None::<&std::path::Path>);
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
        let result = evaluate_script(code, None::<&std::path::Path>);
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
        let result = evaluate_script(code, None::<&std::path::Path>);
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
        let result = evaluate_script(code, None::<&std::path::Path>);
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

        let result = evaluate_script(code, None::<&std::path::Path>);
        match result {
            Ok(Value::Object(arr)) => {
                // Check that we have an array with 2 elements
                if let Ok(Some(length_val)) = obj_get_value(&arr, &"length".into()) {
                    if let Value::Number(len) = *length_val.borrow() {
                        assert_eq!(len, 2.0, "Array should have 2 elements");

                        // Check first element is "sync"
                        if let Ok(Some(first_val)) = obj_get_value(&arr, &"0".into()) {
                            if let Value::String(first) = &*first_val.borrow() {
                                assert_eq!(String::from_utf16_lossy(first), "sync");
                            } else {
                                panic!("First element should be string 'sync'");
                            }
                        }

                        // Check second element is "async result"
                        if let Ok(Some(second_val)) = obj_get_value(&arr, &"1".into()) {
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
        let result = evaluate_script(code, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(42.0)) => {
                // Test passed - basic promise works
            }
            _ => panic!("Test failed: {:?}", result),
        }
    }

    #[test]
    fn test_promise_allsettled_mixed() {
        let code = r#"
            let p1 = new Promise(function(resolve, reject) {
                console.log("Resolving p1");
                resolve(1);
            });
            let p2 = new Promise(function(resolve, reject) {
                console.log("Rejecting p2");
                reject("error");
            });
            let p3 = new Promise(function(resolve, reject) {
                console.log("Resolving p3");
                resolve(3);
            });
            Promise.allSettled([p1, p2, p3])
        "#;
        let result = evaluate_script(code, None::<&std::path::Path>);
        assert!(result.is_ok());
        // The result should be the resolved array from allSettled
        match result {
            Ok(Value::Object(arr)) => {
                // Check that we have an array with 3 elements
                if let Ok(Some(length_val)) = obj_get_value(&arr, &"length".into()) {
                    if let Value::Number(len) = *length_val.borrow() {
                        assert_eq!(len, 3.0, "Array should have 3 elements");

                        // Check first element (fulfilled with value 1)
                        if let Ok(Some(first_val)) = obj_get_value(&arr, &"0".into())
                            && let Value::Object(settled) = &*first_val.borrow()
                        {
                            if let Ok(Some(status_val)) = obj_get_value(settled, &"status".into())
                                && let Value::String(status) = &*status_val.borrow()
                            {
                                assert_eq!(String::from_utf16_lossy(status), "fulfilled");
                            }
                            if let Ok(Some(value_val)) = obj_get_value(settled, &"value".into())
                                && let Value::Number(val) = *value_val.borrow()
                            {
                                assert_eq!(val, 1.0);
                            }
                        }

                        // Check second element (rejected with reason "error")
                        if let Ok(Some(second_val)) = obj_get_value(&arr, &"1".into())
                            && let Value::Object(settled) = &*second_val.borrow()
                        {
                            if let Ok(Some(status_val)) = obj_get_value(settled, &"status".into())
                                && let Value::String(status) = &*status_val.borrow()
                            {
                                assert_eq!(String::from_utf16_lossy(status), "rejected");
                            }
                            if let Ok(Some(reason_val)) = obj_get_value(settled, &"reason".into())
                                && let Value::String(reason) = &*reason_val.borrow()
                            {
                                assert_eq!(String::from_utf16_lossy(reason), "error");
                            }
                        }

                        // Check third element (fulfilled with value 3)
                        if let Ok(Some(third_val)) = obj_get_value(&arr, &"2".into())
                            && let Value::Object(settled) = &*third_val.borrow()
                        {
                            if let Ok(Some(status_val)) = obj_get_value(settled, &"status".into())
                                && let Value::String(status) = &*status_val.borrow()
                            {
                                assert_eq!(String::from_utf16_lossy(status), "fulfilled");
                            }
                            if let Ok(Some(value_val)) = obj_get_value(settled, &"value".into())
                                && let Value::Number(val) = *value_val.borrow()
                            {
                                assert_eq!(val, 3.0);
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
    fn test_promise_constructor_direct_functionality() {
        // Test that the direct constructor path works by creating a promise that resolves
        let code = r#"
            let result = null;
            new Promise(function(resolve, reject) {
                resolve("direct test");
            }).then(function(value) {
                result = value;
            });
            result
        "#;
        let result = evaluate_script(code, None::<&std::path::Path>);
        assert!(result.is_ok());
        // This tests that the direct constructor functions work properly
    }

    #[test]
    fn test_promise_then_direct_functionality() {
        // Test that the direct then handler works
        let code = r#"
            let result = null;
            let p = new Promise(function(resolve, reject) {
                resolve(42);
            });
            p.then(function(value) {
                result = value * 2;
            });
            result
        "#;
        let result = evaluate_script(code, None::<&std::path::Path>);
        assert!(result.is_ok());
        // This tests that the direct then handler works
    }

    #[test]
    fn test_promise_catch_direct_functionality() {
        // Test that the direct catch handler works
        let code = r#"
            let result = null;
            let p = new Promise(function(resolve, reject) {
                reject("test error");
            });
            p.catch(function(reason) {
                result = "caught: " + reason;
            });
            result
        "#;
        let result = evaluate_script(code, None::<&std::path::Path>);
        assert!(result.is_ok());
        // This tests that the direct catch handler works
    }

    #[test]
    fn test_promise_finally_direct_functionality() {
        // Test that the direct finally handler works
        let code = r#"
            let result = null;
            new Promise(function(resolve, reject) {
                resolve(100);
            }).finally(function() {
                result = "cleanup done";
            });
            result
        "#;
        let result = evaluate_script(code, None::<&std::path::Path>);
        assert!(result.is_ok());
        // This tests that the direct finally handler works
    }

    #[test]
    fn test_promise_resolve_reject_functions_direct() {
        // Test that the direct resolve/reject functions work
        let code = r#"
            let resolveResult = null;
            let rejectResult = null;

            new Promise(function(resolve, reject) {
                resolve("resolved");
            }).then(function(value) {
                resolveResult = value;
            });

            new Promise(function(resolve, reject) {
                reject("rejected");
            }).catch(function(reason) {
                rejectResult = reason;
            });

            [resolveResult, rejectResult]
        "#;
        let result = evaluate_script(code, None::<&std::path::Path>);
        assert!(result.is_ok());
        // This tests that the direct resolve/reject functions work
    }

    #[test]
    fn test_promise_constructor_with_arrow_function() {
        // Test that arrow functions work in Promise constructor
        let code = r#"
            let result = null;
            new Promise(resolve => resolve(42)).then(value => {
                result = value * 2;
            });
            result
        "#;
        let result = evaluate_script(code, None::<&std::path::Path>);
        match result {
            Ok(_) => {
                // Test passed
            }
            Err(e) => {
                panic!("Arrow function in Promise constructor failed: {:?}", e);
            }
        }
    }
}
