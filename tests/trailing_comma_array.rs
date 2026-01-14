use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_trailing_comma_in_array_initializer_with_following_statement() {
    let script = r#"
        function f() {
            var arr = ["a", "b", "c",];
            for (var i = 0; i < arr.length; i++) {
                if (arr[i] === undefined) { throw new Error('Bad'); }
                console.log(arr[i]);
            }
            return arr.length;
        }
        f();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "3");

    let script = r#"
        function f() {
            var arr = ["a", "b", "c",];
            for (var i = 0; i < arr.length; i++) {
                if (arr[i] === undefined) throw new Error('Bad');
                console.log(arr[i]);
            }
            return arr.length;
        }
        f();
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "3");
}
