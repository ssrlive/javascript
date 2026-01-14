use javascript::evaluate_script;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn catch_preserves_number_throw() {
    let script = "try { throw 42; } catch (e) { e }";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "42");

    let script = "try { throw 42 } catch (e) { e }";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "42");
}

#[test]
fn catch_preserves_string_throw() {
    let script = "try { throw 'boom'; } catch (e) { e }";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"boom\"");
}

#[test]
fn engine_error_converted_to_string_in_catch() {
    let script = "try { let a = 1; a(); } catch (e) { String(e) }";
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"TypeError: a is not a function\"");
}
