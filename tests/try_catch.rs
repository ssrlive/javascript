use javascript::Value;
use javascript::evaluate_script;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn catch_preserves_number_throw() {
    let script = "try { throw 42; } catch (e) { e }";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 42.0),
        _ => panic!("Expected number 42.0, got {:?}", result),
    }

    let script = "try { throw 42 } catch (e) { e }";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::Number(n)) => assert_eq!(n, 42.0),
        _ => panic!("Expected number 42.0, got {:?}", result),
    }
}

#[test]
fn catch_preserves_string_throw() {
    let script = "try { throw 'boom'; } catch (e) { e }";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::String(s)) => {
            let expected = "boom".encode_utf16().collect::<Vec<u16>>();
            assert_eq!(s, expected);
        }
        _ => panic!("Expected string 'boom', got {:?}", result),
    }
}

#[test]
fn engine_error_converted_to_string_in_catch() {
    // Call `String(e)` so the test passes whether `e` is a raw string
    // (legacy behavior) or an `Error` object created by the engine.
    let script = "try { let a = 1; a(); } catch (e) { String(e) }";
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::String(s)) => {
            // The engine should expose a textual representation of the runtime error
            // to the catch clause; ensure we received a non-empty string value.
            assert!(!s.is_empty(), "expected non-empty string error delivered to catch");
        }
        _ => panic!("Expected string error in catch, got {:?}", result),
    }
}
