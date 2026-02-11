use javascript::evaluate_script;

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
    )
    .unwrap();
    assert_eq!(result, "\"function\"");
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
    )
    .unwrap();
    assert_eq!(result, "\"object\"");
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
    )
    .unwrap();
    assert_eq!(result, "42");
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
    )
    .unwrap();
    assert_eq!(result, "true");
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
    )
    .unwrap();
    assert_eq!(result, "123");
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
    )
    .unwrap();
    assert_eq!(result, "99");
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

#[test]
fn test_generator_method_prototype_links_to_intrinsic() {
    // Test that generator methods defined on object literals have a 'prototype'
    // object whose internal prototype points to the realm's Generator.prototype
    let result = evaluate_script(
        r#"
        var GeneratorPrototype = Object.getPrototypeOf(function* () {}).prototype;
        var method = { *method() {} }.method;
        Object.getPrototypeOf(method.prototype) === GeneratorPrototype;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_yield_as_expression_without_rhs_g1() {
    let v = evaluate_script(
        r#"
        var obj = { *g1() { (yield) } };
        var iter = obj.g1();
        var result = iter.next();
        result.value;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(v, "undefined");

    let d = evaluate_script(
        r#"
        var obj = { *g1() { (yield) } };
        var iter = obj.g1();
        var _ = iter.next();
        var result = iter.next();
        result.done;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(d, "true");
}

#[test]
fn test_yield_as_expression_without_rhs_g2() {
    let v = evaluate_script(
        r#"
        var obj = { *g2() { [yield] } };
        var iter = obj.g2();
        var result = iter.next();
        result.value;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(v, "undefined");

    let d = evaluate_script(
        r#"
        var obj = { *g2() { [yield] } };
        var iter = obj.g2();
        var _ = iter.next();
        var result = iter.next();
        result.done;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(d, "true");
}

#[test]
fn test_yield_as_expression_without_rhs_g3() {
    let v = evaluate_script(
        r#"
        var obj = { *g3() { {yield} } };
        var iter = obj.g3();
        var result = iter.next();
        result.value;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(v, "undefined");

    let d = evaluate_script(
        r#"
        var obj = { *g3() { {yield} } };
        var iter = obj.g3();
        var _ = iter.next();
        var result = iter.next();
        result.done;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(d, "true");
}

#[test]
fn test_yield_as_expression_without_rhs_g4() {
    // comma expression: two yields before completion
    let v1 = evaluate_script(
        r#"
        var obj = { *g4() { yield, yield; } };
        var iter = obj.g4();
        var result = iter.next();
        result.value;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(v1, "undefined");

    let v2 = evaluate_script(
        r#"
        var obj = { *g4() { yield, yield; } };
        var iter = obj.g4();
        iter.next();
        var result = iter.next();
        result.value;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(v2, "undefined");

    let d = evaluate_script(
        r#"
        var obj = { *g4() { yield, yield; } };
        var iter = obj.g4();
        iter.next();
        iter.next();
        var result = iter.next();
        result.done;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(d, "true");
}

#[test]
fn test_yield_as_expression_without_rhs_g5() {
    // conditional operator with bare yields
    let v1 = evaluate_script(
        r#"
        var obj = { *g5() { (yield) ? yield : yield; } };
        var iter = obj.g5();
        var result = iter.next();
        result.value;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(v1, "undefined");

    let v2 = evaluate_script(
        r#"
        var obj = { *g5() { (yield) ? yield : yield; } };
        var iter = obj.g5();
        iter.next();
        var result = iter.next();
        result.value;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(v2, "undefined");

    let d = evaluate_script(
        r#"
        var obj = { *g5() { (yield) ? yield : yield; } };
        var iter = obj.g5();
        iter.next();
        iter.next();
        var result = iter.next();
        result.done;
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();
    assert_eq!(d, "true");
}
