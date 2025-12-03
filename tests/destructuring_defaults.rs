use javascript::*;

// Initialize logger for integration tests.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_object_destructuring_with_defaults() {
    let script = r#"
        let d = {};
        let { a = 3, b = 4 } = d;
        a + b
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 7.0),
        _ => panic!("Expected number 7.0, got {:?}", result),
    }
}

#[test]
fn test_array_destructuring_with_defaults() {
    let script = r#"
        let d = [];
        let [ a = 2, b = 5 ] = d;
        a * b
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 10.0),
        _ => panic!("Expected number 10.0, got {:?}", result),
    }
}
