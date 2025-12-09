use javascript::Value;
use javascript::evaluate_script;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn ternary_basic_true() {
    let script = "true ? 'yes' : 'no'";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::String(s)) => {
            let expected = "yes".encode_utf16().collect::<Vec<u16>>();
            assert_eq!(s, expected);
        }
        _ => panic!("Expected string 'yes', got {:?}", result),
    }
}

#[test]
fn ternary_basic_false() {
    let script = "false ? 1 : 2";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 2.0),
        _ => panic!("Expected number 2.0, got {:?}", result),
    }
}

#[test]
fn ternary_nested() {
    let script = "true ? (false ? 'a' : 'b') : 'c'";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::String(s)) => {
            let expected = "b".encode_utf16().collect::<Vec<u16>>();
            assert_eq!(s, expected);
        }
        _ => panic!("Expected string 'b', got {:?}", result),
    }
}
