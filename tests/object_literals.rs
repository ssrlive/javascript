use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod object_literal_tests {
    use super::*;

    #[test]
    fn test_basic_object_literal() {
        let script = "let obj = {a: 1, b: 2}; obj.a + obj.b";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_object_property_access() {
        let script = "let obj = {name: 'hello', value: 42}; obj.name";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"hello\"");
    }

    #[test]
    fn test_empty_object() {
        let script = "let empty = {}; empty";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "{}");
    }

    #[test]
    fn test_nested_object() {
        let script = "let nested = {a: {b: 1}}; nested.a.b";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "1");
    }

    #[test]
    fn test_object_with_string_keys() {
        let script = "let obj = {'key': 123}; obj.key";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "123");
    }

    #[test]
    fn test_console_log_with_object() {
        // This test verifies that console.log works with objects
        // We can't easily capture stdout in tests, so we just ensure it doesn't crash
        let script = "let obj = {test: 'value'}; console.log(obj.test); obj.test";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"value\"");
    }

    #[test]
    fn test_intentionally_failing_object() {
        let script = "let obj = {a: 1, b: 2}; obj.a + obj.b";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_getter_setter_basic() {
        let script = r#"
            let obj = {
                _value: 0,
                get value() { return this._value; },
                set value(v) { this._value = v * 2; }
            };
            obj.value = 5;
            obj.value
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "10");
    }

    #[test]
    fn test_getter_setter_with_computed_property() {
        let script = r#"
            let obj = {
                _data: {},
                get data() { return this._data; },
                set data(value) { this._data = { processed: value * 10 }; }
            };
            obj.data = 3;
            obj.data.processed
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "30");
    }

    #[test]
    fn test_concise_method_parsing() {
        let script = r#"
            let obj = { foo() { return 7; } };
            obj.foo();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "7");
    }

    #[test]
    fn test_computed_getter_parsing() {
        let script = r#"
            function CustomError() {}
            let obj = { get [Symbol.toPrimitive]() { return 42; } };
            42
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn test_object_to_string() {
        let script = r#"
            let obj = {a: 1, b: 2};
            obj.toString();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"[object Object]\"");
    }

    #[test]
    fn test_object_to_string_with_super() {
        let script = r#"
            class Base {
                toString() {
                    return "Base toString";
                }
            }
            class Derived extends Base {}
            let obj = new Derived();
            obj.toString();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"Base toString\"");
    }

    #[test]
    fn test_object_to_string_with_super_2() {
        let script = r#"
            class A {
                toString() {
                    return "A toString";
                }
            }
            class B extends A {
                toString() {
                    return "B " + super.toString();
                }
            }
            let obj = new B();
            obj.toString();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"B A toString\"");
    }

    #[test]
    fn test_object_to_string_with_super_3() {
        let script = r#"
            var obj = { toString() { return 'obj -> ' + super.toString(); } };
            return obj.toString();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"obj -> [object Object]\"");
    }

    #[test]
    fn test_object_to_string_with_super_4() {
        let script = r#"
            const proto = {
                toString() {
                    return 'proto';
                }
            };

            const obj = {
                toString() {
                    return 'obj -> ' + super.toString();
                }
            };

            Reflect.setPrototypeOf(obj, proto);

            return obj.toString();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"obj -> proto\"");
    }

    #[test]
    fn test_object_to_string_with_super_5() {
        let script = r#"
            const proto = {
                toString() {
                    return 'proto';
                }
            };

            const obj = {
                toString() {
                    return 'obj -> ' + super.toString();
                }
            };

            // Reflect.setPrototypeOf(obj, proto);

            return obj.toString();
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"obj -> [object Object]\"");
    }

    #[test]
    fn test_object_with_reference_error() {
        let script = r#"
            let obj = {
                get value() { return nonExistentVar; }
            };
            obj.value;
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Err(e) => {
                let err_msg = format!("{:?}", e);
                assert!(err_msg.contains("nonExistentVar"), "Expected nonExistentVar, got {:?}", err_msg);
            }
            _ => panic!("Expected nonExistentVar, got {:?}", result),
        }
    }

    #[test]
    fn test_object_with_reference_error_2() {
        let script = r#"
            const obj = {
                __proto__: theProtoObj,
                handler,
            };
            console.log(obj);
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Err(e) => {
                let err_msg = format!("{:?}", e);
                assert!(err_msg.contains("theProtoObj"), "Expected theProtoObj error, got {:?}", err_msg);
            }
            _ => panic!("Expected theProtoObj error, got {:?}", result),
        }
    }
}
