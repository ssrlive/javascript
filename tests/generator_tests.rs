use javascript::{Value, evaluate_script, utf16_to_utf8};

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
        None::<&std::path::Path>,
    );
    assert!(result.is_ok());
    match result.unwrap() {
        Value::String(s) => assert_eq!(utf16_to_utf8(&s), "function"),
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
        None::<&std::path::Path>,
    );
    assert!(result.is_ok());
    // Should return "object" for generator object
    match result.unwrap() {
        Value::String(s) => assert_eq!(utf16_to_utf8(&s), "object"),
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
        None::<&std::path::Path>,
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
        None::<&std::path::Path>,
    );
    assert!(result.is_ok());
    match result.unwrap() {
        Value::Boolean(b) => assert!(b),
        _ => panic!("Expected boolean true"),
    }
}

#[test]
fn test_generator_next_with_value() {
    // Test sending a value back into a generator via next(value)
    let result = evaluate_script(
        r#"
        function* gen() {
            let x = yield 1;
            return x;
        }
        var g = gen();
        g.next();
        var r = g.next(123);
        r.value;
    "#,
        None::<&std::path::Path>,
    );
    assert!(result.is_ok());
    match result.unwrap() {
        Value::Number(n) => assert_eq!(n, 123.0),
        _ => panic!("Expected number 123.0"),
    }
}

#[test]
fn test_generator_throw_caught() {
    let result = evaluate_script(
        r#"
        function* gen() {
            try {
                yield 1;
            } catch (e) {
                return e;
            }
        }
        var g = gen();
        g.next();
        var r = g.throw(99);
        r.value;
    "#,
        None::<&std::path::Path>,
    );
    assert!(result.is_ok());
    match result.unwrap() {
        Value::Number(n) => assert_eq!(n, 99.0),
        _ => panic!("Expected number 99.0"),
    }
}

#[test]
fn test_generator_throw_uncaught() {
    let result = evaluate_script(
        r#"
        function* gen() {
            yield 1;
        }
        var g = gen();
        g.next();
        g.throw(99);
    "#,
        None::<&std::path::Path>,
    );
    assert!(result.is_err());
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
        None::<&std::path::Path>,
    );
    assert!(result.is_err());
}
