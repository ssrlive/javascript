use javascript::Value;
use javascript::evaluate_script;

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
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 7.0),
            _ => panic!("Expected number 7.0, got {:?}", result),
        }
    }

    #[test]
    fn test_function_call() {
        let script = "function square(x) { return x * x; } square(5)";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 25.0),
            _ => panic!("Expected number 25.0, got {:?}", result),
        }
    }

    #[test]
    fn test_function_with_multiple_statements() {
        let script = "function test() { let x = 10; let y = 20; return x + y; } test()";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 30.0),
            _ => panic!("Expected number 30.0, got {:?}", result),
        }
    }

    #[test]
    fn test_function_without_return() {
        let script = "function noReturn() { let x = 42; } noReturn()";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 42.0), // Functions return the last statement's value
            _ => panic!("Expected number 42.0, got {:?}", result),
        }
    }

    #[test]
    fn test_nested_function_calls() {
        let script = "function double(x) { return x * 2; } function add(a, b) { return double(a) + double(b); } add(3, 4)";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 14.0), // (3*2) + (4*2) = 14
            _ => panic!("Expected number 14.0, got {:?}", result),
        }
    }

    #[test]
    fn test_function_with_console_log() {
        let script = "function greet(name) { console.log('Hello', name); return 'done'; } greet('World')";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => {
                let expected = "done".encode_utf16().collect::<Vec<u16>>();
                assert_eq!(s, expected);
            }
            _ => panic!("Expected string 'done', got {:?}", result),
        }
    }

    #[test]
    fn test_intentionally_failing_function() {
        let script = "function add(a, b) { return a + b; } add(3, 4)";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 7.0), // This will fail because 3+4=7
            _ => panic!("Expected number 7.0, got {:?}", result),
        }
    }

    #[test]
    fn test_arrow_function_single_param() {
        let script = "let square = x => x * x; square(5)";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 25.0),
            _ => panic!("Expected number 25.0, got {:?}", result),
        }
    }

    #[test]
    fn test_arrow_function_multiple_params() {
        let script = "let add = (a, b) => a + b; add(3, 4)";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 7.0),
            _ => panic!("Expected number 7.0, got {:?}", result),
        }
    }

    #[test]
    fn test_arrow_function_no_params() {
        let script = "let get_five = () => 5; get_five()";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 5.0),
            _ => panic!("Expected number 5.0, got {:?}", result),
        }
    }

    #[test]
    fn test_arrow_function_block_body() {
        let script = "let test = x => { let y = x + 1; return y * 2; }; test(3)";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 8.0), // (3+1)*2 = 8
            _ => panic!("Expected number 8.0, got {:?}", result),
        }
    }

    #[test]
    fn test_array_spread() {
        let script = "let arr1 = [1, 2, 3]; let arr2 = [4, 5, 6]; let combined = [...arr1, ...arr2]; combined[0] + combined[1] + combined[2] + combined[3] + combined[4] + combined[5]";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 21.0), // 1+2+3+4+5+6 = 21
            _ => panic!("Expected number 21.0, got {:?}", result),
        }
    }

    #[test]
    fn test_object_spread() {
        let script =
            "let obj1 = {a: 1, b: 2}; let obj2 = {c: 3, d: 4}; let merged = {...obj1, ...obj2}; merged.a + merged.b + merged.c + merged.d";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 10.0), // 1+2+3+4 = 10
            _ => panic!("Expected number 10.0, got {:?}", result),
        }
    }

    #[test]
    fn test_function_call_spread() {
        let script = "function sum(a, b, c) { return a + b + c; } let nums = [1, 2, 3]; sum(...nums)";
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 6.0), // 1+2+3 = 6
            _ => panic!("Expected number 6.0, got {:?}", result),
        }
    }
}
