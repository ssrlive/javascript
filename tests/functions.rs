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
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "7");
    }

    #[test]
    fn test_function_call() {
        let script = "function square(x) { return x * x; } square(5)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "25");
    }

    #[test]
    fn test_function_with_multiple_statements() {
        let script = "function test() { let x = 10; let y = 20; return x + y; } test()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "30");
    }

    #[test]
    fn test_function_without_return() {
        let script = "function noReturn() { let x = 42; } noReturn()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");
    }

    #[test]
    fn test_nested_function_calls() {
        let script = "function double(x) { return x * 2; } function add(a, b) { return double(a) + double(b); } add(3, 4)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "14"); // (3*2) + (4*2) = 6 + 8 = 14
    }

    #[test]
    fn test_function_with_console_log() {
        let script = "function greet(name) { console.log('Hello', name); return 'done'; } greet('World')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"done\"");
    }

    #[test]
    fn test_intentionally_failing_function() {
        let script = "function add(a, b) { return a + b; } add(3, 4)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "7");
    }

    #[test]
    fn test_arrow_function_single_param() {
        let script = "let square = x => x * x; square(5)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "25");
    }

    #[test]
    fn test_arrow_function_multiple_params() {
        let script = "let add = (a, b) => a + b; add(3, 4)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "7");
    }

    #[test]
    fn test_arrow_function_no_params() {
        let script = "let get_five = () => 5; get_five()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "5");
    }

    #[test]
    fn test_arrow_function_block_body() {
        let script = "let test = x => { let y = x + 1; return y * 2; }; test(3)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "8");
    }

    #[test]
    fn test_array_spread() {
        let script = "let arr1 = [1, 2, 3]; let arr2 = [4, 5, 6]; let combined = [...arr1, ...arr2]; combined[0] + combined[1] + combined[2] + combined[3] + combined[4] + combined[5]";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "21");
    }

    #[test]
    fn test_object_spread() {
        let script =
            "let obj1 = {a: 1, b: 2}; let obj2 = {c: 3, d: 4}; let merged = {...obj1, ...obj2}; merged.a + merged.b + merged.c + merged.d";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "10");
    }

    #[test]
    fn test_function_call_spread() {
        let script = "function sum(a, b, c) { return a + b + c; } let nums = [1, 2, 3]; sum(...nums)";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_default_param_comma_operator() {
        let script = "function g(a = (1,2), b = 4) { return a + b; } g()";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_function_declaration_hoisting() {
        let script = "hoistedFunction(); function hoistedFunction() { return 42; }";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
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
                let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
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
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
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
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
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
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
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
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "undefined");
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
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "15");
    }
}
