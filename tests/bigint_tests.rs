use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_bigint_literal() {
    let script = "123n";
    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "123", "expected BigInt(123) string, got {}", v);
}

#[test]
fn test_bigint_addition() {
    let script = "1n + 2n";
    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "3", "expected BigInt(3) string, got {}", v);
}

#[test]
fn test_bigint_bitwise_and() {
    let script = "6n & 3n";
    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "2", "expected BigInt(2) string, got {}", v);
}

#[test]
fn test_bigint_bitwise_or() {
    let script = "6n | 1n";
    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "7", "expected BigInt(7) string, got {}", v);
}

#[test]
fn test_bigint_bitwise_xor() {
    let script = "5n ^ 3n";
    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "6", "expected BigInt(6) string, got {}", v);
}

#[test]
fn test_bigint_left_shift() {
    let script = "1n << 3n";
    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "8", "expected BigInt(8) string, got {}", v);
}

#[test]
fn test_bigint_right_shift() {
    let script = "8n >> 2n";
    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "2", "expected BigInt(2) string, got {}", v);
}

#[test]
fn test_bigint_unsigned_right_shift_error() {
    let script = "5n >>> 1n";
    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(res.is_err(), "expected unsigned right shift on BigInt to error");
}

#[test]
fn test_bigint_division_truncates() {
    let script = "5n / 2n"; // integer division
    let v = evaluate_script(script, None::<&std::path::Path>).expect("script ran");
    assert_eq!(v, "2", "expected BigInt(2) string, got {}", v);
}

#[test]
fn test_bigint_mixing_number_error() {
    let script = "1n + 2";
    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(res.is_err(), "expected mixing BigInt and Number to error");
}

#[test]
fn test_bigint_negative_exponent_error() {
    let script = "2n ** -1n";
    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(res.is_err(), "expected negative exponent for BigInt to error");
}
