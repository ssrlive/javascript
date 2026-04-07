use javascript::*;
use serde_json::Value;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_async_n_throw_async_tests_regression() {
    // Increase stack size to handle recursion in event loop during async tests
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let path = std::path::Path::new("js-scripts/async_n_throw_async_tests_regression.js");
            let script = read_script_file(path).expect("failed to read regression script");

            // Execute the script, await its exposed top-level completion promise,
            // then extract the exposed summary as the final async result.
            let wrapped = format!(
                "(async function() {{\n{}\nawait globalThis.__async_regression_done;\nreturn JSON.stringify(globalThis.__async_regression_summary);\n}})()",
                script
            );

            match evaluate_script_with_unwrap(&wrapped, false, Some(path), true) {
                Ok(result) => {
                    let _ = tx.send(Ok(result));
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("evaluate_script failed: {:?}", e)));
                }
            }
        })
        .expect("failed to spawn thread");

    // Timeout to avoid deadlock/infinite loop in event loop
    let timeout = std::time::Duration::from_secs(10);
    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => {
            // evaluate_script wraps returned JS strings as JSON quoted strings, so parse twice
            let json_str: String = serde_json::from_str(&result).expect("expected JSON string");
            let v: Value = serde_json::from_str(&json_str).expect("expected JSON object");

            println!("==== {V} ====", V = json_str);

            assert_eq!(v["passed"].as_i64().expect("passed is number"), 3, "expected 3 passed tests");
            assert_eq!(v["failed"].as_i64().expect("failed is number"), 0, "expected 0 failed tests");
        }
        Ok(Err(err_msg)) => panic!("{}", err_msg),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("test timed out after {:?}; possible event loop deadlock", timeout),
        Err(e) => panic!("channel recv error: {:?}", e),
    }
}

#[test]
fn test_var_scope_await_regression() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let path = std::path::Path::new("js-scripts/var_scope_await_regression.js");
            let script = read_script_file(path).expect("failed to read script");

            let wrapped = format!(
                "(async function() {{\n{}\nawait globalThis.__var_scope_done;\nreturn JSON.stringify(globalThis.__var_scope_result);\n}})()",
                script
            );
            match evaluate_script_with_unwrap(&wrapped, false, Some(path), true) {
                Ok(result) => {
                    let _ = tx.send(Ok(result));
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("evaluate_script failed: {:?}", e)));
                }
            }
        })
        .expect("failed to spawn thread");

    let timeout = std::time::Duration::from_secs(10);
    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => {
            let json_str: String = serde_json::from_str(&result).expect("expected JSON string");
            let val: String = serde_json::from_str(&json_str).expect("expected JSON string value");
            assert_eq!(val, "number", "var should survive across await and remain defined");
        }
        Ok(Err(err_msg)) => panic!("{}", err_msg),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("test timed out after {:?}; possible event loop deadlock", timeout),
        Err(e) => panic!("channel recv error: {:?}", e),
    }
}

#[test]
fn test_handlers_repro() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let path = std::path::Path::new("js-scripts/handlers_repro.js");
            let script = read_script_file(path).expect("failed to read script");

            match evaluate_script_with_unwrap(&script, false, Some(path), true) {
                Ok(result) => {
                    let _ = tx.send(Ok(result));
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("evaluate_script failed: {:?}", e)));
                }
            }
        })
        .expect("failed to spawn thread");

    let timeout = std::time::Duration::from_secs(10);
    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => {
            // result is a JSON boolean represented as a string; parse it
            let b: bool = serde_json::from_str(&result).expect("expected boolean");
            assert!(b, "handlers_repro should call registered handler without ReferenceError");
        }
        Ok(Err(err_msg)) => panic!("{}", err_msg),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("test timed out after {:?}; possible event loop deadlock", timeout),
        Err(e) => panic!("channel recv error: {:?}", e),
    }
}

#[test]
fn test_sync_throw_in_async_initial_step_reports_error_object() {
    let script = r#"
        async function f() { 
            result; // ReferenceError thrown synchronously during initial step
            await Promise.resolve();
        }
        f();
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>);
    match result {
        Err(e) => {
            let s = format!("{:?}", e);
            assert!(
                s.contains("ReferenceError") || s.contains("result is not defined"),
                "unexpected error: {:?}",
                e
            );
        }
        Ok(v) => panic!("Expected Err for sync-throw in async initial step, got Ok: {:?}", v),
    }
}

#[test]
fn test_for_await_of_assignment_and_declaration_destructuring_regression() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || {
            let script = r#"
                (async function() {
                    let arrow;
                    let prop_target = {};
                    let decl_array = 0;
                    let decl_object = 0;

                    for await ([arrow = () => {}] of [[]]) {}
                    for await ({ value: prop_target.value } of [{ value: 7 }]) {}
                    for await (let [x] of [[11]]) { decl_array = x; }
                    for await (const { y } of [{ y: 13 }]) { decl_object = y; }

                    return [arrow.name, prop_target.value, decl_array, decl_object];
                })()
            "#;

            match evaluate_script_with_unwrap(script, false, None::<&std::path::Path>, true) {
                Ok(result) => {
                    let _ = tx.send(Ok(result));
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("evaluate_script failed: {:?}", e)));
                }
            }
        })
        .expect("failed to spawn thread");

    let timeout = std::time::Duration::from_secs(10);
    match rx.recv_timeout(timeout) {
        Ok(Ok(result)) => assert_eq!(result, "[\"arrow\",7,11,13]"),
        Ok(Err(err_msg)) => panic!("{}", err_msg),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("test timed out after {:?}; possible event loop deadlock", timeout)
        }
        Err(e) => panic!("channel recv error: {:?}", e),
    }
}
