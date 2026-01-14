use javascript::evaluate_script;

#[test]
fn bigint_nan_infinite_and_sign_edgecases() {
    // NaN and Infinity comparisons with BigInt should be handled.
    let cases = vec![
        ("1n < NaN", false),
        ("1n > NaN", false),
        ("1n <= NaN", false),
        ("1n >= NaN", false),
        ("1n < Infinity", true),
        ("1n > Infinity", false),
        ("1n < -Infinity", false),
        ("1n > -Infinity", true),
    ];

    for (expr, expected) in cases {
        let res = evaluate_script(expr, None::<&std::path::Path>).expect("eval failed");
        let b = res == "true";
        assert_eq!(b, expected, "{} should be {}", expr, expected);
    }
}

#[test]
fn bigint_relational_le_ge_and_negative_fractional() {
    // <= and >= with integer numbers should compare exactly
    let cases = vec![
        ("5n <= 5", true),
        ("5n >= 5", true),
        ("5.0 <= 5n", true),
        ("5.0 >= 5n", true),
        // negative fractional behaviour: -3n < -2.5 -> true (floor(-2.5) = -3 => -3 <= -3)
        ("-3n < -2.5", true),
        // zero edgecases
        ("0n < -0.0", false),
        ("0n >= -0.0", true),
    ];

    for (expr, expected) in cases {
        let res = evaluate_script(expr, None::<&std::path::Path>).expect("eval failed");
        let b = res == "true";
        assert_eq!(b, expected, "{} should be {}", expr, expected);
    }
}

#[test]
fn bigint_relational_small_fuzz_returns_boolean() {
    // A small deterministic fuzz set — ensure all relational expressions evaluate and return Booleans.
    let bigints = vec![
        "0n",
        "1n",
        "-1n",
        "9007199254740991n",
        "9007199254740993n",
        "123456789123456789123456789n",
        "-123456789123456789123456789n",
    ];
    let numbers = vec![
        "NaN",
        "Infinity",
        "-Infinity",
        "9007199254740991",
        "9007199254740992",
        "9007199254740993",
        "5.0",
        "5.1",
        "-2.5",
        "4.9",
    ];
    let ops = vec!["<", "<=", ">", ">="];

    for bi in &bigints {
        for n in &numbers {
            for op in &ops {
                let expr = format!("{} {} {}", bi, op, n);
                let res = evaluate_script(&expr, None::<&std::path::Path>);
                match res {
                    Ok(s) if s == "true" || s == "false" => {} // good
                    Ok(other) => panic!("Expected boolean for '{}', got {:?}", expr, other),
                    Err(e) => panic!("Evaluation error for '{}': {:?}", expr, e),
                }
            }
        }
    }
}
