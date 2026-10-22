use javascript::evaluate_script;

#[test]
fn test_unary_neg_on_bigint() {
    let r = evaluate_script("-1n", false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r {
        Ok(h) => assert_eq!(h, "-1"),
        other => panic!("expected bigint -1, got {:?}", other),
    }
}

#[test]
fn test_bigint_assignment_ops() {
    // +=
    let script = "let a = 1n; a += 2n; a";
    let r1 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r1 {
        Ok(h) => assert_eq!(h, "3"),
        other => panic!("expected bigint 3, got {:?}", other),
    }

    // -=
    let script = "let b = 5n; b -= 2n; b";
    let r2 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r2 {
        Ok(h) => assert_eq!(h, "3"),
        other => panic!("expected bigint 3, got {:?}", other),
    }

    // *=
    let script = "let c = 2n; c *= 3n; c";
    let r3 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r3 {
        Ok(h) => assert_eq!(h, "6"),
        other => panic!("expected bigint 6, got {:?}", other),
    }

    // /= integer division
    let script = "let d = 7n; d /= 2n; d";
    let r4 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r4 {
        Ok(h) => assert_eq!(h, "3"),
        other => panic!("expected bigint 3, got {:?}", other),
    }

    // %= modulo
    let script = "let e = 7n; e %= 3n; e";
    let r5 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r5 {
        Ok(h) => assert_eq!(h, "1"),
        other => panic!("expected bigint 1, got {:?}", other),
    }

    // **=
    let script = "let f = 2n; f **= 3n; f";
    let r6 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r6 {
        Ok(h) => assert_eq!(h, "8"),
        other => panic!("expected bigint 8, got {:?}", other),
    }
}

#[test]
fn test_mixing_bigint_number_errors() {
    // arithmetic mixing should error
    assert!(evaluate_script("1n - 1", false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).is_err());
    assert!(evaluate_script("1n * 2", false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).is_err());
    assert!(evaluate_script("1n / 2", false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).is_err());
    assert!(evaluate_script("5n % 2", false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).is_err());
    assert!(evaluate_script("2n ** 3", false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).is_err());
    // assignment mixing should also error
    let script = "let a = 1n; a += 2";
    assert!(evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).is_err());
    let script = "let a = 1n; a -= 1";
    assert!(evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).is_err());
    let script = "let a = 1n; a *= 2";
    assert!(evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).is_err());
    let script = "let a = 4n; a /= 2";
    assert!(evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).is_err());
    let script = "let a = 5n; a %= 2";
    assert!(evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).is_err());
}
