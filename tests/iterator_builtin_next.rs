use javascript::evaluate_script_with_vm;

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
        console.log(out);
        JSON.stringify(out)
    "#;

    // Return a JSON string so we can assert easily from Rust test harness
    let res = evaluate_script_with_vm(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "\"[1,2,3]\"");
}
