use javascript::evaluate_script;

#[test]
fn test_generator_method_super_property() {
    // Test that `super` inside a generator method body resolves properties on the object's prototype
    let result = evaluate_script(
        r#"
        var obj = { *foo() { return super.toString; } };
        obj.toString = null;
        obj.foo().next().value === Object.prototype.toString;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}
