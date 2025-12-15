use javascript::{Token, parse_statement, tokenize};

#[test]
fn parse_is_canonicalized_tag() {
    let src = r#"function isCanonicalizedStructurallyValidLanguageTag(locale) {

  /**
   * Regular expression defining Unicode BCP 47 Locale Identifiers.
   *
   * Spec: https://unicode.org/reports/tr35/#Unicode_locale_identifier
   */
  var alpha = "[a-z]",
    digit = "[0-9]",
    alphanum = "[a-z0-9]",
    variant = "(" + alphanum + "{5,8}|(?:" + digit + alphanum + "{3}))",
    region = "(" + alpha + "{2}|" + digit + "{3})",
    script = "(" + alpha + "{4})",
    language = "(" + alpha + "{2,3}|" + alpha + "{5,8})",
    privateuse = "(x(-[a-z0-9]{1,8})+)",
    singleton = "(" + digit + "|[a-wy-z])",
    attribute= "(" + alphanum + "{3,8})",
    keyword = "(" + alphanum + alpha + "(-" + alphanum + "{3,8})*)",
    unicode_locale_extensions = "(u((-" + keyword + ")+|((-" + attribute + ")\n+(-" + keyword + ")*)))",
    tlang = "(" + language + "(-" + script + ")?(-" + region + ")?(-" + variant + ")*)",
    tfield = "(" + alpha + digit + "(-" + alphanum + "{3,8})+)",
    transformed_extensions = "(t((-" + tlang + "(-" + tfield + ")*)|(-" + tfield + ")+))",
    other_singleton = "(" + digit + "|[a-sv-wy-z])",
    other_extensions = "(" + other_singleton + "(-" + alphanum + "{2,8})+)",
    extension = "(" + unicode_locale_extensions + "|" + transformed_extensions + "|" + other_extensions + ")",
    locale_id = language + "(-" + script + ")?(-" + region + ")?(-" + variant + ")*(-" + extension + ")*(-" + privateuse + ")?",
    languageTag = "^(" + locale_id + ")$",
    languageTagRE = new RegExp(languageTag, "i");

  var __tagMappings = {
    "art-lojban": "jbo",
    "cel-gaulish": "xtg",
    "zh-guoyu": "zh",
    "zh-hakka": "hak",
    "zh-xiang": "hsn",
  };
}
"#;

    let tokens = tokenize(src).expect("tokenize should succeed");
    // Ensure tokenizer finds the function token
    assert!(
        tokens.iter().any(|t| matches!(t.token, Token::Function)),
        "should include a Function token"
    );

    let mut toks = tokens.clone();
    parse_statement(&mut toks).expect("parse_statement should succeed for this function snippet");
}
