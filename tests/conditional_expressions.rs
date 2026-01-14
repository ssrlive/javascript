use javascript::evaluate_script;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn ternary_basic_true() {
    let script = "true ? 'yes' : 'no'";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"yes\"");
}

#[test]
fn ternary_basic_false() {
    let script = "false ? 1 : 2";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "2");
}

#[test]
fn ternary_nested() {
    let script = "true ? (false ? 'a' : 'b') : 'c'";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"b\"");
}
