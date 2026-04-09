use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod function_tests {
    use super::*;

    #[test]
    fn test_function_definition() {
        let script = "function add(a, b) { return a + b; } add(3, 4)";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "7");
    }

    #[test]
    fn test_function_call() {
        let script = "function square(x) { return x * x; } square(5)";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "25");
    }

    #[test]
    fn test_function_with_multiple_statements() {
        let script = "function test() { let x = 10; let y = 20; return x + y; } test()";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "30");
    }

    #[test]
    fn test_function_without_return() {
        let script = "function noReturn() { let x = 42; } noReturn()";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");
    }

    #[test]
    fn test_named_evaluation_sets_name_flags() {
        let script = r#"
            var xFn;
            xFn = function () {};
            var d = Object.getOwnPropertyDescriptor(xFn, 'name');
            [d.value === "xFn", d.writable, d.enumerable, d.configurable].toString();
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"true,false,false,true\"");
    }

    #[test]
    fn test_function_length_non_writable() {
        let script = r#"
            "use strict";
            (function() {
                try {
                    Function.length = 42;
                    return 'NO THROW';
                } catch (e) {
                    return 'THROW ' + (e.name || e);
                }
            })();
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"THROW TypeError\"");
    }

    #[test]
    fn test_arrow_named_evaluation_sets_name_flags() {
        let script = r#"
            var arrow;
            arrow = () => {};
            var d = Object.getOwnPropertyDescriptor(arrow, 'name');
            [d.value === "arrow", d.writable, d.enumerable, d.configurable].toString();
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"true,false,false,true\"");
    }

    #[test]
    fn test_eval_strict_arguments_callee_descriptor() {
        let script = r#"
            let d = eval('(function(){ "use strict"; return Object.getOwnPropertyDescriptor(arguments, "callee"); })()');
            d !== undefined &&
            typeof d.get === "function" &&
            d.enumerable === false &&
            d.configurable === false
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_new_function_strict_arguments_callee_getter() {
        let script = r#"
            let f = new Function('return (function(){ "use strict"; return Object.getOwnPropertyDescriptor(arguments, "callee").get; })()');
            typeof f() === "function"
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_strict_arguments_thrower_is_reflectable_function_object() {
        let script = r#"
            let thrower = (function(){ "use strict"; return Object.getOwnPropertyDescriptor(arguments, "callee").get; })();
            let descs = Object.getOwnPropertyDescriptors(thrower);
            descs.length.value === 0 &&
            descs.length.writable === false &&
            descs.name.value === ""
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_function_constructor_does_not_expose_origin_global_slot() {
        let script = r#"
            !Object.getOwnPropertyNames(Function).includes("__origin_global")
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_simple_object_method_to_string_drives_computed_property_keys() {
        let script = r#"
            let method = ({ a(){} }).a;
            typeof ({ [method](){ } })["a(){}"] === "function"
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_function_constructor_coerces_body_via_tostring() {
        let script = r#"
            let f = new Function({ toString() { return "return 1;"; } });
            f() === 1
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_function_constructor_coerces_params_via_tostring() {
        let script = r#"
            let param = { toString() { return "a1"; } };
            let f = new Function(param, "return a1;");
            f(42) === 42
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_function_constructor_preserves_tostring_throw_value() {
        let script = r#"
            try {
                new Function({ toString() { throw 7; } });
                false
            } catch (e) {
                e === 7
            }
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_function_constructor_rejects_invalid_body_after_tostring() {
        let script = r#"
            try {
                new Function({});
                false
            } catch (e) {
                e instanceof SyntaxError
            }
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_function_apply_reads_array_like_arguments() {
        let script = r#"
            (function() {
                function f(a, b, c) {
                    return a + b + c;
                }
                return f.apply(null, arguments) === "abc";
            })("a", "b", "c")
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_function_apply_rejects_primitive_argarray() {
        let script = r#"
            try {
                (function() {}).apply(null, true);
                false
            } catch (e) {
                e instanceof TypeError
            }
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_cross_realm_function_apply_uses_foreign_type_errors() {
        let script = r#"
            let other = __createRealm__().global;
            let otherApply = other.Function.prototype.apply;
            let otherFn = other.Function();
            let protoOk = otherFn.__proto__ === other.Function.prototype;
            let callWorks = otherApply.call(otherFn, null, []) === undefined;
            let thisNotCallable = false;
            let badArgArray = false;
            try {
                otherApply.call(undefined, {}, []);
            } catch (e) {
                thisNotCallable = e.constructor === other.TypeError && e.constructor !== TypeError;
            }
            try {
                otherFn.apply(null, true);
            } catch (e) {
                badArgArray = e.constructor === other.TypeError && e.constructor !== TypeError;
            }
            otherApply !== Function.prototype.apply && protoOk && callWorks && thisNotCallable && badArgArray
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_function_call_and_bind_require_callable_receiver() {
        let script = r#"
            let callThrows = false;
            let bindThrows = false;
            try {
                Function.prototype.call.call(undefined, {});
            } catch (e) {
                callThrows = e instanceof TypeError;
            }
            try {
                Function.prototype.bind.call(undefined, {});
            } catch (e) {
                bindThrows = e instanceof TypeError;
            }
            callThrows && bindThrows
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_function_builtin_metadata_and_property_order() {
        let script = r#"
            let applyName = Object.getOwnPropertyDescriptor(Function.prototype.apply, "name");
            let applyLength = Object.getOwnPropertyDescriptor(Function.prototype.apply, "length");
            let callName = Object.getOwnPropertyDescriptor(Function.prototype.call, "name");
            let props = Object.getOwnPropertyNames(Function);
            applyName.value === "apply" &&
            applyName.writable === false &&
            applyName.enumerable === false &&
            applyName.configurable === true &&
            applyLength.value === 2 &&
            applyLength.writable === false &&
            applyLength.enumerable === false &&
            applyLength.configurable === true &&
            callName.value === "call" &&
            props.indexOf("length") >= 0 &&
            props.indexOf("name") === props.indexOf("length") + 1
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_function_prototype_metadata_and_restricted_property_descriptors() {
        let script = r#"
            let nameDesc = Object.getOwnPropertyDescriptor(Function.prototype, "name");
            let lengthDesc = Object.getOwnPropertyDescriptor(Function.prototype, "length");
            let argumentsDesc = Object.getOwnPropertyDescriptor(Function.prototype, "arguments");
            let callerDesc = Object.getOwnPropertyDescriptor(Function.prototype, "caller");
            let thrower = (function(){ "use strict"; return Object.getOwnPropertyDescriptor(arguments, "callee").get; })();
            let evalThrower = eval('(function() { "use strict"; return Object.getOwnPropertyDescriptor(arguments, "callee").get })()');
            let argumentsThrow = false;
            let callerThrow = false;
            let restoreArgumentsThrow = false;
            let restoreCallerThrow = false;
            try { Function.prototype.arguments; } catch (e) { argumentsThrow = e instanceof TypeError; }
            try { Function.prototype.caller; } catch (e) { callerThrow = e instanceof TypeError; }
            Object.defineProperty(Function.prototype, "arguments", {
                enumerable: false,
                configurable: true,
                get: argumentsDesc.get,
                set: argumentsDesc.set
            });
            Object.defineProperty(Function.prototype, "caller", {
                enumerable: false,
                configurable: true,
                get: callerDesc.get,
                set: callerDesc.set
            });
            try { Function.prototype.arguments; } catch (e) { restoreArgumentsThrow = e instanceof TypeError; }
            try { Function.prototype.caller; } catch (e) { restoreCallerThrow = e instanceof TypeError; }
            nameDesc.value === "" &&
            nameDesc.writable === false &&
            nameDesc.enumerable === false &&
            nameDesc.configurable === true &&
            lengthDesc.value === 0 &&
            lengthDesc.writable === false &&
            lengthDesc.enumerable === false &&
            lengthDesc.configurable === true &&
            typeof argumentsDesc.get === "function" &&
            argumentsDesc.get === argumentsDesc.set &&
            argumentsDesc.enumerable === false &&
            argumentsDesc.configurable === true &&
            argumentsDesc.get === thrower &&
            argumentsDesc.get === evalThrower &&
            typeof callerDesc.get === "function" &&
            callerDesc.get === callerDesc.set &&
            callerDesc.enumerable === false &&
            callerDesc.configurable === true &&
            callerDesc.get === thrower &&
            callerDesc.get === evalThrower &&
            argumentsThrow &&
            callerThrow &&
            restoreArgumentsThrow &&
            restoreCallerThrow
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_bound_native_constructor_targets_call_like_functions() {
        let script = r#"
            Number.bind(null)(42) === 42 &&
            String.bind(null)("hello") === "hello" &&
            Boolean.bind(null)(true) === true &&
            Object.bind(null)(42) == 42 &&
            Array.bind(null)(3).length === 3 &&
            typeof Date.bind(null)(0, 0, 0) === "string" &&
            typeof Function.prototype.call.call(Date, null, 0, 0, 0) === "string" &&
            typeof Function.prototype.apply.call(Date, null, [0, 0, 0]) === "string"
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_bound_function_name_length_and_restricted_props() {
        let script = r#"
            function target(a, b) {}
            Object.defineProperty(target, "name", { value: "target" });
            let bound = target.bind(null, 1);
            let chained = bound.bind(null);
            function weird() {}
            Object.defineProperty(weird, "name", { value: 123 });
            let weirdBound = weird.bind(null);
            let nameDesc = Object.getOwnPropertyDescriptor(bound, "name");
            let lengthDesc = Object.getOwnPropertyDescriptor(bound, "length");
            let callerThrows = false;
            let argumentsThrows = false;
            try { bound.caller; } catch (e) { callerThrows = e instanceof TypeError; }
            try { bound.arguments; } catch (e) { argumentsThrows = e instanceof TypeError; }
            bound.name === "bound target" &&
            chained.name === "bound bound target" &&
            weirdBound.name === "bound " &&
            bound.length === 1 &&
            nameDesc.writable === false &&
            nameDesc.enumerable === false &&
            nameDesc.configurable === true &&
            lengthDesc.writable === false &&
            lengthDesc.enumerable === false &&
            lengthDesc.configurable === true &&
            callerThrows &&
            argumentsThrows
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_bind_propagates_name_getter_errors() {
        let script = r#"
            function target() {}
            Object.defineProperty(target, "name", {
                get() { throw 42; }
            });
            let threw = false;
            try {
                target.bind(null);
            } catch (e) {
                threw = e === 42;
            }
            threw
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_bind_length_ignores_inherited_length() {
        let script = r#"
            function bar() {}
            Object.setPrototypeOf(bar, { length: 42 });
            delete bar.length;
            Function.prototype.bind.call(bar, null, 1).length === 0
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_bound_constructor_preserves_explicit_wrapper_returns() {
        let script = r#"
            function makeBool() {
                return new Boolean(arguments.length === 1 && arguments[0] === true);
            }
            let Bound = makeBool.bind(null, true);
            let out = new Bound();
            Object.getPrototypeOf(out) === Boolean.prototype &&
            out.valueOf() === true
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_derived_constructor_retargets_wrapper_prototype() {
        let script = r#"
            class DerivedBoolean extends Boolean {
                constructor(value) {
                    super(value);
                }
            }
            let out = new DerivedBoolean(true);
            Object.getPrototypeOf(out) === DerivedBoolean.prototype &&
            out.valueOf() === true
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_nested_function_calls() {
        let script = "function double(x) { return x * 2; } function add(a, b) { return double(a) + double(b); } add(3, 4)";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "14"); // (3*2) + (4*2) = 6 + 8 = 14
    }

    #[test]
    fn test_function_with_console_log() {
        let script = "function greet(name) { console.log('Hello', name); return 'done'; } greet('World')";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"done\"");
    }

    #[test]
    fn test_intentionally_failing_function() {
        let script = "function add(a, b) { return a + b; } add(3, 4)";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "7");
    }

    #[test]
    fn test_arrow_function_single_param() {
        let script = "let square = x => x * x; square(5)";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "25");
    }

    #[test]
    fn test_arrow_function_multiple_params() {
        let script = "let add = (a, b) => a + b; add(3, 4)";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "7");
    }

    #[test]
    fn test_arrow_function_no_params() {
        let script = "let get_five = () => 5; get_five()";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "5");
    }

    #[test]
    fn test_arrow_function_block_body() {
        let script = "let test = x => { let y = x + 1; return y * 2; }; test(3)";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "8");
    }

    #[test]
    fn test_array_spread() {
        let script = "let arr1 = [1, 2, 3]; let arr2 = [4, 5, 6]; let combined = [...arr1, ...arr2]; combined[0] + combined[1] + combined[2] + combined[3] + combined[4] + combined[5]";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "21");
    }

    #[test]
    fn test_object_spread() {
        let script =
            "let obj1 = {a: 1, b: 2}; let obj2 = {c: 3, d: 4}; let merged = {...obj1, ...obj2}; merged.a + merged.b + merged.c + merged.d";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "10");
    }

    #[test]
    fn test_function_call_spread() {
        let script = "function sum(a, b, c) { return a + b + c; } let nums = [1, 2, 3]; sum(...nums)";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_default_param_comma_operator() {
        let script = "function g(a = (1,2), b = 4) { return a + b; } g()";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_function_declaration_hoisting() {
        let script = "hoistedFunction(); function hoistedFunction() { return 42; }";
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    // #[ignore = "Known issue: some function is so huge that it causes stack overflow in current implementation"]
    fn test_named_function_expression() {
        let script = r#"
            // Named Function Expressions (NFE).
            const factorial = function fac(n) {
                return n < 2 ? 1 : n * fac(n - 1);
            };

            console.log(factorial(3));

            factorial(8)
        "#;
        // Run evaluation on a dedicated thread with increased stack to avoid stack overflow
        let handle = std::thread::Builder::new()
            .name("named_function_expression".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
                assert_eq!(result, "40320"); // 8! = 40320
            })
            .unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn test_function_expression_as_immediate_argument() {
        let script = r#"
            function assert(condition, message) {
                if (!condition) {
                    throw new Error(message || "Assertion failed");
                }
            }

            function map(f, a) {
                const result = new Array(a.length);
                for (let i = 0; i < a.length; i++) {
                    result[i] = f(a[i]);
                }
                return result;
            }

            const cube = function (x) {
                return x * x * x;
            };

            const numbers = [0, 1, 2, 5, 10];
            var result = map(cube, numbers);
            console.log(result); // [0, 1, 8, 125, 1000]
            assert(
                JSON.stringify(result) === JSON.stringify([0, 1, 8, 125, 1000]),
                "Higher-order function map is not working correctly"
            );
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");
    }

    #[test]
    fn test_dymamic_function_creation() {
        let script = r#"
            let myFunc;
            let car = { make: "Honda", model: "Accord", year: 1998 };
            let num = 0;
            if (num === 0) {
                myFunc = function (theObject) {
                    theObject.make = "Toyota";
                };
            }
            myFunc(car);
            return car.make;
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"Toyota\"");
    }

    #[test]
    fn test_function_scopes_and_closures() {
        let script = r#"
            const _name = "Chamakh";
            // A nested function example
            function getScore() {
                const num1 = 2;
                const num2 = 3;

                function add() {
                    return `${_name} scored ${num1 + num2}`;
                }

                return add();
            }

            return getScore(); // "Chamakh scored 5"
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"Chamakh scored 5\"");
    }

    #[test]
    fn test_function_with_iterators() {
        let script = r#"
            let node = {
              nodeName: "DIV",
              childNodes: [
                {
                  nodeName: "SPAN",
                  childNodes: [],
                },
                {
                  nodeName: "A",
                  childNodes: [
                    {
                      nodeName: "IMG",
                      childNodes: [],
                    },
                  ],
                },
              ],
            };

            function walkTree(node) {
              if (node === null) {
                return;
              }
              // do something with node
              console.log(node.nodeName);
              for (let i = 0; i < node.childNodes.length; i++) {
                walkTree(node.childNodes[i]);
              }
            }

            walkTree(node);
        "#;
        std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
                assert_eq!(result, "undefined");
            })
            .expect("failed to spawn thread")
            .join()
            .expect("thread panicked");
    }

    #[test]
    fn test_arguments_object() {
        let script = r#"
            function sum() {
                let total = 0;
                for (let i = 0; i < arguments.length; i++) {
                    total += arguments[i];
                }
                return total;
            }
            sum(1, 2, 3, 4, 5)
        "#;
        let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "15");
    }
}
