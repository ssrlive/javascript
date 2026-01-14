use javascript::evaluate_script;

#[test]
fn bigint_relational_more_edgecases() {
    // Equality where Number is exactly representable
    let r1 = evaluate_script("9007199254740991n == 9007199254740991", None::<&std::path::Path>);
    match r1 {
        Ok(s) => assert_eq!(s, "true", "9007199254740991n == 9007199254740991 should be true, got {}", s),
        other => panic!("unexpected result for 9007199254740991n == 9007199254740991: {:?}", other),
    }

    // 2^53 (9007199254740992) is exactly representable as f64; equality should be true
    let r2 = evaluate_script("9007199254740992n == 9007199254740992", None::<&std::path::Path>);
    match r2 {
        Ok(s) => assert_eq!(s, "true", "9007199254740992n == 9007199254740992 should be true, got {}", s),
        other => panic!("unexpected result for 9007199254740992n == 9007199254740992: {:?}", other),
    }

    // An integer that is not exactly representable in Number -> equality should be false
    let r3 = evaluate_script("9007199254740993n == 9007199254740993", None::<&std::path::Path>);
    match r3 {
        Ok(s) => assert_eq!(s, "false", "9007199254740993n == 9007199254740993 should be false, got {}", s),
        other => panic!("unexpected result for 9007199254740993n == 9007199254740993: {:?}", other),
    }

    // Negative representable integer (2^53-1) equality
    let r4 = evaluate_script("(0n - 9007199254740991n) == -9007199254740991", None::<&std::path::Path>);
    match r4 {
        Ok(s) => assert_eq!(s, "true", "negative representable equality should be true, got {}", s),
        other => panic!("unexpected result for negative representable equality: {:?}", other),
    }

    // Fractional comparisons with BigInt
    let r5 = evaluate_script("5n < 5.1", None::<&std::path::Path>);
    let s5 = r5.unwrap();
    assert_eq!(s5, "true", "5n < 5.1 should be true, got {}", s5);

    let r6 = evaluate_script("5n < 5.0", None::<&std::path::Path>);
    let s6 = r6.unwrap();
    assert_eq!(s6, "false", "5n < 5.0 should be false, got {}", s6);

    let r7 = evaluate_script("5.1 < 6n", None::<&std::path::Path>);
    let s7 = r7.unwrap();
    assert_eq!(s7, "true", "5.1 < 6n should be true, got {}", s7);

    let r8 = evaluate_script("5.9999 < 6n", None::<&std::path::Path>);
    let s8 = r8.unwrap();
    assert_eq!(s8, "true", "5.9999 < 6n should be true, got {}", s8);

    // Larger magnitude comparisons
    let r9 = evaluate_script("123456789123456789123456789n > 1e20", None::<&std::path::Path>);
    let s9 = r9.unwrap();
    assert_eq!(s9, "true", "huge BigInt > 1e20 should be true, got {}", s9);

    let r10 = evaluate_script("(0n - 123456789123456789123456789n) < -1e20", None::<&std::path::Path>);
    let s10 = r10.unwrap();
    assert_eq!(s10, "true", "huge negative BigInt < -1e20 should be true, got {}", s10);

    // Cross-check: borderline where floor/ceil rules matter
    // 4.9 < 5n -> floor(4.9) = 4 -> 4 < 5 => 4.9 < 5 true
    let r11 = evaluate_script("4.9 < 5n", None::<&std::path::Path>);
    let s11 = r11.unwrap();
    assert_eq!(s11, "true", "4.9 < 5n should be true, got {}", s11);

    // 5.0 < 5n -> false
    let r12 = evaluate_script("5.0 < 5n", None::<&std::path::Path>);
    let s12 = r12.unwrap();
    assert_eq!(s12, "false", "5.0 < 5n should be false, got {}", s12);

    // ceil based: 5.1 > 5n -> ceil(5.1)=6; 6 > 5 -> true
    let r13 = evaluate_script("5.1 > 5n", None::<&std::path::Path>);
    let s13 = r13.unwrap();
    assert_eq!(s13, "true", "5.1 > 5n should be true, got {}", s13);
}
