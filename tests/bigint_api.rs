use javascript::evaluate_script;

#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn bigint_global_and_prototype_to_string_value_of() {
    // BigInt with numeric literal string
    let script = "BigInt('123')";
    let r1 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r1 {
        Ok(h) => assert_eq!(h, "123"),
        other => panic!("expected BigInt result for BigInt('123'), got {:?}", other),
    }

    // BigInt with number argument (integer)
    let r2 = evaluate_script("BigInt(42)", false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r2 {
        Ok(h) => assert_eq!(h, "42"),
        other => panic!("expected BigInt result for BigInt(42), got {:?}", other),
    }

    // toString on boxed BigInt via Object wrapper
    // Converting BigInt to string via global String() should work
    let script = "String(123n)";
    let r3 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r3 {
        Ok(h) => assert_eq!(h, "\"123\""),
        other => panic!("expected string result for String(123n), got {:?}", other),
    }

    // BigInt constructed from number equals BigInt literal
    let script = "BigInt(7) === 7n";
    let r4 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r4 {
        Ok(h) => assert_eq!(h, "true"),
        other => panic!("expected boolean result for BigInt(7) === 7n, got {:?}", other),
    }

    // Boxed BigInt: Object(123n).toString() should use BigInt.prototype.toString
    let script = "Object(123n).toString()";
    let r5 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r5 {
        Ok(h) => assert_eq!(h, "\"123\""),
        other => panic!("expected string result for Object(123n).toString(), got {:?}", other),
    }

    // Boxed BigInt: Object(7n).valueOf() === 7n
    let script = "Object(7n).valueOf() === 7n";
    let r6 = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match r6 {
        Ok(h) => assert_eq!(h, "true"),
        other => panic!("expected boolean result for Object(7n).valueOf() === 7n, got {:?}", other),
    }
}
