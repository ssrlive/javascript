use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod symbol_static_tests {
    use super::*;

    #[test]
    fn test_symbol_for_same_key_returns_same_symbol() {
        let script = r#"
            let sym1 = Symbol.for("test");
            let sym2 = Symbol.for("test");
            sym1 === sym2
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b), // true
            _ => panic!("Expected true for same key, got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_for_different_keys_different_symbols() {
        let script = r#"
            let sym1 = Symbol.for("test1");
            let sym2 = Symbol.for("test2");
            sym1 !== sym2
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b), // true
            _ => panic!("Expected true for different keys, got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_key_for_registered_symbol() {
        let script = r#"
            let sym = Symbol.for("myKey");
            Symbol.keyFor(sym)
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "myKey"),
            _ => panic!("Expected string 'myKey', got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_key_for_unregistered_symbol() {
        let script = r#"
            let sym = Symbol("not registered");
            Symbol.keyFor(sym)
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => (), // Should be undefined
            _ => panic!("Expected undefined for unregistered symbol, got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_for_with_non_string_key() {
        let script = r#"
            let sym1 = Symbol.for(123);
            let sym2 = Symbol.for("123");
            sym1 === sym2
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b), // true, since toString makes them same
            _ => panic!("Expected true for number and string key '123', got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_for_no_args_error() {
        let script = r#"
            try {
                Symbol.for();
            } catch (e) {
                "error"
            }
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "error"),
            _ => panic!("Expected error for no args, got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_key_for_no_args_error() {
        let script = r#"
            try {
                Symbol.keyFor();
            } catch (e) {
                "error"
            }
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "error"),
            _ => panic!("Expected error for no args, got {:?}", result),
        }
    }
}
