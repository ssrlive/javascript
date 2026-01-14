use javascript::evaluate_script;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn undefined_equality() {
    let script = "undefined == undefined";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn undefined_strict_equality() {
    let script = "undefined === undefined";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn object_identity_strict_equal() {
    let script = "let a = {}; let b = a; a === b";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn object_identity_distinct_objects() {
    let script = "let a = {}; a === {}";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "false");
}

#[test]
fn string_concat_with_undefined_right() {
    let script = "'a' + undefined";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"aundefined\"");
}

#[test]
fn string_concat_with_undefined_left() {
    let script = "undefined + 'b'";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"undefinedb\"");
}

#[test]
fn number_strict_equality_same() {
    let script = "10 === 10";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn number_strict_equality_different() {
    let script = "10 === 20";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "false");
}

#[test]
fn string_strict_equality_same() {
    let script = "'hello' === 'hello'";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn string_strict_equality_different() {
    let script = "'hello' === 'world'";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "false");
}

#[test]
fn boolean_strict_equality_true() {
    let script = "true === true";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn boolean_strict_equality_false() {
    let script = "true === false";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "false");
}

#[test]
fn strict_equality_different_types() {
    let script = "10 === '10'";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "false");
}
