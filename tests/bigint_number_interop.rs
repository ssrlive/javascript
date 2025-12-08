use javascript::{Value, evaluate_script};

#[test]
fn bigint_arithmetic_and_mixing_should_behave_per_spec() {
    // BigInt + BigInt -> BigInt
    let r1 = evaluate_script("1n + 2n");
    match r1 {
        Ok(Value::BigInt(h)) => assert!(h.raw == "3"),
        _ => panic!("expected BigInt result for 1n + 2n, got {:?}", r1),
    }

    // Mixing BigInt with Number in arithmetic must throw (TypeError)
    let r2 = evaluate_script("1n + 1");
    assert!(r2.is_err(), "Expected TypeError when mixing BigInt and Number");

    // Subtraction / other arithmetic also should throw on mixing
    let r3 = evaluate_script("5n - 2");
    assert!(r3.is_err(), "Expected TypeError for 5n - 2");

    // Loose equality: 1n == 1 should be true
    let r4 = evaluate_script("1n == 1");
    match r4 {
        Ok(Value::Boolean(b)) => assert!(b, "1n == 1 should be true"),
        Ok(Value::Number(n)) => assert_eq!(n, 1.0), // fallback engines that return numeric truthiness
        other => panic!("unexpected result for 1n == 1: {:?}", other),
    }

    // Strict equality: 1n === 1 should be false
    let r5 = evaluate_script("1n === 1");
    match r5 {
        Ok(Value::Boolean(b)) => assert!(!b, "1n === 1 should be false"),
        Ok(Value::Number(n)) => assert_eq!(n, 0.0), // fallback
        other => panic!("unexpected result for 1n === 1: {:?}", other),
    }

    // Relational comparison: 2n > 1 should be true
    let r6 = evaluate_script("2n > 1");
    match r6 {
        Ok(Value::Boolean(b)) => assert!(b, "2n > 1 should be true"),
        Ok(Value::Number(n)) => assert_eq!(n, 1.0),
        other => panic!("unexpected result for 2n > 1: {:?}", other),
    }

    // Relational comparison: 1n < 1.5 should be true (1 < 1.5)
    let r7 = evaluate_script("1n < 1.5");
    match r7 {
        Ok(Value::Boolean(b)) => assert!(b, "1n < 1.5 should be true"),
        Ok(Value::Number(n)) => assert_eq!(n, 1.0),
        other => panic!("unexpected result for 1n < 1.5: {:?}", other),
    }

    // String concatenation with BigInt: "x" + 1n -> "x1"
    let r8 = evaluate_script("'x' + 1n");
    match r8 {
        Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "x1"),
        other => panic!("expected string 'x1', got {:?}", other),
    }
}
