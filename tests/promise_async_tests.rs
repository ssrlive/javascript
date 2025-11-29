use javascript::{Value, evaluate_script};

#[test]
fn test_promise_async_resolution() {
    // Test that we can get the async result of a Promise
    let script = r#"
        new Promise((resolve, reject) => {
            resolve("async result");
        })
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(value) => {
            // Should get the resolved value
            match value {
                Value::String(s) => {
                    assert_eq!(String::from_utf16_lossy(&s), "async result");
                }
                _ => panic!("Expected string result, got {:?}", value),
            }
        }
        Err(e) => panic!("Script evaluation failed: {:?}", e),
    }
}

#[test]
fn test_await_async_function() {
    // Test that await works in async functions
    let script = r#"
        async function getResult() {
            let promise = new Promise((resolve, reject) => {
                resolve(42);
            });
            return await promise;
        }
        getResult()
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(value) => {
            // Should get the awaited result
            match value {
                Value::Number(n) => {
                    assert_eq!(n, 42.0);
                }
                _ => panic!("Expected number result, got {:?}", value),
            }
        }
        Err(e) => panic!("Script evaluation failed: {:?}", e),
    }
}

#[test]
fn test_promise_chaining_async() {
    // Test Promise chaining with async resolution
    let script = r#"
        new Promise((resolve, reject) => {
            resolve(10);
        }).then((value) => {
            return value * 2;
        }).then((value) => {
            return value + 5;
        })
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(value) => {
            // Should get the final chained result
            match value {
                Value::Number(n) => {
                    assert_eq!(n, 25.0); // (10 * 2) + 5 = 25
                }
                _ => panic!("Expected number result, got {:?}", value),
            }
        }
        Err(e) => panic!("Script evaluation failed: {:?}", e),
    }
}

#[test]
fn test_promise_rejection_async() {
    // Test that Promise rejection is properly handled
    let script = r#"
        new Promise((resolve, reject) => {
            reject("error occurred");
        })
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(_) => panic!("Expected rejection but got success"),
        Err(e) => {
            // Should get the rejection error
            assert!(e.to_string().contains("error occurred"));
        }
    }
}
