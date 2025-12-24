use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod optional_chaining_tests {
    use super::*;

    #[test]
    fn test_optional_property_access_valid_object() {
        let script = "let obj = {prop: 'value'}; obj?.prop";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "value"),
            _ => panic!("Expected string 'value', got {:?}", result),
        }
    }

    #[test]
    fn test_optional_property_access_null_object() {
        let script = "let obj = null; obj?.prop";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Undefined) => {}
            _ => panic!("Expected undefined, got {:?}", result),
        }
    }

    #[test]
    fn test_optional_method_call_valid_object() {
        let script = "let obj = {method: function() { return 'called'; }}; obj?.method()";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "called"),
            _ => panic!("Expected string 'called', got {:?}", result),
        }
    }

    #[test]
    fn test_optional_method_call_null_object() {
        let script = "let obj = null; obj?.method()";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Undefined) => {}
            _ => panic!("Expected undefined, got {:?}", result),
        }
    }

    #[test]
    fn test_chained_optional_operations() {
        let script = "let obj = {nested: {method: function() { return 'nested called'; }}}; obj?.nested?.method()";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "nested called"),
            _ => panic!("Expected string 'nested called', got {:?}", result),
        }
    }

    #[test]
    fn test_optional_computed_property_access() {
        let script = "let obj = {a: 'value'}; obj?.['a']";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "value"),
            _ => panic!("Expected string 'value', got {:?}", result),
        }
    }

    #[test]
    fn test_optional_computed_property_null_object() {
        let script = "let obj = null; obj?.['a']";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Undefined) => {}
            _ => panic!("Expected undefined, got {:?}", result),
        }
    }

    #[test]
    fn test_optional_chaining_assignment_lhs_errors() {
        // Using optional chaining on LHS for direct assignment should be invalid / parse error
        let code1 = "let o = {}; o?.prop = 5";
        let res1 = evaluate_script(code1, None::<&std::path::Path>);
        assert!(
            res1.is_err(),
            "expected parse error for optional chaining on LHS assignment: {:?}",
            res1
        );

        let code2 = "let o = {}; o?.['a'] = 3";
        let res2 = evaluate_script(code2, None::<&std::path::Path>);
        assert!(
            res2.is_err(),
            "expected parse error for optional computed LHS assignment: {:?}",
            res2
        );

        // Using optional chaining with nullish assignment should be invalid too
        let code3 = "let o = {}; o?.['a'] ??= 7";
        let res3 = evaluate_script(code3, None::<&std::path::Path>);
        assert!(
            res3.is_err(),
            "expected parse error for optional computed LHS nullish-assignment: {:?}",
            res3
        );
    }

    #[test]
    fn test_nullish_assign_on_property_and_index() {
        // non-optional property/index should work with ??=
        let code1 = "let o = {}; o.x ??= 9; o.x";
        let res1 = evaluate_script(code1, None::<&std::path::Path>).unwrap();
        match res1 {
            Value::Number(n) => assert_eq!(n, 9.0),
            _ => panic!("expected number 9.0, got {:?}", res1),
        }

        let code2 = "let o = {}; o['x'] ??= 11; o['x']";
        let res2 = evaluate_script(code2, None::<&std::path::Path>).unwrap();
        match res2 {
            Value::Number(n) => assert_eq!(n, 11.0),
            _ => panic!("expected number 11.0, got {:?}", res2),
        }
    }
}
