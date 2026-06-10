use javascript::*;

// Initialize logger for these tests
#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn named_function_expression_has_name() {
    let script = r#"
        (function(){
            let o = {};
            o.x = function foo() { return 1; };
            return o.x.name;
        })()
    "#;
    let res = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "\"foo\"");
}
