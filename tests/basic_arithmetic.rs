use rust_quickjs::quickjs::evaluate_script;
use rust_quickjs::quickjs::Value;

#[cfg(test)]
mod basic_arithmetic_tests {
    use super::*;

    #[test]
    fn test_basic_arithmetic() {
        let script = "let x = 1; let y = 2; x + y";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected number 3.0, got {:?}", result),
        }
    }

    #[test]
    fn test_variable_assignment() {
        let script = "let a = 5; a";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 5.0),
            _ => panic!("Expected number 5.0, got {:?}", result),
        }
    }

    #[test]
    fn test_multiple_operations() {
        let script = "let x = 10; let y = 3; x - y";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 7.0),
            _ => panic!("Expected number 7.0, got {:?}", result),
        }
    }

    #[test]
    fn test_multiplication() {
        let script = "let x = 4; let y = 5; x * y";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 20.0),
            _ => panic!("Expected number 20.0, got {:?}", result),
        }
    }

    #[test]
    fn test_intentionally_failing_arithmetic() {
        let script = "let x = 1; let y = 2; x + y";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected number 3.0, got {:?}", result),
        }
    }
}
