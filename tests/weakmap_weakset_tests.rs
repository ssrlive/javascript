use javascript::PropertyKey;
use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_weakmap_constructor() {
    let result = evaluate_script("new WeakMap()", None::<&std::path::Path>).unwrap();
    assert!(matches!(result, Value::WeakMap(_)));

    let result = evaluate_script("new WeakMap([])", None::<&std::path::Path>).unwrap();
    assert!(matches!(result, Value::WeakMap(_)));
}

#[test]
fn test_weakmap_set_get_has_delete() {
    let result = evaluate_script(
        r#"
        let wm = new WeakMap();
        let key = {};
        wm.set(key, 'value');
        wm.get(key)
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();

    match result {
        Value::String(s) => {
            assert_eq!(String::from_utf16_lossy(&s), "value");
        }
        _ => panic!("Expected string value"),
    }

    let result = evaluate_script(
        r#"
        let wm = new WeakMap();
        let key = {};
        wm.set(key, 'value');
        wm.has(key)
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();

    match result {
        Value::Boolean(b) => assert!(b),
        _ => panic!("Expected boolean"),
    }

    let result = evaluate_script(
        r#"
        let wm = new WeakMap();
        let key = {};
        wm.set(key, 'value');
        let deleted = wm.delete(key);
        let hasAfter = wm.has(key);
        [deleted, hasAfter]
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();

    // Should return an array-like object with [true, false]
    if let Value::Object(obj) = result {
        let key0 = PropertyKey::from("0");
        let deleted_val = obj.borrow().properties.get(&key0).unwrap().borrow().clone();
        let key1 = PropertyKey::from("1");
        let has_after_val = obj.borrow().properties.get(&key1).unwrap().borrow().clone();

        assert!(matches!(deleted_val, Value::Boolean(true)));
        assert!(matches!(has_after_val, Value::Boolean(false)));
    } else {
        panic!("Expected object");
    }
}

#[test]
fn test_weakmap_non_object_key() {
    let result = evaluate_script(
        r#"
        let wm = new WeakMap();
        try {
            wm.set('string', 'value');
            'no_error'
        } catch (e) {
            'error'
        }
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();

    match result {
        Value::String(s) => {
            assert_eq!(String::from_utf16_lossy(&s), "error");
        }
        _ => panic!("Expected error string"),
    }
}

#[test]
fn test_weakset_constructor() {
    let result = evaluate_script("new WeakSet()", None::<&std::path::Path>).unwrap();
    assert!(matches!(result, Value::WeakSet(_)));

    let result = evaluate_script("new WeakSet([])", None::<&std::path::Path>).unwrap();
    assert!(matches!(result, Value::WeakSet(_)));
}

#[test]
fn test_weakset_add_has_delete() {
    let result = evaluate_script(
        r#"
        let ws = new WeakSet();
        let obj = {};
        ws.add(obj);
        ws.has(obj)
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();

    match result {
        Value::Boolean(b) => assert!(b),
        _ => panic!("Expected boolean"),
    }

    let result = evaluate_script(
        r#"
        let ws = new WeakSet();
        let obj = {};
        ws.add(obj);
        let deleted = ws.delete(obj);
        let hasAfter = ws.has(obj);
        [deleted, hasAfter]
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();

    // Should return an array-like object with [true, false]
    if let Value::Object(obj) = result {
        let key0 = PropertyKey::from("0");
        let deleted_val = obj.borrow().properties.get(&key0).unwrap().borrow().clone();
        let key1 = PropertyKey::from("1");
        let has_after_val = obj.borrow().properties.get(&key1).unwrap().borrow().clone();

        assert!(matches!(deleted_val, Value::Boolean(true)));
        assert!(matches!(has_after_val, Value::Boolean(false)));
    } else {
        panic!("Expected object");
    }
}

#[test]
fn test_weakset_non_object_value() {
    let result = evaluate_script(
        r#"
        let ws = new WeakSet();
        try {
            ws.add('string');
            'no_error'
        } catch (e) {
            'error'
        }
    "#,
        None::<&std::path::Path>,
    )
    .unwrap();

    match result {
        Value::String(s) => {
            assert_eq!(String::from_utf16_lossy(&s), "error");
        }
        _ => panic!("Expected error string"),
    }
}

#[test]
fn test_weakmap_weakset_to_string() {
    let result = evaluate_script("new WeakMap().toString()", None::<&std::path::Path>).unwrap();
    match result {
        Value::String(s) => {
            assert_eq!(String::from_utf16_lossy(&s), "[object WeakMap]");
        }
        _ => panic!("Expected string"),
    }

    let result = evaluate_script("new WeakSet().toString()", None::<&std::path::Path>).unwrap();
    match result {
        Value::String(s) => {
            assert_eq!(String::from_utf16_lossy(&s), "[object WeakSet]");
        }
        _ => panic!("Expected string"),
    }
}
