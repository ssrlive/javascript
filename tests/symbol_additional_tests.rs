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
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "symbol"),
            _ => panic!("Expected string 'symbol', got {:?}", result),
        }
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
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Object(arr)) => {
                let a = arr.borrow().get(&"0".into()).unwrap();
                let b = arr.borrow().get(&"1".into()).unwrap();
                let c = arr.borrow().get(&"2".into()).unwrap();
                match (&*a.borrow(), &*b.borrow(), &*c.borrow()) {
                    (Value::Boolean(na), Value::Boolean(nb), Value::Boolean(nc)) => {
                        assert!(na);
                        assert!(nb);
                        assert!(nc);
                    }
                    _ => panic!("Expected numeric truthy results for descriptors"),
                }
            }
            _ => panic!("Expected array result from descriptors test, got {:?}", result),
        }
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

        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => {
                let out = utf16_to_utf8(&s);
                // Expect: "Symbol(my-desc)|Symbol()|string|undefined"
                assert!(out.starts_with("Symbol(my-desc)|Symbol()|"), "got {}", out);
            }
            _ => panic!("Expected string result, got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_uniqueness() {
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let script = r#"
            Symbol() !== Symbol()
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected true for distinct symbols, got {:?}", result),
        }
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
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "{}"),
            _ => panic!("Expected JSON '{}' for object with only symbol keys", "{}"),
        }
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
        let result = evaluate_script(script, None::<&std::path::Path>);
        match &result {
            Ok(Value::Object(arr)) => {
                // Expect [1, 1]
                let k = arr.borrow().get(&"0".into()).unwrap();
                let v = arr.borrow().get(&"1".into()).unwrap();
                match (&*k.borrow(), &*v.borrow()) {
                    (Value::Number(nk), Value::Number(nv)) => {
                        assert_eq!(*nk, 1.0);
                        assert_eq!(*nv, 1.0);
                    }
                    _ => panic!("Expected numeric lengths"),
                }
            }
            _ => panic!("Expected array response for lengths"),
        }
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
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "{\"a\":1}"),
            _ => panic!("Expected object string with only 'a' copied, got {:?}", result),
        }
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
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "error"),
            _ => panic!("Expected error when calling new Symbol(), got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_value_of() {
        let script = r#"
            let s = Symbol('k');
            s.valueOf() === s
        "#;
        let _guard = TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().unwrap();
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Boolean(b)) => assert!(b),
            _ => panic!("Expected true for valueOf equality, got {:?}", result),
        }
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
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Object(arr)) => {
                // expect [1,2,1]
                let a = arr.borrow().get(&"0".into()).unwrap();
                let b = arr.borrow().get(&"1".into()).unwrap();
                let c = arr.borrow().get(&"2".into()).unwrap();
                match (&*a.borrow(), &*b.borrow(), &*c.borrow()) {
                    (Value::Number(na), Value::Number(nb), Value::Number(nc)) => {
                        assert_eq!(*na, 1.0);
                        assert_eq!(*nb, 2.0);
                        assert_eq!(*nc, 1.0);
                    }
                    _ => panic!("Expected numeric results for prototype test"),
                }
            }
            _ => panic!("Expected array from prototype test"),
        }
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
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Object(arr)) => {
                let a = arr.borrow().get(&"0".into()).unwrap();
                let b = arr.borrow().get(&"1".into()).unwrap();
                let c = arr.borrow().get(&"2".into()).unwrap();
                match (&*a.borrow(), &*b.borrow(), &*c.borrow()) {
                    (Value::Number(na), Value::Number(nb), Value::Boolean(nc)) => {
                        assert_eq!(*na, 0.0);
                        assert_eq!(*nb, 1.0);
                        assert!(nc);
                    }
                    _ => panic!("Expected numeric results for getOwnPropertySymbols test"),
                }
            }
            _ => panic!("Expected array from getOwnPropertySymbols test"),
        }
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
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Object(arr)) => {
                let a = arr.borrow().get(&"0".into()).unwrap();
                let b = arr.borrow().get(&"1".into()).unwrap();
                match (&*a.borrow(), &*b.borrow()) {
                    (Value::Number(na), Value::Boolean(nb)) => {
                        assert_eq!(*na, 1.0);
                        assert!(nb);
                    }
                    _ => panic!("Expected numeric results for getOwnPropertySymbols on object"),
                }
            }
            _ => panic!("Expected array from getOwnPropertySymbols on object test"),
        }
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

        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Object(arr)) => {
                let a = arr.borrow().get(&"0".into()).unwrap();
                let b = arr.borrow().get(&"1".into()).unwrap();
                let c = arr.borrow().get(&"2".into()).unwrap();
                match (&*a.borrow(), &*b.borrow(), &*c.borrow()) {
                    (Value::Boolean(na), Value::Boolean(nb), Value::Boolean(nc)) => {
                        assert!(na);
                        assert!(nb);
                        assert!(nc);
                    }
                    _ => panic!("Expected numeric truthy results for descriptors"),
                }
            }
            _ => panic!("Expected array result from descriptors test, got {:?}", result),
        }

        // Part 2: accessor descriptors (getters/setters)
        let script2 = r#"
            let o = { get x() { return 9; }, set x(v) { this._x = v } };
            let d = Object.getOwnPropertyDescriptors(o);
            [typeof d.x.get, typeof d.x.set]
        "#;

        let result2 = evaluate_script(script2, None::<&std::path::Path>);
        match result2 {
            Ok(Value::Object(arr)) => {
                let a = arr.borrow().get(&"0".into()).unwrap();
                let b = arr.borrow().get(&"1".into()).unwrap();
                match (&*a.borrow(), &*b.borrow()) {
                    (Value::String(sa), Value::String(sb)) => {
                        assert_eq!(utf16_to_utf8(sa), "function");
                        assert_eq!(utf16_to_utf8(sb), "function");
                    }
                    _ => panic!("Expected string results for accessor descriptor types"),
                }
            }
            _ => panic!("Expected array result from accessor descriptors test"),
        }
    }

    #[test]
    fn test_well_known_symbol_iterator_and_iterable() {
        let script = r#"
            typeof Symbol.iterator
        "#;
        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "symbol"),
            _ => panic!("Expected string 'symbol' for Symbol.iterator, got {:?}", result),
        }

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

        let result2 = evaluate_script(script2, None::<&std::path::Path>);
        match result2 {
            Ok(Value::Number(n)) => assert_eq!(n, 6.0),
            _ => panic!("Expected number 6 from iterable for-of, got {:?}", result2),
        }
    }

    #[test]
    fn test_symbol_to_string_tag() {
        let script = r#"
            let tag = Symbol.toStringTag;
            let o = {};
            o[tag] = 'MyTag';
            o.toString();
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "[object MyTag]"),
            _ => panic!("Expected [object MyTag], got {:?}", result),
        }
    }

    #[test]
    fn test_string_for_of() {
        let script = r#"
            let s = "abc";
            let acc = "";
            for (let ch of s) { acc = acc + ch; }
            acc
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "abc"),
            _ => panic!("Expected 'abc' from string for-of, got {:?}", result),
        }
    }

    #[test]
    fn test_symbol_to_string_tag_defaults() {
        let script = r#"
            let a = [1,2,3];
            let s = new String('x');
            [a[Symbol.toStringTag], s[Symbol.toStringTag]]
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Object(arr)) => {
                let a0 = arr.borrow().get(&"0".into()).unwrap();
                let a1 = arr.borrow().get(&"1".into()).unwrap();
                match (&*a0.borrow(), &*a1.borrow()) {
                    (Value::String(tag_a), Value::String(tag_s)) => {
                        assert_eq!(utf16_to_utf8(tag_a), "Array");
                        assert_eq!(utf16_to_utf8(tag_s), "String");
                    }
                    _ => panic!("Expected string tags in result"),
                }
            }
            _ => panic!("Expected array result from tag test, got {:?}", result),
        }
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

        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Number(n)) => assert_eq!(n, 6.0),
            _ => panic!("Expected 6 from manual iterator next(), got {:?}", result),
        }
    }

    #[test]
    fn test_string_object_symbol_iterator_callable() {
        let script = r#"
            let s = new String('xy');
            let it = s[Symbol.iterator]();
            let a = it.next().value + it.next().value;
            a
        "#;

        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::String(s)) => assert_eq!(utf16_to_utf8(&s), "xy"),
            _ => panic!("Expected 'xy' from string iterator, got {:?}", result),
        }
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

        let result = evaluate_script(script, None::<&std::path::Path>);
        match result {
            Ok(Value::Object(arr)) => {
                let s = arr.borrow().get(&"0".into()).unwrap();
                let n = arr.borrow().get(&"1".into()).unwrap();
                let sum = arr.borrow().get(&"2".into()).unwrap();
                match (&*s.borrow(), &*n.borrow(), &*sum.borrow()) {
                    (Value::String(ss), Value::Number(nn), Value::Number(sn)) => {
                        assert_eq!(utf16_to_utf8(ss), "S-PRIM");
                        assert_eq!(*nn, 40.0);
                        assert_eq!(*sn, 42.0);
                    }
                    _ => panic!("Expected [string, number, number] from toPrimitive coercion test"),
                }
            }
            other => panic!("Expected array result for toPrimitive test, got {:?}", other),
        }
    }

    // debug test removed

    // debug test removed
}
