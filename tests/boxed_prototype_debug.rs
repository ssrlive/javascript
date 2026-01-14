use javascript::evaluate_script;

#[test]
fn debug_boxed_prototype_identity() {
    let script = r#"
        // Attach a marker to Number.prototype and inspect the boxed object's prototype
        Number.prototype.__marker = 'NUM_PROTO_MARKER';
        const boxed = Object(123);
        const protoMarker = boxed.__proto__ && boxed.__proto__.__marker;
        const ctorMarker = Number.prototype.__marker;
        const eq = boxed.__proto__ === Number.prototype;
        protoMarker + '|' + ctorMarker + '|' + (eq ? 'EQ' : 'NEQ');
    "#;

    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "\"NUM_PROTO_MARKER|NUM_PROTO_MARKER|EQ\"");
}
