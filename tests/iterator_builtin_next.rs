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

#[test]
fn map_iterator_prototype_exposes_next_and_tostringtag() {
    let script = r#"
        let iterator = new Map([[1, 11]]).values();
        let proto = Object.getPrototypeOf(iterator);
        proto.next.name === "next" &&
        proto.next.length === 0 &&
        proto[Symbol.toStringTag] === "Map Iterator" &&
        !Object.prototype.hasOwnProperty.call(iterator, "next")
    "#;

    let res = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "true");
}

#[test]
fn set_iterator_prototype_exposes_next_and_tostringtag() {
    let script = r#"
        let iterator = new Set([1, 2]).values();
        let proto = Object.getPrototypeOf(iterator);
        proto.next.name === "next" &&
        proto.next.length === 0 &&
        proto[Symbol.toStringTag] === "Set Iterator" &&
        !Object.prototype.hasOwnProperty.call(iterator, "next")
    "#;

    let res = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "true");
}

#[test]
fn map_iterator_stays_done_after_mutation() {
    let script = r#"
        let map = new Map([[1, 11], [2, 22]]);
        let iterator = map[Symbol.iterator]();
        iterator.next();
        map.set(3, 33);
        iterator.next();
        iterator.next();
        let exhausted = iterator.next();
        map.set(4, 44);
        let repeated = iterator.next();
        exhausted.value === undefined &&
        exhausted.done === true &&
        repeated.value === undefined &&
        repeated.done === true
    "#;

    let res = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "true");
}

#[test]
fn set_iterator_stays_done_after_mutation() {
    let script = r#"
        let set = new Set([1, 2]);
        let iterator = set[Symbol.iterator]();
        iterator.next();
        set.add(3);
        iterator.next();
        iterator.next();
        let exhausted = iterator.next();
        set.add(4);
        let repeated = iterator.next();
        exhausted.value === undefined &&
        exhausted.done === true &&
        repeated.value === undefined &&
        repeated.done === true
    "#;

    let res = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "true");
}

#[test]
fn iterator_prototype_symbol_iterator_returns_this_value() {
    let script = r#"
        let iteratorPrototype = Object.getPrototypeOf(
            Object.getPrototypeOf([][Symbol.iterator]())
        );
        let getIterator = iteratorPrototype[Symbol.iterator];
        let values = [{}, Symbol(), 4, 4n, true, undefined, null];
        values.every(function(value) {
            return getIterator.call(value) === value;
        })
    "#;

    let res = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "true");
}

#[test]
fn iterator_prototype_symbol_iterator_has_standard_metadata() {
    let script = r#"
        let iteratorPrototype = Object.getPrototypeOf(
            Object.getPrototypeOf([][Symbol.iterator]())
        );
        let descriptor = Object.getOwnPropertyDescriptor(iteratorPrototype, Symbol.iterator);
        descriptor.value.name === "[Symbol.iterator]" &&
        descriptor.value.length === 0 &&
        descriptor.writable === true &&
        descriptor.enumerable === false &&
        descriptor.configurable === true
    "#;

    let res = evaluate_script(script, false, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "true");
}
