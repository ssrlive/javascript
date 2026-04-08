use javascript::evaluate_script;

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
    let res = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "\"[1,2,3]\"");
}

#[test]
fn map_iterator_next_throws_on_primitive_receivers() {
    let script = r#"
        let iterator = new Map([[1, 11], [2, 22]]).keys();
        let cases = [false, 1, '', undefined, null, Symbol()];
        cases.every(function(value) {
            try {
                iterator.next.call(value);
                return false;
            } catch (err) {
                return err instanceof TypeError;
            }
        })
    "#;

    let res = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "true");
}

#[test]
fn set_iterator_next_throws_on_primitive_receivers() {
    let script = r#"
        let iterator = new Set([1, 2]).values();
        let cases = [false, 1, '', undefined, null, Symbol()];
        cases.every(function(value) {
            try {
                iterator.next.call(value);
                return false;
            } catch (err) {
                return err instanceof TypeError;
            }
        })
    "#;

    let res = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "true");
}
