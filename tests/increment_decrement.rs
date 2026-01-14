use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_prefix_increment_variable() {
    let res = evaluate_script("let i = 1; ++i; i", None::<&std::path::Path>).unwrap();
    assert_eq!(res, "2");
}

#[test]
fn test_prefix_decrement_variable() {
    let res = evaluate_script("let i = 3; --i; i", None::<&std::path::Path>).unwrap();
    assert_eq!(res, "2");
}

#[test]
fn test_postfix_increment_variable() {
    let res = evaluate_script("let i = 4; i++; i", None::<&std::path::Path>).unwrap();
    assert_eq!(res, "5");
}

#[test]
fn test_postfix_decrement_variable() {
    let res = evaluate_script("let i = 5; i--; i", None::<&std::path::Path>).unwrap();
    assert_eq!(res, "4");
}

#[test]
fn test_increment_property() {
    let res = evaluate_script("let obj = {x: 10}; ++obj.x; obj.x", None::<&std::path::Path>).unwrap();
    assert_eq!(res, "11");
}

#[test]
fn test_postfix_increment_array_index() {
    let res = evaluate_script("let a = [1,2,3]; a[0]++; a[0]", None::<&std::path::Path>).unwrap();
    assert_eq!(res, "2");
}
