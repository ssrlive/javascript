use javascript::evaluate_script;

// Initialize logger for these tests
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn parse_error_carries_file_and_line_defaults() {
    // Feed malformed script to trigger a parse error
    let res = evaluate_script("let x = { ");
    match res {
        Err(err) => {
            println!("ParseError reported at {err}");
            // Parse error sites now include the originating source file and
            // line number via `file!()` / `line!()`. We assert that the
            // reported file is inside `src/core/` and a non-zero line is set.
            let (file, line, method) = (err.inner.file, err.inner.line, err.inner.method);
            {
                // Normalize path separators so test passes whether running on Windows (backslashes)
                // or Unix-like systems (forward slashes).
                let normalized = file.replace("\\", "/");
                assert!(normalized.contains("src/core/"), "expected core source path, got {:?}", file);
                assert!(line > 0usize, "expected non-zero line number");
                println!("expected method name, got {method:?}");
            }
        }
        other => panic!("Expected ParseError, got {:?}", other),
    }
}
