use javascript::{Value, evaluate_script};

#[test]
fn bigint_vs_number_comparison_edgecases() {
    // 1n == 1
    let r1 = evaluate_script("1n == 1");
    match r1 {
        Ok(Value::Boolean(b)) => assert!(b),
        Ok(Value::Number(n)) => assert_eq!(n, 1.0),
        other => panic!("unexpected result for 1n == 1: {:?}", other),
    }

    // 1n == 1.5 -> false
    let r2 = evaluate_script("1n == 1.5");
    match r2 {
        Ok(Value::Boolean(b)) => assert!(!b),
        Ok(Value::Number(n)) => assert_eq!(n, 0.0),
        other => panic!("unexpected result for 1n == 1.5: {:?}", other),
    }

    // Integer conversion path: for integers that are exactly representable in Number
    // (e.g. 2^53-1) comparison with BigInt should succeed
    let r3 = evaluate_script("9007199254740991n == 9007199254740991");
    match r3 {
        Ok(Value::Boolean(b)) => assert!(b),
        Ok(Value::Number(n)) => assert_eq!(n, 1.0),
        other => panic!("unexpected result for equality of representable integer and BigInt: {:?}", other),
    }

    // But if a Number literal cannot represent the integer exactly, equality should be false
    let r3b = evaluate_script("123456789123456789n == 123456789123456789");
    match r3b {
        Ok(Value::Boolean(b)) => assert!(!b),
        Ok(Value::Number(n)) => assert_eq!(n, 0.0),
        other => panic!("unexpected result for equality of imprecise integer and BigInt: {:?}", other),
    }

    // Non-integer number with BigInt: should compare numerically -> 1n < 1.5 true
    let r4 = evaluate_script("1n < 1.5");
    match r4 {
        Ok(Value::Boolean(b)) => assert!(b),
        Ok(Value::Number(n)) => assert_eq!(n, 1.0),
        other => panic!("unexpected result for 1n < 1.5: {:?}", other),
    }

    // Very large BigInt vs a large Number (1e20): expect BigInt which is larger to compare greater
    let r5 = evaluate_script("123456789123456789123456789n > 1e20");
    match r5 {
        Ok(Value::Boolean(b)) => assert!(b),
        Ok(Value::Number(n)) => assert_eq!(n, 1.0),
        other => panic!("unexpected result for huge BigInt vs 1e20: {:?}", other),
    }

    // Conversely, a huge negative BigInt < a negative floating number
    // Note: parser currently doesn't treat unary - on BigInt as a bigint literal, use 0n - Xn
    let r6 = evaluate_script("(0n - 123456789123456789123456789n) < -1e20");
    match r6 {
        Ok(Value::Boolean(b)) => assert!(b),
        Ok(Value::Number(n)) => assert_eq!(n, 1.0),
        other => panic!("unexpected result for negative huge BigInt vs -1e20: {:?}", other),
    }
}
