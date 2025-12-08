use javascript::{Value, evaluate_script};

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn bigint_asintn_asuintn_basic() {
    // asUintN: simple masking
    let r1 = evaluate_script("BigInt.asUintN(3, 7n)");
    match r1 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "7"),
        other => panic!("expected BigInt result for asUintN, got {:?}", other),
    }

    // asIntN: interpret as signed
    let r2 = evaluate_script("BigInt.asIntN(3, 7n)");
    match r2 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "-1"),
        other => panic!("expected BigInt result for asIntN, got {:?}", other),
    }

    // bits == 0
    let r3 = evaluate_script("BigInt.asUintN(0, 123n)");
    match r3 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "0"),
        other => panic!("expected BigInt result for asUintN bits=0, got {:?}", other),
    }

    let r4 = evaluate_script("BigInt.asIntN(0, -5n)");
    match r4 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "0"),
        other => panic!("expected BigInt result for asIntN bits=0, got {:?}", other),
    }

    // asUintN with negative input: -1 mod 16 => 15
    let r5 = evaluate_script("BigInt.asUintN(4, -1n)");
    match r5 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "15"),
        other => panic!("expected BigInt result for asUintN negative input, got {:?}", other),
    }

    // asIntN with negative input stays negative
    let r6 = evaluate_script("BigInt.asIntN(4, -1n)");
    match r6 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "-1"),
        other => panic!("expected BigInt result for asIntN negative input, got {:?}", other),
    }

    // asIntN truncation: for 4 bits, 8 -> -8
    let r7 = evaluate_script("BigInt.asIntN(4, 8n)");
    match r7 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "-8"),
        other => panic!("expected BigInt result for asIntN truncation, got {:?}", other),
    }

    // 64-bit boundary: 2^64 -> asUintN(64) == 0
    let r8 = evaluate_script("BigInt.asUintN(64, 18446744073709551616n)");
    match r8 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "0"),
        other => panic!("expected BigInt result for 2^64 mod 2^64 == 0, got {:?}", other),
    }

    // 64-bit signed boundary: 2^63 -> asIntN(64) == -2^63
    let r9 = evaluate_script("BigInt.asIntN(64, 9223372036854775808n)");
    match r9 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "-9223372036854775808"),
        other => panic!("expected BigInt result for 2^63 -> -2^63, got {:?}", other),
    }
}
