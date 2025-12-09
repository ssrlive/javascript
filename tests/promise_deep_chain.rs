use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod deep_chain_tests {
    use super::*;

    #[test]
    fn test_deep_promise_chain_no_stack_overflow() {
        // Build a script that chains many .then() calls where each step
        // returns a Promise that resolves to the previous value + 1.
        // The final resolved value should equal the chain depth.
        // Use a smaller depth to keep the test fast and reliable on CI
        // while still exercising deep chained .then() behavior.
        let depth = 200;
        let script = format!(
            r#"
function asyncOperation(x) {{
  return new Promise(function(resolve, _reject) {{ resolve(x + 1); }});
}}
let p = Promise.resolve(0);
for (let i = 0; i < {depth}; i = i + 1) {{
  p = p.then(asyncOperation);
}}
// Return the final promise so the runtime will wait for it to settle
p
"#
        );

        let result = evaluate_script(&script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n as i64, depth),
            other => panic!("Expected number {}, got {:?}", depth, other),
        }
    }

    #[test]
    fn test_promise_then_with_closure() {
        // Test using closures inside then statements that capture variables
        let script = r#"
let multiplier = 3;
let offset = 10;
let p = Promise.resolve(5);
p = p.then(function(x) {
    return x * multiplier;  // Closure captures 'multiplier'
});
p = p.then(function(y) {
    return y + offset;  // Closure captures 'offset'
});
// Return the final promise
p
"#;

        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n as i64, 25), // 5 * 3 + 10 = 25
            other => panic!("Expected number 25, got {:?}", other),
        }
    }

    #[test]
    fn promise_complex_chain_another() {
        let script = r#"
                function asyncOperation(value) {
                    return new Promise((resolve, reject) => {
                        if (value > 0) {
                            resolve(value * 2);
                        } else {
                            reject("negative value");
                        }
                    });
                }

                Promise.resolve(5)
                    .then(asyncOperation)
                    .then(result => result + 10)
                    .then(asyncOperation)
                    .catch(err => "error: " + err)
                    .finally(() => console.log("finally: cleanup"));
            "#;
        evaluate_script(script, None::<&std::path::Path>).unwrap();
    }
}
