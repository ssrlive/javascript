use javascript::core::*;
use javascript::error::JSError;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

fn eval(script: &str) -> Result<Value, JSError> {
    evaluate_script(script)
}

#[test]
fn test_add_assign_numbers() {
    let res = eval("let i = 5; i += 7; i").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 12.0));
}

#[test]
fn test_sub_assign_numbers() {
    let res = eval("let i = 10; i -= 3; i").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 7.0));
}

#[test]
fn test_mul_assign_numbers() {
    let res = eval("let i = 6; i *= 3; i").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 18.0));
}

#[test]
fn test_div_assign_numbers() {
    let res = eval("let i = 20; i /= 4; i").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 5.0));
}

#[test]
fn test_mod_assign_numbers() {
    let res = eval("let i = 20; i %= 6; i").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 2.0));
}

#[test]
fn test_add_assign_string_concat() {
    let res = eval("let s = 'foo'; s += 'bar'; s").unwrap();
    assert!(matches!(res, Value::String(ref v) if String::from_utf16_lossy(v) == "foobar"));
}

#[test]
fn test_property_add_assign() {
    let res = eval("let obj = {v: 1}; obj.v += 4; obj.v").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 5.0));
}

#[test]
fn test_index_mul_assign() {
    let res = eval("let a = [2]; a[0] *= 5; a[0]").unwrap();
    assert!(matches!(res, Value::Number(n) if n == 10.0));
}
