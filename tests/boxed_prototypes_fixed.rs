use javascript::{JSError, Value, evaluate_script};

#[test]
fn boxed_primitives_have_constructor_prototype() -> Result<(), JSError> {
    // Number
    let script = r#"
        const n = Object(123);
        n instanceof Number;
    "#;
    let res = evaluate_script(script)?;
    println!("Number check res: {:?}", res);
    match res {
        Value::Boolean(true) => {}
        other => panic!("Number prototype check failed: {:?}", other),
    }

    // String
    let script = r#"
        const s = Object('x');
        s instanceof String;
    "#;
    let res = evaluate_script(script)?;
    match res {
        Value::Boolean(true) => {}
        other => panic!("String prototype check failed: {:?}", other),
    }

    // Boolean
    let script = r#"
        const b = Object(true);
        b instanceof Boolean;
    "#;
    let res = evaluate_script(script)?;
    match res {
        Value::Boolean(true) => {}
        other => panic!("Boolean prototype check failed: {:?}", other),
    }

    // BigInt
    let script = r#"
        const bi = Object(7n);
        bi instanceof Object && typeof bi === 'object';
    "#;
    let res = evaluate_script(script)?;
    match res {
        Value::Boolean(true) => {}
        other => panic!("BigInt prototype check failed: {:?}", other),
    }

    Ok(())
}
