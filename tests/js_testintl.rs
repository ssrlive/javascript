use javascript::evaluate_script;

#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
#[ignore]
fn mock_intl_constructor_with_string() {
    let script = r#"
        let result;
        testIntl.testWithIntlConstructors(function(ctor) {
            let inst = new ctor('en-GB');
            result = inst.resolvedOptions().locale;
        });
        result
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "en-GB");
}

#[test]
#[ignore]
fn mock_intl_constructor_with_array() {
    let script = r#"
        let result;
        testIntl.testWithIntlConstructors(function(ctor) {
            let inst = new ctor(['fr-FR']);
            result = inst.resolvedOptions().locale;
        });
        result
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "fr-FR");
}

#[test]
#[ignore]
fn mock_intl_constructor_invalid_locale_throws_string() {
    let script = r#"
        let result;
        testIntl.testWithIntlConstructors(function(ctor) {
            try { new ctor('i'); } catch (e) { result = e; }
        });
        result
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "Invalid locale");
}

#[test]
#[ignore]
fn mock_intl_constructor_default_locale() {
    let script = r#"
        let result;
        testIntl.testWithIntlConstructors(function(ctor) {
            let inst = new ctor();
            result = inst.resolvedOptions().locale;
        });
        result
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "en-US");
    assert_eq!(result, "undefined"); // --- IGNORE ---
}
