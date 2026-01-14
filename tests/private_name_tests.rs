use javascript::*;

#[test]
fn duplicate_private_identifier_is_syntax_error() {
    let script = r#"
    class BadIdeas {
        #firstName;
        #firstName;
    }
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(res.is_err(), "Expected script to fail with a syntax error");
    let err = res.unwrap_err();
    let msg = err.message();
    assert!(msg.starts_with("SyntaxError"), "Expected a SyntaxError, got: {}", msg);
    assert!(
        msg.contains("#firstName"),
        "Expected message to mention '#firstName' but got: {msg}",
    );
    assert!(
        err.js_line().is_some() && err.js_column().is_some(),
        "Expected error to include js line and column"
    );
}

#[test]
fn delete_private_field_is_syntax_error() {
    let script = r#"
    class BadIdeas {
        #lastName;
        constructor() {
            delete this.#lastName;
        }
    }
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(res.is_err(), "Expected script to fail with a syntax error");
    let err = res.unwrap_err();
    let msg = err.message();
    assert!(msg.starts_with("SyntaxError"), "Expected a SyntaxError, got: {msg}");
    assert!(
        err.js_line().is_some() && err.js_column().is_some(),
        "Expected delete error to include js line and column"
    );
}

#[test]
fn private_field_access_within_class_succeeds() {
    let script = r#"
    class Color {
        #values;
        constructor(r,g,b) { this.#values = [r,g,b]; }
        log() {
            console.log(this.#values);
        }
    }
    let tmp = new Color(1,2,3);
    tmp.log();
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(res.is_ok(), "Expected script to run without a syntax error");
}

#[test]
fn private_field_access_outside_class_reports_location() {
    let script = r#"
    class Color {
        #values;
    }
    // Accessing a private field outside the declaring class must be a SyntaxError
    console.log((new Color()).#values);
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    assert!(
        res.is_err(),
        "Expected script to fail with a syntax error when accessing private field outside class"
    );
    let err = res.unwrap_err();
    let msg = err.message();
    assert!(msg.starts_with("SyntaxError"), "Expected a SyntaxError, got: {}", msg);
    assert!(
        err.js_line().is_some() && err.js_column().is_some(),
        "Expected error to include js line and column"
    );
}

#[test]
fn eval_produces_throwable_syntax_error_instanceof() {
    let script = r#"
    try {
        eval("class BadIdeas { #firstName; #firstName; }");
        throw new Error('No error thrown');
    } catch (e) {
        if (!(e instanceof SyntaxError)) {
            throw new Error('Caught error is not a SyntaxError');
        }
    }
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "undefined");
}
