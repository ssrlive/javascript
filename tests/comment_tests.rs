use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod comment_tests {
    use super::*;

    #[test]
    fn test_comment_removal() {
        // Test single-line comments
        let result = evaluate_script("// This is a comment\nconsole.log('Hello');");
        assert!(result.is_ok());

        // Test multi-line comments
        let result = evaluate_script("/* Multi-line\ncomment */ console.log('World');");
        assert!(result.is_ok());

        // Test comments in strings are preserved
        let result = evaluate_script("console.log('// Not a comment'); console.log('/* Not a comment */');");
        assert!(result.is_ok());

        // Test mixed comments
        let script = r#"
// Single line comment
console.log('Line 1 /* not a comment */' /* this is a comment */);

/* Multi-line
   comment */
console.log('Line 2'); // just a space

// Inline comment
console.log('Line 3 // not a comment in line 3'); // Another comment
"#;
        let result = evaluate_script(script);
        assert!(result.is_ok());
    }
}
