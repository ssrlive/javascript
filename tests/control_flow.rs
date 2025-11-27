use rust_quickjs::quickjs::evaluate_script;
use rust_quickjs::quickjs::Value;
use rust_quickjs::quickjs::MAX_LOOP_ITERATIONS;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod control_flow_tests {
    use rust_quickjs::error::JSError;

    use super::*;

    #[test]
    fn test_if_statement_true() {
        let script = "let x = 5; if (x > 3) { x = x + 1; } x";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 6.0),
            _ => panic!("Expected number 6.0, got {:?}", result),
        }
    }

    #[test]
    fn test_if_statement_false() {
        let script = "let x = 2; if (x > 3) { x = x + 1; } x";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 2.0),
            _ => panic!("Expected number 2.0, got {:?}", result),
        }
    }

    #[test]
    fn test_if_else_statement() {
        let script = "let x = 2; if (x > 3) { x = 10; } else { x = 20; } x";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 20.0),
            _ => panic!("Expected number 20.0, got {:?}", result),
        }
    }

    #[test]
    fn test_variable_assignment_in_if() {
        let script = "let result = 0; if (1) { result = 42; } result";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0),
            _ => panic!("Expected number 42.0, got {:?}", result),
        }
    }

    #[test]
    fn test_for_loop() {
        let script = "let sum = 0; for (let i = 1; i <= 5; i = i + 1) { sum = sum + i; } sum";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 15.0), // 1+2+3+4+5 = 15
            _ => panic!("Expected number 15.0, got {:?}", result),
        }
    }

    #[test]
    fn test_infinite_loop_detection() {
        // This should trigger infinite loop detection after MAX_LOOP_ITERATIONS iterations
        let script = "for (let i = 0; true; i = i + 1) { }";
        let result = evaluate_script(script);
        match result {
            Err(JSError::InfiniteLoopError { iterations }) => {
                assert_eq!(iterations, MAX_LOOP_ITERATIONS);
            }
            _ => panic!("Expected InfiniteLoopError, got {:?}", result),
        }
    }

    #[test]
    fn test_for_of_loop() {
        let script = "let arr = []; arr.push(1); arr.push(2); arr.push(3); let sum = 0; for (let x of arr) { sum = sum + x; } sum";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 6.0), // 1+2+3 = 6
            _ => panic!("Expected number 6.0, got {:?}", result),
        }
    }

    #[test]
    fn test_for_of_loop_empty_array() {
        let script = "let arr = []; let count = 0; for (let x of arr) { count = count + 1; } count";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 0.0),
            _ => panic!("Expected number 0.0, got {:?}", result),
        }
    }

    #[test]
    fn test_for_of_loop_single_element() {
        let script = "let arr = []; arr.push(42); let result = 0; for (let x of arr) { result = x; } result";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0),
            _ => panic!("Expected number 42.0, got {:?}", result),
        }
    }
}
