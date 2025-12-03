use javascript::{Token, parse_statement, tokenize};

#[test]
fn parse_regex_in_call_arg() {
    let src = "locale = locale.split(/-x-/)[0];";
    let tokens = tokenize(src).expect("tokenize should succeed");

    // Ensure tokenizer recognizes a regex literal
    assert!(tokens.iter().any(|t| matches!(t, Token::Regex(_, _))), "should have a Regex token");

    // Parsing the full statement should succeed
    let mut toks = tokens.clone();
    parse_statement(&mut toks).expect("parse_statement should succeed for assignment with regex literal");
}
