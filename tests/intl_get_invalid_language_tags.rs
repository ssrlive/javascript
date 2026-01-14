use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_get_invalid_language_tags_array_parsing() {
    // This snippet mimics the 'getInvalidLanguageTags' array initializer seen in the
    // Test262 Intl harness and reproduces many line-terminators and blank lines between
    // elements which historically caused a parser failure.
    let script = r#"
        let getInvalidLanguageTags = [
            "",

            "i",

            "xx",

            "u",

            "ıd", // non-ASCII letters
            "en\u0000", // null-terminator sequence

            "pl-PL-pl",

            "cn-hans-CN",

            "en-12345-12345-en-US",

            "pt-u-ca-gregory-u-nu-latn",

            "es-419",

            "x-foo",

            "en  "
        ];

        getInvalidLanguageTags.length
    "#;

    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "13");
}
