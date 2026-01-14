use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod basic_arithmetic_tests {
    use super::*;

    #[test]
    fn test_basic_arithmetic() {
        let script = "let x = 1; let y = 2; x + y";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(n) => assert_eq!(n, "3"),
            _ => panic!("Expected number 3, got {:?}", result),
        }
    }

    #[test]
    fn test_variable_assignment() {
        let script = "let a = 5; a";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(n) => assert_eq!(n, "5"),
            _ => panic!("Expected number 5, got {:?}", result),
        }
    }

    #[test]
    fn test_multiple_operations() {
        let script = "let x = 10; let y = 3; x - y";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(n) => assert_eq!(n, "7"),
            _ => panic!("Expected number 7, got {:?}", result),
        }
    }

    #[test]
    fn test_multiplication() {
        let script = "let x = 4; let y = 5; x * y";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(n) => assert_eq!(n, "20"),
            _ => panic!("Expected number 20, got {:?}", result),
        }
    }

    #[test]
    fn test_intentionally_failing_arithmetic() {
        let script = "let x = 1; let y = 2; x + y";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(n) => assert_eq!(n, "3"),
            _ => panic!("Expected number 3, got {:?}", result),
        }
    }

    #[test]
    fn test_modulo_operation() {
        let script = "let x = 7; let y = 3; x % y";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(n) => assert_eq!(n, "1"),
            _ => panic!("Expected number 1, got {:?}", result),
        }
    }

    #[test]
    fn test_modulo_zero_remainder() {
        let script = "let x = 6; let y = 3; x % y";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(n) => assert_eq!(n, "0"),
            _ => panic!("Expected number 0, got {:?}", result),
        }
    }

    #[test]
    fn test_addition_associativity() {
        let script = "54 + 76 + 'yyuiyu'";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(n) => assert_eq!(n, "\"130yyuiyu\""),
            _ => panic!("Expected '130yyuiyu', got {:?}", result),
        }
    }
}
