use javascript::{Value, evaluate_script};

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
            let s = String::from_utf16_lossy(&s);
            assert_eq!(s, "[1,2,3]");
        }
        other => panic!("Expected JSON string result, got {:?}", other),
    }
}
