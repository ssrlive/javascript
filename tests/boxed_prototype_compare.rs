use javascript::{JSError, Value, evaluate_script, utf16_to_utf8};

#[test]
fn compare_proto_and_instanceof() -> Result<(), JSError> {
    let script = r#"
        const n = Object(123);
        const protoEq = (n.__proto__ === Number.prototype) ? 'EQ' : 'NEQ';
        const inst = (n instanceof Number) ? 'I' : 'N';
        protoEq + '|' + inst;
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>)?;
    match res {
        Value::String(s) => {
            println!("proto vs instanceof: {}", utf16_to_utf8(&s));
            Ok(())
        }
        other => panic!("Unexpected result: {:?}", other),
    }
}
