use javascript::*;

#[test]
fn for_of_destructuring_var_object() {
    let script = r#"
        var values = [{x:1},{x:2}];
        for (var {x} of values) { }
        x;
    "#;

    let result = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(result, "2");
}
