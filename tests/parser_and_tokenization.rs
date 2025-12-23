use javascript::*;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn multiple_var_declarations_without_initializers() {
    let script = "var a, b; a = 1; b = 2; a + b";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 3.0),
        _ => panic!("Expected number 3.0, got {:?}", result),
    }
}

#[test]
fn skip_empty_semicolons_and_let() {
    let script = ";; let x = 5; ; x";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 5.0),
        _ => panic!("Expected number 5.0, got {:?}", result),
    }
}

#[test]
fn single_line_and_block_comments_ignored() {
    let script = "// leading comment\n/* block comment */ let x = 7; x";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 7.0),
        _ => panic!("Expected number 7.0, got {:?}", result),
    }
}

#[test]
fn trailing_comma_and_newline_before_rbrace_is_allowed() {
    // tokens for: { \n seconds = 0, \n }
    let raw_tokens = vec![
        Token::LBrace,
        Token::LineTerminator,
        Token::Identifier("seconds".to_string()),
        Token::Assign,
        Token::Number(0.0),
        Token::Comma,
        Token::LineTerminator,
        Token::RBrace,
    ];
    let mut tokens: Vec<javascript::TokenData> = raw_tokens
        .into_iter()
        .map(|t| javascript::TokenData {
            token: t,
            line: 0,
            column: 0,
        })
        .collect();

    let pattern = parse_object_destructuring_pattern(&mut tokens).expect("should parse pattern");
    // pattern should contain one property
    assert_eq!(pattern.len(), 1);
    // and the tokens left should be empty
    assert!(tokens.is_empty());
}

#[test]
fn exponentiation_and_numeric_separators_supported() {
    // Exponentiation for numbers
    let res = evaluate_script("2 ** 3;", None::<&std::path::Path>);
    match res {
        Ok(crate::Value::Number(n)) => assert_eq!(n, 8.0),
        _ => panic!("expected numeric result for 2 ** 3"),
    }

    let res2 = evaluate_script("2 ** 3 ** 2;", None::<&std::path::Path>);
    match res2 {
        Ok(crate::Value::Number(n)) => assert_eq!(n, 512.0),
        _ => panic!("expected numeric result for 2 ** 3 ** 2"),
    }

    // Numeric separators
    let res3 = evaluate_script("1_000_000 + 2000;", None::<&std::path::Path>);
    match res3 {
        Ok(crate::Value::Number(n)) => assert_eq!(n, 1_002_000.0),
        _ => panic!("expected numeric result for 1_000_000 + 2000"),
    }

    // BigInt with separators and exponentiation
    let res4 = evaluate_script("1_000n ** 2n;", None::<&std::path::Path>);
    match res4 {
        Ok(crate::Value::BigInt(s)) => assert_eq!(s.to_string(), "1000000".to_string()),
        _ => panic!("expected bigint result for 1_000n ** 2n"),
    }
}

#[test]
fn parse_accepts_eval_literal_at_declaration() {
    let script = r#"
        // script variable is a static template string that will be eval'd later
        let script = `
            class Test { #values; }
            console.log(red.#values);
        `;
        // The parser should accept the declaration and defer parsing of the
        // string literal contents until eval/runtime.
    "#;
    let res = parse_statements(&mut tokenize(script).expect("tokenize outer"));
    assert!(
        res.is_ok(),
        "Expected parser to accept static string initializer (parsing deferred until eval)"
    );
}

#[test]
fn eval_throws_at_runtime_and_is_catchable() {
    let script = r#"
    try {
        let s = "class Test { #values; } console.log(red.#values);";
        eval(s);
        throw new Error('No error thrown');
    } catch (e) {
        if (!(e instanceof SyntaxError)) {
            throw new Error('Caught error is not a SyntaxError');
        }
    }
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(
        res.is_ok(),
        "Expected script to run and catch SyntaxError at runtime, got: {:?}",
        res.err()
    );
}

#[test]
fn parse_rejects_outside_private_access() {
    let script = r#"
    class Color { #values; }
    console.log((new Color()).#values);
    "#;
    let tokens = tokenize(script).expect("tokenize failed");
    let res = parse_statements(&mut tokens.clone());
    assert!(res.is_err(), "Expected parse to fail for outside private access");
}
