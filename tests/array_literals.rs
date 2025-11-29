use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_empty_array_literal() {
    let script = r#"
        let arr = [];
        arr.length
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 0.0),
        _ => panic!("Expected number 0.0, got {:?}", result),
    }
}

#[test]
fn test_array_literal_with_elements() {
    let script = r#"
        let arr = [1, 2, 3];
        arr.length
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 3.0),
        _ => panic!("Expected number 3.0, got {:?}", result),
    }
}

#[test]
fn test_array_literal_indexing() {
    let script = r#"
        let arr = [10, 20, 30];
        arr[0] + arr[1] + arr[2]
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 60.0),
        _ => panic!("Expected number 60.0, got {:?}", result),
    }
}

#[test]
fn test_array_literal_mixed_types() {
    let script = r#"
        let arr = [1, "hello", true];
        arr.length
    "#;
    let result = evaluate_script(script);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 3.0),
        _ => panic!("Expected number 3.0, got {:?}", result),
    }
}
