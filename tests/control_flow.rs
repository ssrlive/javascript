use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod control_flow_tests {
    use javascript::JSError;

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
    fn test_while_loop() {
        let script = "let sum = 0; let i = 1; while (i <= 5) { sum = sum + i; i = i + 1; } sum";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 15.0), // 1+2+3+4+5 = 15
            _ => panic!("Expected number 15.0, got {:?}", result),
        }
    }

    #[test]
    fn test_while_loop_zero_iterations() {
        let script = "let count = 0; let i = 5; while (i < 5) { count = count + 1; i = i + 1; } count";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 0.0),
            _ => panic!("Expected number 0.0, got {:?}", result),
        }
    }

    #[test]
    fn test_do_while_loop() {
        let script = "let sum = 0; let i = 1; do { sum = sum + i; i = i + 1; } while (i <= 5); sum";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 15.0), // 1+2+3+4+5 = 15
            _ => panic!("Expected number 15.0, got {:?}", result),
        }
    }

    #[test]
    fn test_do_while_loop_executes_once() {
        let script = "let count = 0; let i = 5; do { count = count + 1; i = i + 1; } while (i < 5); count";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 1.0), // Executes once even though condition is false
            _ => panic!("Expected number 1.0, got {:?}", result),
        }
    }

    #[test]
    fn test_switch_statement() {
        let script = "let result = 0; switch (2) { case 1: result = 10; case 2: result = 20; case 3: result = 30; } result";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 30.0), // Falls through to case 3
            _ => panic!("Expected number 30.0, got {:?}", result),
        }
    }

    #[test]
    fn test_switch_statement_with_default() {
        let script = "let result = 0; switch (5) { case 1: result = 10; case 2: result = 20; default: result = 99; } result";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 99.0),
            _ => panic!("Expected number 99.0, got {:?}", result),
        }
    }

    #[test]
    fn test_switch_statement_no_match() {
        let script = "let result = 0; switch (5) { case 1: result = 10; case 2: result = 20; } result";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 0.0), // No match, result unchanged
            _ => panic!("Expected number 0.0, got {:?}", result),
        }
    }

    #[test]
    fn test_switch_break_statement_match() {
        let script = "let result = 0; switch (1) { case 1: result = 10; break; case 2: result = 20; } result";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 10.0),
            _ => panic!("Expected number 10.0, got {:?}", result),
        }
    }

    #[test]
    fn test_break_error() {
        let script = "break;";
        let result = evaluate_script(script);
        match result {
            Err(JSError::EvaluationError { message }) => {
                assert!(message.contains("break statement not in loop or switch"));
            }
            _ => panic!("Expected EvaluationError for break, got {:?}", result),
        }
    }

    #[test]
    fn test_break_with_loop() {
        let script = "let sum = 0; for (let i = 1; i <= 5; i = i + 1) { if (i == 3) { break; } sum = sum + i; } sum";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0), // 1 + 2 = 3
            _ => panic!("Expected number 3.0, got {:?}", result),
        }
    }

    #[test]
    fn test_continue_error() {
        let script = "continue;";
        let result = evaluate_script(script);
        match result {
            Err(JSError::EvaluationError { message }) => {
                assert!(message.contains("continue statement not in loop"));
            }
            _ => panic!("Expected EvaluationError for continue, got {:?}", result),
        }
    }

    #[test]
    fn test_continue_statment() {
        let script = "let sum = 0; for (let i = 1; i <= 5; i = i + 1) { if (i % 2 == 0) { continue; } sum = sum + i; } sum";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 9.0), // 1 + 3 + 5 = 9
            _ => panic!("Expected EvaluationError for continue, got {:?}", result),
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

    #[test]
    fn test_var_hoisting() {
        let script = "function f() { a = 10; return a; var a; } f()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 10.0),
            _ => panic!("Expected number 10.0, got {:?}", result),
        }
    }

    #[test]
    fn test_var_in_block_scope() {
        let script = "function f() { if (true) { var a = 5; } return a; } f()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 5.0),
            _ => panic!("Expected number 5.0, got {:?}", result),
        }
    }

    #[test]
    fn test_var_redeclaration() {
        let script = "var a = 1; var a = 2; a";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 2.0),
            _ => panic!("Expected number 2.0, got {:?}", result),
        }
    }

    #[test]
    fn test_var_in_for_loop() {
        let script = "for (var i = 0; i < 3; i++) {} i";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected number 3.0, got {:?}", result),
        }
    }
}
