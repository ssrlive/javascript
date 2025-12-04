use javascript::*;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn multiple_var_declarations_without_initializers() {
    let script = "var a, b; a = 1; b = 2; a + b";
    let result = evaluate_script(script);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 3.0),
        _ => panic!("Expected number 3.0, got {:?}", result),
    }
}

#[test]
fn skip_empty_semicolons_and_let() {
    let script = ";; let x = 5; ; x";
    let result = evaluate_script(script);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 5.0),
        _ => panic!("Expected number 5.0, got {:?}", result),
    }
}

#[test]
fn single_line_and_block_comments_ignored() {
    let script = "// leading comment\n/* block comment */ let x = 7; x";
    let result = evaluate_script(script);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 7.0),
        _ => panic!("Expected number 7.0, got {:?}", result),
    }
}

#[test]
fn trailing_comma_and_newline_before_rbrace_is_allowed() {
    // tokens for: { \n seconds = 0, \n }
    let mut tokens = vec![
        Token::LBrace,
        Token::LineTerminator,
        Token::Identifier("seconds".to_string()),
        Token::Assign,
        Token::Number(0.0),
        Token::Comma,
        Token::LineTerminator,
        Token::RBrace,
    ];

    let pattern = parse_object_destructuring_pattern(&mut tokens).expect("should parse pattern");
    // pattern should contain one property
    assert_eq!(pattern.len(), 1);
    // and the tokens left should be empty
    assert!(tokens.is_empty());
}
