use javascript::evaluate_script;

// Initialize logger for these tests
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn object_literal_named_function_has_name() {
    let script = r#"
        (function(){
            let o = { x: function foo() { return 1; } };
            console.log(o.x());
            return o.x.name;
        })()
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "\"foo\"");
}

#[test]
fn assigned_generator_named_function_has_name() {
    let script = r#"
        (function(){
            let o = {};
            o.x = function* foo() { yield 1; };
            console.log(o.x());
            console.log(o.x().next());
            console.log(o.x().next().value);
            return o.x.name;
        })()
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "\"foo\"");
}

#[test]
#[ignore]
fn assigned_async_named_function_has_name() {
    let script = r#"
        (function(){
            let o = {};
            o.x = async function foo() { return 99; };
            console.log(await o.x());
            o.x().then(v => console.log(v));
            return o.x.name;
        })()
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "\"foo\"");
}
