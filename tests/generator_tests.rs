use javascript::{Value, evaluate_script};

#[test]
fn test_generator_function_syntax() {
    // Test basic generator function syntax parsing
    let result = evaluate_script(
        r#"
        function* gen() {
            yield 1;
            yield 2;
            return 3;
        }
        typeof gen;
    "#,
    );
    assert!(result.is_ok());
    match result.unwrap() {
        Value::String(s) => assert_eq!(String::from_utf16_lossy(&s), "function"),
        _ => panic!("Expected string 'function'"),
    }
}

#[test]
fn test_generator_function_call() {
    // Test calling a generator function returns a generator object
    let result = evaluate_script(
        r#"
        function* gen() {
            yield 42;
        }
        var g = gen();
        typeof g;
    "#,
    );
    assert!(result.is_ok());
    // Should return "object" for generator object
    match result.unwrap() {
        Value::String(s) => assert_eq!(String::from_utf16_lossy(&s), "object"),
        _ => panic!("Expected string 'object'"),
    }
}

#[test]
fn test_generator_next() {
    // Test generator.next() method
    let result = evaluate_script(
        r#"
        function* gen() {
            yield 42;
        }
        var g = gen();
        var result = g.next();
        result.value;
    "#,
    );
    assert!(result.is_ok());
    match result.unwrap() {
        Value::Number(n) => assert_eq!(n, 42.0),
        _ => panic!("Expected number 42.0"),
    }
}

#[test]
fn test_generator_done() {
    // Test generator completion
    let result = evaluate_script(
        r#"
        function* gen() {
            yield 42;
        }
        var g = gen();
        g.next(); // first call
        var result = g.next(); // second call should be done
        result.done;
    "#,
    );
    assert!(result.is_ok());
    match result.unwrap() {
        Value::Boolean(b) => assert!(b),
        _ => panic!("Expected boolean true"),
    }
}

#[test]
fn test_yield_without_generator() {
    // Test that yield outside generator throws error
    let result = evaluate_script(
        r#"
        function regular() {
            yield 42;
        }
        regular();
    "#,
    );
    assert!(result.is_err());
}
