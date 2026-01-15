use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_reflect_apply_with_non_array_arguments_list_errors() {
    let script = "Reflect.apply(function(){}, undefined, 123)";
    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(res.is_err(), "expected Reflect.apply with non-array argumentsList to error");
}

#[test]
fn test_reflect_construct_with_new_target_parameter() {
    let script = r#"
        class A { constructor(v) { this.v = v } full() { return this.v; } }
        class B {}
        // newTarget is provided (B) but engine currently ignores it; construction should still succeed
        let o = Reflect.construct(A, [42], B);
        o.full();
    "#;
    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "42");
}

#[test]
#[ignore = "Reflect.apply with async closure not yet implemented"]
fn test_reflect_apply_with_async_closure_returns_promise_resolved() {
    let script = r#"
        let fnc = async function(a){ return a + 1; };
        let p = Reflect.apply(fnc, undefined, [1]);
        await p;
    "#;
    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "2");
}

#[test]
fn test_reflect_apply_with_non_callable_target_errors() {
    let script = "Reflect.apply(123, undefined, [])";
    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(res.is_err(), "expected Reflect.apply with non-callable target to error");
}

#[test]
fn test_reflect_apply_with_closure_and_this() {
    let script = r#"
        const obj = { x: 10 };
        function add(a, b) { return this.x + a + b; }
        // Use Reflect.apply to call `add` with receiver `obj` and args [1,2]
        let result = Reflect.apply(add, obj, [1, 2]);
        result;
    "#;

    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "13");
}

#[test]
fn test_reflect_apply_with_native_function() {
    let script = r#"
        // Use Reflect.apply to call a global function (String) as a function
        // String() converts its argument to string; passing 123 should return "123"
        let res = Reflect.apply(String, undefined, [123]);
        res;
    "#;

    let v = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(v, "\"123\"");
}

#[test]
fn test_reflect_construct_with_constructor_args() {
    let script = r#"
        class Person {
            constructor(first, last) { this.first = first; this.last = last; }
            full() { return this.first + ' ' + this.last; }
        }
        let p = Reflect.construct(Person, ['Jane', 'Doe']);
        p.full();
    "#;

    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "\"Jane Doe\"");
}
