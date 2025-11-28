use javascript::core::evaluate_script;
use javascript::core::Value;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod std_tests {
    use super::*;

    #[test]
    fn test_sprintf() {
        let script = "import * as std from 'std'; std.sprintf('a=%d s=%s', 123, 'abc')";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let out = String::from_utf16_lossy(&s);
                assert_eq!(out, "a=123 s=abc");
            }
            _ => panic!("Expected formatted string, got {:?}", result),
        }
    }

    #[test]
    fn test_tmpfile_puts_read() {
        let script = "import * as std from 'std'; let f = std.tmpfile(); f.puts('hello'); f.readAsString();";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let out = String::from_utf16_lossy(&s);
                assert_eq!(out, "hello");
            }
            _ => panic!("Expected string 'hello', got {:?}", result),
        }
    }

    #[test]
    fn test_try_catch_captures_error() {
        let script = "try { nonExistent(); } catch(e) { e }";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let out = String::from_utf16_lossy(&s);
                assert!(out.contains("EvaluationError") || out.contains("ParseError"));
            }
            _ => panic!("Expected error string in catch body, got {:?}", result),
        }
    }

    #[test]
    fn test_throw_statement() {
        let script = "try { throw 'custom error'; } catch(e) { e }";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let out = String::from_utf16_lossy(&s);
                assert!(out.contains("custom error"));
            }
            _ => panic!("Expected error string in catch body, got {:?}", result),
        }
    }

    #[test]
    fn test_throw_number() {
        let script = "try { throw 42; } catch(e) { e }";
        let result = evaluate_script(script);
        match result {
            Ok(Value::String(s)) => {
                let out = String::from_utf16_lossy(&s);
                assert!(out.contains("42"));
            }
            _ => panic!("Expected error string in catch body, got {:?}", result),
        }
    }
}
