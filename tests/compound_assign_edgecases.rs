use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor(unsafe)]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_add_assign_with_nan() {
    let script = "let i = NaN; i += 5; i";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "NaN");
}

#[test]
fn test_add_assign_with_infinity() {
    // Use exponent literal that overflows to +Infinity
    let script = "let i = 1e309; i += 1; i";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "Infinity");
}

#[test]
fn test_exponent_literal_parsing() {
    // Check that exponent notation is recognized and parsed
    let script = "let a = 1e3; a";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "1000");
}

#[test]
fn test_div_assign_by_zero_error() {
    let script = "let i = 5; i /= 0";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "Infinity");
}

#[test]
fn test_mod_assign_by_zero_error() {
    let script = "let i = 5; i %= 0";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "NaN");
}

#[test]
fn test_assign_to_const_error() {
    let script = "const x = 1; x += 2";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET);
    match res {
        Err(err) => match err.kind() {
            JSErrorKind::TypeError { message, .. } => assert!(message.contains("Assignment to constant") || message.contains("constant")),
            _ => panic!("Expected TypeError for assignment to const, got {:?}", err),
        },
        other => panic!("Expected TypeError for assignment to const, got {:?}", other),
    }
}

#[test]
fn test_sub_assign_non_number_error() {
    let script = "let s = 'a'; s -= 1";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "NaN");
}

#[test]
fn test_mul_assign_non_number_error() {
    let script = "let s = 'a'; s *= 2";
    let res = evaluate_script(script, false, None::<&std::path::Path>, javascript::DEFAULT_GC_OPCODE_BUDGET).unwrap();
    assert_eq!(res, "NaN");
}
