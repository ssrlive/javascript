use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_prefix_increment_variable() {
    let script = "let i = 1; ++i; i";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "2");
}

#[test]
fn test_prefix_decrement_variable() {
    let script = "let i = 3; --i; i";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "2");
}

#[test]
fn test_postfix_increment_variable() {
    let script = "let i = 4; i++; i";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "5");
}

#[test]
fn test_postfix_decrement_variable() {
    let script = "let i = 5; i--; i";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "4");
}

#[test]
fn test_increment_property() {
    let script = "let obj = {x: 10}; ++obj.x; obj.x";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "11");
}

#[test]
fn test_postfix_increment_array_index() {
    let script = "let a = [1,2,3]; a[0]++; a[0]";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "2");
}
