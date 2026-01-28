use javascript::*;

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

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert!(result.contains("Error: boom"));
    println!("STACK:\n{result}");
}

#[test]
fn throw_stack_includes_decl_site() {
    // Synchronous throw inside a function should include the declaration site
    // (file:line:column) in the stack frame for the function.
    let script = r#"
        function doThirdThing() { throw new Error('boom'); }
        try { doThirdThing(); } catch (e) { String(e.stack) }
    "#;

    let result = evaluate_script(script, Some(std::path::Path::new("some.js"))).unwrap();
    assert_eq!(
        result,
        "\"Error: boom\\n    at doThirdThing (some.js:2:35)\\n    at <anonymous> (some.js:3:15)\""
    );
}

#[test]
fn async_unhandled_rejection_points_to_throw_site() {
    // A Promise chain where a callback throws should surface as an unhandled
    // rejection and the reported error should point to the actual throw site.
    let script = r#"
        function boom() { throw new Error('async-boom'); }
        new Promise(function(resolve, reject) { resolve(1); }).then(function() { boom(); });
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Err(e) => {
            // The surfaced unhandled rejection should carry the thrown message
            // and a recorded JS line indicating where it happened.
            assert!(e.js_line().is_some(), "Expected js location on unhandled rejection");
            assert!(
                e.user_message().contains("async-boom"),
                "Expected thrown message in user_message: {}",
                e.user_message()
            );
        }
        Ok(v) => panic!("Expected error for unhandled rejection, got {:?}", v),
    }
}

#[test]
fn assert_throw_shows_thrown_site_and_stack_shows_callsite() {
    // Simpler, clearer construction:
    // - The `assert` helper throws inside its body (throw site should be line 3)
    // - We add blank lines so the call-site appears later in the file (around line 53)
    let script = r#"
        function assert(condition, message) {
            if (!condition) {
                throw new Error(message);
            }
        }
        assert(false, 'boom');
    "#;

    let result = evaluate_script(script, Some(std::path::Path::new("file.js")));
    match result {
        Err(e) => {
            // Compute expected thrown-site and call-site lines from the script
            let lines: Vec<&str> = script.split('\n').collect();
            let thrown_line = lines
                .iter()
                .position(|l| l.contains("throw new Error(message);"))
                .map(|i| i + 1)
                .expect("could not find throw site in script");
            let callsite_line = lines
                .iter()
                .position(|l| l.contains("assert(false"))
                .map(|i| i + 1)
                .expect("could not find call site in script");

            // The thrown-site should be reported as the throw line inside `assert`
            assert!(
                e.user_message().contains(&format!("line {}:", thrown_line)),
                "Expected thrown-site in user_message: {}",
                e.user_message()
            );

            // Stack should include an `assert` frame pointing at the thrown-site
            let stack_str = e.stack().join("\n");
            println!("STACK_STR:\n{}", stack_str);
            assert!(
                stack_str.contains(&format!("at assert (file.js:{}:", thrown_line))
                    || stack_str.contains(&format!("assert (file.js:{}:", thrown_line)),
                "Expected assert frame at thrown-site in stack: {}",
                stack_str
            );

            // And the stack should also include the call-site line
            assert!(
                stack_str.contains(&format!("file.js:{}:", callsite_line)),
                "Expected call-site line in stack: {}",
                stack_str
            );
        }
        Ok(v) => panic!("Expected thrown error, got {:?}", v),
    }
}
