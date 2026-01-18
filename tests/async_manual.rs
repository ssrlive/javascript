use javascript::evaluate_script;

// Initialize logger for tests
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn async_function_returns_value() {
    let res = evaluate_script("async function f(){ return 7; } f()", None::<&std::path::Path>);
    match res {
        Ok(value) => assert_eq!(value, "7"),
        Err(e) => panic!("Script evaluation failed: {e:?}"),
    }
}

#[test]
fn async_function_awaits_promise_resolve() {
    let res = evaluate_script(
        "async function f(){ return await Promise.resolve(8); } f()",
        None::<&std::path::Path>,
    );
    match res {
        Ok(value) => assert_eq!(value, "8"),
        Err(e) => panic!("Script evaluation failed: {e:?}"),
    }
}

#[test]
fn async_arrow_awaits_and_computes() {
    let script = "let f = async () => { const x = await Promise.resolve(9); return x + 1; }; f()";
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(value) => assert_eq!(value, "10"),
        Err(e) => panic!("Script evaluation failed: {e:?}"),
    }
}
