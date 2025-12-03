use javascript::{Value, evaluate_script};

#[test]
fn bigint_literal_evaluates() {
    let script = "1n;";
    let result = evaluate_script(script);
    assert!(result.is_ok());
    if let Ok(val) = result {
        match val {
            Value::BigInt(s) => assert!(s == "1" || s == "1n"),
            other => panic!("expected BigInt value, got: {:?}", other),
        }
    }
}
