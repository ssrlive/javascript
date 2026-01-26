use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod error_to_string_tests {
    use super::*;

    #[test]
    fn reference_error_to_string() {
        let script = r#"
            try { nonExistent(); } catch(e) { String(e) }
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"ReferenceError: nonExistent is not defined\"");
    }
}
