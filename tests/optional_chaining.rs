use javascript::Value;
use javascript::evaluate_script;

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
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "value"),
            _ => panic!("Expected string 'value', got {:?}", result),
        }
    }

    #[test]
    fn test_optional_property_access_null_object() {
        let script = "let obj = null; obj?.prop";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => {}
            _ => panic!("Expected undefined, got {:?}", result),
        }
    }

    #[test]
    fn test_optional_method_call_valid_object() {
        let script = "let obj = {method: function() { return 'called'; }}; obj?.method()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "called"),
            _ => panic!("Expected string 'called', got {:?}", result),
        }
    }

    #[test]
    fn test_optional_method_call_null_object() {
        let script = "let obj = null; obj?.method()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => {}
            _ => panic!("Expected undefined, got {:?}", result),
        }
    }

    #[test]
    fn test_chained_optional_operations() {
        let script = "let obj = {nested: {method: function() { return 'nested called'; }}}; obj?.nested?.method()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "nested called"),
            _ => panic!("Expected string 'nested called', got {:?}", result),
        }
    }

    #[test]
    fn test_optional_computed_property_access() {
        let script = "let obj = {a: 'value'}; obj?.['a']";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "value"),
            _ => panic!("Expected string 'value', got {:?}", result),
        }
    }

    #[test]
    fn test_optional_computed_property_null_object() {
        let script = "let obj = null; obj?.['a']";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => {}
            _ => panic!("Expected undefined, got {:?}", result),
        }
    }
}
