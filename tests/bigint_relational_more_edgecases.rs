use javascript::{Value, evaluate_script};

#[test]
fn bigint_relational_more_edgecases() {
    // Equality where Number is exactly representable
    let r1 = evaluate_script("9007199254740991n == 9007199254740991", None::<&std::path::Path>);
    match r1 {
        Ok(Value::Boolean(b)) => assert!(b),
        other => panic!("unexpected result for 9007199254740991n == 9007199254740991: {:?}", other),
    }

    // 2^53 (9007199254740992) is exactly representable as f64; equality should be true
    let r2 = evaluate_script("9007199254740992n == 9007199254740992", None::<&std::path::Path>);
    match r2 {
        Ok(Value::Boolean(b)) => assert!(b),
        other => panic!("unexpected result for 9007199254740992n == 9007199254740992: {:?}", other),
    }

    // An integer that is not exactly representable in Number -> equality should be false
    let r3 = evaluate_script("9007199254740993n == 9007199254740993", None::<&std::path::Path>);
    match r3 {
        Ok(Value::Boolean(b)) => assert!(!b),
        other => panic!("unexpected result for 9007199254740993n == 9007199254740993: {:?}", other),
    }

    // Negative representable integer (2^53-1) equality
    let r4 = evaluate_script("(0n - 9007199254740991n) == -9007199254740991", None::<&std::path::Path>);
    match r4 {
        Ok(Value::Boolean(b)) => assert!(b),
        other => panic!("unexpected result for negative representable equality: {:?}", other),
    }

    // Fractional comparisons with BigInt
    let r5 = evaluate_script("5n < 5.1", None::<&std::path::Path>);
    match r5.unwrap() {
        Value::Boolean(b) => assert!(b),
        other => panic!("unexpected result for 5n < 5.1: {:?}", other),
    }

    let r6 = evaluate_script("5n < 5.0", None::<&std::path::Path>);
    match r6.unwrap() {
        Value::Boolean(b) => assert!(!b),
        other => panic!("unexpected result for 5n < 5.0: {:?}", other),
    }

    let r7 = evaluate_script("5.1 < 6n", None::<&std::path::Path>);
    match r7.unwrap() {
        Value::Boolean(b) => assert!(b),
        other => panic!("unexpected result for 5.1 < 6n: {:?}", other),
    }

    let r8 = evaluate_script("5.9999 < 6n", None::<&std::path::Path>);
    match r8.unwrap() {
        Value::Boolean(b) => assert!(b),
        other => panic!("unexpected result for 5.9999 < 6n: {:?}", other),
    }

    // Larger magnitude comparisons
    let r9 = evaluate_script("123456789123456789123456789n > 1e20", None::<&std::path::Path>);
    match r9.unwrap() {
        Value::Boolean(b) => assert!(b),
        other => panic!("unexpected result for huge BigInt > 1e20: {:?}", other),
    }

    let r10 = evaluate_script("(0n - 123456789123456789123456789n) < -1e20", None::<&std::path::Path>);
    match r10.unwrap() {
        Value::Boolean(b) => assert!(b),
        other => panic!("unexpected result for huge negative BigInt < -1e20: {:?}", other),
    }

    // Cross-check: borderline where floor/ceil rules matter
    // 4.9 < 5n -> floor(4.9) = 4 -> 4 < 5 => 4.9 < 5 true
    let r11 = evaluate_script("4.9 < 5n", None::<&std::path::Path>);
    match r11.unwrap() {
        Value::Boolean(b) => assert!(b),
        other => panic!("unexpected result for 4.9 < 5n: {:?}", other),
    }

    // 5.0 < 5n -> false
    let r12 = evaluate_script("5.0 < 5n", None::<&std::path::Path>);
    match r12.unwrap() {
        Value::Boolean(b) => assert!(!b),
        other => panic!("unexpected result for 5.0 < 5n: {:?}", other),
    }

    // ceil based: 5.1 > 5n -> ceil(5.1)=6; 6 > 5 -> true
    let r13 = evaluate_script("5.1 > 5n", None::<&std::path::Path>);
    match r13.unwrap() {
        Value::Boolean(b) => assert!(b),
        other => panic!("unexpected result for 5.1 > 5n: {:?}", other),
    }
}
