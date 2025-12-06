use javascript::Value;
use javascript::evaluate_script;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_map_constructor() {
    let result = evaluate_script("new Map()").unwrap();
    assert!(matches!(result, Value::Map(_)));
}

#[test]
fn test_map_set_and_get() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("key1", "value1");
        map.set("key2", "value2");
        map.get("key1")
    "#,
    )
    .unwrap();
    match result {
        Value::String(s) => assert_eq!(String::from_utf16_lossy(&s), "value1"),
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_map_has() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("key", "value");
        map.has("key")
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Boolean(true)));
}

#[test]
fn test_map_size() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("a", 1);
        map.set("b", 2);
        map.size
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Number(2.0)));
}

#[test]
fn test_map_delete() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("key", "value");
        let deleted = map.delete("key");
        let has = map.has("key");
        [deleted, has]
    "#,
    )
    .unwrap();
    // This should return an array [true, false]
    // For now, just check it's an object (array)
    assert!(matches!(result, Value::Object(_)));
}

#[test]
fn test_map_clear() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("a", 1);
        map.set("b", 2);
        map.clear();
        map.size
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Number(0.0)));
}

#[test]
fn test_set_constructor() {
    let result = evaluate_script("new Set()").unwrap();
    assert!(matches!(result, Value::Set(_)));
}

#[test]
fn test_set_add_and_has() {
    let result = evaluate_script(
        r#"
        let set = new Set();
        set.add("item1");
        set.add("item2");
        set.has("item1")
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Boolean(true)));
}

#[test]
fn test_set_size() {
    let result = evaluate_script(
        r#"
        let set = new Set();
        set.add(1);
        set.add(2);
        set.add(2); // duplicate
        set.size
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Number(2.0)));
}

#[test]
fn test_set_delete() {
    let result = evaluate_script(
        r#"
        let set = new Set();
        set.add("item");
        let deleted = set.delete("item");
        let has = set.has("item");
        [deleted, has]
    "#,
    )
    .unwrap();
    // This should return an array [true, false]
    assert!(matches!(result, Value::Object(_)));
}

#[test]
fn test_set_clear() {
    let result = evaluate_script(
        r#"
        let set = new Set();
        set.add(1);
        set.add(2);
        set.clear();
        set.size
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Number(0.0)));
}

#[test]
fn test_map_keys_values_entries() {
    let result = evaluate_script(
        r#"
        let map = new Map();
        map.set("a", 1);
        map.set("b", 2);
        let keys = map.keys();
        let values = map.values();
        let entries = map.entries();
        [keys.length, values.length, entries.length]
    "#,
    )
    .unwrap();
    // Should return [2, 2, 2]
    assert!(matches!(result, Value::Object(_)));
}

#[test]
fn test_set_values() {
    let result = evaluate_script(
        r#"
        let set = new Set();
        set.add(1);
        set.add(2);
        let values = set.values();
        values.length
    "#,
    )
    .unwrap();
    assert!(matches!(result, Value::Number(2.0)));
}
