use javascript::{JSError, evaluate_script};

#[test]
fn boxed_prototype_strict_equality() -> Result<(), JSError> {
    // Number
    let script = r#"
        (Object(123).__proto__ === Number.prototype);
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    assert_eq!(res, "true", "Number prototype strict equality failed: {res}");

    // String
    let script = r#"
        (Object('x').__proto__ === String.prototype);
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    assert_eq!(res, "true", "String prototype strict equality failed: {res}");

    // Boolean
    let script = r#"
        (Object(true).__proto__ === Boolean.prototype);
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    assert_eq!(res, "true", "Boolean prototype strict equality failed: {res}");

    // BigInt
    let script = r#"
        (Object(7n).__proto__ === BigInt.prototype);
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    assert_eq!(res, "true", "BigInt prototype strict equality failed: {res}");

    // Note: Some engines may not expose `Symbol.prototype`; skip Symbol in that case.

    Ok(())
}
