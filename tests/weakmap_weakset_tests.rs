use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_weakmap_constructor() {
    let result = evaluate_script("new WeakMap()", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[object WeakMap]");

    let result = evaluate_script("new WeakMap([])", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[object WeakMap]");
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
    assert_eq!(result, "\"value\"");

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
    assert_eq!(result, "true");

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
    assert_eq!(result, "[true,false]");
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
    assert_eq!(result, "\"error\"");
}

#[test]
fn test_weakset_constructor() {
    let result = evaluate_script("new WeakSet()", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[object WeakSet]");

    let result = evaluate_script("new WeakSet([])", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[object WeakSet]");
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
    assert_eq!(result, "true");

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

    assert_eq!(result, "[true,false]");
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

    assert_eq!(result, "\"error\"");
}

#[test]
fn test_weakmap_weakset_to_string() {
    let result = evaluate_script("new WeakMap().toString()", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"[object WeakMap]\"");

    let result = evaluate_script("new WeakSet().toString()", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"[object WeakSet]\"");
}
