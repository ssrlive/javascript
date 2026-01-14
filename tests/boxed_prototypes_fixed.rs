use javascript::{JSError, evaluate_script};

#[test]
fn boxed_primitives_have_constructor_prototype() -> Result<(), JSError> {
    // Number
    let script = r#"
        const n = Object(123);
        n instanceof Number;
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    println!("Number check res: {}", res);
    if res != "true" {
        panic!("Number prototype check failed: {}", res);
    }

    // String
    let script = r#"
        const s = Object('x');
        s instanceof String;
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    if res != "true" {
        panic!("String prototype check failed: {}", res);
    }

    // Boolean
    let script = r#"
        const b = Object(true);
        b instanceof Boolean;
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    if res != "true" {
        panic!("Boolean prototype check failed: {}", res);
    }

    // BigInt
    let script = r#"
        const bi = Object(7n);
        bi instanceof Object && typeof bi === 'object';
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    if res != "true" {
        panic!("BigInt prototype check failed: {}", res);
    }

    Ok(())
}
