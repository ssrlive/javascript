use javascript::{JSError, evaluate_script};

#[test]
fn compare_proto_and_instanceof() -> Result<(), JSError> {
    let script = r#"
        const n = Object(123);
        const protoEq = (n.__proto__ === Number.prototype) ? 'EQ' : 'NEQ';
        const inst = (n instanceof Number) ? 'I' : 'N';
        protoEq + '|' + inst;
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(
        res, "\"EQ|I\"",
        "Expected boxed Number to have Number prototype and be instanceof Number, got {res}",
    );
    Ok(())
}
