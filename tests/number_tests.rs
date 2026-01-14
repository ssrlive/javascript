use javascript::*;

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
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "1.7976931348623157e+308");
    }

    #[test]
    fn test_number_min_value() {
        let script = "Number.MIN_VALUE";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "5e-324");
    }

    #[test]
    fn test_number_nan() {
        let script = "Number.NaN";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "NaN");
    }

    #[test]
    fn test_number_positive_infinity() {
        let script = "Number.POSITIVE_INFINITY";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "Infinity");
    }

    #[test]
    fn test_number_negative_infinity() {
        let script = "Number.NEGATIVE_INFINITY";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "-Infinity");
    }

    #[test]
    fn test_number_epsilon() {
        let script = "Number.EPSILON";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "2.220446049250313e-16");
    }

    #[test]
    fn test_number_max_safe_integer() {
        let script = "Number.MAX_SAFE_INTEGER";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "9007199254740991");
    }

    #[test]
    fn test_number_min_safe_integer() {
        let script = "Number.MIN_SAFE_INTEGER";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "-9007199254740991");
    }

    #[test]
    fn test_number_is_nan() {
        // Test with NaN
        let script = "Number.isNaN(NaN)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test with number
        let script = "Number.isNaN(42)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test with string that parses to NaN
        let script = "Number.isNaN('not a number')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test with undefined
        let script = "Number.isNaN(undefined)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");
    }

    #[test]
    fn test_number_is_finite() {
        // Test with finite number
        let script = "Number.isFinite(42)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test with Infinity
        let script = "Number.isFinite(Infinity)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test with NaN
        let script = "Number.isFinite(NaN)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test with string
        let script = "Number.isFinite('42')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");
    }

    #[test]
    fn test_number_is_integer() {
        // Test with integer
        let script = "Number.isInteger(42)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test with float
        let script = "Number.isInteger(42.5)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test with Infinity
        let script = "Number.isInteger(Infinity)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test with NaN
        let script = "Number.isInteger(NaN)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test with string
        let script = "Number.isInteger('42')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");
    }

    #[test]
    fn test_number_is_safe_integer() {
        // Test with safe integer
        let script = "Number.isSafeInteger(42)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test with MAX_SAFE_INTEGER
        let script = "Number.isSafeInteger(9007199254740991)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test with MIN_SAFE_INTEGER
        let script = "Number.isSafeInteger(-9007199254740991)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");

        // Test with unsafe integer (too large)
        let script = "Number.isSafeInteger(9007199254740992)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test with float
        let script = "Number.isSafeInteger(42.5)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        // Test with Infinity
        let script = "Number.isSafeInteger(Infinity)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");
    }

    #[test]
    fn test_number_parse_float() {
        // Test with valid float string
        let script = "Number.parseFloat('3.16')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3.16");

        // Test with integer string
        let script = "Number.parseFloat('42')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");

        // Test with invalid string
        let script = "Number.parseFloat('not a number')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "NaN");

        // Test with number
        let script = "Number.parseFloat(42.5)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42.5");

        // Test with whitespace
        let script = "Number.parseFloat('  3.16  ')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3.16");
    }

    #[test]
    fn test_shift_edge_cases_and_bigint_mixing() {
        // Left shift with large shift amount (masked by 0x1f)
        let res = evaluate_script("let a = 1; a <<= 33; a", None::<&std::path::Path>).unwrap();
        assert_eq!(res, "2");

        // Left shift with negative shift amount -> ToUint32(-1) & 0x1f == 31
        let res = evaluate_script("let a = 1; a <<= -1; a", None::<&std::path::Path>).unwrap();
        assert_eq!(res, "-2147483648");

        // Unsigned right shift on negative number
        let res = evaluate_script("let a = -1; a >>>= 1; a", None::<&std::path::Path>).unwrap();
        assert_eq!(res, "2147483647");

        // Mixing BigInt with Number in shift should throw TypeError
        let res = evaluate_script("let a = 1n; let b = 2; a <<= b", None::<&std::path::Path>);
        match res {
            Err(err) => match err.kind() {
                javascript::JSErrorKind::TypeError { message, .. } => assert!(message.contains("Cannot mix BigInt")),
                _ => panic!("Expected TypeError for mixing BigInt and Number in <<=, got {:?}", err),
            },
            other => panic!("Expected TypeError for mixing BigInt and Number in <<=, got {:?}", other),
        }

        // Unsigned right shift on BigInt should throw TypeError with specific message
        let res = evaluate_script("let a = 1n; a >>>= 1n", None::<&std::path::Path>);
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
        let res = evaluate_script(
            "let a = 1n; a <<= 100000000000000000000000000000000000000n",
            None::<&std::path::Path>,
        );
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
        let res = evaluate_script("let a = 1n; a <<= -1n", None::<&std::path::Path>);
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
        let res = evaluate_script("let a = 1n; let b = 2; a ^= b", None::<&std::path::Path>);
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
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");

        // Test with float string (should truncate)
        let script = "Number.parseInt('42.5')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");

        // Test with invalid string
        let script = "Number.parseInt('not a number')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "NaN");

        // Test with radix
        let script = "Number.parseInt('101', 2)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "5"); // 101 in binary is 5

        // Test with hex
        let script = "Number.parseInt('FF', 16)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "255");

        // Test with number
        let script = "Number.parseInt(42.7)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn test_number_constructor_no_args() {
        let script = "Number()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "0");
    }

    #[test]
    fn test_number_constructor_with_number() {
        let script = "Number(42.5)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42.5");
    }

    #[test]
    fn test_number_constructor_with_string() {
        let script = "Number('42.5')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42.5");
    }

    #[test]
    fn test_number_constructor_with_boolean() {
        let script = "Number(true)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "1");

        let script = "Number(false)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "0");
    }

    #[test]
    fn test_number_constructor_with_invalid_string() {
        let script = "Number('not a number')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "NaN");
    }

    #[test]
    fn test_number_constructor_with_undefined() {
        let script = "Number(undefined)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "NaN");
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
            let result = evaluate_script(&script, None::<&std::path::Path>).unwrap();
            assert_ne!(result, "undefined", "Number.{} should exist", prop);
        }
    }

    #[test]
    fn test_bitwise_xor_numbers() {
        let script = "5 ^ 3";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_bitwise_xor_negative_numbers() {
        let script = "-5 ^ 3";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "-8");
    }

    #[test]
    fn test_bitwise_xor_assignment() {
        let script = "let a = 5; a ^= 3; a";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_bitwise_compound_assignments() {
        // Test bitwise AND assignment (&=)
        let script1 = "let a = 5; a &= 3; a";
        let result1 = evaluate_script(script1, None::<&std::path::Path>).unwrap();
        assert_eq!(result1, "1");

        // Test bitwise OR assignment (|=)
        let script2 = "let b = 5; b |= 3; b";
        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "7");

        // Test bitwise XOR assignment (^=)
        let script3 = "let c = 5; c ^= 3; c";
        let result3 = evaluate_script(script3, None::<&std::path::Path>).unwrap();
        assert_eq!(result3, "6");

        // Test left shift assignment (<<=)
        let script4 = "let d = 5; d <<= 1; d";
        let result4 = evaluate_script(script4, None::<&std::path::Path>).unwrap();
        assert_eq!(result4, "10");

        // Test right shift assignment (>>=)
        let script5 = "let e = 5; e >>= 1; e";
        let result5 = evaluate_script(script5, None::<&std::path::Path>).unwrap();
        assert_eq!(result5, "2");

        // Test unsigned right shift assignment (>>>=)
        let script6 = "let f = -5; f >>>= 1; f";
        let result6 = evaluate_script(script6, None::<&std::path::Path>).unwrap();
        assert_eq!(result6, "2147483645");
    }
}
