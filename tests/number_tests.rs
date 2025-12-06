use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod number_tests {
    use super::*;

    #[test]
    fn test_number_max_value() {
        let script = "Number.MAX_VALUE";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert_eq!(n, f64::MAX);
            }
            _ => panic!("Expected Number.MAX_VALUE to be f64::MAX, got {:?}", result),
        }
    }

    #[test]
    fn test_number_min_value() {
        let script = "Number.MIN_VALUE";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert_eq!(n, f64::MIN_POSITIVE);
            }
            _ => panic!("Expected Number.MIN_VALUE to be f64::MIN_POSITIVE, got {:?}", result),
        }
    }

    #[test]
    fn test_number_nan() {
        let script = "Number.NaN";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert!(n.is_nan());
            }
            _ => panic!("Expected Number.NaN to be NaN, got {:?}", result),
        }
    }

    #[test]
    fn test_number_positive_infinity() {
        let script = "Number.POSITIVE_INFINITY";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert_eq!(n, f64::INFINITY);
            }
            _ => panic!("Expected Number.POSITIVE_INFINITY to be f64::INFINITY, got {:?}", result),
        }
    }

    #[test]
    fn test_number_negative_infinity() {
        let script = "Number.NEGATIVE_INFINITY";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert_eq!(n, f64::NEG_INFINITY);
            }
            _ => panic!("Expected Number.NEGATIVE_INFINITY to be f64::NEG_INFINITY, got {:?}", result),
        }
    }

    #[test]
    fn test_number_epsilon() {
        let script = "Number.EPSILON";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert_eq!(n, f64::EPSILON);
            }
            _ => panic!("Expected Number.EPSILON to be f64::EPSILON, got {:?}", result),
        }
    }

    #[test]
    fn test_number_max_safe_integer() {
        let script = "Number.MAX_SAFE_INTEGER";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert_eq!(n, 9007199254740991.0);
            }
            _ => panic!("Expected Number.MAX_SAFE_INTEGER to be 9007199254740991.0, got {:?}", result),
        }
    }

    #[test]
    fn test_number_min_safe_integer() {
        let script = "Number.MIN_SAFE_INTEGER";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert_eq!(n, -9007199254740991.0);
            }
            _ => panic!("Expected Number.MIN_SAFE_INTEGER to be -9007199254740991.0, got {:?}", result),
        }
    }

    #[test]
    fn test_number_is_nan() {
        // Test with NaN
        let script = "Number.isNaN(NaN)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected Number.isNaN(NaN) to be true, got {:?}", result),
        }

        // Test with number
        let script = "Number.isNaN(42)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isNaN(42) to be false, got {:?}", result),
        }

        // Test with string that parses to NaN
        let script = "Number.isNaN('not a number')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b), // String is not NaN, it's just a string
            _ => panic!("Expected Number.isNaN('not a number') to be false, got {:?}", result),
        }

        // Test with undefined
        let script = "Number.isNaN(undefined)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isNaN(undefined) to be false, got {:?}", result),
        }
    }

    #[test]
    fn test_number_is_finite() {
        // Test with finite number
        let script = "Number.isFinite(42)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected Number.isFinite(42) to be true, got {:?}", result),
        }

        // Test with Infinity
        let script = "Number.isFinite(Infinity)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isFinite(Infinity) to be false, got {:?}", result),
        }

        // Test with NaN
        let script = "Number.isFinite(NaN)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isFinite(NaN) to be false, got {:?}", result),
        }

        // Test with string
        let script = "Number.isFinite('42')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isFinite('42') to be false, got {:?}", result),
        }
    }

    #[test]
    fn test_number_is_integer() {
        // Test with integer
        let script = "Number.isInteger(42)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected Number.isInteger(42) to be true, got {:?}", result),
        }

        // Test with float
        let script = "Number.isInteger(42.5)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isInteger(42.5) to be false, got {:?}", result),
        }

        // Test with Infinity
        let script = "Number.isInteger(Infinity)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isInteger(Infinity) to be false, got {:?}", result),
        }

        // Test with NaN
        let script = "Number.isInteger(NaN)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isInteger(NaN) to be false, got {:?}", result),
        }

        // Test with string
        let script = "Number.isInteger('42')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isInteger('42') to be false, got {:?}", result),
        }
    }

    #[test]
    fn test_number_is_safe_integer() {
        // Test with safe integer
        let script = "Number.isSafeInteger(42)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected Number.isSafeInteger(42) to be true, got {:?}", result),
        }

        // Test with MAX_SAFE_INTEGER
        let script = "Number.isSafeInteger(9007199254740991)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected Number.isSafeInteger(9007199254740991) to be true, got {:?}", result),
        }

        // Test with MIN_SAFE_INTEGER
        let script = "Number.isSafeInteger(-9007199254740991)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected Number.isSafeInteger(-9007199254740991) to be true, got {:?}", result),
        }

        // Test with unsafe integer (too large)
        let script = "Number.isSafeInteger(9007199254740992)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isSafeInteger(9007199254740992) to be false, got {:?}", result),
        }

        // Test with float
        let script = "Number.isSafeInteger(42.5)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isSafeInteger(42.5) to be false, got {:?}", result),
        }

        // Test with Infinity
        let script = "Number.isSafeInteger(Infinity)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Boolean(b)) => assert!(!b),
            _ => panic!("Expected Number.isSafeInteger(Infinity) to be false, got {:?}", result),
        }
    }

    #[test]
    fn test_number_parse_float() {
        // Test with valid float string
        let script = "Number.parseFloat('3.16')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.16),
            _ => panic!("Expected Number.parseFloat('3.16') to be 3.16, got {:?}", result),
        }

        // Test with integer string
        let script = "Number.parseFloat('42')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0),
            _ => panic!("Expected Number.parseFloat('42') to be 42.0, got {:?}", result),
        }

        // Test with invalid string
        let script = "Number.parseFloat('not a number')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert!(n.is_nan()),
            _ => panic!("Expected Number.parseFloat('not a number') to be NaN, got {:?}", result),
        }

        // Test with number
        let script = "Number.parseFloat(42.5)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.5),
            _ => panic!("Expected Number.parseFloat(42.5) to be 42.5, got {:?}", result),
        }

        // Test with whitespace
        let script = "Number.parseFloat('  3.16  ')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 3.16),
            _ => panic!("Expected Number.parseFloat('  3.16  ') to be 3.16, got {:?}", result),
        }
    }

    #[test]
    fn test_number_parse_int() {
        // Test with valid integer string
        let script = "Number.parseInt('42')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0),
            _ => panic!("Expected Number.parseInt('42') to be 42.0, got {:?}", result),
        }

        // Test with float string (should truncate)
        let script = "Number.parseInt('42.5')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0),
            _ => panic!("Expected Number.parseInt('42.5') to be 42.0, got {:?}", result),
        }

        // Test with invalid string
        let script = "Number.parseInt('not a number')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert!(n.is_nan()),
            _ => panic!("Expected Number.parseInt('not a number') to be NaN, got {:?}", result),
        }

        // Test with radix
        let script = "Number.parseInt('101', 2)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 5.0), // 101 in binary is 5
            _ => panic!("Expected Number.parseInt('101', 2) to be 5.0, got {:?}", result),
        }

        // Test with hex
        let script = "Number.parseInt('FF', 16)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 255.0),
            _ => panic!("Expected Number.parseInt('FF', 16) to be 255.0, got {:?}", result),
        }

        // Test with number
        let script = "Number.parseInt(42.7)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0),
            _ => panic!("Expected Number.parseInt(42.7) to be 42.0, got {:?}", result),
        }
    }

    #[test]
    fn test_number_constructor_no_args() {
        let script = "Number()";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 0.0),
            _ => panic!("Expected Number() to be 0.0, got {:?}", result),
        }
    }

    #[test]
    fn test_number_constructor_with_number() {
        let script = "Number(42.5)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.5),
            _ => panic!("Expected Number(42.5) to be 42.5, got {:?}", result),
        }
    }

    #[test]
    fn test_number_constructor_with_string() {
        let script = "Number('42.5')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.5),
            _ => panic!("Expected Number('42.5') to be 42.5, got {:?}", result),
        }
    }

    #[test]
    fn test_number_constructor_with_boolean() {
        let script = "Number(true)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 1.0),
            _ => panic!("Expected Number(true) to be 1.0, got {:?}", result),
        }

        let script = "Number(false)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 0.0),
            _ => panic!("Expected Number(false) to be 0.0, got {:?}", result),
        }
    }

    #[test]
    fn test_number_constructor_with_invalid_string() {
        let script = "Number('not a number')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert!(n.is_nan()),
            _ => panic!("Expected Number('not a number') to be NaN, got {:?}", result),
        }
    }

    #[test]
    fn test_number_constructor_with_undefined() {
        let script = "Number(undefined)";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => assert!(n.is_nan()),
            _ => panic!("Expected Number(undefined) to be NaN, got {:?}", result),
        }
    }

    #[test]
    fn test_number_object_properties_exist() {
        // Test that all expected properties exist on the Number object
        let properties = vec![
            "MAX_VALUE",
            "MIN_VALUE",
            "NaN",
            "POSITIVE_INFINITY",
            "NEGATIVE_INFINITY",
            "EPSILON",
            "MAX_SAFE_INTEGER",
            "MIN_SAFE_INTEGER",
            "isNaN",
            "isFinite",
            "isInteger",
            "isSafeInteger",
            "parseFloat",
            "parseInt",
        ];

        for prop in properties {
            let script = format!("typeof Number.{}", prop);
            let result = evaluate_script(&script);
            match result {
                Ok(Value::String(s)) => {
                    let type_str = String::from_utf16_lossy(&s);
                    assert_ne!(type_str, "undefined", "Number.{} should exist", prop);
                }
                _ => panic!("Expected typeof Number.{} to return a string, got {:?}", prop, result),
            }
        }
    }

    #[test]
    fn test_bitwise_xor_numbers() {
        let script = "5 ^ 3";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert_eq!(n, 6.0); // 5 ^ 3 = 6
            }
            _ => panic!("Expected 5 ^ 3 to evaluate to 6, got {:?}", result),
        }
    }

    #[test]
    fn test_bitwise_xor_negative_numbers() {
        let script = "-5 ^ 3";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert_eq!(n, -8.0); // -5 ^ 3 = -8
            }
            _ => panic!("Expected -5 ^ 3 to evaluate to -8, got {:?}", result),
        }
    }

    #[test]
    fn test_bitwise_xor_assignment() {
        let script = "let a = 5; a ^= 3; a";
        let result = evaluate_script(script);
        match result {
            Ok(Value::Number(n)) => {
                assert_eq!(n, 6.0); // a = 5; a ^= 3; a = 6
            }
            _ => panic!("Expected a ^= 3 to evaluate to 6, got {:?}", result),
        }
    }
}
