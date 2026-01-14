use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod nullish_coalescing_tests {
    use super::*;

    #[test]
    fn test_nullish_coalescing() {
        // Test undefined ?? default
        let result = evaluate_script("undefined ?? 'default'", None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"default\"");

        // Test null ?? default
        let result = evaluate_script("null ?? 'default'", None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"default\"");

        // Test falsy values ?? default (should return the falsy value)
        let result = evaluate_script("0 ?? 'default'", None::<&std::path::Path>).unwrap();
        assert_eq!(result, "0");

        let result = evaluate_script("false ?? 'default'", None::<&std::path::Path>).unwrap();
        assert_eq!(result, "false");

        let result = evaluate_script("'' ?? 'default'", None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"\"");

        // Test truthy values ?? default (should return the truthy value)
        let result = evaluate_script("'hello' ?? 'default'", None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"hello\"");

        let result = evaluate_script("42 ?? 'default'", None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");

        // Test chained nullish coalescing
        let result = evaluate_script("undefined ?? null ?? 'fallback'", None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"fallback\"");

        // Test with variables
        let result = evaluate_script("let x = undefined; x ?? 'default'", None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"default\"");

        let result = evaluate_script("let x = 'value'; x ?? 'default'", None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"value\"");
    }
}
