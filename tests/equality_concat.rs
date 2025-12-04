use javascript::Value;
use javascript::evaluate_script;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn undefined_equality() {
    let script = "undefined == undefined";
    let result = evaluate_script(script);
    match result {
        Ok(Value::Boolean(b)) => assert!(b),
        _ => panic!("Expected equality true as number 1.0, got {:?}", result),
    }
}

#[test]
fn undefined_strict_equality() {
    let script = "undefined === undefined";
    let result = evaluate_script(script);
    match result {
        Ok(Value::Boolean(b)) => assert!(b),
        _ => panic!("Expected strict equality true as number 1.0, got {:?}", result),
    }
}

#[test]
fn object_identity_strict_equal() {
    let script = "let a = {}; let b = a; a === b";
    let result = evaluate_script(script);
    match result {
        Ok(Value::Boolean(b)) => assert!(b),
        _ => panic!("Expected objects identical to be strict equal (1.0), got {:?}", result),
    }
}

#[test]
fn object_identity_distinct_objects() {
    let script = "let a = {}; a === {}";
    let result = evaluate_script(script);
    match result {
        Ok(Value::Boolean(b)) => assert!(!b),
        _ => panic!("Expected different objects to not be strict equal (0.0), got {:?}", result),
    }
}

#[test]
fn string_concat_with_undefined_right() {
    let script = "'a' + undefined";
    let result = evaluate_script(script);
    match result {
        Ok(Value::String(s)) => {
            let expected = "aundefined".encode_utf16().collect::<Vec<u16>>();
            assert_eq!(s, expected);
        }
        _ => panic!("Expected string 'aundefined', got {:?}", result),
    }
}

#[test]
fn string_concat_with_undefined_left() {
    let script = "undefined + 'b'";
    let result = evaluate_script(script);
    match result {
        Ok(Value::String(s)) => {
            let expected = "undefinedb".encode_utf16().collect::<Vec<u16>>();
            assert_eq!(s, expected);
        }
        _ => panic!("Expected string 'undefinedb', got {:?}", result),
    }
}
