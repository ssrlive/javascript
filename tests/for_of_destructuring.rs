use javascript::*;

#[test]
fn for_of_destructuring_var_object() {
    let script = r#"
        var values = [{x:1},{x:2}];
        for (var {x} of values) { }
        x;
    "#;

    let result = evaluate_script_with_vm(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "2");
}
