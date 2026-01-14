use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod control_flow_tests {
    use javascript::JSErrorKind;

    use super::*;

    #[test]
    fn test_if_statement_true() {
        let script = "let x = 5; if (x > 3) { x = x + 1; } x";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_if_statement_false() {
        let script = "let x = 2; if (x > 3) { x = x + 1; } x";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "2");
    }

    #[test]
    fn test_if_else_statement() {
        let script = "let x = 2; if (x > 3) { x = 10; } else { x = 20; } x";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "20");
    }

    #[test]
    fn test_variable_assignment_in_if() {
        let script = "let result = 0; if (1) { result = 42; } result";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn test_for_loop() {
        let script = "let sum = 0; for (let i = 1; i <= 5; i = i + 1) { sum = sum + i; } sum";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "15");
    }

    #[test]
    fn test_for_of_loop() {
        let script = "let arr = []; arr.push(1); arr.push(2); arr.push(3); let sum = 0; for (let x of arr) { sum = sum + x; } sum";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_for_of_loop_empty_array() {
        let script = "let arr = []; let count = 0; for (let x of arr) { count = count + 1; } count";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "0");
    }

    #[test]
    fn test_while_loop() {
        let script = "let sum = 0; let i = 1; while (i <= 5) { sum = sum + i; i = i + 1; } sum";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "15");
    }

    #[test]
    fn test_while_loop_zero_iterations() {
        let script = "let count = 0; let i = 5; while (i < 5) { count = count + 1; i = i + 1; } count";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "0");
    }

    #[test]
    fn test_do_while_loop() {
        let script = "let sum = 0; let i = 1; do { sum = sum + i; i = i + 1; } while (i <= 5); sum";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "15");
    }

    #[test]
    fn test_do_while_loop_executes_once() {
        let script = "let count = 0; let i = 5; do { count = count + 1; i = i + 1; } while (i < 5); count";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "1");
    }

    #[test]
    fn test_switch_statement() {
        let script = "let result = 0; switch (2) { case 1: result = 10; case 2: result = 20; case 3: result = 30; } result";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "30");
    }

    #[test]
    fn test_switch_statement_with_default() {
        let script = "let result = 0; switch (5) { case 1: result = 10; case 2: result = 20; default: result = 99; } result";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "99");
    }

    #[test]
    fn test_switch_statement_no_match() {
        let script = "let result = 0; switch (5) { case 1: result = 10; case 2: result = 20; } result";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "0");
    }

    #[test]
    fn test_switch_break_statement_match() {
        let script = "let result = 0; switch (1) { case 1: result = 10; break; case 2: result = 20; } result";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "10");
    }

    #[test]
    fn test_break_error() {
        let script = "break;";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Err(err) => match err.kind() {
                JSErrorKind::EvaluationError { message, .. } => {
                    assert!(message.contains("break statement not in loop or switch"));
                }
                _ => panic!("Expected EvaluationError for break, got {:?}", err),
            },
            _ => panic!("Expected EvaluationError for break, got {:?}", result),
        }
    }

    #[test]
    fn test_break_with_loop() {
        let script = "let sum = 0; for (let i = 1; i <= 5; i = i + 1) { if (i == 3) { break; } sum = sum + i; } sum";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_continue_error() {
        let script = "continue;";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Err(err) => match err.kind() {
                JSErrorKind::EvaluationError { message, .. } => {
                    assert!(message.contains("continue statement not in loop"));
                }
                _ => panic!("Expected EvaluationError for continue, got {:?}", err),
            },
            _ => panic!("Expected EvaluationError for continue, got {:?}", result),
        }
    }

    #[test]
    fn test_continue_statment() {
        let script = "let sum = 0; for (let i = 1; i <= 5; i = i + 1) { if (i % 2 == 0) { continue; } sum = sum + i; } sum";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "9");
    }

    #[test]
    fn test_for_of_loop_single_element() {
        let script = "let arr = []; arr.push(42); let result = 0; for (let x of arr) { result = x; } result";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn test_for_of_loop_with_break() {
        let script = r#"
            let arr = [10, 20, 30, 40];
            let sum = 0;
            for (let x of arr) {
                if (x == 30) { break; }
                sum = sum + x;
            }
            sum
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "30");
    }

    #[test]
    fn test_var_hoisting() {
        let script = "function f() { a = 10; return a; var a; } f()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "10");
    }

    #[test]
    fn test_var_in_block_scope() {
        let script = "function f() { if (true) { var a = 5; } return a; } f()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "5");
    }

    #[test]
    fn test_var_redeclaration() {
        let script = "var a = 1; var a = 2; a";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "2");
    }

    #[test]
    fn test_var_in_for_loop() {
        let script = "for (var i = 0; i < 3; i++) {} i";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_for_in_loop_array() {
        let script = r#"
            let arr = [10, 20, 30];
            let sum = 0;
            for (let key in arr) {
                sum += arr[key];
            }
            sum
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "60");
    }

    #[test]
    fn test_for_in_loop_object() {
        let script = r#"
            let obj = {a: 1, b: 2, c: 3};
            let sum = 0;
            for (let key in obj) {
                sum = sum + obj[key];
            }
            sum
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_for_in_loop_empty_object() {
        let script = r#"
            let obj = {};
            let count = 0;
            for (let key in obj) {
                count = count + 1;
            }
            count
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "0");
    }

    #[test]
    fn test_for_in_loop_with_break() {
        let script = r#"
            let obj = {a: 1, b: 2, c: 3};
            let count = 0;
            for (let key in obj) {
                if (count > 0) { break; }
                count = count + 1;
            }
            count
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "1");
    }

    #[test]
    fn test_for_in_loop_array_length() {
        let script = r#"
            let arr = [1, 2];
            let count = 0;
            for (var key in arr) {
                count = count + 1;
            }
            count
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "2");
    }
}
