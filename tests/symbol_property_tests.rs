use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod symbol_property_tests {
    use super::*;

    #[test]
    fn test_symbol_property_assignment_and_access() {
        let script = r#"
            let sym = Symbol("test");
            let obj = {};
            obj[sym] = "symbol value";
            obj[sym]
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "symbol value"),
            _ => panic!("Expected string 'symbol value', got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_property_different_symbols() {
        let script = r#"
            let sym1 = Symbol("test1");
            let sym2 = Symbol("test2");
            let obj = {};
            obj[sym1] = "value1";
            obj[sym2] = "value2";
            obj[sym1] != obj[sym2]
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 1.0), // true
            _ => panic!("Expected number 1.0 (true), got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_property_deletion() {
        let script = r#"
            let sym = Symbol("test");
            let obj = {};
            obj[sym] = "value";
            delete obj[sym];
            obj[sym]
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => (), // Should be undefined after deletion
            _ => panic!("Expected undefined after deletion, got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_property_increment() {
        let script = r#"
            let sym = Symbol("test");
            let obj = {};
            obj[sym] = 5;
            obj[sym]++;
            obj[sym]
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 6.0),
            _ => panic!("Expected number 6.0 after increment, got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_property_decrement() {
        let script = r#"
            let sym = Symbol("test");
            let obj = {};
            obj[sym] = 5;
            obj[sym]--;
            obj[sym]
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 4.0),
            _ => panic!("Expected number 4.0 after decrement, got {:?}", result),
        }
    }
}
