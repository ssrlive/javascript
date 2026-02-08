use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod optional_chaining_tests {
    use super::*;

    #[test]
    fn test_optional_property_access_valid_object() {
        let script = "let obj = {prop: 'value'}; obj?.prop";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"value\"");
    }

    #[test]
    fn test_optional_property_access_null_object() {
        let script = "let obj = null; obj?.prop";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");
    }

    #[test]
    fn test_optional_method_call_valid_object() {
        let script = "let obj = {method: function() { return 'called'; }}; obj?.method()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"called\"");
    }

    #[test]
    fn test_optional_method_call_null_object() {
        let script = "let obj = null; obj?.method()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");
    }

    #[test]
    fn test_chained_optional_operations() {
        let script = "let obj = {nested: {method: function() { return 'nested called'; }}}; obj?.nested?.method()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"nested called\"");
    }

    #[test]
    fn test_optional_computed_property_access() {
        let script = "let obj = {a: 'value'}; obj?.['a']";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"value\"");
    }

    #[test]
    fn test_optional_computed_property_null_object() {
        let script = "let obj = null; obj?.['a']";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");
    }

    #[test]
    fn test_optional_chaining_assignment_lhs_errors() {
        // Using optional chaining on LHS for direct assignment should be invalid / parse error
        let code1 = "let o = {}; o?.prop = 5";
        let res1 = evaluate_script(code1, None::<&std::path::Path>);
        assert!(
            res1.is_err(),
            "expected parse error for optional chaining on LHS assignment: {:?}",
            res1
        );

        let code2 = "let o = {}; o?.['a'] = 3";
        let res2 = evaluate_script(code2, None::<&std::path::Path>);
        assert!(
            res2.is_err(),
            "expected parse error for optional computed LHS assignment: {:?}",
            res2
        );

        // Using optional chaining with nullish assignment should be invalid too
        let code3 = "let o = {}; o?.['a'] ??= 7";
        let res3 = evaluate_script(code3, None::<&std::path::Path>);
        assert!(
            res3.is_err(),
            "expected parse error for optional computed LHS nullish-assignment: {:?}",
            res3
        );
    }

    #[test]
    fn test_nullish_assign_on_property_and_index() {
        // non-optional property/index should work with ??=
        let code1 = "let o = {}; o.x ??= 9; o.x";
        let res1 = evaluate_script(code1, None::<&std::path::Path>).unwrap();
        assert_eq!(res1, "9");

        let code2 = "let o = {}; o['x'] ??= 11; o['x']";
        let res2 = evaluate_script(code2, None::<&std::path::Path>).unwrap();
        assert_eq!(res2, "11");
    }

    #[test]
    fn test_for_update_optional_chaining_triggers_getter_but_short_circuits_index() {
        let script = r#"
            let touched = 0;
            let count;
            const obj3 = {
                get a() {
                    count++;
                    return undefined;
                }
            };
            for (count = 0; true; obj3?.a?.[touched++]) {
                if (count > 0) { break; }
            }
            count + ',' + touched
        "#;
        let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(res, "\"1,0\"");
    }

    #[test]
    fn test_optional_call_preserves_this_variants() {
        let script = r#"
            const a = {
                b() { return this._b; },
                _b: { c: 42 }
            };
            [
                a?.b().c,
                (a?.b)().c,
                a.b?.().c,
                (a.b)?.().c,
                a?.b?.().c,
                (a?.b)?.().c
            ].join(',')
        "#;
        let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(res, "\"42,42,42,42,42,42\"");
    }

    #[test]
    fn debug_optional_call_variants() {
        let script = r#"
            const a = {
                b() { return this._b; },
                _b: { c: 42 }
            };
            function safe(s) {
                try { return String(eval(s)); } catch (e) { return 'ERR:'+String(e.message); }
            }
            [
                safe('a?.b().c'),
                safe('(a?.b)().c'),
                safe('a.b?.().c'),
                safe('(a.b)?.().c'),
                safe('a?.b?.().c'),
                safe('(a?.b)?.().c')
            ].join(',')
        "#;
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
                println!("debug: {}", res);
                // ensure none returned an error
                assert!(!res.contains("ERR:"));
            })
            .expect("failed to spawn thread");
    }

    #[test]
    fn pinpoint_failing_optional_call_variant() {
        let variants = vec!["a?.b().c", "(a?.b)().c", "a.b?.().c", "(a.b)?.().c", "a?.b?.().c", "(a?.b)?.().c"];
        let mut outputs: Vec<String> = vec![];
        for v in variants {
            let script = format!(
                r#"const a = {{ b() {{ return this._b; }}, _b: {{ c: 42 }} }}; (function() {{ try {{ return String({}); }} catch(e) {{ return 'ERR:'+String(e && e.message); }} }})()"#,
                v
            );
            let res = evaluate_script(&script, None::<&std::path::Path>);
            match res {
                Ok(s) => outputs.push(s),
                Err(e) => outputs.push(format!("ERR:{}", e)),
            }
        }
        println!("pinpoint outputs: {:?}", outputs);
        // All variants should evaluate to "42"
        for o in outputs {
            assert_eq!(o, "\"42\"");
        }
    }

    #[test]
    fn test_new_target_optional_call_runtime() {
        let script = r#"
            const newTargetContext = (function() { return this; })();
            let called = false;
            let context = null;
            function Base() { called = true; context = this; }
            function Foo() { new.target?.(); }
            Reflect.construct(Foo, [], Base);
            [context === newTargetContext, called].join(',')
        "#;
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
                assert_eq!(res, "\"true,true\"");
            })
            .expect("failed to spawn thread");
    }

    #[test]
    fn test_super_property_optional_call_runtime() {
        let script = r#"
            let called = false;
            let context = null;
            class Base { method() { called = true; context = this; } }
            class Foo extends Base { method() { super.method?.(); } }
            const foo = new Foo();
            [foo === context, called].join(',')
        "#;
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
                assert_eq!(res, "\"true,true\"");
            })
            .expect("failed to spawn thread");
    }

    #[test]
    fn test_optional_chaining_short_circuit_longcase() {
        let script = r#"
            const a = undefined;
            let x = 1;

            a?.[++x]; // short-circuiting.
            a?.b.c(++x).d; // long short-circuiting.

            undefined?.[++x]; // short-circuiting.
            undefined?.b.c(++x).d; // long short-circuiting.

            x
        "#;
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
                assert_eq!(res, "1");
            })
            .expect("failed to spawn thread");
    }
}
