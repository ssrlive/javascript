use javascript::JSErrorKind;
use javascript::{Value, evaluate_script};

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_write_to_read_only_accessor_throws() {
    let script = "class C { get r() { return 1 } } let c = new C(); c.r = 2";
    let result = evaluate_script(script, None::<&std::path::Path>);
    assert!(result.is_err());
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
    let script = "class C { set r(v) { this._r = v } } let c = new C(); c.r = 5; c.r";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Undefined) => (),
        Ok(v) => panic!("Expected undefined from reading write-only accessor, got {:?}", v),
        Err(e) => panic!("evaluate_script error: {:?}", e),
    }
}
