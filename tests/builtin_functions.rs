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
}
