use javascript::{Value, evaluate_script, utf16_to_utf8};

// Initialize logger for these tests
#[ctor::ctor]
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
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(Value::String(s)) => {
            let s = utf16_to_utf8(&s);
            assert_eq!(s, "foo");
        }
        other => panic!("Expected string result, got {:?}", other),
    }
}
