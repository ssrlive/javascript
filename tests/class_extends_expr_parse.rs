use javascript::{parse_statements, tokenize};

#[test]
fn class_extends_expression_parses() {
    let script = r#"
        class MyPluralRules extends Intl.PluralRules {
          constructor(locales, options) {
            super(locales, options);
          }
        }
    "#;

    let tokens = tokenize(script).expect("tokenize");
    let mut index = 0;
    let stmts = parse_statements(&tokens, &mut index).expect("parse_statements");
    // Expect a single class statement and successful parse
    assert_eq!(stmts.len(), 1);
}
