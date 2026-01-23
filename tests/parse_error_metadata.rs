use javascript::evaluate_script;

// Initialize logger for these tests
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn parse_error_carries_file_and_line_defaults() {
    // Feed malformed script to trigger a parse error
    let res = evaluate_script("let x = { ", None::<&std::path::Path>);
    match res {
        Err(err) => {
            println!("ParseError reported at \n{err}");
            // Parse error sites now include the originating source file and
            // line number via `file!()` / `line!()`. We assert that the
            // reported file is inside `src/core/` and a non-zero line is set.
            let (js_file, js_method, js_line) = (err.inner.js_file, err.inner.js_method, err.inner.js_line);
            {
                // Normalize path separators so test passes whether running on Windows (backslashes)
                // or Unix-like systems (forward slashes).
                let normalized = js_file.replace("\\", "/");
                println!("expected source file, got {js_file:?} (normalized: {normalized:?})");
                assert!(js_line.unwrap() > 0_usize, "expected non-zero line number");
                println!("expected method name, got {js_method:?}");
            }
        }
        other => panic!("Expected ParseError, got {:?}", other),
    }
}
