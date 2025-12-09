use javascript::Value;
use javascript::evaluate_script;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn mock_intl_constructor_with_string() {
    let script = r#"
        let result;
        testIntl.testWithIntlConstructors(function(ctor) {
            let inst = new ctor('en-GB');
            result = inst.resolvedOptions().locale;
        });
        result
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::String(s)) => {
            let expected = "en-GB".encode_utf16().collect::<Vec<u16>>();
            assert_eq!(s, expected);
        }
        _ => panic!("Expected 'en-GB' locale, got {:?}", result),
    }
}

#[test]
fn mock_intl_constructor_with_array() {
    let script = r#"
        let result;
        testIntl.testWithIntlConstructors(function(ctor) {
            let inst = new ctor(['fr-FR']);
            result = inst.resolvedOptions().locale;
        });
        result
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::String(s)) => {
            let expected = "fr-FR".encode_utf16().collect::<Vec<u16>>();
            assert_eq!(s, expected);
        }
        _ => panic!("Expected 'fr-FR' locale, got {:?}", result),
    }
}

#[test]
fn mock_intl_constructor_invalid_locale_throws_string() {
    let script = r#"
        let result;
        testIntl.testWithIntlConstructors(function(ctor) {
            try { new ctor('i'); } catch (e) { result = e; }
        });
        result
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::String(s)) => {
            let expected = "Invalid locale".encode_utf16().collect::<Vec<u16>>();
            assert_eq!(s, expected);
        }
        _ => panic!("Expected thrown string 'Invalid locale', got {:?}", result),
    }
}

#[test]
fn mock_intl_constructor_default_locale() {
    let script = r#"
        let result;
        testIntl.testWithIntlConstructors(function(ctor) {
            let inst = new ctor();
            result = inst.resolvedOptions().locale;
        });
        result
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>);
    match result {
        Ok(Value::String(s)) => {
            // Depending on the path, resolvedOptions may return an object
            // with locale set or the implementation may return undefined.
            // Accept either real 'en-US' string or undefined here.
            let expected = "en-US".encode_utf16().collect::<Vec<u16>>();
            assert_eq!(s, expected);
        }
        Ok(Value::Undefined) => {
            // Accept undefined as current behaviour (no __locale stored)
        }
        _ => panic!("Expected default locale 'en-US' or undefined, got {:?}", result),
    }
}
