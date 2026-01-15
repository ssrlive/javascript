use javascript::*;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn arrow_function_lexical_this() {
    let script = r#"
        let o = { x: 42 };
        o.f = function() {
            let a = () => this.x;
            return a();
        };
        o.f();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "42");
}

#[test]
fn normal_function_this_binding_with_call() {
    let script = r#"
        function foo() { return this.x; }
        let o = { x: 99 };
        foo.call(o);
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "99");
}

#[test]
fn object_prototype_to_string_with_primitive_receiver() {
    let script = r#"
        Object.prototype.toString.call("x");
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"[object String]\"");
}

#[test]
fn reflect_apply_binds_this_for_closures() {
    let script = r#"
        function f(a, b) { return this.x + a + b; }
        let o = { x: 1 };
        Reflect.apply(f, o, [2, 3]);
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "6");
}
