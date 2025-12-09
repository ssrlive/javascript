use javascript::{Value, evaluate_script};

#[test]
fn nested_object_defaults() {
    let script = r#"
        // nested object destructuring without defaults on intermediate nodes
        let {a: {b: {c = 42}}} = {a: {b: {}}};
        c
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 42.0),
        other => panic!("Expected number 42 from nested object default, got {:?}", other),
    }
}

#[test]
fn nested_array_defaults() {
    let script = r#"
        let [[a = 7]] = [[undefined]];
        a
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 7.0),
        other => panic!("Expected number 7 from nested array default, got {:?}", other),
    }
}

#[test]
fn combined_nested_defaults() {
    let script = r#"
        // nested array within object; defaults for elements but no default for the whole array
        let {p: [a = 1, b = 2]} = {p: [undefined]};
        a + b
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 3.0),
        other => panic!("Expected sum 3 from combined nested defaults, got {:?}", other),
    }
}
