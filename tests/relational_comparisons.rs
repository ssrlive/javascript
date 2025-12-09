use javascript::Value;
use javascript::evaluate_script;

#[test]
fn test_number_less_than_number() {
    let script = "1 < 2";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Boolean(b)) => assert!(b),
        _ => panic!("Expected true (boolean), got {:?}", result),
    }
}

#[test]
fn test_string_less_than_string() {
    let script = "'a' < 'b'";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Boolean(b)) => assert!(b),
        _ => panic!("Expected true (boolean), got {:?}", result),
    }
}

#[test]
fn test_string_number_coercion() {
    let script = "'2' < 3";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Boolean(b)) => assert!(b),
        _ => panic!("Expected true (boolean), got {:?}", result),
    }
}

#[test]
fn test_boolean_number_comparison() {
    let script = "true < 2";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Boolean(b)) => assert!(b),
        _ => panic!("Expected true (boolean), got {:?}", result),
    }
}

#[test]
fn test_undefined_comparison_is_false() {
    let script = "undefined < 1";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Boolean(b)) => assert!(!b),
        _ => panic!("Expected false (boolean), got {:?}", result),
    }
}

#[test]
fn test_bigint_comparison() {
    let script = "0n < 1n";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Boolean(b)) => assert!(b),
        _ => panic!("Expected true (boolean) for bigint comparison, got {:?}", result),
    }
}
