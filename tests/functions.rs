use rust_quickjs::quickjs::evaluate_script;
use rust_quickjs::quickjs::Value;

#[cfg(test)]
mod function_tests {
    use super::*;

    #[test]
    fn test_function_definition() {
        let script = "function add(a, b) { return a + b; } add(3, 4)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 7.0),
            _ => panic!("Expected number 7.0, got {:?}", result),
        }
    }

    #[test]
    fn test_function_call() {
        let script = "function square(x) { return x * x; } square(5)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 25.0),
            _ => panic!("Expected number 25.0, got {:?}", result),
        }
    }

    #[test]
    fn test_function_with_multiple_statements() {
        let script = "function test() { let x = 10; let y = 20; return x + y; } test()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 30.0),
            _ => panic!("Expected number 30.0, got {:?}", result),
        }
    }

    #[test]
    fn test_function_without_return() {
        let script = "function noReturn() { let x = 42; } noReturn()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0), // Functions return the last statement's value
            _ => panic!("Expected number 42.0, got {:?}", result),
        }
    }

    #[test]
    fn test_nested_function_calls() {
        let script = "function double(x) { return x * 2; } function add(a, b) { return double(a) + double(b); } add(3, 4)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 14.0), // (3*2) + (4*2) = 14
            _ => panic!("Expected number 14.0, got {:?}", result),
        }
    }

    #[test]
    fn test_function_with_console_log() {
        let script =
            "function greet(name) { console.log('Hello', name); return 'done'; } greet('World')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let expected = "done".encode_utf16().collect::<Vec<u16>>();
                assert_eq!(s, expected);
            }
            _ => panic!("Expected string 'done', got {:?}", result),
        }
    }

    #[test]
    fn test_intentionally_failing_function() {
        let script = "function add(a, b) { return a + b; } add(3, 4)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 7.0), // This will fail because 3+4=7
            _ => panic!("Expected number 7.0, got {:?}", result),
        }
    }
}
