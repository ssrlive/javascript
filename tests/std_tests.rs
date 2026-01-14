#![allow(unused)]

use javascript::*;

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
    #[cfg(feature = "std")]
    fn test_sprintf() {
        let script = "import * as std from 'std'; std.sprintf('a=%d s=%s', 123, 'abc')";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"a=123 s=abc\"");
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_tmpfile_puts_read() {
        let script = "import * as std from 'std'; let f = std.tmpfile(); f.puts('hello'); f.readAsString();";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"hello\"");
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_try_catch_captures_error() {
        // Use `String(e)` so the test passes whether `e` is a string
        // (old behavior) or an `Error` object with a `message`/toString.
        let script = "try { nonExistent(); } catch(e) { String(e) }";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        println!("DEBUG eval result: {}", result);
        assert!(result.contains("nonExistent is not defined"));
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_throw_statement() {
        let script = "try { throw 'custom error'; } catch(e) { e }";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert!(result.contains("custom error"));
    }

    #[test]
    #[cfg(feature = "std")]
    fn test_throw_number() {
        let script = "try { throw 42; } catch(e) { e }";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "42");
    }
}
