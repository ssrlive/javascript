use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

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
                assert!((0.0..1.0).contains(&n));
            }
            _ => panic!("Expected Math.random() to be a number between 0 and 1, got {:?}", result),
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
    #[allow(clippy::approx_constant)]
    fn test_parse_float() {
        let script = "parseFloat('3.14')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.14_f64),
            _ => panic!("Expected parseFloat('3.14') to be 3.14, got {:?}", result),
        }
    }

    #[test]
    fn test_is_nan() {
        let script = "isNaN(NaN)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected isNaN(NaN) to be true, got {:?}", result),
        }

        let script = "isNaN(42)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected isNaN(42) to be false, got {:?}", result),
        }
    }

    #[test]
    fn test_is_finite() {
        let script = "isFinite(42)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected isFinite(42) to be true, got {:?}", result),
        }

        let script = "isFinite(Infinity)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
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
    fn test_json_parse_stringify_roundtrip() {
        let script = r#"let obj = JSON.parse('{"name":"John","age":30,"city":"New York"}'); JSON.stringify(obj)"#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                // Should be the same as input (order may differ, but for this simple case it should match)
                assert_eq!(str_val, r#"{"age":30,"city":"New York","name":"John"}"#);
            }
            _ => panic!("Expected JSON roundtrip to return string, got {:?}", result),
        }
    }

    #[test]
    fn test_json_parse_array() {
        let script = r#"let arr = JSON.parse('[1, "hello", true, null]'); arr.length"#;
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 4.0),
            _ => panic!("Expected parsed array length to be 4.0, got {:?}", result),
        }

        // Check elements
        let script2 = r#"let arr = JSON.parse('[1, "hello", true, null]'); arr[0]"#;
        let result2 = evaluate_script(script2);
        match result2 {
            Ok(Value::Number(n)) => assert_eq!(n, 1.0),
            _ => panic!("Expected arr[0] to be 1.0, got {:?}", result2),
        }

        let script3 = r#"let arr = JSON.parse('[1, "hello", true, null]'); arr[1]"#;
        let result3 = evaluate_script(script3);
        match result3 {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello");
            }
            _ => panic!("Expected arr[1] to be 'hello', got {:?}", result3),
        }

        let script4 = r#"let arr = JSON.parse('[1, "hello", true, null]'); arr[2]"#;
        let result4 = evaluate_script(script4);
        match result4 {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected arr[2] to be true, got {:?}", result4),
        }

        let script5 = r#"let arr = JSON.parse('[1, "hello", true, null]'); arr[3]"#;
        let result5 = evaluate_script(script5);
        match result5 {
            Ok(Value::Undefined) => {}
            _ => panic!("Expected arr[3] to be undefined (null), got {:?}", result5),
        }
    }

    #[test]
    fn test_array_push() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.length";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 2.0),
            _ => panic!("Expected array length to be 2.0, got {:?}", result),
        }
    }

    #[test]
    fn test_array_pop() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.pop()";
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
            _ => panic!("Expected Object.keys(obj).length to be 2.0, got {:?}", result),
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
            _ => panic!("Expected encodeURIComponent('hello world') to be 'hello%20world', got {:?}", result),
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
            _ => panic!("Expected decodeURIComponent('hello%20world') to be 'hello world', got {:?}", result),
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
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected Boolean(1) to be true, got {:?}", result),
        }
    }

    #[test]
    fn test_eval_function() {
        let script = "eval('\"hello\"')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello");
            }
            _ => panic!("Expected eval('\"hello\"') to return 'hello', got {:?}", result),
        }
    }

    #[test]
    fn test_eval_expression() {
        let script = "eval('1 + 2')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected eval('1 + 2') to return 3.0, got {:?}", result),
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
            _ => panic!("Expected encodeURI('hello world') to be 'hello%20world', got {:?}", result),
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
            _ => panic!("Expected decodeURI('hello%20world') to be 'hello world', got {:?}", result),
        }
    }

    #[test]
    fn test_array_for_each() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.forEach(function(x) { return x; })";
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
            _ => panic!("Expected split empty-sep length to be 3.0, got {:?}", result),
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

    #[test]
    fn test_string_trim() {
        let script = "'  hello world  '.trim()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello world");
            }
            _ => panic!("Expected trim to return 'hello world', got {:?}", result),
        }
    }

    #[test]
    fn test_string_starts_with() {
        let script = "'hello world'.startsWith('hello')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected startsWith to return true, got {:?}", result),
        }
    }

    #[test]
    fn test_string_ends_with() {
        let script = "'hello world'.endsWith('world')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected endsWith to return true, got {:?}", result),
        }
    }

    #[test]
    fn test_string_includes() {
        let script = "'hello world'.includes('lo wo')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected includes to return true, got {:?}", result),
        }
    }

    #[test]
    fn test_string_repeat() {
        let script = "'ha'.repeat(3)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hahaha");
            }
            _ => panic!("Expected repeat to return 'hahaha', got {:?}", result),
        }
    }

    #[test]
    fn test_string_concat() {
        let script = "'hello'.concat(' ', 'world', '!')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "hello world!");
            }
            _ => panic!("Expected concat to return 'hello world!', got {:?}", result),
        }
    }

    #[test]
    fn test_string_pad_start() {
        let script = "'5'.padStart(3, '0')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "005");
            }
            _ => panic!("Expected padStart to return '005', got {:?}", result),
        }
    }

    #[test]
    fn test_string_pad_end() {
        let script = "'5'.padEnd(3, '0')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "500");
            }
            _ => panic!("Expected padEnd to return '500', got {:?}", result),
        }
    }

    #[test]
    fn test_array_find() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.find(function(x) { return x > 2; })";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected arr.find to return 3.0, got {:?}", result),
        }

        // Test find with no match
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.find(function(x) { return x > 5; })";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => {
                // find returns undefined when no element matches
            }
            _ => panic!("Expected arr.find to return undefined, got {:?}", result),
        }
    }

    #[test]
    fn test_array_find_index() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.findIndex(function(x) { return x > 2; })";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 2.0),
            _ => panic!("Expected arr.findIndex to return 2.0, got {:?}", result),
        }

        // Test findIndex with no match
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.findIndex(function(x) { return x > 5; })";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, -1.0),
            _ => panic!("Expected arr.findIndex to return -1.0, got {:?}", result),
        }
    }

    #[test]
    fn test_array_some() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.some(function(x) { return x > 2; })";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected arr.some to return true, got {:?}", result),
        }

        // Test some with no match
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.some(function(x) { return x > 5; })";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected arr.some to return false, got {:?}", result),
        }
    }

    #[test]
    fn test_array_every() {
        let script = "let arr = Array(); let arr2 = arr.push(2); let arr3 = arr2.push(4); let arr4 = arr3.push(6); arr4.every(function(x) { return x > 1; })";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected arr.every to return true, got {:?}", result),
        }

        // Test every with some elements not matching
        let script = "let arr = Array(); let arr2 = arr.push(2); let arr3 = arr2.push(1); let arr4 = arr3.push(6); arr4.every(function(x) { return x > 1; })";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected arr.every to return false, got {:?}", result),
        }
    }

    #[test]
    fn test_array_concat() {
        let script = "let arr1 = Array(); let arr2 = arr1.push(1); let arr3 = arr2.push(2); let arr4 = Array(); let arr5 = arr4.push(3); let arr6 = arr5.push(4); let result = arr3.concat(arr6); result.length";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 4.0),
            _ => panic!("Expected concat result length to be 4.0, got {:?}", result),
        }

        // Test concat with non-array values
        let script = "let arr1 = Array(); let arr2 = arr1.push(1); let result = arr2.concat(2, 3); result.length";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected concat result length to be 3.0, got {:?}", result),
        }
    }

    #[test]
    fn test_array_index_of() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.indexOf(2)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 1.0),
            _ => panic!("Expected arr.indexOf(2) to return 1.0, got {:?}", result),
        }

        // Test indexOf with element not found
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.indexOf(5)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, -1.0),
            _ => panic!("Expected arr.indexOf(5) to return -1.0, got {:?}", result),
        }

        // Test indexOf with fromIndex
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(2); arr4.indexOf(2, 2)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 2.0),
            _ => panic!("Expected arr.indexOf(2, 2) to return 2.0, got {:?}", result),
        }
    }

    #[test]
    fn test_array_includes() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.includes(2)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected arr.includes(2) to return true, got {:?}", result),
        }

        // Test includes with element not found
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.includes(5)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected arr.includes(5) to return false, got {:?}", result),
        }

        // Test includes with fromIndex
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(2); arr4.includes(2, 2)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected arr.includes(2, 2) to return true, got {:?}", result),
        }
    }

    #[test]
    fn test_array_sort() {
        let script = "let arr = Array(); let arr2 = arr.push(3); let arr3 = arr2.push(1); let arr4 = arr3.push(2); let sorted = arr4.sort(); sorted.join(',')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "1,2,3");
            }
            _ => panic!("Expected sorted array join to return '1,2,3', got {:?}", result),
        }

        // Test sort with custom compare function
        let script = "let arr = Array(); let arr2 = arr.push(3); let arr3 = arr2.push(1); let arr4 = arr3.push(2); let sorted = arr4.sort(function(a, b) { return b - a; }); sorted.join(',')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "3,2,1");
            }
            _ => panic!("Expected sorted array join to return '3,2,1', got {:?}", result),
        }
    }

    #[test]
    fn test_array_reverse() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let reversed = arr4.reverse(); reversed.join(',')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "3,2,1");
            }
            _ => panic!("Expected reversed array join to return '3,2,1', got {:?}", result),
        }
    }

    #[test]
    fn test_array_splice() {
        // Test basic splice - remove elements
        let script =
            "let arr = Array(); arr.push(1); arr.push(2); arr.push(3); arr.push(4); let removed = arr.splice(1, 2); removed.join(',')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "2,3");
            }
            _ => panic!("Expected splice to return '2,3', got {:?}", result),
        }

        // Test splice with insertion (no elements removed)
        let script2 = "let arr = Array(); arr.push(1); arr.push(4); let removed = arr.splice(1, 0, 2, 3); removed.length";
        let result2 = evaluate_script(script2);
        match result2 {
            Ok(Value::Number(n)) => assert_eq!(n, 0.0), // No elements were removed
            _ => panic!("Expected splice with insertion to return empty array (length 0), got {:?}", result2),
        }
    }

    #[test]
    fn test_array_shift() {
        let script = "let arr = Array(); arr.push(1); arr.push(2); arr.push(3); let first = arr.shift(); arr.join(',')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "2,3");
            }
            _ => panic!("Expected shift to result in '2,3', got {:?}", result),
        }

        // Test shift on empty array
        let script2 = "let arr = Array(); arr.shift()";
        let result2 = evaluate_script(script2);
        match result2 {
            Ok(Value::Undefined) => {}
            _ => panic!("Expected shift on empty array to return undefined, got {:?}", result2),
        }
    }

    #[test]
    fn test_array_unshift() {
        let script = "let arr = Array(); arr.push(3); arr.push(4); let len = arr.unshift(1, 2); arr.join(',') + ',' + len";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "1,2,3,4,4");
            }
            _ => panic!("Expected unshift to return '1,2,3,4,4', got {:?}", result),
        }

        // Test unshift on empty array
        let script2 = "let arr = Array(); let len = arr.unshift(1, 2, 3); len";
        let result2 = evaluate_script(script2);
        match result2 {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected unshift on empty array to return 3, got {:?}", result2),
        }
    }

    #[test]
    fn test_array_fill() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let arr5 = arr4.push(4); let filled = arr5.fill(9, 1, 3); filled.join(',')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "1,9,9,4");
            }
            _ => panic!("Expected fill to return '1,9,9,4', got {:?}", result),
        }

        // Test fill entire array
        let script2 = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let filled = arr4.fill(0); filled.join(',')";
        let result2 = evaluate_script(script2);
        match result2 {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "0,0,0");
            }
            _ => panic!("Expected fill entire array to return '0,0,0', got {:?}", result2),
        }
    }

    #[test]
    fn test_array_last_index_of() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let arr5 = arr4.push(2); let arr6 = arr5.push(1); arr6.lastIndexOf(2)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.0),
            _ => panic!("Expected lastIndexOf(2) to return 3, got {:?}", result),
        }

        // Test element not found
        let script2 = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.lastIndexOf(4)";
        let result2 = evaluate_script(script2);
        match result2 {
            Ok(Value::Number(n)) => assert_eq!(n, -1.0),
            _ => panic!("Expected lastIndexOf(4) to return -1, got {:?}", result2),
        }

        // Test with fromIndex
        let script3 = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let arr5 = arr4.push(2); arr5.lastIndexOf(2, 2)";
        let result3 = evaluate_script(script3);
        match result3 {
            Ok(Value::Number(n)) => assert_eq!(n, 1.0),
            _ => panic!("Expected lastIndexOf(2, 2) to return 1, got {:?}", result3),
        }
    }

    #[test]
    fn test_array_to_string() {
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); arr4.toString()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "1,2,3");
            }
            _ => panic!("Expected toString to return '1,2,3', got {:?}", result),
        }

        // Test empty array
        let script2 = "let arr = Array(); arr.toString()";
        let result2 = evaluate_script(script2);
        match result2 {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "");
            }
            _ => panic!("Expected empty array toString to return '', got {:?}", result2),
        }

        // Test array with different types
        let script3 =
            "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push('hello'); let arr4 = arr3.push(true); arr4.toString()";
        let result3 = evaluate_script(script3);
        match result3 {
            Ok(Value::String(s)) => {
                let str_val = String::from_utf16_lossy(&s);
                assert_eq!(str_val, "1,hello,true");
            }
            _ => panic!("Expected mixed array toString to return '1,hello,true', got {:?}", result3),
        }
    }

    #[test]
    fn test_array_flat() {
        // Test basic flat - create nested array manually
        let script = "let arr = Array(); let subarr = Array(); let subarr2 = subarr.push(2); let subarr3 = subarr2.push(3); let arr2 = arr.push(1); let arr3 = arr2.push(subarr3); let arr4 = arr3.push(4); arr4.flat()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Object(obj)) => {
                let obj = obj.borrow();
                // Check length
                if let Some(length_val) = obj.get("length") {
                    match *length_val.borrow() {
                        Value::Number(len) => assert_eq!(len, 4.0),
                        _ => panic!("Expected length to be 4.0"),
                    }
                }
                // Check elements
                if let Some(val0) = obj.get("0") {
                    match *val0.borrow() {
                        Value::Number(n) => assert_eq!(n, 1.0),
                        _ => panic!("Expected element 0 to be 1.0"),
                    }
                }
                if let Some(val1) = obj.get("1") {
                    match *val1.borrow() {
                        Value::Number(n) => assert_eq!(n, 2.0),
                        _ => panic!("Expected element 1 to be 2.0"),
                    }
                }
                if let Some(val2) = obj.get("2") {
                    match *val2.borrow() {
                        Value::Number(n) => assert_eq!(n, 3.0),
                        _ => panic!("Expected element 2 to be 3.0"),
                    }
                }
                if let Some(val3) = obj.get("3") {
                    match *val3.borrow() {
                        Value::Number(n) => assert_eq!(n, 4.0),
                        _ => panic!("Expected element 3 to be 4.0"),
                    }
                }
            }
            _ => panic!("Expected flat() to return an array, got {:?}", result),
        }
    }

    #[test]
    fn test_array_flat_map() {
        // Test basic flatMap - create arrays manually
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.flatMap(function(x) { let result = Array(); let r2 = result.push(x); let r3 = r2.push(x*2); return r3; })";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Object(obj)) => {
                let obj = obj.borrow();
                // Check length
                if let Some(length_val) = obj.get("length") {
                    match *length_val.borrow() {
                        Value::Number(len) => assert_eq!(len, 4.0),
                        _ => panic!("Expected length to be 4.0"),
                    }
                }
                // Check elements
                if let Some(val0) = obj.get("0") {
                    match *val0.borrow() {
                        Value::Number(n) => assert_eq!(n, 1.0),
                        _ => panic!("Expected element 0 to be 1.0"),
                    }
                }
                if let Some(val1) = obj.get("1") {
                    match *val1.borrow() {
                        Value::Number(n) => assert_eq!(n, 2.0),
                        _ => panic!("Expected element 1 to be 2.0"),
                    }
                }
                if let Some(val2) = obj.get("2") {
                    match *val2.borrow() {
                        Value::Number(n) => assert_eq!(n, 2.0),
                        _ => panic!("Expected element 2 to be 2.0"),
                    }
                }
                if let Some(val3) = obj.get("3") {
                    match *val3.borrow() {
                        Value::Number(n) => assert_eq!(n, 4.0),
                        _ => panic!("Expected element 3 to be 4.0"),
                    }
                }
            }
            _ => panic!("Expected flatMap() to return an array, got {:?}", result),
        }
    }

    #[test]
    fn test_array_copy_within() {
        // Test copyWithin
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); let arr4 = arr3.push(3); let arr5 = arr4.push(4); arr5.copyWithin(0, 2, 4)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Object(obj)) => {
                let obj = obj.borrow();
                // Check length
                if let Some(length_val) = obj.get("length") {
                    match *length_val.borrow() {
                        Value::Number(len) => assert_eq!(len, 4.0),
                        _ => panic!("Expected length to be 4.0"),
                    }
                }
                // Check elements after copyWithin(0, 2, 4)
                if let Some(val0) = obj.get("0") {
                    match *val0.borrow() {
                        Value::Number(n) => assert_eq!(n, 3.0),
                        _ => panic!("Expected element 0 to be 3.0"),
                    }
                }
                if let Some(val1) = obj.get("1") {
                    match *val1.borrow() {
                        Value::Number(n) => assert_eq!(n, 4.0),
                        _ => panic!("Expected element 1 to be 4.0"),
                    }
                }
                if let Some(val2) = obj.get("2") {
                    match *val2.borrow() {
                        Value::Number(n) => assert_eq!(n, 3.0),
                        _ => panic!("Expected element 2 to be 3.0"),
                    }
                }
                if let Some(val3) = obj.get("3") {
                    match *val3.borrow() {
                        Value::Number(n) => assert_eq!(n, 4.0),
                        _ => panic!("Expected element 3 to be 4.0"),
                    }
                }
            }
            _ => panic!("Expected copyWithin() to return an array, got {:?}", result),
        }
    }

    #[test]
    fn test_array_entries() {
        // Test entries
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); arr3.entries()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Object(obj)) => {
                let obj = obj.borrow();
                // Check length
                if let Some(length_val) = obj.get("length") {
                    match *length_val.borrow() {
                        Value::Number(len) => assert_eq!(len, 2.0),
                        _ => panic!("Expected length to be 2.0"),
                    }
                }
                // Check first entry [0, 1]
                if let Some(entry0) = obj.get("0") {
                    match &*entry0.borrow() {
                        Value::Object(entry_obj) => {
                            let entry_obj = entry_obj.borrow();
                            if let Some(idx) = entry_obj.get("0") {
                                match *idx.borrow() {
                                    Value::Number(n) => assert_eq!(n, 0.0),
                                    _ => panic!("Expected entry[0][0] to be 0.0"),
                                }
                            }
                            if let Some(val) = entry_obj.get("1") {
                                match *val.borrow() {
                                    Value::Number(n) => assert_eq!(n, 1.0),
                                    _ => panic!("Expected entry[0][1] to be 1.0"),
                                }
                            }
                        }
                        _ => panic!("Expected entry to be an object"),
                    }
                }
                // Check second entry [1, 2]
                if let Some(entry1) = obj.get("1") {
                    match &*entry1.borrow() {
                        Value::Object(entry_obj) => {
                            let entry_obj = entry_obj.borrow();
                            if let Some(idx) = entry_obj.get("0") {
                                match *idx.borrow() {
                                    Value::Number(n) => assert_eq!(n, 1.0),
                                    _ => panic!("Expected entry[1][0] to be 1.0"),
                                }
                            }
                            if let Some(val) = entry_obj.get("1") {
                                match *val.borrow() {
                                    Value::Number(n) => assert_eq!(n, 2.0),
                                    _ => panic!("Expected entry[1][1] to be 2.0"),
                                }
                            }
                        }
                        _ => panic!("Expected entry to be an object"),
                    }
                }
            }
            _ => panic!("Expected entries() to return an array of entries, got {:?}", result),
        }
    }

    #[test]
    fn test_array_from() {
        // Test Array.from with array-like object
        let script = "let arr = Array(); let arr2 = arr.push(1); let arr3 = arr2.push(2); Array.from(arr3)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Object(obj)) => {
                let obj = obj.borrow();
                // Check length
                if let Some(length_val) = obj.get("length") {
                    match *length_val.borrow() {
                        Value::Number(len) => assert_eq!(len, 2.0),
                        _ => panic!("Expected length to be 2.0"),
                    }
                }
                // Check elements
                if let Some(val0) = obj.get("0") {
                    match *val0.borrow() {
                        Value::Number(n) => assert_eq!(n, 1.0),
                        _ => panic!("Expected element 0 to be 1.0"),
                    }
                }
                if let Some(val1) = obj.get("1") {
                    match *val1.borrow() {
                        Value::Number(n) => assert_eq!(n, 2.0),
                        _ => panic!("Expected element 1 to be 2.0"),
                    }
                }
            }
            _ => panic!("Expected Array.from() to return an array, got {:?}", result),
        }
    }

    #[test]
    fn test_array_is_array() {
        // Test Array.isArray with array
        let script = "let arr = Array(); let arr2 = arr.push(1); Array.isArray(arr2)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected Array.isArray(array) to return true, got {:?}", result),
        }

        // Test Array.isArray with non-array
        let script2 = "Array.isArray(42)";
        let result2 = evaluate_script(script2);
        match result2 {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Array.isArray(number) to return false, got {:?}", result2),
        }
    }

    #[test]
    fn test_array_of() {
        // Test Array.of
        let script = "Array.of(1, 2, 3)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Object(obj)) => {
                let obj = obj.borrow();
                // Check length
                if let Some(length_val) = obj.get("length") {
                    match *length_val.borrow() {
                        Value::Number(len) => assert_eq!(len, 3.0),
                        _ => panic!("Expected length to be 3.0"),
                    }
                }
                // Check elements
                if let Some(val0) = obj.get("0") {
                    match *val0.borrow() {
                        Value::Number(n) => assert_eq!(n, 1.0),
                        _ => panic!("Expected element 0 to be 1.0"),
                    }
                }
                if let Some(val1) = obj.get("1") {
                    match *val1.borrow() {
                        Value::Number(n) => assert_eq!(n, 2.0),
                        _ => panic!("Expected element 1 to be 2.0"),
                    }
                }
                if let Some(val2) = obj.get("2") {
                    match *val2.borrow() {
                        Value::Number(n) => assert_eq!(n, 3.0),
                        _ => panic!("Expected element 2 to be 3.0"),
                    }
                }
            }
            _ => panic!("Expected Array.of() to return an array, got {:?}", result),
        }
    }

    #[test]
    fn test_typeof_operator() {
        // Test typeof with different types
        let script = "typeof 42";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "number"),
            _ => panic!("Expected typeof 42 to be 'number', got {:?}", result),
        }

        let script = "typeof 'hello'";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "string"),
            _ => panic!("Expected typeof 'hello' to be 'string', got {:?}", result),
        }

        let script = "typeof true";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "boolean"),
            _ => panic!("Expected typeof true to be 'boolean', got {:?}", result),
        }

        let script = "typeof undefined";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "undefined"),
            _ => panic!("Expected typeof undefined to be 'undefined', got {:?}", result),
        }

        let script = "typeof {}";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "object"),
            _ => panic!("Expected typeof {{}} to be 'object', got {:?}", result),
        }

        let script = "typeof function(){}";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "function"),
            _ => panic!("Expected typeof function(){{}} to be 'function', got {:?}", result),
        }
    }

    #[test]
    fn test_delete_operator() {
        // Test deleting a property from an object
        let script = "let obj = {}; obj.x = 42; delete obj.x; obj.x";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => (), // obj.x should be undefined after deletion
            _ => panic!("Expected obj.x to be undefined after deletion, got {:?}", result),
        }

        // Test deleting a non-existent property
        let script = "let obj = {}; delete obj.nonexistent";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b), // Should return true even for non-existent properties
            _ => panic!("Expected delete on non-existent property to return true, got {:?}", result),
        }

        // Test deleting a variable (should return false)
        let script = "let x = 42; delete x";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b), // Cannot delete local variables
            _ => panic!("Expected delete on local variable to return false, got {:?}", result),
        }
    }

    #[test]
    fn test_void_operator() {
        // Test void with a number
        let script = "void 42";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => (),
            _ => panic!("Expected void 42 to return undefined, got {:?}", result),
        }

        // Test void with an expression
        let script = "void (1 + 2 + 3)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => (),
            _ => panic!("Expected void (1 + 2 + 3) to return undefined, got {:?}", result),
        }

        // Test void with a string
        let script = "void 'hello'";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Undefined) => (),
            _ => panic!("Expected void 'hello' to return undefined, got {:?}", result),
        }
    }

    #[test]
    fn test_in_operator() {
        // Test property exists
        let script = "let obj = {}; obj.x = 42; 'x' in obj";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected 'x' in obj to return true, got {:?}", result),
        }

        // Test property doesn't exist
        let script = "let obj = {}; 'nonexistent' in obj";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected 'nonexistent' in obj to return false, got {:?}", result),
        }

        // Test inherited property
        let script = "let proto = {}; proto.inherited = 'yes'; let obj = {}; obj.__proto__ = proto; 'inherited' in obj";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected 'inherited' in obj to return true for inherited property, got {result:?}"),
        }
    }
}
