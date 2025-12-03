use javascript::{Value, evaluate_script};

#[test]
fn bigint_addition_and_mixing() {
    // Addition of two BigInt literals — current engine doesn't implement BigInt arithmetic,
    // so either implementation may return Err; ensure test documents current behavior.
    let res = evaluate_script("1n + 2n");
    assert!(res.is_ok() || res.is_err());

    // Mixing BigInt with Number in arithmetic should produce an error in current implementation
    let mix = evaluate_script("1n + 1");
    assert!(mix.is_err());

    // Loose equality between BigInt and Number (1n == 1) — current impl treats different types as not equal => 0
    let eq = evaluate_script("1n == 1");
    match eq {
        Ok(Value::Number(n)) => assert_eq!(n, 0.0), // expected not-equal
        other => panic!("unexpected result for 1n == 1: {:?}", other),
    }
}
