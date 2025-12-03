use javascript::{Value, evaluate_script};

#[test]
fn for_of_destructuring_var_object() {
    let script = r#"
        var values = [{x:1},{x:2}];
        for (var {x} of values) { }
        x;
    "#;

    let result = evaluate_script(script);
    assert!(result.is_ok(), "evaluation should succeed");
    if let Ok(val) = result {
        match val {
            Value::Number(n) => assert_eq!(n, 2.0),
            other => panic!("expected number result, got: {:?}", other),
        }
    }
}
