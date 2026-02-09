use javascript::*;

// Ensure logger initialization for tests
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod index_nullish_tests {
    use super::*;

    #[test]
    fn test_index_on_null_does_not_evaluate_key() {
        // The key object would throw if its toString was evaluated; but since the base is null
        // we must throw a TypeError before evaluating the key (per spec).
        let src = r#"
            var base = null;
            var prop = { toString: function() { throw new Error('key-evaluated'); } };
            base[prop];
        "#;

        let res = evaluate_script(src, None::<&std::path::Path>);
        assert!(res.is_err(), "Expected an error when accessing property on null");
        let err = res.unwrap_err();
        assert!(err.message().contains("TypeError: Cannot read properties of null or undefined"));
    }

    #[test]
    fn test_index_on_non_null_evaluates_key() {
        // When base is an object, the key's toString should be evaluated and its exception should propagate.
        let src = r#"
            var base = {};
            var prop = { toString: function() { throw new Error('key-evaluated'); } };
            base[prop];
        "#;

        let res = evaluate_script(src, None::<&std::path::Path>);
        assert!(res.is_err(), "Expected an error when key.toString throws");
        let err = res.unwrap_err();
        assert!(err.message().contains("Error: key-evaluated"));
    }
}
