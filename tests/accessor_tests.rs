use javascript::JSErrorKind;
use javascript::evaluate_script_with_vm;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_write_to_read_only_accessor_throws() {
    let script = "\"use strict\"; class C { get r() { return 1 } } let c = new C(); c.r = 2";
    let result = evaluate_script_with_vm(script, false, None::<&std::path::Path>);
    match result {
        Err(err) => match err.kind() {
            JSErrorKind::TypeError { .. } => (),
            _ => panic!("Expected TypeError for assignment to read-only accessor, got {:?}", err),
        },
        _ => panic!("Expected error for assignment to read-only accessor, got {:?}", result),
    }
}

#[test]
fn test_read_write_only_accessor_returns_undefined() {
    let script = "\"use strict\"; class C { set r(v) { this._r = v } } let c = new C(); c.r = 5; c.r";
    let result = evaluate_script_with_vm(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "undefined");
}
