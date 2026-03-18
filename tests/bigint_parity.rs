use javascript::evaluate_script_with_vm;

#[test]
fn test_unary_neg_on_bigint() {
    let r = evaluate_script_with_vm("-1n", false, None::<&std::path::Path>);
    match r {
        Ok(h) => assert_eq!(h, "-1"),
        other => panic!("expected bigint -1, got {:?}", other),
    }
}

#[test]
fn test_bigint_assignment_ops() {
    // +=
    let r1 = evaluate_script_with_vm("let a = 1n; a += 2n; a", false, None::<&std::path::Path>);
    match r1 {
        Ok(h) => assert_eq!(h, "3"),
        other => panic!("expected bigint 3, got {:?}", other),
    }

    // -=
    let r2 = evaluate_script_with_vm("let b = 5n; b -= 2n; b", false, None::<&std::path::Path>);
    match r2 {
        Ok(h) => assert_eq!(h, "3"),
        other => panic!("expected bigint 3, got {:?}", other),
    }

    // *=
    let r3 = evaluate_script_with_vm("let c = 2n; c *= 3n; c", false, None::<&std::path::Path>);
    match r3 {
        Ok(h) => assert_eq!(h, "6"),
        other => panic!("expected bigint 6, got {:?}", other),
    }

    // /= integer division
    let r4 = evaluate_script_with_vm("let d = 7n; d /= 2n; d", false, None::<&std::path::Path>);
    match r4 {
        Ok(h) => assert_eq!(h, "3"),
        other => panic!("expected bigint 3, got {:?}", other),
    }

    // %= modulo
    let r5 = evaluate_script_with_vm("let e = 7n; e %= 3n; e", false, None::<&std::path::Path>);
    match r5 {
        Ok(h) => assert_eq!(h, "1"),
        other => panic!("expected bigint 1, got {:?}", other),
    }

    // **=
    let r6 = evaluate_script_with_vm("let f = 2n; f **= 3n; f", false, None::<&std::path::Path>);
    match r6 {
        Ok(h) => assert_eq!(h, "8"),
        other => panic!("expected bigint 8, got {:?}", other),
    }
}

#[test]
fn test_mixing_bigint_number_errors() {
    // arithmetic mixing should error
    assert!(evaluate_script_with_vm("1n - 1", false, None::<&std::path::Path>).is_err());
    assert!(evaluate_script_with_vm("1n * 2", false, None::<&std::path::Path>).is_err());
    assert!(evaluate_script_with_vm("1n / 2", false, None::<&std::path::Path>).is_err());
    assert!(evaluate_script_with_vm("5n % 2", false, None::<&std::path::Path>).is_err());
    assert!(evaluate_script_with_vm("2n ** 3", false, None::<&std::path::Path>).is_err());
    // assignment mixing should also error
    assert!(evaluate_script_with_vm("let a = 1n; a += 2", false, None::<&std::path::Path>).is_err());
    assert!(evaluate_script_with_vm("let a = 1n; a -= 1", false, None::<&std::path::Path>).is_err());
    assert!(evaluate_script_with_vm("let a = 1n; a *= 2", false, None::<&std::path::Path>).is_err());
    assert!(evaluate_script_with_vm("let a = 4n; a /= 2", false, None::<&std::path::Path>).is_err());
    assert!(evaluate_script_with_vm("let a = 5n; a %= 2", false, None::<&std::path::Path>).is_err());
}
