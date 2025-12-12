use javascript::{Value, evaluate_script};

#[test]
fn test_unary_neg_on_bigint() {
    let r = evaluate_script("-1n", None::<&std::path::Path>);
    match r {
        Ok(Value::BigInt(h)) => assert_eq!(h.to_string(), "-1"),
        other => panic!("expected bigint -1, got {:?}", other),
    }
}

#[test]
fn test_bigint_assignment_ops() {
    // +=
    let r1 = evaluate_script("let a = 1n; a += 2n; a", None::<&std::path::Path>);
    match r1 {
        Ok(Value::BigInt(h)) => assert_eq!(h.to_string(), "3"),
        other => panic!("expected bigint 3, got {:?}", other),
    }

    // -=
    let r2 = evaluate_script("let b = 5n; b -= 2n; b", None::<&std::path::Path>);
    match r2 {
        Ok(Value::BigInt(h)) => assert_eq!(h.to_string(), "3"),
        other => panic!("expected bigint 3, got {:?}", other),
    }

    // *=
    let r3 = evaluate_script("let c = 2n; c *= 3n; c", None::<&std::path::Path>);
    match r3 {
        Ok(Value::BigInt(h)) => assert_eq!(h.to_string(), "6"),
        other => panic!("expected bigint 6, got {:?}", other),
    }

    // /= integer division
    let r4 = evaluate_script("let d = 7n; d /= 2n; d", None::<&std::path::Path>);
    match r4 {
        Ok(Value::BigInt(h)) => assert_eq!(h.to_string(), "3"),
        other => panic!("expected bigint 3, got {:?}", other),
    }

    // %= modulo
    let r5 = evaluate_script("let e = 7n; e %= 3n; e", None::<&std::path::Path>);
    match r5 {
        Ok(Value::BigInt(h)) => assert_eq!(h.to_string(), "1"),
        other => panic!("expected bigint 1, got {:?}", other),
    }

    // **=
    let r6 = evaluate_script("let f = 2n; f **= 3n; f", None::<&std::path::Path>);
    match r6 {
        Ok(Value::BigInt(h)) => assert_eq!(h.to_string(), "8"),
        other => panic!("expected bigint 8, got {:?}", other),
    }
}

#[test]
fn test_mixing_bigint_number_errors() {
    // arithmetic mixing should error
    assert!(evaluate_script("1n - 1", None::<&std::path::Path>).is_err());
    assert!(evaluate_script("1n * 2", None::<&std::path::Path>).is_err());
    assert!(evaluate_script("1n / 2", None::<&std::path::Path>).is_err());
    assert!(evaluate_script("5n % 2", None::<&std::path::Path>).is_err());
    assert!(evaluate_script("2n ** 3", None::<&std::path::Path>).is_err());
    // assignment mixing should also error
    assert!(evaluate_script("let a = 1n; a += 2", None::<&std::path::Path>).is_err());
    assert!(evaluate_script("let a = 1n; a -= 1", None::<&std::path::Path>).is_err());
    assert!(evaluate_script("let a = 1n; a *= 2", None::<&std::path::Path>).is_err());
    assert!(evaluate_script("let a = 4n; a /= 2", None::<&std::path::Path>).is_err());
    assert!(evaluate_script("let a = 5n; a %= 2", None::<&std::path::Path>).is_err());
}
