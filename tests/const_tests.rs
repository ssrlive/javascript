use javascript::core::evaluate_script;
use javascript::core::Value;
use javascript::error::JSError;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod const_tests {
    use super::*;

    #[test]
    fn test_const_declaration() {
        let script = "const x = 42; x";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0),
            _ => panic!("Expected const x to be 42, got {:?}", result),
        }
    }

    #[test]
    fn test_const_reassignment_error() {
        let script = "const x = 42; x = 24";
        let result = evaluate_script(script);
        assert!(result.is_err());
        if let Err(JSError::TypeError { message }) = result {
            assert!(message.contains("Assignment to constant variable"));
        } else {
            panic!("Expected TypeError, got {:?}", result);
        }
    }

    #[test]
    fn test_const_vs_let() {
        // let should allow reassignment
        let script1 = "let x = 42; x = 24; x";
        let result = evaluate_script(script1);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 24.0),
            _ => panic!("Expected let reassignment to work, got {:?}", result),
        }

        // const should not allow reassignment
        let script2 = "const y = 42; y = 24";
        let result2 = evaluate_script(script2);
        assert!(result2.is_err());
    }
}
