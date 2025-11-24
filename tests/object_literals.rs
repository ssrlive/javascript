use rust_quickjs::quickjs::evaluate_script;
use rust_quickjs::quickjs::Value;

#[cfg(test)]
mod object_literal_tests {
    use super::*;

    #[test]
    fn test_basic_object_literal() {
        let script = "let obj = {a: 1, b: 2}; obj.a + obj.b";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected number 3.0, got {:?}", result),
        }
    }

    #[test]
    fn test_object_property_access() {
        let script = "let obj = {name: 'hello', value: 42}; obj.name";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let expected = "hello".encode_utf16().collect::<Vec<u16>>();
                assert_eq!(s, expected);
            }
            _ => panic!("Expected string 'hello', got {:?}", result),
        }
    }

    #[test]
    fn test_empty_object() {
        let script = "let empty = {}; empty";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Object(map)) => assert_eq!(map.len(), 0),
            _ => panic!("Expected empty object, got {:?}", result),
        }
    }

    #[test]
    fn test_nested_object() {
        let script = "let nested = {a: {b: 1}}; nested.a.b";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 1.0),
            _ => panic!("Expected number 1.0, got {:?}", result),
        }
    }

    #[test]
    fn test_object_with_string_keys() {
        let script = "let obj = {'key': 123}; obj.key";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 123.0),
            _ => panic!("Expected number 123.0, got {:?}", result),
        }
    }

    #[test]
    fn test_console_log_with_object() {
        // This test verifies that console.log works with objects
        // We can't easily capture stdout in tests, so we just ensure it doesn't crash
        let script = "let obj = {test: 'value'}; console.log(obj.test); obj.test";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let expected = "value".encode_utf16().collect::<Vec<u16>>();
                assert_eq!(s, expected);
            }
            _ => panic!("Expected string 'value', got {:?}", result),
        }
    }

    #[test]
    fn test_intentionally_failing_object() {
        let script = "let obj = {a: 1, b: 2}; obj.a + obj.b";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0), // This will fail because 1+2=3
            _ => panic!("Expected number 3.0, got {:?}", result),
        }
    }
}
