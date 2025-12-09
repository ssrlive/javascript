use javascript::{Value, evaluate_script};

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn bigint_global_and_prototype_to_string_value_of() {
    // BigInt with numeric literal string
    let r1 = evaluate_script("BigInt('123')", None::<&std::path::Path>);
    match r1 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "123"),
        other => panic!("expected BigInt result for BigInt('123'), got {:?}", other),
    }

    // BigInt with number argument (integer)
    let r2 = evaluate_script("BigInt(42)", None::<&std::path::Path>);
    match r2 {
        Ok(Value::BigInt(h)) => assert_eq!(h.raw, "42"),
        other => panic!("expected BigInt result for BigInt(42), got {:?}", other),
    }

    // toString on boxed BigInt via Object wrapper
    // Converting BigInt to string via global String() should work
    let r3 = evaluate_script("String(123n)", None::<&std::path::Path>);
    match r3 {
        Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "123"),
        other => panic!("expected string result for String(123n), got {:?}", other),
    }

    // BigInt constructed from number equals BigInt literal
    let r4 = evaluate_script("BigInt(7) === 7n", None::<&std::path::Path>);
    match r4 {
        Ok(Value::Boolean(b)) => assert!(b),
        other => panic!("expected boolean result for BigInt(7) === 7n, got {:?}", other),
    }

    // Boxed BigInt: Object(123n).toString() should use BigInt.prototype.toString
    let r5 = evaluate_script("Object(123n).toString()", None::<&std::path::Path>);
    match r5 {
        Ok(Value::String(s)) => assert_eq!(String::from_utf16_lossy(&s), "123"),
        other => panic!("expected string result for Object(123n).toString(), got {:?}", other),
    }

    // Boxed BigInt: Object(7n).valueOf() === 7n
    let r6 = evaluate_script("Object(7n).valueOf() === 7n", None::<&std::path::Path>);
    match r6 {
        Ok(Value::Boolean(b)) => assert!(b),
        other => panic!("expected boolean result for Object(7n).valueOf() === 7n, got {:?}", other),
    }
}
