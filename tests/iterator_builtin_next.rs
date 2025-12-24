use javascript::{Value, evaluate_script, utf16_to_utf8};

#[test]
fn builtin_next_function_iterator() {
    let script = r#"
        function makeIterable(limit) {
            return {
                [Symbol.iterator]() {
                    let i = 0;
                    return { 
                        next() { 
                            if (i >= limit) return { done: true };
                            i++;
                            return { value: i, done: false };
                        }
                    };
                }
            };
        }
        let out = [];
        for (let v of makeIterable(3)) { out.push(v); }
        JSON.stringify(out)
    "#;

    // Return a JSON string so we can assert easily from Rust test harness
    let res = evaluate_script(script, None::<&std::path::Path>);

    match res {
        Ok(Value::String(s)) => {
            let s = utf16_to_utf8(&s);
            assert_eq!(s, "[1,2,3]");
        }
        other => panic!("Expected JSON string result, got {:?}", other),
    }
}
