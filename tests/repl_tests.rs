use javascript::Repl;
// use javascript::evaluate_script; // Commenting out unused import

#[test]
fn repl_persists_values_between_calls() {
    let repl = Repl::new();
    // define x
    let r1 = repl.eval("let x = 42;");
    assert!(r1.is_ok());
    // now retrieve
    let r2 = repl.eval("x");
    match r2 {
        Ok(javascript::Value::Number(n)) => assert_eq!(n, 42.0),
        other => panic!("Expected number 42 from repl, got {:?}", other),
    }
}

#[test]
fn repl_allows_function_persistence() {
    let repl = Repl::new();
    let _ = repl.eval("function add(a,b){ return a + b; }");
    let r = repl.eval("add(2,3)");
    match r {
        Ok(javascript::Value::Number(n)) => assert_eq!(n, 5.0),
        other => panic!("Expected number 5 from repl, got {:?}", other),
    }
}

#[cfg(test)]
mod tests {
    use javascript::is_complete_input;

    #[test]
    fn test_balanced_simple() {
        assert!(is_complete_input("1 + 1"));
        assert!(is_complete_input("let a = 10;"));
    }

    #[test]
    fn test_unbalanced_brackets() {
        assert!(!is_complete_input("(1 + 2"));
        assert!(!is_complete_input("function f() {"));
        assert!(!is_complete_input("[1, 2"));
    }

    #[test]
    fn test_strings_and_comments() {
        assert!(is_complete_input("let s = '\\'not a bracket\\'';"));
        assert!(is_complete_input("// comment with { [ ( "));
        assert!(is_complete_input("/* block comment with { [ ( */"));
        assert!(is_complete_input("'a string with } inside'"));
    }

    #[test]
    fn test_template_literals() {
        // unterminated template (missing closing backtick) -> incomplete
        assert!(!is_complete_input("`unterminated template"));
        // closed template -> complete
        assert!(is_complete_input("`unterminated template`"));
        assert!(is_complete_input("`simple`"));
        // template with expression
        assert!(is_complete_input("`a ${1 + 2} b`"));
        // incomplete template expression
        assert!(!is_complete_input("`x ${ {`"));
    }

    #[test]
    fn test_regex_handling() {
        assert!(is_complete_input("/abc/.test('x')"));
        // regex with brackets shouldn't upset brackets counting
        assert!(is_complete_input("/([a-z]{2})/g"));
        // division (not regex) combined with open paren
        assert!(!is_complete_input("(a / 1"));
    }
}
