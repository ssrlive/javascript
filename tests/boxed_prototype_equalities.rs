use javascript::{JSError, Value, evaluate_script};

#[test]
fn boxed_prototype_strict_equality() -> Result<(), JSError> {
    // Number
    let script = r#"
        (Object(123).__proto__ === Number.prototype);
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    match res {
        Value::Boolean(true) => {}
        other => panic!("Number prototype strict equality failed: {:?}", other),
    }

    // String
    let script = r#"
        (Object('x').__proto__ === String.prototype);
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    match res {
        Value::Boolean(true) => {}
        other => panic!("String prototype strict equality failed: {:?}", other),
    }

    // Boolean
    let script = r#"
        (Object(true).__proto__ === Boolean.prototype);
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    match res {
        Value::Boolean(true) => {}
        other => panic!("Boolean prototype strict equality failed: {:?}", other),
    }

    // BigInt
    let script = r#"
        (Object(7n).__proto__ === BigInt.prototype);
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    match res {
        Value::Boolean(true) => {}
        other => panic!("BigInt prototype strict equality failed: {:?}", other),
    }

    // Note: Some engines may not expose `Symbol.prototype`; skip Symbol in that case.

    Ok(())
}
