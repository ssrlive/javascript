use rust_quickjs::quickjs::evaluate_script;
use rust_quickjs::quickjs::Value;

#[cfg(test)]
mod builtin_functions_tests {
    use super::*;

    #[test]
    fn test_math_constants() {
        let script = "Math.PI";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert!((n - std::f64::consts::PI).abs() < 0.0001);
            }
            _ => panic!("Expected Math.PI to be a number, got {:?}", result),
        }

        let script = "Math.E";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert!((n - std::f64::consts::E).abs() < 0.0001);
            }
            _ => panic!("Expected Math.E to be a number, got {:?}", result),
        }
    }

    #[test]
    fn test_math_floor() {
        let script = "Math.floor(3.7)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected Math.floor(3.7) to be 3.0, got {:?}", result),
        }
    }

    #[test]
    fn test_math_ceil() {
        let script = "Math.ceil(3.1)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 4.0),
            _ => panic!("Expected Math.ceil(3.1) to be 4.0, got {:?}", result),
        }
    }

    #[test]
    fn test_math_sqrt() {
        let script = "Math.sqrt(9)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected Math.sqrt(9) to be 3.0, got {:?}", result),
        }
    }

    #[test]
    fn test_math_pow() {
        let script = "Math.pow(2, 3)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 8.0),
            _ => panic!("Expected Math.pow(2, 3) to be 8.0, got {:?}", result),
        }
    }

    #[test]
    fn test_math_sin() {
        let script = "Math.sin(0)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 0.0),
            _ => panic!("Expected Math.sin(0) to be 0.0, got {:?}", result),
        }
    }

    #[test]
    fn test_math_random() {
        let script = "Math.random()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert!(n >= 0.0 && n < 1.0);
            }
            _ => panic!(
                "Expected Math.random() to be a number between 0 and 1, got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_parse_int() {
        let script = "parseInt('42')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0),
            _ => panic!("Expected parseInt('42') to be 42.0, got {:?}", result),
        }

        let script = "parseInt('3.14')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected parseInt('3.14') to be 3.0, got {:?}", result),
        }
    }

    #[test]
    fn test_parse_float() {
        let script = "parseFloat('3.14')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.14),
            _ => panic!("Expected parseFloat('3.14') to be 3.14, got {:?}", result),
        }
    }

    #[test]
    fn test_is_nan() {
        let script = "isNaN(NaN)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert_eq!(b, true),
            _ => panic!("Expected isNaN(NaN) to be true, got {:?}", result),
        }

        let script = "isNaN(42)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert_eq!(b, false),
            _ => panic!("Expected isNaN(42) to be false, got {:?}", result),
        }
    }

    #[test]
    fn test_is_finite() {
        let script = "isFinite(42)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert_eq!(b, true),
            _ => panic!("Expected isFinite(42) to be true, got {:?}", result),
        }

        let script = "isFinite(Infinity)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert_eq!(b, false),
            _ => panic!("Expected isFinite(Infinity) to be false, got {:?}", result),
        }
    }

    #[test]
    fn test_json_stringify() {
        let script = "JSON.stringify(42)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "42");
            }
            _ => panic!("Expected JSON.stringify(42) to be '42', got {:?}", result),
        }
    }

    #[test]
    fn test_array_push() {
        let script =
            "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.length";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 2.0),
            _ => panic!("Expected array length to be 2.0, got {:?}", result),
        }
    }

    #[test]
    fn test_array_pop() {
        let script =
            "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.pop()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 2.0),
            _ => panic!("Expected arr.pop() to return 2.0, got {:?}", result),
        }
    }

    #[test]
    fn test_array_join() {
        let script = "let arr = Array(); let arr2 = arr.push('a'); let arr3 = arr2.push('b'); arr3.join('-')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "a-b");
            }
            _ => panic!("Expected arr.join('-') to be 'a-b', got {:?}", result),
        }
    }

    #[test]
    fn test_object_keys() {
        let script = "let obj = {a: 1, b: 2}; Object.keys(obj).length";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 2.0),
            _ => panic!(
                "Expected Object.keys(obj).length to be 2.0, got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_encode_uri_component() {
        let script = "encodeURIComponent('hello world')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello%20world");
            }
            _ => panic!(
                "Expected encodeURIComponent('hello world') to be 'hello%20world', got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_decode_uri_component() {
        let script = "decodeURIComponent('hello%20world')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello world");
            }
            _ => panic!(
                "Expected decodeURIComponent('hello%20world') to be 'hello world', got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_number_constructor() {
        let script = "Number('42.5')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.5),
            _ => panic!("Expected Number('42.5') to be 42.5, got {:?}", result),
        }
    }

    #[test]
    fn test_boolean_constructor() {
        let script = "Boolean(1)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert_eq!(b, true),
            _ => panic!("Expected Boolean(1) to be true, got {:?}", result),
        }
    }

    #[test]
    fn test_eval_function() {
        let script = "eval('hello')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello");
            }
            _ => panic!("Expected eval('hello') to return 'hello', got {:?}", result),
        }
    }

    #[test]
    fn test_encode_uri() {
        let script = "encodeURI('hello world')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello%20world");
            }
            _ => panic!(
                "Expected encodeURI('hello world') to be 'hello%20world', got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_decode_uri() {
        let script = "decodeURI('hello%20world')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello world");
            }
            _ => panic!(
                "Expected decodeURI('hello%20world') to be 'hello world', got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_array_for_each() {
        let script =
            "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.forEach(function(x) { return x; })";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => {
                // forEach returns undefined
            }
            _ => panic!("Expected arr.forEach to return undefined, got {:?}", result),
        }
    }

    #[test]
    fn test_array_map() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let mapped = arr3.map(function(x) { return x * 2; }); mapped.length";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 2.0),
            _ => panic!("Expected mapped array length to be 2.0, got {:?}", result),
        }
    }

    #[test]
    fn test_array_filter() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let filtered = arr3.filter(function(x) { return x > 1; }); filtered.length";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 1.0),
            _ => panic!("Expected filtered array length to be 1.0, got {:?}", result),
        }
    }

    #[test]
    fn test_array_reduce() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.reduce(function(acc, x) { return acc + x; }, 0)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 6.0),
            _ => panic!("Expected arr.reduce to return 6.0, got {:?}", result),
        }
    }

    #[test]
    fn test_string_split_simple() {
        let script = "let parts = 'a,b,c'.split(','); parts.length";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected split length to be 3.0, got {:?}", result),
        }
    }

    #[test]
    fn test_string_split_empty_sep() {
        let script = "let parts = 'abc'.split(''); parts.length";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!(
                "Expected split empty-sep length to be 3.0, got {:?}",
                result
            ),
        }
    }

    #[test]
    fn test_string_char_at() {
        let script = "'hello'.charAt(1)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "e");
            }
            _ => panic!("Expected charAt to return 'e', got {:?}", result),
        }
    }

    #[test]
    fn test_string_replace_functional() {
        let script = "'hello world'.replace('world', 'there')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello there");
            }
            _ => panic!("Expected replace to return 'hello there', got {:?}", result),
        }
    }

    #[test]
    fn test_array_map_values() {
        let script = "let arr = Array(); let a2 = arr.push(1); let a3 = a2.push(2); let mapped = a3.map(function(x) { return x * 2; }); mapped.join(',')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "2,4");
            }
            _ => panic!("Expected mapped.join(',') to be '2,4', got {:?}", result),
        }
    }
}
