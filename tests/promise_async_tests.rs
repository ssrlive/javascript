use javascript::{Value, evaluate_script, obj_get_key_value};

#[test]
fn test_promise_async_resolution() {
    // Test that we can get the async result of a Promise
    let script = r#"
        new Promise((resolve, reject) => {
            resolve("async result");
        })
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
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
    let result = evaluate_script(script, None::<&std::path::Path>);
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
    let result = evaluate_script(script, None::<&std::path::Path>);
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
fn test_promise_allsettled() {
    // Test Promise.allSettled with mixed resolve/reject
    let script = r#"
        Promise.allSettled([
            new Promise(function(resolve, reject) { resolve(1); console.log("executor 1 called"); }),
            new Promise(function(resolve, reject) { reject(2); console.log("executor 2 called"); }),
            new Promise(function(resolve, reject) { resolve(3); console.log("executor 3 called"); })
        ])
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(value) => {
            // Should get an array of settled results
            match value {
                Value::Object(arr) => {
                    // Check that we have 3 elements
                    if let Some(len_val) = obj_get_key_value(&arr, &"length".into()).unwrap()
                        && let Value::Number(len) = *len_val.borrow()
                    {
                        assert_eq!(len, 3.0);
                    }
                    // Check first element is fulfilled with 1
                    if let Some(elem0) = obj_get_key_value(&arr, &"0".into()).unwrap()
                        && let Value::Object(result_obj) = &*elem0.borrow()
                    {
                        if let Some(status) = obj_get_key_value(result_obj, &"status".into()).unwrap()
                            && let Value::String(s) = &*status.borrow()
                        {
                            assert_eq!(String::from_utf16_lossy(s), "fulfilled");
                        }
                        if let Some(value) = obj_get_key_value(result_obj, &"value".into()).unwrap()
                            && let Value::Number(n) = *value.borrow()
                        {
                            assert_eq!(n, 1.0);
                        }
                    }
                    // Check second element is rejected with 2
                    if let Some(elem1) = obj_get_key_value(&arr, &"1".into()).unwrap()
                        && let Value::Object(result_obj) = &*elem1.borrow()
                    {
                        if let Some(status) = obj_get_key_value(result_obj, &"status".into()).unwrap()
                            && let Value::String(s) = &*status.borrow()
                        {
                            assert_eq!(String::from_utf16_lossy(s), "rejected");
                        }
                        if let Some(reason) = obj_get_key_value(result_obj, &"reason".into()).unwrap()
                            && let Value::Number(n) = *reason.borrow()
                        {
                            assert_eq!(n, 2.0);
                        }
                    }
                    // Check third element is fulfilled with 3
                    if let Some(elem2) = obj_get_key_value(&arr, &"2".into()).unwrap()
                        && let Value::Object(result_obj) = &*elem2.borrow()
                    {
                        if let Some(status) = obj_get_key_value(result_obj, &"status".into()).unwrap()
                            && let Value::String(s) = &*status.borrow()
                        {
                            assert_eq!(String::from_utf16_lossy(s), "fulfilled");
                        }
                        if let Some(value) = obj_get_key_value(result_obj, &"value".into()).unwrap()
                            && let Value::Number(n) = *value.borrow()
                        {
                            assert_eq!(n, 3.0);
                        }
                    }
                }
                _ => panic!("Expected array result, got {:?}", value),
            }
        }
        Err(e) => panic!("Script evaluation failed: {:?}", e),
    }
}

#[test]
fn test_main() {
    let script = r#"
        Promise.allSettled([
            new Promise((resolve, reject) => { resolve(1); }),
            new Promise((resolve, reject) => { reject(2); }),
            new Promise((resolve, reject) => { resolve(3); })
        ])
    "#;
    match evaluate_script(script, None::<&std::path::Path>) {
        Ok(result) => println!("Success:{:?}", result),
        Err(e) => println!("Error:{:?}", e),
    }
}
