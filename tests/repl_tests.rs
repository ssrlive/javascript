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
