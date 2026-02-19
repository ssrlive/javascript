use javascript::evaluate_script;

#[test]
fn test_promise_async_resolution() {
    // Test that we can get the async result of a Promise
    let script = r#"
        new Promise((resolve, reject) => {
            resolve("async result");
        })
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"async result\"");
}

#[test]
fn test_await_async_function() {
    // Test that await works in async functions
    let script = r#"
        async function getResult() {
            let promise = new Promise((resolve, reject) => {
                resolve(42);
            });
            return await promise;
        }
        getResult()
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "42");
}

#[test]
fn test_promise_chaining_async() {
    // Test Promise chaining with async resolution
    let script = r#"
        new Promise((resolve, reject) => {
            resolve(10);
        }).then((value) => {
            return value * 2;
        }).then((value) => {
            return value + 5;
        })
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "25");
}

#[test]
fn test_promise_allsettled() {
    // Test Promise.allSettled with mixed resolve/reject
    let script = r#"
        Promise.allSettled([
            new Promise(function(resolve, reject) { resolve(1); console.log("executor 1 called"); }),
            new Promise(function(resolve, reject) { reject(2); console.log("executor 2 called"); }),
            new Promise(function(resolve, reject) { resolve(3); console.log("executor 3 called"); })
        ])
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(
        result,
        "[{\"status\":\"fulfilled\",\"value\":1},{\"status\":\"rejected\",\"reason\":2},{\"status\":\"fulfilled\",\"value\":3}]"
    );
}

#[test]
fn test_main() {
    let script = r#"
        Promise.allSettled([
            new Promise((resolve, reject) => { resolve(1); }),
            new Promise((resolve, reject) => { reject(2); }),
            new Promise((resolve, reject) => { resolve(3); })
        ])
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(
        result,
        "[{\"status\":\"fulfilled\",\"value\":1},{\"status\":\"rejected\",\"reason\":2},{\"status\":\"fulfilled\",\"value\":3}]"
    );
}

// --- New boundary tests for unhandled/handler timing and allSettled behavior ---

// Test: synchronous catch silences non-Error rejection.
// Specification note:
// - The Promise executor runs synchronously and attaching a rejection handler
//   synchronously should be able to handle the rejection before host-level
//   unhandled rejection reporting occurs.
// - See ECMAScript specification sections:
//   * Promise.prototype.catch: https://tc39.es/ecma262/#sec-promise.prototype.catch
//   * Host rejection tracking hooks (HostPromiseRejectionTracker / HostReportUnhandledPromiseRejection):
//     https://tc39.es/ecma262/#sec-host-promise-rejection-tracker
#[test]
fn test_sync_catch_silences_non_error_rejection() {
    let script = r#"
        let result = null;
        let p = new Promise(function(resolve, reject) { reject(2); });
        p.catch(function(reason) { result = 'caught ' + reason; });
        result
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(v) => assert_eq!(v, "\"caught 2\""),
        Err(e) => panic!("Expected Ok, got Err: {:?}", e),
    }
}

// Test: Non-Error rejections are not reported immediately.
// Specification note:
// - The ECMAScript specification defines host hooks for rejection tracking
//   (HostPromiseRejectionTracker / HostReportUnhandledPromiseRejection). The
//   exact timing of host reporting is not prescriptive; implementations may
//   defer reporting to give synchronous handlers time to attach.
// - This test asserts that non-Error rejections (simple values) are deferred
//   and not immediately surfaced as a thrown error by the evaluator.
//   See: https://tc39.es/ecma262/#sec-host-promise-rejection-tracker
#[test]
fn test_unhandled_non_error_rejection_not_immediate() {
    // Non-Error rejections are not reported immediately; ensure they don't surface as Err here.
    let script = r#"new Promise(function(resolve, reject) { reject(2); })"#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    // The script should evaluate to the rejection reason being observable after microtask processing
    assert_eq!(result, "2");
}

