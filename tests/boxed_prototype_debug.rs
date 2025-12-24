use javascript::{JSError, Value, evaluate_script, utf16_to_utf8};

#[test]
fn debug_boxed_prototype_identity() -> Result<(), JSError> {
    let script = r#"
        // Attach a marker to Number.prototype and inspect the boxed object's prototype
        Number.prototype.__marker = 'NUM_PROTO_MARKER';
        const boxed = Object(123);
        const protoMarker = boxed.__proto__ && boxed.__proto__.__marker;
        const ctorMarker = Number.prototype.__marker;
        const eq = boxed.__proto__ === Number.prototype;
        protoMarker + '|' + ctorMarker + '|' + (eq ? 'EQ' : 'NEQ');
    "#;

    let res = evaluate_script(script, None::<&std::path::Path>)?;
    match res {
        Value::String(s) => {
            let out = utf16_to_utf8(&s);
            // Print to stdout so test logs show the result for debugging
            println!("boxed prototype debug: {}", out);
            Ok(())
        }
        other => panic!("Unexpected result from debug script: {:?}", other),
    }
}
