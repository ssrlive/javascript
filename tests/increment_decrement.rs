use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

fn eval(script: &str) -> Result<Value, JSError> {
    evaluate_script(script, None::<&std::path::Path>)
}

#[test]
fn test_prefix_increment_variable() {
    let res = eval("let i = 1; ++i; i").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 2.0));
}

#[test]
fn test_prefix_decrement_variable() {
    let res = eval("let i = 3; --i; i").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 2.0));
}

#[test]
fn test_postfix_increment_variable() {
    let res = eval("let i = 4; i++; i").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 5.0));
}

#[test]
fn test_postfix_decrement_variable() {
    let res = eval("let i = 5; i--; i").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 4.0));
}

#[test]
fn test_increment_property() {
    let res = eval("let obj = {x: 10}; ++obj.x; obj.x").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 11.0));
}

#[test]
fn test_postfix_increment_array_index() {
    let res = eval("let a = [1,2,3]; a[0]++; a[0]").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 2.0));
}
