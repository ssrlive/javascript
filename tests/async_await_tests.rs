use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_async_function_syntax() {
    // Test that async function syntax is accepted (even if execution is synchronous)
    let script = "async function foo() { return 42; }; await foo()";
    match evaluate_script(script, None::<&std::path::Path>) {
        Ok(value) => assert_eq!(value, "42"),
        Err(e) => panic!("Script evaluation failed: {e:?}"),
    }
}

#[test]
fn test_await_syntax() {
    // Test that await syntax is accepted
    let script = "let p = Promise.resolve(42); await p";
    match evaluate_script(script, None::<&std::path::Path>) {
        Ok(value) => assert_eq!(value, "42"),
        Err(e) => panic!("Script evaluation failed: {e:?}"),
    }
}

#[test]
fn test_async_arrow_function_syntax() {
    // Test that async arrow function syntax is accepted
    let script = "let foo = async () => { return 42; }; await foo()";
    match evaluate_script(script, None::<&std::path::Path>) {
        Ok(value) => assert_eq!(value, "42"),
        Err(e) => panic!("Script evaluation failed: {e:?}"),
    }
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
    match evaluate_script(script, None::<&std::path::Path>) {
        Ok(value) => assert_eq!(value, "[\"sync\",\"async\"]"),
        Err(e) => panic!("Script evaluation failed: {e:?}"),
    }
}
