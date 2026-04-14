use javascript::evaluate_script;

#[test]
fn test_ishtmldda_probe_semantics() {
    let script = r#"
        (function() {
            return typeof __isHTMLDDA__ === "undefined" &&
                __isHTMLDDA__() === null &&
                !__isHTMLDDA__;
        })()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_ishtmldda_equality_semantics() {
    let script = r#"
        (function() {
            var x = __isHTMLDDA__;
            return x == undefined &&
                undefined == x &&
                x == null &&
                null == x &&
                x !== undefined &&
                x !== null &&
                Object.is(x, undefined) === false &&
                Object.is(x, null) === false &&
                Object.is(x, x);
        })()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_ishtmldda_still_counts_as_method() {
    let script = r#"
        (function() {
            var items = {};
            items[Symbol.iterator] = __isHTMLDDA__;
            try {
                Array.from(items);
                return false;
            } catch (e) {
                return e.name === "TypeError";
            }
        })()
    "#;
    let result = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}
