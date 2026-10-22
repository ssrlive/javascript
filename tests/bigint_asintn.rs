use javascript::evaluate_script;

#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn bigint_asintn_asuintn_basic() {
    // asUintN: simple masking
    let script = "BigInt.asUintN(3, 7n)";
    let r1 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r1 {
        Ok(h) => assert_eq!(h, "7"),
        other => panic!("expected BigInt result for asUintN, got {:?}", other),
    }

    // asIntN: interpret as signed
    let script = "BigInt.asIntN(3, 7n)";
    let r2 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r2 {
        Ok(h) => assert_eq!(h, "-1"),
        other => panic!("expected BigInt result for asIntN, got {:?}", other),
    }

    // bits == 0
    let script = "BigInt.asUintN(0, 123n)";
    let r3 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r3 {
        Ok(h) => assert_eq!(h, "0"),
        other => panic!("expected BigInt result for asUintN bits=0, got {:?}", other),
    }

    let script = "BigInt.asIntN(0, -5n)";
    let r4 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r4 {
        Ok(h) => assert_eq!(h, "0"),
        other => panic!("expected BigInt result for asIntN bits=0, got {:?}", other),
    }

    // asUintN with negative input: -1 mod 16 => 15
    let script = "BigInt.asUintN(4, -1n)";
    let r5 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r5 {
        Ok(h) => assert_eq!(h, "15"),
        other => panic!("expected BigInt result for asUintN negative input, got {:?}", other),
    }

    // asIntN with negative input stays negative
    let script = "BigInt.asIntN(4, -1n)";
    let r6 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r6 {
        Ok(h) => assert_eq!(h, "-1"),
        other => panic!("expected BigInt result for asIntN negative input, got {:?}", other),
    }

    // asIntN truncation: for 4 bits, 8 -> -8
    let script = "BigInt.asIntN(4, 8n)";
    let r7 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r7 {
        Ok(h) => assert_eq!(h, "-8"),
        other => panic!("expected BigInt result for asIntN truncation, got {:?}", other),
    }

    // 64-bit boundary: 2^64 -> asUintN(64) == 0
    let script = "BigInt.asUintN(64, 18446744073709551616n)";
    let r8 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r8 {
        Ok(h) => assert_eq!(h, "0"),
        other => panic!("expected BigInt result for 2^64 mod 2^64 == 0, got {:?}", other),
    }

    // 64-bit signed boundary: 2^63 -> asIntN(64) == -2^63
    let script = "BigInt.asIntN(64, 9223372036854775808n)";
    let r9 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r9 {
        Ok(h) => assert_eq!(h, "-9223372036854775808"),
        other => panic!("expected BigInt result for 2^63 -> -2^63, got {:?}", other),
    }
}

#[test]
fn bigint_asintn_conversion_cases() {
    // String input
    let script = "BigInt.asUintN(5, '15')";
    let r1 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r1 {
        Ok(h) => assert_eq!(h, "15"),
        other => panic!("expected BigInt result for asUintN string input, got {:?}", other),
    }

    // Number input (integer float) — ToBigInt(Number) always throws TypeError per spec
    let script = "BigInt.asUintN(3, 7.0)";
    let r2 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    assert!(r2.is_err(), "expected TypeError for numeric input to ToBigInt");

    // Number input (non-integer) should also fail
    let script = "BigInt.asUintN(3, 7.5)";
    let r3 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    assert!(r3.is_err(), "expected TypeError for non-integer numeric input");

    // Boolean inputs
    let script = "BigInt.asUintN(4, true)";
    let r4 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r4 {
        Ok(h) => assert_eq!(h, "1"),
        other => panic!("expected BigInt result for asUintN boolean true, got {:?}", other),
    }
    let script = "BigInt.asUintN(4, false)";
    let r5 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r5 {
        Ok(h) => assert_eq!(h, "0"),
        other => panic!("expected BigInt result for asUintN boolean false, got {:?}", other),
    }

    // Object with valueOf returning BigInt
    let script = "BigInt.asUintN(4, { valueOf: function(){ return 7n; } })";
    let r6 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r6 {
        Ok(h) => assert_eq!(h, "7"),
        other => panic!("expected BigInt result for asUintN object valueOf BigInt, got {:?}", other),
    }

    // Object with valueOf returning Number — ToBigInt(Number) throws TypeError
    let script = "BigInt.asUintN(4, { valueOf: function(){ return 7; } })";
    let r6b = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    assert!(r6b.is_err(), "expected TypeError for object valueOf returning Number");

    // bits as float — ToIndex(3.5) truncates to 3, so this succeeds
    let script = "BigInt.asUintN(3.5, 7n)";
    let r7 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r7 {
        Ok(h) => assert_eq!(h, "7"),
        other => panic!("expected BigInt result for asUintN with float bits (truncated), got {:?}", other),
    }
}
