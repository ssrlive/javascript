use javascript::*;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_string_escapes_and_continuations() {
    let script = r#"
        let results = [];
        const str =
          "this string \     \
is broken \
across multiple \
lines.";
        results.push(str);
        results.push("Line1\nLine2");
        results.push("Tab\tTab");
        results.push("Backslash \\");
        results.push("Quote \"");
        results.push("Single Quote '");
        results.push("Unknown escape \z");
        results.push(`Template
Line 1 \
Line 2`);
        results.join('\n---\n')
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::String(s)) => {
            let out = String::from_utf16_lossy(&s);
            let expected = "this string      is broken across multiple lines.\n---\nLine1\nLine2\n---\nTab\tTab\n---\nBackslash \\\n---\nQuote \"\n---\nSingle Quote '\n---\nUnknown escape z\n---\nTemplate\nLine 1 Line 2";
            assert_eq!(out, expected);
        }
        _ => panic!("Expected string, got {:?}", result),
    }
}

#[test]
fn test_string_escapes_failed() {
    let script = r#"
        let results = [];
        const str =
          "this string \     
is broken \
across multiple \
lines.";
        results.join('\n---\n')
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Err(err) => {
            let msg = err.user_message();
            assert!(
                msg.contains("SyntaxError: Unterminated string literal (newline in string) at line 4:30"),
                "Unexpected error message: {msg}",
            );
        }
        _ => panic!("Expected syntax error, got {:?}", result),
    }
}
