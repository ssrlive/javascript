use javascript::evaluate_script;

#[test]
fn for_of_destructuring_var_object() {
    let script = r#"
        var values = [{x:1},{x:2}];
        for (var {x} of values) { }
        x;
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "2");
}
