use javascript::*;

// Initialize logger for these tests
#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn object_literal_named_function_has_name() {
    let script = r#"
        (function(){
            let o = { x: function foo() { return 1; } };
            return o.x.name;
        })()
    "#;
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "\"foo\"");
}

#[test]
fn assigned_generator_named_function_has_name() {
    let script = r#"
        (function(){
            let o = {};
            o.x = function* foo() { yield 1; };
            return o.x.name;
        })()
    "#;
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "\"foo\"");
}

#[test]
fn assigned_async_named_function_has_name() {
    let script = r#"
        (async function () {
          let o = {};
          o.x = async function foo() { return 99; };
          return o.x.name;
        })();
    "#;
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert!(res.contains("\"foo\""));
}
