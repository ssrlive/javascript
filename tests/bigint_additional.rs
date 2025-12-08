use javascript::{JSError, Value, evaluate_script};

#[test]
fn bigint_literal_and_arithmetic() -> Result<(), JSError> {
    // Basic arithmetic with BigInt literals
    let script = r#"
        (1n + 2n === 3n) && (5n - 2n === 3n) && (2n * 3n === 6n) && (6n / 2n === 3n);
    "#;
    let res = evaluate_script(script)?;
    match res {
        Value::Boolean(true) => Ok(()),
        other => panic!("BigInt arithmetic failed: {:?}", other),
    }
}

#[test]
fn bigint_comparisons() -> Result<(), JSError> {
    let script = r#"
        (2n > 1n) && (1n < 2n) && (3n >= 3n) && (3n <= 3n) && (3n === 3n);
    "#;
    let res = evaluate_script(script)?;
    match res {
        Value::Boolean(true) => Ok(()),
        other => panic!("BigInt comparisons failed: {:?}", other),
    }
}

#[test]
fn bigint_and_number_mixing_errors_for_add() {
    // '+' between BigInt and Number should throw TypeError
    let res = evaluate_script("1n + 1");
    match res {
        Err(err) => match err.kind() {
            javascript::JSErrorKind::TypeError { message, .. } => assert!(message.contains("Cannot mix BigInt")),
            _ => panic!("Expected TypeError for mixing BigInt and Number in +, got {:?}", err),
        },
        other => panic!("Expected TypeError for mixing BigInt and Number in +, got {:?}", other),
    }
}

#[test]
fn bigint_bitwise_and_shift_operations() -> Result<(), JSError> {
    // Bitwise ops should work on BigInt
    let script = r#"
        ((5n & 3n) === 1n) && ((5n | 2n) === 7n) && ((5n ^ 1n) === 4n);
    "#;
    let res = evaluate_script(script)?;
    match res {
        Value::Boolean(true) => Ok(()),
        other => panic!("BigInt bitwise ops failed: {:?}", other),
    }
}
