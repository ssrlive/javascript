use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod deep_chain_tests {
    use super::*;

    const WINDOWS_LIKE_STACK_SIZE: usize = 1024 * 1024;
    const TEST_STACK_SIZE: usize = if cfg!(debug_assertions) {
        8 * 1024 * 1024
    } else {
        WINDOWS_LIKE_STACK_SIZE
    };

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
            (async function() {{
                let v = 0;
                for (let i = 0; i < {depth}; i = i + 1) {{
                    v = await asyncOperation(v);
                }}
                return v;
            }})()
        "#
        );

        let result = evaluate_script(&script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, depth.to_string());
    }

    #[cfg_attr(debug_assertions, ignore = "debug frames are too large for the 1 MiB Windows-like stack budget")]
    #[test]
    fn test_deep_promise_chain_on_windows_like_stack() {
        let depth = 2_000;
        let script = format!(
            r#"
            function asyncOperation(x) {{
              return Promise.resolve(x + 1);
            }}
            (async function() {{
                let v = 0;
                for (let i = 0; i < {depth}; i = i + 1) {{
                    v = await asyncOperation(v);
                }}
                return v;
            }})()
        "#
        );

        std::thread::Builder::new()
            .stack_size(TEST_STACK_SIZE)
            .spawn(move || {
                let result = evaluate_script(&script, false, None::<&std::path::Path>).unwrap();
                assert_eq!(result, depth.to_string());
            })
            .expect("failed to spawn thread")
            .join()
            .expect("thread panicked");
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
            (async function() {
                return await p;
            })()
        "#;

        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "25"); // 5 * 3 + 10 = 25
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

                (async function() {
                    let finallyCount = 0;

                    let success = await (async function() {
                        try {
                            let x = await asyncOperation(5);
                            x = x + 10;
                            x = await asyncOperation(x);
                            return x;
                        } finally {
                            finallyCount = finallyCount + 1;
                        }
                    })();

                    let failure = await (async function() {
                        try {
                            await asyncOperation(-1);
                            return "unexpected";
                        } catch (err) {
                            return "error: " + err;
                        } finally {
                            finallyCount = finallyCount + 1;
                        }
                    })();

                    return [success, failure, finallyCount];
                })();
            "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[40,\"error: negative value\",2]");
    }
}
