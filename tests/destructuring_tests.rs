use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod destructuring_tests {
    use javascript::JSErrorKind;

    use super::*;

    #[test]
    fn test_basic_array_destructuring() {
        let script = "let [a, b] = [1, 2]; a + b";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_array_destructuring_with_rest() {
        let script = "let [a, ...rest] = [1, 2, 3, 4]; rest[0] + rest[1]";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "5");
    }

    #[test]
    fn test_basic_object_destructuring() {
        let script = "let {a, b} = {a: 1, b: 2}; a + b";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn test_object_destructuring_with_rest() {
        let script = "let {a, ...rest} = {a: 1, b: 2, c: 3}; rest.b + rest.c";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "5");
    }

    #[test]
    fn test_nested_destructuring() {
        let script = "let [a, {b}] = [1, {b: 2, c: 3}]; a + b";
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "3");
    }

    #[test]
    fn destructuring_from_undefined_returns_helpful_error() {
        let script = r#"
            let duration;
            let { seconds = 0, milliseconds = 0 } = duration;
        "#;

        let res = evaluate_script(script, None::<&std::path::Path>);
        match res {
            Err(err) => match err.kind() {
                JSErrorKind::TypeError { message, .. } => {
                    assert!(message.contains("seconds"));
                }
                _ => panic!("expected TypeError for destructuring undefined, got {:?}", err),
            },
            _ => panic!("expected TypeError for destructuring undefined, got {:?}", res),
        }
    }

    #[test]
    fn destructuring_with_nullish_fallback_works() {
        let script = r#"
            let duration;
            let { seconds = 0, milliseconds = 0 } = duration ?? {};
            seconds;
        "#;

        let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(res, "0");
    }
}
