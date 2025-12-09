use javascript::Value;
use javascript::evaluate_script;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn nested_method_stack_contains_frames() {
    // Create two functions as object properties so the evaluator's method-call
    // path will attach minimal `__frame` names ('a' and 'b') and populate
    // the captured frames used in Error.stack.
    let script = r#"
        let obj = {};
        obj.a = function() { obj.b(); };
        obj.b = function() { throw new Error('boom'); };
        try { obj.a(); } catch (e) { String(e.stack) }
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::String(s)) => {
            let out = String::from_utf16_lossy(&s);
            // Should include the error message and at least two frames
            assert!(
                out.contains("Error") && out.contains("boom"),
                "stack should include error name/message: {}",
                out
            );
            assert!(
                out.contains("at a") || out.contains("at obj.a"),
                "stack should include frame for 'a': {}",
                out
            );
            assert!(
                out.contains("at b") || out.contains("at obj.b"),
                "stack should include frame for 'b': {}",
                out
            );
        }
        other => panic!("Expected string stack trace, got {:?}", other),
    }
}
