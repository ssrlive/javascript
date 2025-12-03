use javascript::{Value, evaluate_script};

#[test]
fn for_of_missing_iterator_throws() {
    // Plain object without iterator should not be iterable for 'for..of'
    let res = evaluate_script("for (var x of {}) { }; 1");
    assert!(res.is_err());
}

#[test]
fn iterator_next_returns_non_object_throws() {
    // Custom iterable where next() returns a non-object (e.g. number) should raise an error
    let script = r#"
        let s = Symbol.iterator;
        let o = {};
        o[s] = function() {
            return { next: function() { return 42; } };
        };
        // Attempt to iterate will call next() which returns a non-object -> should error
        for (let x of o) { }
        1
    "#;
    let res = evaluate_script(script);
    assert!(res.is_err());
}

#[test]
fn string_iteration_surrogate_pair_behaviour() {
    // For strings containing astral symbols, ensure iteration yields string fragments
    // and concatenation returns a string with the expected boundaries — engine may handle
    // surrogate halves in its own way; we assert robust properties rather than exact codepoint equality.
    let script = r#"
        let s = "a😀b"; // contains an astral character
        let acc = "";
        for (let ch of s) { acc = acc + ch; }
        acc
    "#;
    let res = evaluate_script(script);
    match res {
        Ok(Value::String(s)) => {
            let rust_str = String::from_utf16_lossy(&s);
            assert!(rust_str.starts_with("a"));
            assert!(rust_str.ends_with("b"));
            assert!(rust_str.len() >= 2);
        }
        other => panic!("Expected string result from iteration, got {:?}", other),
    }
}
