use javascript::{Value, evaluate_script};

#[test]
fn bigint_addition_and_mixing() {
    // Addition of two BigInt literals — current engine doesn't implement BigInt arithmetic,
    // so either implementation may return Err; ensure test documents current behavior.
    let res = evaluate_script("1n + 2n");
    match res {
        Ok(Value::BigInt(s)) => assert_eq!(s, "3"),
        Ok(other) => panic!("expected BigInt result for 1n + 2n, got {:?}", other),
        Err(_) => panic!("expected BigInt result for 1n + 2n, got error"),
    }

    // Mixing BigInt with Number in arithmetic should produce an error in current implementation
    let mix = evaluate_script("1n + 1");
    assert!(mix.is_err());

    // Loose equality between BigInt and Number (1n == 1) — per spec this should be true
    let eq = evaluate_script("1n == 1");
    match eq {
        Ok(Value::Boolean(b)) => assert!(b),
        Ok(Value::Number(n)) => assert_eq!(n, 1.0),
        other => panic!("unexpected result for 1n == 1: {:?}", other),
    }
}
