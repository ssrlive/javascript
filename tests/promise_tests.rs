use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod promise_tests {
    use super::*;

    #[test]
    fn test_promise_then_basic() {
        let code = r#"
            let result = null;
            let p = new Promise(function(resolve, reject) {
                resolve(100);
            });
            p.then(function(val) {
                result = val * 2;
            });
            result
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "200");
    }

    #[test]
    fn test_promise_chaining() {
        let code = r#"
            let finalResult = null;
            let p = new Promise(function(resolve, reject) {
                resolve(10);
            });
            p.then(function(val) {
                return val + 5;
            }).then(function(val) {
                return val * 2;
            }).then(function(val) {
                finalResult = val;
            });
            finalResult
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "30");
    }

    #[test]
    fn test_promise_all_resolved() {
        let code = r#"
            let result = null;
            let p1 = new Promise(function(resolve, reject) {
                resolve(1);
            });
            let p2 = new Promise(function(resolve, reject) {
                resolve(2);
            });
            let p3 = new Promise(function(resolve, reject) {
                resolve(3);
            });
            Promise.all([p1, p2, p3]).then(function(values) {
                result = values[0] + values[1] + values[2];
            });
            result
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_promise_race_resolved() {
        let code = r#"
            let result = null;
            let p1 = new Promise(function(resolve, reject) {
                resolve(1);
            });
            let p2 = new Promise(function(resolve, reject) {
                resolve(2);
            });
            Promise.race([p1, p2]).then(function(value) {
                result = value;
            });
            result
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "1");
    }

    #[test]
    fn test_promise_async_execution_order() {
        // Test that Promise then callbacks execute asynchronously after synchronous code
        let code = r#"
            let executionOrder = [];
            executionOrder.push("sync");
            let p = new Promise((res, rej) => res("async result"));
            p.then((value) => {
                executionOrder.push(value);
            });
            executionOrder
        "#;

        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[\"sync\",\"async result\"]");
    }

    #[test]
    fn test_promise_finally() {
        let code = r#"
            var finalResult = "not set";
            new Promise(function(resolve, reject) { resolve(42); })
                .finally(function() { console.log('finally executed'); })
                .then(function(v) { finalResult = v; });
            finalResult
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn test_promise_allsettled_mixed() {
        let code = r#"
            let p1 = new Promise(function(resolve, reject) {
                console.log("Resolving p1");
                resolve(1);
            });
            let p2 = new Promise(function(resolve, reject) {
                console.log("Rejecting p2");
                reject("error");
            });
            let p3 = new Promise(function(resolve, reject) {
                console.log("Resolving p3");
                resolve(3);
            });
            Promise.allSettled([p1, p2, p3])
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(
            result,
            "[{\"status\":\"fulfilled\",\"value\":1},{\"status\":\"rejected\",\"reason\":\"error\"},{\"status\":\"fulfilled\",\"value\":3}]"
        );
    }

    #[test]
    fn test_promise_any_fulfilled() {
        let code = r#"
            let result = null;
            Promise.any([
                Promise.reject("no"),
                Promise.resolve(7),
                Promise.reject("later")
            ]).then(function(value) {
                result = value;
            });
            result
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "7");
    }

    #[test]
    fn test_promise_any_all_rejected_aggregate_error() {
        let code = r#"
            let out = null;
            Promise.any([
                Promise.reject("e1"),
                Promise.reject("e2")
            ]).catch(function(err) {
                out = [err.name, err.message, err.errors[0], err.errors[1]];
            });
            out
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[\"AggregateError\",\"All promises were rejected\",\"e1\",\"e2\"]");
    }

    #[test]
    fn test_promise_any_pending_plus_rejected_stays_pending() {
        let code = r#"
            let pending = new Promise(function(resolve, reject) {
                // intentionally never settled
            });
            Promise.any([
                pending,
                Promise.reject("boom")
            ])
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert!(result.contains("pending"), "expected pending promise, got: {result}");
    }

    #[test]
    fn test_promise_any_thenable_fulfilled() {
        let code = r#"
            let out = null;
            let thenable = {
                then: function(resolve, reject) {
                    resolve("from-thenable");
                }
            };
            Promise.any([
                Promise.reject("first"),
                thenable
            ]).then(function(value) {
                out = value;
            });
            out
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"from-thenable\"");
    }

    #[test]
    fn test_promise_any_thenable_rejected_all_rejected() {
        let code = r#"
            let out = null;
            let thenable = {
                then: function(resolve, reject) {
                    reject("thenable-reject");
                }
            };
            Promise.any([
                Promise.reject("p-reject"),
                thenable
            ]).catch(function(err) {
                out = [err.name, err.errors[0], err.errors[1]];
            });
            out
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[\"AggregateError\",\"p-reject\",\"thenable-reject\"]");
    }

    #[test]
    fn test_promise_any_thenable_then_throws_all_rejected() {
        let code = r#"
            let out = null;
            let thenable = {
                then: function(resolve, reject) {
                    throw "then-throw";
                }
            };
            Promise.any([
                Promise.reject("p-reject"),
                thenable
            ]).catch(function(err) {
                out = [err.name, err.errors[0], err.errors[1]];
            });
            out
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        // assert_eq!(result, "[\"AggregateError\",\"p-reject\",\"then-throw\"]");
        assert!(
            result.contains("p-reject") && result.contains("then-throw") && result.contains("AggregateError"),
            "unexpected result: {result}"
        );
    }

    #[test]
    fn test_promise_any_then_getter_throws_all_rejected() {
        let code = r#"
            let out = null;
            let thenable = {};
            Object.defineProperty(thenable, "then", {
                get: function() {
                    throw "getter-throw";
                }
            });
            Promise.any([
                Promise.reject("p-reject"),
                thenable
            ]).catch(function(err) {
                out = [err.name, err.errors[0], err.errors[1]];
            });
            out
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert!(
            result.contains("p-reject") && result.contains("getter-throw") && result.contains("AggregateError"),
            "unexpected result: {result}"
        );
    }

    #[test]
    fn test_aggregate_error_constructor_shape() {
        let code = r#"
            let out = null;
            let e = AggregateError(["x", "y"], "boom");
            out = [e.name, e.message, e.errors[0], e.errors[1]];
            out
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[\"AggregateError\",\"boom\",\"x\",\"y\"]");
    }

    #[test]
    fn test_promise_constructor_direct_functionality() {
        // Test that the direct constructor path works by creating a promise that resolves
        let code = r#"
            let result = null;
            new Promise(function(resolve, reject) {
                resolve("direct test");
            }).then(function(value) {
                result = value;
            });
            result
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"direct test\"");
    }

    #[test]
    fn test_promise_then_direct_functionality() {
        // Test that the direct then handler works
        let code = r#"
            let result = null;
            let p = new Promise(function(resolve, reject) {
                resolve(42);
            });
            p.then(function(value) {
                result = value * 2;
            });
            result
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "84");
    }

    #[test]
    fn test_promise_catch_direct_functionality() {
        // Test that the direct catch handler works
        let code = r#"
            let result = null;
            let p = new Promise(function(resolve, reject) {
                reject("test error");
            });
            p.catch(function(reason) {
                result = "caught: " + reason;
            });
            result
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"caught: test error\"");
    }

    #[test]
    fn test_promise_finally_direct_functionality() {
        // Test that the direct finally handler works
        let code = r#"
            let result = null;
            new Promise(function(resolve, reject) {
                resolve(100);
            }).finally(function() {
                result = "cleanup done";
            });
            result
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"cleanup done\"");
    }

    #[test]
    fn test_promise_resolve_reject_functions_direct() {
        // Test that the direct resolve/reject functions work
        let code = r#"
            let resolveResult = null;
            let rejectResult = null;

            Promise.resolve("resolved").then(function(value) {
                resolveResult = value;
            });

            Promise.reject("rejected").catch(function(reason) {
                rejectResult = reason;
            });

            [resolveResult, rejectResult]
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[\"resolved\",\"rejected\"]");
    }

    #[test]
    fn test_promise_constructor_with_arrow_function() {
        // Test that arrow functions work in Promise constructor
        let code = r#"
            let result = null;
            new Promise(resolve => resolve(42)).then(value => {
                result = value * 2;
            });
            result
        "#;
        let result = evaluate_script_with_vm(code, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "84");
    }

    #[test]
    fn then_calls_native_callback_and_prints() {
        let script = r#"
        let printed = "";
        console.log = function(x) { printed = x; };

        async function example() {
            return new Promise((resolve) => {
                setTimeout(() => resolve("Done!"), 0);
            });
        }

        example().then(console.log);
        // Wait briefly to let timers / microtasks run and observe side-effects
        new Promise((resolve) => setTimeout(() => resolve(printed), 10));
    "#;

        let res = evaluate_script_with_vm(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(res, "\"Done!\"");
    }
}
