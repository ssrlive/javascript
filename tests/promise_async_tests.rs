use javascript::evaluate_script;

#[test]
#[ignore]
fn test_promise_async_resolution() {
    // Test that we can get the async result of a Promise
    let script = r#"
        new Promise((resolve, reject) => {
            resolve("async result");
        })
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"async result\"");
}

#[test]
#[ignore]
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
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "42");
}

#[test]
#[ignore]
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
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "25");
}

#[test]
#[ignore]
fn test_promise_allsettled() {
    // Test Promise.allSettled with mixed resolve/reject
    let script = r#"
        Promise.allSettled([
            new Promise(function(resolve, reject) { resolve(1); console.log("executor 1 called"); }),
            new Promise(function(resolve, reject) { reject(2); console.log("executor 2 called"); }),
            new Promise(function(resolve, reject) { resolve(3); console.log("executor 3 called"); })
        ])
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(
        result,
        "[{\"status\":\"fulfilled\",\"value\":1},{\"status\":\"rejected\",\"reason\":2},{\"status\":\"fulfilled\",\"value\":3}]"
    );
}

#[test]
#[ignore]
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
