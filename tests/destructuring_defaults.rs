use javascript::*;

// Initialize logger for integration tests.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_object_destructuring_with_defaults() {
    let script = r#"
        let d = {};
        let { a = 3, b = 4 } = d;
        a + b
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "7");
}

#[test]
fn test_array_destructuring_with_defaults() {
    let script = r#"
        let d = [];
        let [ a = 2, b = 5 ] = d;
        a * b
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "10");
}

// Temporary debug test to inspect tokenization of the scripts used above
#[test]
fn debug_tokenize_destructuring_defaults() {
    let script_obj = r#"
        let d = {};
        let { a = 3, b = 4 } = d;
        a + b
    "#;
    let script_arr = r#"
        let d = [];
        let [ a = 2, b = 5 ] = d;
        a * b
    "#;

    let tokens_obj = javascript::tokenize(script_obj).expect("tokenize failed for object script");
    println!("Object tokens: {:?}", tokens_obj);

    let tokens_arr = javascript::tokenize(script_arr).expect("tokenize failed for array script");
    println!("Array tokens: {:?}", tokens_arr);
}
