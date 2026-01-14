use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod timeout_tests {
    use super::*;

    #[test]
    #[ignore = "Promises are not yet supported in the test environment"]
    fn test_set_timeout_basic() {
        let script = r#"
            new Promise((resolve) => {
                let result = "not called";
                setTimeout(() => {
                    result = "called";
                    resolve(result);
                }, 0);
            })
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"called\"");
    }

    #[test]
    #[ignore = "Promises are not yet supported in the test environment"]
    fn test_set_timeout_with_args() {
        let script = r#"
            new Promise((resolve) => {
                setTimeout((x, y) => {
                    resolve(x + y);
                }, 0, 5, 10);
            })
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "15");
    }

    #[test]
    #[ignore = "Promises are not yet supported in the test environment"]
    fn test_set_timeout_returns_id() {
        let script = r#"
            let id = setTimeout(() => {}, 0);
            typeof id === "number" && id >= 0
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    #[ignore = "Promises are not yet supported in the test environment"]
    fn test_clear_timeout() {
        let script = r#"
            new Promise((resolve) => {
                let result = "not called";
                let id = setTimeout(() => { result = "called"; }, 0);
                clearTimeout(id);
                // Wait a bit to ensure timeout doesn't fire
                setTimeout(() => { resolve(result); }, 1);
            })
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"not called\"");
    }

    #[test]
    #[ignore = "Promises are not yet supported in the test environment"]
    fn test_multiple_set_timeout() {
        let script = r#"
            new Promise((resolve) => {
                let results = [];
                setTimeout(() => { results.push(1); }, 0);
                setTimeout(() => { results.push(2); }, 0);
                setTimeout(() => { results.push(3); }, 0);
                setTimeout(() => {
                    resolve(results.length === 3 && results[0] === 1 && results[1] === 2 && results[2] === 3);
                }, 1);
            })
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    #[ignore = "Promises are not yet supported in the test environment"]
    fn test_set_timeout_with_function_reference() {
        let script = r#"
            new Promise((resolve) => {
                let result = 0;
                function increment() { result += 1; }
                setTimeout(increment, 0);
                setTimeout(increment, 0);
                setTimeout(() => { resolve(result); }, 1);
            })
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "2");
    }
}