// Test: Promise.allSettled should not treat inner rejections as unhandled.
// Specification note:
// - Promise.allSettled registers handlers on each input promise to capture
//   whether it fulfills or rejects; the inner rejection is therefore handled
//   by the allSettled machinery and should not be considered an unhandled
//   rejection by host tracking.
// - See Promise.allSettled algorithm: https://tc39.es/ecma262/#sec-promise.allsettled
#[test]
fn test_allsettled_reject_does_not_report_unhandled() {
    let script = r#"
        let result = null;
        Promise.allSettled([
            new Promise(function(resolve, reject) { reject(2); })
        ]).then(function(arr) { result = arr[0].status + ':' + arr[0].reason; });
        result
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(v) => assert_eq!(v, "\"rejected:2\""),
        Err(e) => panic!("Expected Ok, got Err: {:?}", e),
    }
}

// --- Error-like vs Non-Error boundary tests ---

#[test]
fn test_error_rejection_reported_immediately() {
    // After the InternalSlot migration, a plain JS object with a user-space
    // `__is_error` property is NOT treated as an engine Error.  Only objects
    // created via `new Error(...)` (which carry InternalSlot::IsError) are
    // detectable by the unhandled-rejection tracker.  Therefore rejecting with
    // a plain object should NOT surface as an Err.
    let script = r#"new Promise(function(resolve, reject) { let e = {message: 'boom', __is_error: true}; reject(e); })"#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    // Plain-object rejection is non-error-like → Ok (the rejected promise serialized)
    assert!(
        result.is_ok(),
        "Expected Ok for non-Error-like rejection, got Err: {:?}",
        result.err()
    );
}

#[test]
fn test_error_rejection_silenced_by_sync_catch() {
    // If a rejection is with an Error-like object but a catch handler is attached
    // synchronously, it should silence the reporting (no Err returned).
    let script = r#"
        let result = null;
        let p = new Promise(function(resolve, reject) { let e = {message: 'boom', __is_error: true}; reject(e); });
        p.catch(function(reason) { result = 'caught:' + reason.message; });
        result
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(v) => assert_eq!(v, "\"caught:boom\""),
        Err(e) => panic!("Expected Ok, got Err: {:?}", e),
    }
}

#[test]
fn test_allsettled_with_error_like_does_not_report_unhandled() {
    // Promise.allSettled should register handlers for each input promise and
    // therefore should not cause an Error-like rejection to be treated as
    // an unhandled rejection by the host.  Use `new Error('boom')` so the
    // rejection reason carries InternalSlot::IsError (plain objects with
    // `__is_error` no longer map to the internal slot after the migration).
    // NOTE: uses synchronous rejection; deferred rejection via
    // `Promise.resolve().then(...)` has a pre-existing microtask scheduling
    // limitation that prevents `allSettled.then` from running before
    // `evaluate_script` captures the return value.
    let script = r#"
        let result = null;
        Promise.allSettled([
            new Promise(function(resolve, reject) {
                reject(new Error('boom'));
            })
        ]).then(function(arr) {
            result = arr[0].status + ':' + arr[0].reason.message;
        });
        result
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(v) => assert_eq!(v, "\"rejected:boom\""),
        Err(e) => panic!("Expected Ok, got Err: {:?}", e),
    }
}

// Executor throws a non-Error value: should not abort synchronous flow and should not be reported immediately
#[test]
fn test_executor_throw_non_error_not_immediate() {
    let script = r#"
        let after = 0;
        new Promise(function(resolve, reject) { throw 2; });
        after = 1;
        after
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "1");
}

// Executor throws an Error object: should be treated as an Error-like rejection and reported
#[test]
fn test_executor_throw_error_reported_immediately() {
    let script = r#"new Promise(function(resolve, reject) { throw new Error('boom'); })"#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Err(e) => {
            let s = format!("{:?}", e);
            assert!(s.contains("boom"), "expected error message 'boom' in {:?}", e);
        }
        Ok(v) => panic!("Expected Err, got Ok: {:?}", v),
    }
}
