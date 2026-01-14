use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod const_tests {
    use javascript::JSErrorKind;

    use super::*;

    #[test]
    fn test_const_declaration() {
        let script = "const x = 42; x";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn test_const_reassignment_error() {
        let script = "const x = 42; x = 24";
        let result = evaluate_script(script, None::<&std::path::Path>);
        assert!(result.is_err());
        match result {
            Err(err) => match err.kind() {
                JSErrorKind::TypeError { message, .. } => {
                    assert!(message.contains("Assignment to constant") || message.contains("constant"))
                }
                _ => panic!("Expected TypeError for assignment to const, got {:?}", err),
            },
            _ => panic!("Expected error for const reassignment, got {:?}", result),
        }
    }

    #[test]
    fn test_const_vs_let() {
        // let should allow reassignment
        let script1 = "let x = 42; x = 24; x";
        let result = evaluate_script(script1, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "24");

        // const should not allow reassignment
        let script2 = "const y = 42; y = 24";
        let result2 = evaluate_script(script2, None::<&std::path::Path>);
        assert!(result2.is_err());
    }
}
