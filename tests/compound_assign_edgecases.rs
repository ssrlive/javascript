use javascript::*;

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
fn test_add_assign_with_nan() {
    let res = eval("let i = NaN; i += 5; i").unwrap();
    match res {
        Value::Number(n) => assert!(n.is_nan()),
        _ => panic!("Expected NaN, got {:?}", res),
    }
}

#[test]
fn test_add_assign_with_infinity() {
    // Use exponent literal that overflows to +Infinity
    let res = eval("let i = 1e309; i += 1; i").unwrap();
    match res {
        Value::Number(n) => assert!(n.is_infinite() && n.is_sign_positive()),
        _ => panic!("Expected +Infinity, got {:?}", res),
    }
}

#[test]
fn test_exponent_literal_parsing() {
    // Check that exponent notation is recognized and parsed
    let res = eval("let a = 1e3; a").unwrap();
    match res {
        Value::Number(n) => assert_eq!(n, 1000.0),
        _ => panic!("Expected 1000.0, got {:?}", res),
    }
}

#[test]
fn test_div_assign_by_zero_error() {
    let res = eval("let i = 5; i /= 0");
    match res {
        Err(JSError::EvaluationError { message }) => assert!(message.contains("Division by zero") || message == "Division by zero"),
        other => panic!("Expected Division by zero error, got {:?}", other),
    }
}

#[test]
fn test_mod_assign_by_zero_error() {
    let res = eval("let i = 5; i %= 0");
    match res {
        Err(JSError::EvaluationError { message }) => assert!(message.contains("Division by zero") || message == "Division by zero"),
        other => panic!("Expected Division by zero error, got {:?}", other),
    }
}

#[test]
fn test_assign_to_const_error() {
    let res = eval("const x = 1; x += 2");
    match res {
        Err(JSError::TypeError { message }) => assert!(message.contains("Assignment to constant") || message.contains("constant")),
        other => panic!("Expected TypeError for assignment to const, got {:?}", other),
    }
}

#[test]
fn test_sub_assign_non_number_error() {
    let res = eval("let s = 'a'; s -= 1");
    match res {
        Err(JSError::EvaluationError { message }) => assert!(message.contains("Invalid operands") || message.contains("error")),
        other => panic!("Expected EvaluationError for non-number -=, got {:?}", other),
    }
}

#[test]
fn test_mul_assign_non_number_error() {
    let res = eval("let s = 'a'; s *= 2");
    match res {
        Err(JSError::EvaluationError { message }) => assert!(message.contains("Invalid operands") || message.contains("error")),
        other => panic!("Expected EvaluationError for non-number *=, got {:?}", other),
    }
}
