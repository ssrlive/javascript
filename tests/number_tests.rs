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
    fn test_shift_edge_cases_and_bigint_mixing() {
        // Left shift with large shift amount (masked by 0x1f)
        let res = evaluate_script("let a = 1; a <<= 33; a").unwrap();
        match res {
            Value::Number(n) => assert_eq!(n, 2.0),
            _ => panic!("Expected 2.0 for 1 <<= 33, got {:?}", res),
        }

        // Left shift with negative shift amount -> ToUint32(-1) & 0x1f == 31
        let res = evaluate_script("let a = 1; a <<= -1; a").unwrap();
        match res {
            Value::Number(n) => assert_eq!(n, -2147483648.0),
            _ => panic!("Expected -2147483648.0 for 1 <<= -1, got {:?}", res),
        }

        // Unsigned right shift on negative number
        let res = evaluate_script("let a = -1; a >>>= 1; a").unwrap();
        match res {
            Value::Number(n) => assert_eq!(n, 2147483647.0),
            _ => panic!("Expected 2147483647.0 for -1 >>>= 1, got {:?}", res),
        }

        // Mixing BigInt with Number in shift should throw TypeError
        let res = evaluate_script("let a = 1n; let b = 2; a <<= b");
        match res {
            Err(err) => match err.kind() {
                javascript::JSErrorKind::TypeError { message, .. } => assert!(message.contains("Cannot mix BigInt")),
                _ => panic!("Expected TypeError for mixing BigInt and Number in <<=, got {:?}", err),
            },
            other => panic!("Expected TypeError for mixing BigInt and Number in <<=, got {:?}", other),
        }

        // Unsigned right shift on BigInt should throw TypeError with specific message
        let res = evaluate_script("let a = 1n; a >>>= 1n");
        match res {
            Err(err) => match err.kind() {
                javascript::JSErrorKind::TypeError { message, .. } => assert!(message.contains("Unsigned right shift")),
                _ => panic!("Expected TypeError for BigInt >>>=, got {:?}", err),
            },
            other => panic!("Expected TypeError for BigInt >>>=, got {:?}", other),
        }
    }

    #[test]
    fn test_bigint_shift_and_bitwise_mixing_errors() {
        // Huge BigInt shift amount should produce an evaluation error (invalid bigint shift)
        let res = evaluate_script("let a = 1n; a <<= 100000000000000000000000000000000000000n");
        match res {
            Err(err) => match err.kind() {
                javascript::JSErrorKind::EvaluationError { message, .. } => {
                    assert!(
                        message.contains("invalid bigint shift") || message.contains("invalid bigint"),
                        "message={}",
                        message
                    )
                }
                _ => panic!("Expected EvaluationError for huge BigInt shift, got {:?}", err),
            },
            other => panic!("Expected EvaluationError for huge BigInt shift, got {:?}", other),
        }

        // Negative BigInt shift (e.g. -1n) should also error when converting to usize
        let res = evaluate_script("let a = 1n; a <<= -1n");
        match res {
            Err(err) => match err.kind() {
                javascript::JSErrorKind::EvaluationError { message, .. } => {
                    assert!(
                        message.contains("invalid bigint shift") || message.contains("invalid bigint"),
                        "message={}",
                        message
                    )
                }
                _ => panic!("Expected EvaluationError for negative BigInt shift, got {:?}", err),
            },
            other => panic!("Expected EvaluationError for negative BigInt shift, got {:?}", other),
        }

        // Mixing BigInt and Number in bitwise XOR should throw TypeError
        let res = evaluate_script("let a = 1n; let b = 2; a ^= b");
        match res {
            Err(err) => match err.kind() {
                javascript::JSErrorKind::TypeError { message, .. } => assert!(message.contains("Cannot mix BigInt")),
                _ => panic!("Expected TypeError for BigInt ^ Number mixing, got {:?}", err),
            },
            other => panic!("Expected TypeError for BigInt ^ Number mixing, got {:?}", other),
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

    #[test]
    fn test_bitwise_compound_assignments() {
        // Test bitwise AND assignment (&=)
        let script1 = "let a = 5; a &= 3; a";
        let result1 = evaluate_script(script1);
        assert!(result1.is_ok(), "evaluate_script(script1) failed: {:?}", result1);
        match result1 {
            Ok(Value::Number(n)) => assert_eq!(n, 1.0), // 5 & 3 = 1
            _ => panic!("Expected 1.0, got {:?}", result1),
        }

        // Test bitwise OR assignment (|=)
        let script2 = "let b = 5; b |= 3; b";
        let result2 = evaluate_script(script2);
        assert!(result2.is_ok(), "evaluate_script(script2) failed: {:?}", result2);
        match result2 {
            Ok(Value::Number(n)) => assert_eq!(n, 7.0), // 5 | 3 = 7
            _ => panic!("Expected 7.0, got {:?}", result2),
        }

        // Test bitwise XOR assignment (^=)
        let script3 = "let c = 5; c ^= 3; c";
        let result3 = evaluate_script(script3);
        assert!(result3.is_ok(), "evaluate_script(script3) failed: {:?}", result3);
        match result3 {
            Ok(Value::Number(n)) => assert_eq!(n, 6.0), // 5 ^ 3 = 6
            _ => panic!("Expected 6.0, got {:?}", result3),
        }

        // Test left shift assignment (<<=)
        let script4 = "let d = 5; d <<= 1; d";
        let result4 = evaluate_script(script4);
        assert!(result4.is_ok(), "evaluate_script(script4) failed: {:?}", result4);
        match result4 {
            Ok(Value::Number(n)) => assert_eq!(n, 10.0), // 5 << 1 = 10
            _ => panic!("Expected 10.0, got {:?}", result4),
        }

        // Test right shift assignment (>>=)
        let script5 = "let e = 5; e >>= 1; e";
        let result5 = evaluate_script(script5);
        assert!(result5.is_ok(), "evaluate_script(script5) failed: {:?}", result5);
        match result5 {
            Ok(Value::Number(n)) => assert_eq!(n, 2.0), // 5 >> 1 = 2
            _ => panic!("Expected 2.0, got {:?}", result5),
        }

        // Test unsigned right shift assignment (>>>=)
        let script6 = "let f = -5; f >>>= 1; f";
        let result6 = evaluate_script(script6);
        assert!(result6.is_ok(), "evaluate_script(script6) failed: {:?}", result6);
        match result6 {
            Ok(Value::Number(n)) => assert_eq!(n, 2147483645.0), // -5 >>> 1 = 2147483645
            _ => panic!("Expected 2147483645.0, got {:?}", result6),
        }
    }
}
