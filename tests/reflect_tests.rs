use javascript::*;

// Initialize logger for this integration test binary so `RUST_LOG` is honored.
// Using `ctor` ensures initialization runs before tests start.
#[ctor::ctor]
fn __init_test_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default()).is_test(true).try_init();
}

#[test]
fn test_reflect_has() {
    // Test Reflect.has with existing property
    let result = evaluate_script("Reflect.has({test: 1}, 'test')", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");

    // Test Reflect.has with non-existing property
    let result = evaluate_script("Reflect.has({}, 'test')", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "false");
}

#[test]
fn test_reflect_get() {
    // Test Reflect.get with existing property
    let result = evaluate_script("Reflect.get({test: 42}, 'test')", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "42");

    // Test Reflect.get with non-existing property
    let result = evaluate_script("Reflect.get({}, 'test')", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "undefined");
}

#[test]
fn test_reflect_set() {
    // Test Reflect.set
    let result = evaluate_script("let obj = {}; Reflect.set(obj, 'test', 123); obj.test", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "123");
}

#[test]
fn test_reflect_own_keys() {
    // Test Reflect.ownKeys
    let result = evaluate_script("Reflect.ownKeys({a: 1, b: 2}).length", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "2");
}

#[test]
fn test_reflect_is_extensible() {
    // Test Reflect.isExtensible
    let result = evaluate_script("Reflect.isExtensible({})", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "true");
}

#[test]
fn test_reflect_get_prototype_of() {
    // Test Reflect.getPrototypeOf returns an object (not null for regular objects)
    let result = evaluate_script("typeof Reflect.getPrototypeOf({})", None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"object\"");
}
