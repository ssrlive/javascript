use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[cfg(test)]
mod symbol_additional_tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn test_symbol_typeof() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            typeof Symbol('x')
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"symbol\"");
    }

    #[test]
    fn debug_descriptor_array() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            let s = Symbol('k');
            let o = { a: 1 };
            o[s] = 2;
            let d = Object.getOwnPropertyDescriptors(o);
            [d.a.value === 1, d[s].value === 2, d.a.writable === true]
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[true,true,true]");
    }

    #[test]
    fn test_symbol_to_string_and_description() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            let a = Symbol('my-desc');
            let b = Symbol();
            let s1 = a.toString();
            let s2 = b.toString();
            let d1 = a.description;
            let d2 = b.description;
            s1 + '|' + s2 + '|' + (typeof d1) + '|' + (typeof d2)
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();

        // Expect: "Symbol(my-desc)|Symbol()|string|undefined"
        assert_eq!(result, "\"Symbol(my-desc)|Symbol()|string|undefined\"");
    }

    #[test]
    fn test_symbol_uniqueness() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            Symbol() !== Symbol()
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_json_stringify_ignores_symbol_keys() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            let s = Symbol('k');
            let o = {};
            o[s] = 1;
            JSON.stringify(o);
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"{}\"");
    }

    #[test]
    fn test_object_keys_values_ignore_symbol_keys() {
        // Object.keys and Object.values should not include symbol-keyed properties
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            let s = Symbol('k');
            let o = { a: 1 };
            o[s] = 2;
            [Object.keys(o).length, Object.values(o).length]
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[1,1]");
    }

    #[test]
    fn test_object_assign_ignores_symbol_keys() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            let s = Symbol('k');
            let src = { a: 1 };
            src[s] = 2;
            let target = {};
            Object.assign(target, src);
            JSON.stringify(target)
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"{\\\"a\\\":1}\"");
    }

    #[test]
    fn test_new_symbol_throws() {
        let script = r#"
            try {
                new Symbol();
            } catch (e) {
                'error'
            }
        "#;
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"error\"");
    }

    #[test]
    fn test_symbol_value_of() {
        let script = r#"
            let s = Symbol('k');
            s.valueOf() === s
        "#;
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "true");
    }

    #[test]
    fn test_symbol_prototype_shadowing_and_assignment() {
        let script = r#"
            let s = Symbol('p');
            let proto = {};
            proto[s] = 1;
            let obj = Object.create(proto);
            let init = obj[s];
            obj[s] = 2;
            let after = obj[s];
            let fromProto = proto[s];
            [init, after, fromProto]
        "#;

        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[1,2,1]");
    }

    #[test]
    fn test_get_own_property_symbols_own_vs_inherited() {
        let script = r#"
            let s = Symbol('p');
            let proto = {};
            proto[s] = 1;
            let obj = Object.create(proto);
            let ownLen = Object.getOwnPropertySymbols(obj).length;
            let protoLen = Object.getOwnPropertySymbols(proto).length;
            let same = Object.getOwnPropertySymbols(proto)[0] === s;
            [ownLen, protoLen, same]
        "#;

        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[0,1,true]");
    }

    #[test]
    fn test_get_own_property_symbols_on_object() {
        let script = r#"
            let s = Symbol('k');
            let o = { a: 1 };
            o[s] = 2;
            let arr = Object.getOwnPropertySymbols(o);
            [arr.length, arr[0] === s]
        "#;

        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[1,true]");
    }

    #[test]
    fn test_get_own_property_descriptors() {
        // Part 1: string and symbol keys
        let script = r#"
            let s = Symbol('k');
            let o = { a: 1 };
            o[s] = 2;
            let d = Object.getOwnPropertyDescriptors(o);
            [d.a.value === 1, d[s].value === 2, d.a.writable === true]
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[true,true,true]");

        // Part 2: accessor descriptors (getters/setters)
        let script2 = r#"
            let o = { get x() { return 9; }, set x(v) { this._x = v } };
            let d = Object.getOwnPropertyDescriptors(o);
            [typeof d.x.get, typeof d.x.set]
        "#;

        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "[\"function\",\"function\"]");
    }

    #[test]
    fn test_well_known_symbol_iterator_and_iterable() {
        let script = r#"
            typeof Symbol.iterator
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"symbol\"");

        // Custom iterable using computed symbol key
        let script2 = r#"
            let s = Symbol.iterator;
            let o = {};
            o[s] = function() {
                let i = 1;
                return { next: function() { if (i <= 3) { return { value: i++, done: false } } else { return { done: true } } } };
            };
            let sum = 0;
            for (let x of o) { sum = sum + x; }
            sum
        "#;

        let result2 = evaluate_script(script2, None::<&std::path::Path>).unwrap();
        assert_eq!(result2, "6");
    }

    #[test]
    fn test_symbol_to_string_tag() {
        let script = r#"
            let tag = Symbol.toStringTag;
            let o = {};
            o[tag] = 'MyTag';
            o.toString();
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"[object MyTag]\"");
    }

    #[test]
    fn test_string_for_of() {
        let script = r#"
            let s = "abc";
            let acc = "";
            for (let ch of s) { acc = acc + ch; }
            acc
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"abc\"");
    }

    #[test]
    fn test_symbol_to_string_tag_defaults() {
        let script = r#"
            let a = [1,2,3];
            let s = new String('x');
            [a[Symbol.toStringTag], s[Symbol.toStringTag]]
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[\"Array\",\"String\"]");
    }

    #[test]
    fn test_array_symbol_iterator_callable() {
        let script = r#"
            let a = [1,2,3];
            let iter = a[Symbol.iterator]();
            let s = 0;
            s = s + iter.next().value;
            s = s + iter.next().value;
            s = s + iter.next().value;
            s
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "6");
    }

    #[test]
    fn test_string_object_symbol_iterator_callable() {
        let script = r#"
            let s = new String('xy');
            let it = s[Symbol.iterator]();
            let a = it.next().value + it.next().value;
            a
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "\"xy\"");
    }

    #[test]
    fn test_symbol_to_primitive_coercion() {
        // Objects may define [Symbol.toPrimitive] to customize coercion
        let script = r#"
            let tp = Symbol.toPrimitive;
            let o = {};
            o[tp] = function(hint) {
                if (hint === 'string') { return 'S-PRIM'; }
                return 40;
            };
                let res = [String(o), Number(o), o + 2]; res
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
        assert_eq!(result, "[\"S-PRIM\",40,42]");
    }
}
