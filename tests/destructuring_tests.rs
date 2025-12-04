use javascript::evaluate_script;
use javascript::{JSError, Value};

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod destructuring_tests {
    use super::*;

    #[test]
    fn test_basic_array_destructuring() {
        let script = "let [a, b] = [1, 2]; a + b";
        let result = evaluate_script(script).unwrap();
        match result {
            Value::Number(3.0) => (),
            _ => panic!("Expected 3.0, got {:?}", result),
        }
    }

    #[test]
    fn test_array_destructuring_with_rest() {
        let script = "let [a, ...rest] = [1, 2, 3, 4]; rest[0] + rest[1]";
        let result = evaluate_script(script).unwrap();
        match result {
            Value::Number(5.0) => (),
            _ => panic!("Expected 5.0, got {:?}", result),
        }
    }

    #[test]
    fn test_basic_object_destructuring() {
        let script = "let {a, b} = {a: 1, b: 2}; a + b";
        let result = evaluate_script(script).unwrap();
        match result {
            Value::Number(3.0) => (),
            _ => panic!("Expected 3.0, got {:?}", result),
        }
    }

    #[test]
    fn test_object_destructuring_with_rest() {
        let script = "let {a, ...rest} = {a: 1, b: 2, c: 3}; rest.b + rest.c";
        let result = evaluate_script(script).unwrap();
        match result {
            Value::Number(5.0) => (),
            _ => panic!("Expected 5.0, got {:?}", result),
        }
    }

    #[test]
    fn test_nested_destructuring() {
        let script = "let [a, {b}] = [1, {b: 2, c: 3}]; a + b";
        let result = evaluate_script(script).unwrap();
        match result {
            Value::Number(3.0) => (),
            _ => panic!("Expected 3.0, got {:?}", result),
        }
    }

    #[test]
    fn destructuring_from_undefined_returns_helpful_error() {
        let script = r#"
            let duration;
            let { seconds = 0, milliseconds = 0 } = duration;
        "#;

        let res = evaluate_script(script);
        match res {
            Err(JSError::EvaluationError { message, .. }) => {
                assert!(message.contains("Cannot destructure property"));
                assert!(message.contains("seconds"));
            }
            _ => panic!("expected EvaluationError for destructuring undefined"),
        }
    }

    #[test]
    fn destructuring_with_nullish_fallback_works() {
        let script = r#"
            let duration;
            let { seconds = 0, milliseconds = 0 } = duration ?? {};
            seconds;
        "#;

        let res = evaluate_script(script);
        assert!(res.is_ok());
        // Last evaluation should be the `seconds` value (0)
        let v = res.unwrap();
        match v {
            crate::Value::Number(n) => assert_eq!(n, 0.0),
            _ => panic!("expected numeric result"),
        }
    }
}
