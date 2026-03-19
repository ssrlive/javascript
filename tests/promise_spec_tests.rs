use javascript::*;

fn eval_vm(script: &str) -> String {
    evaluate_script_with_vm(script, false, None::<&std::path::Path>).unwrap()
}

fn eval_vm_result(script: &str) -> Result<String, JSError> {
    evaluate_script_with_vm(script, false, None::<&std::path::Path>)
}

fn eval_vm_async_iife(body: &str) -> String {
    let wrapped = format!("(async function() {{\n{}\n}})()", body);
    eval_vm(&wrapped)
}

#[test]
fn spec_promise_constructor_returns_promise_object() {
    let script = r#"
        let p = new Promise(function(resolve, reject) { resolve(1); });
        typeof p.then
    "#;
    assert_eq!(eval_vm(script), "\"function\"");
}

#[test]
fn spec_async_function_call_returns_promise() {
    let script = r#"
        async function f() { return 42; }
        let p = f();
        typeof p.then
    "#;
    assert_eq!(eval_vm(script), "\"function\"");
}

#[test]
fn spec_allsettled_mixed_outcomes() {
    let body = r#"
        return await Promise.allSettled([
            Promise.resolve(1),
            Promise.reject(2),
            Promise.resolve(3)
        ]);
    "#;
    let result = eval_vm_async_iife(body);
    assert_eq!(
        result,
        "[{\"status\":\"fulfilled\",\"value\":1},{\"status\":\"rejected\",\"reason\":2},{\"status\":\"fulfilled\",\"value\":3}]"
    );
}

#[test]
fn spec_executor_throw_non_error_does_not_throw_sync() {
    let script = r#"
        let threw = false;
        try {
            new Promise(function(resolve, reject) { throw 2; });
        } catch (e) {
            threw = true;
        }
        threw
    "#;
    assert_eq!(eval_vm(script), "false");
}

#[test]
fn spec_executor_throw_error_does_not_throw_sync() {
    let script = r#"
        let threw = false;
        try {
            new Promise(function(resolve, reject) { throw new Error('boom'); });
        } catch (e) {
            threw = true;
        }
        threw
    "#;
    assert_eq!(eval_vm(script), "false");
}

#[test]
fn spec_await_rejected_promise_enters_catch() {
    let body = r#"
        try {
            await Promise.reject(2);
            return 'unexpected';
        } catch (e) {
            return e;
        }
    "#;
    assert_eq!(eval_vm_async_iife(body), "2");
}

#[test]
fn spec_rejected_promise_is_value_not_host_err() {
    let script = r#"new Promise(function(resolve, reject) { throw new Error('boom'); })"#;
    let result = eval_vm_result(script);
    assert!(result.is_ok(), "expected Promise completion, got Err: {:?}", result.err());
}

#[test]
fn spec_await_thenable_fulfill_assimilates_value() {
    let body = r#"
        let thenable = {
            then(resolve, reject) {
                resolve(99);
            }
        };
        return await thenable;
    "#;
    assert_eq!(eval_vm_async_iife(body), "99");
}

#[test]
fn spec_await_thenable_reject_enters_catch() {
    let body = r#"
        let thenable = {
            then(resolve, reject) {
                reject('bad');
            }
        };
        try {
            await thenable;
            return 'unexpected';
        } catch (e) {
            return e;
        }
    "#;
    assert_eq!(eval_vm_async_iife(body), "\"bad\"");
}

#[test]
fn spec_await_non_callable_then_returns_object() {
    let body = r#"
        let obj = { then: 123, value: 7 };
        let out = await obj;
        return out.value;
    "#;
    assert_eq!(eval_vm_async_iife(body), "7");
}

#[test]
fn spec_async_finally_rejection_overrides_prior_rejection() {
    let body = r#"
        let f = async function() {
            try {
                await Promise.reject('early-reject');
            } finally {
                await Promise.reject('override');
            }
        };

        return await f().then(
            function() { return 'fulfilled'; },
            function(reason) { return reason; }
        );
    "#;
    let res = eval_vm_async_iife(body);
    assert!(res.contains("override"));
}

#[test]
fn spec_async_finally_override_matches_cli_script_pattern() {
    let script = r#"
        "use strict";

        function assert(b, msg) {
            if (!b) {
                throw new Error("Assertion failed" + (msg ? ": " + msg : ""));
            }
        }

        function $DONE(err) {
            if (err) {
                console.error("Test failed:", err);
            }
        }

        {
            var f = async() => {
                try {
                    await new Promise(function(resolve, reject) {
                        reject("early-reject");
                    });
                } finally {
                    await new Promise(function(resolve, reject) {
                        reject("override");
                    });
                }
            };

            f().then($DONE, function(value) {
                assert(value === "override", "Awaited rejection in finally block, got: " + value);
            }).then($DONE, $DONE);
        }
    "#;

    let result = eval_vm_result(script);
    assert!(result.is_ok(), "expected no uncaught error, got: {:?}", result.err());
}
