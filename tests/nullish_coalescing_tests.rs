use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod nullish_coalescing_tests {
    use super::*;

    #[test]
    fn test_nullish_coalescing() {
        // Test undefined ?? default
        let result = evaluate_script("undefined ?? 'default'", None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => assert_eq!(utf16_to_utf8(&s), "default"),
            _ => panic!("Expected string result"),
        }

        // Test null ?? default
        let result = evaluate_script("null ?? 'default'", None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => assert_eq!(utf16_to_utf8(&s), "default"),
            _ => panic!("Expected string result"),
        }

        // Test falsy values ?? default (should return the falsy value)
        let result = evaluate_script("0 ?? 'default'", None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::Number(n) => assert_eq!(n, 0.0),
            _ => panic!("Expected number result"),
        }

        let result = evaluate_script("false ?? 'default'", None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::Boolean(b) => assert!(!b),
            _ => panic!("Expected boolean result"),
        }

        let result = evaluate_script("'' ?? 'default'", None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => assert_eq!(utf16_to_utf8(&s), ""),
            _ => panic!("Expected string result"),
        }

        // Test truthy values ?? default (should return the truthy value)
        let result = evaluate_script("'hello' ?? 'default'", None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => assert_eq!(utf16_to_utf8(&s), "hello"),
            _ => panic!("Expected string result"),
        }

        let result = evaluate_script("42 ?? 'default'", None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::Number(n) => assert_eq!(n, 42.0),
            _ => panic!("Expected number result"),
        }

        // Test chained nullish coalescing
        let result = evaluate_script("undefined ?? null ?? 'fallback'", None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => assert_eq!(utf16_to_utf8(&s), "fallback"),
            _ => panic!("Expected string result"),
        }

        // Test with variables
        let result = evaluate_script("let x = undefined; x ?? 'default'", None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => assert_eq!(utf16_to_utf8(&s), "default"),
            _ => panic!("Expected string result"),
        }

        let result = evaluate_script("let x = 'value'; x ?? 'default'", None::<&std::path::Path>);
        assert!(result.is_ok());
        match result.unwrap() {
            Value::String(s) => assert_eq!(utf16_to_utf8(&s), "value"),
            _ => panic!("Expected string result"),
        }
    }
}
