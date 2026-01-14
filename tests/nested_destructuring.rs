use javascript::evaluate_script;

#[test]
fn nested_object_defaults() {
    let script = r#"
        // nested object destructuring without defaults on intermediate nodes
        let {a: {b: {c = 42}}} = {a: {b: {}}};
        c
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "42");
}

#[test]
fn nested_array_defaults() {
    let script = r#"
        let [[a = 7]] = [[undefined]];
        a
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "7");
}

#[test]
fn combined_nested_defaults() {
    let script = r#"
        // nested array within object; defaults for elements but no default for the whole array
        let {p: [a = 1, b = 2]} = {p: [undefined]};
        a + b
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "3");
}
