use javascript::*;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn multiple_var_declarations_without_initializers() {
    let script = "var a, b; a = 1; b = 2; a + b";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "3");
}

#[test]
fn skip_empty_semicolons_and_let() {
    let script = ";; let x = 5; ; x";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "5");
}

#[test]
fn single_line_and_block_comments_ignored() {
    let script = "// leading comment\n/* block comment */ let x = 7; x";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "7");
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
    let tokens: Vec<javascript::TokenData> = raw_tokens
        .into_iter()
        .map(|t| javascript::TokenData {
            token: t,
            line: 0,
            column: 0,
        })
        .collect();

    let mut index = 0;
    let pattern = parse_object_destructuring_pattern(&tokens, &mut index).expect("should parse pattern");
    // pattern should contain one property
    assert_eq!(pattern.len(), 1);
    // and the parser should have consumed all tokens (index at end)
    assert_eq!(index, tokens.len());
}

#[test]
fn exponentiation_and_numeric_separators_supported() {
    // Exponentiation for numbers
    let res = evaluate_script("2 ** 3;", None::<&std::path::Path>).unwrap();
    assert_eq!(res, "8");

    let res2 = evaluate_script("2 ** 3 ** 2;", None::<&std::path::Path>).unwrap();
    assert_eq!(res2, "512");

    // Numeric separators
    let res3 = evaluate_script("1_000_000 + 2000;", None::<&std::path::Path>).unwrap();
    assert_eq!(res3, "1002000");

    // BigInt with separators and exponentiation
    let res4 = evaluate_script("1_000n ** 2n;", None::<&std::path::Path>).unwrap();
    assert_eq!(res4, "1000000");
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
    let mut index = 0;
    let res = parse_statements(&tokenize(script).expect("tokenize outer"), &mut index);
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
    let mut index = 0;
    let res = parse_statements(&tokens.clone(), &mut index);
    assert!(res.is_err(), "Expected parse to fail for outside private access");
}

#[test]
fn parse_addition_object_plus_function_expression() {
    // Reproducer for c2.js CHECK#1: ensure parser accepts ({} + function(){...})
    let script = r#"
    if (({} + function(){return 1}) !== ({}.toString() + function(){return 1}.toString())) {
      throw new Error('Parse or runtime mismatch');
    }
    "#;
    let tokens = tokenize(script).expect("tokenize failed");
    // Print tokens to help debug why the parser rejects this case
    for (i, t) in tokens.iter().enumerate() {
        eprintln!("{}: {:?} (line {}, col {})", i, t.token, t.line, t.column);
    }
    let mut index = 0usize;
    let res = parse_statements(&tokens, &mut index);
    if let Err(err) = &res {
        eprintln!("parse error: {:?}", err);
    }
    assert!(
        res.is_ok(),
        "Expected parser to accept object + function expression: {:?}",
        res.err()
    );
}

#[test]
fn parse_inner_object_plus_function_expr_alone() {
    let script = "({}.toString() + function(){return 1}.toString());";
    let tokens = tokenize(script).expect("tokenize failed");
    for (i, t) in tokens.iter().enumerate() {
        eprintln!("{}: {:?} (line {}, col {})", i, t.token, t.line, t.column);
    }
    let mut index = 0usize;
    let res = parse_statements(&tokens, &mut index);
    if let Err(err) = &res {
        eprintln!("parse error stmt-only: {:?}", err);
    }
    assert!(
        res.is_ok(),
        "Expected parsing inner expression statement to succeed: {:?}",
        res.err()
    );
}

#[test]
fn bigint_in_object_and_class_and_destructuring() {
    // Object literal method using BigInt property name
    let res_obj = evaluate_script("let o = { 1n() { return 'bar'; } }; o['1']();", None::<&std::path::Path>).unwrap();
    // evaluate_script returns JS values using JS's string representation (with quotes)
    assert_eq!(res_obj, "\"bar\"");

    // Class method using BigInt property name
    let res_class = evaluate_script(
        "class C { 1n() { return 'baz'; } } let c = new C(); c['1']();",
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(res_class, "\"baz\"");

    // Destructuring with BigInt property name
    let tokens = vec![
        Token::LBrace,
        Token::BigInt("1".to_string()),
        Token::Colon,
        Token::Identifier("a".to_string()),
        Token::RBrace,
    ];
    let token_data: Vec<javascript::TokenData> = tokens
        .into_iter()
        .map(|t| javascript::TokenData {
            token: t,
            line: 0,
            column: 0,
        })
        .collect();
    let mut idx = 0usize;
    let pattern = parse_object_destructuring_pattern(&token_data, &mut idx).expect("should parse bigint key in destructuring");
    assert_eq!(pattern.len(), 1);
}
