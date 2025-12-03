use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

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
            Ok(Value::Object(map)) => {
                assert_eq!(map.borrow().properties.len(), 0)
            }
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

    #[test]
    fn test_getter_setter_basic() {
        let script = r#"
            let obj = {
                _value: 0,
                get value() { return this._value; },
                set value(v) { this._value = v * 2; }
            };
            obj.value = 5;
            obj.value
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 10.0), // setter multiplies by 2, getter returns 10
            _ => panic!("Expected number 10.0, got {:?}", result),
        }
    }

    #[test]
    fn test_getter_setter_with_computed_property() {
        let script = r#"
            let obj = {
                _data: {},
                get data() { return this._data; },
                set data(value) { this._data = { processed: value * 10 }; }
            };
            obj.data = 3;
            obj.data.processed
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 30.0), // setter processes value * 10
            _ => panic!("Expected number 30.0, got {:?}", result),
        }
    }

    #[test]
    fn test_concise_method_parsing() {
        let script = r#"
            let obj = { foo() { return 7; } };
            obj.foo();
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 7.0),
            _ => panic!("Expected number 7.0, got {:?}", result),
        }
    }

    #[test]
    fn test_computed_getter_parsing() {
        let script = r#"
            function CustomError() {}
            let obj = { get [Symbol.toPrimitive]() { return 42; } };
            42
        "#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0),
            _ => panic!("Expected number 42.0, got {:?}", result),
        }
    }
}
