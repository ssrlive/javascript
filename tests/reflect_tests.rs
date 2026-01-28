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
#[test]
fn test_object_define_property_defaults() {
    let script = r#"
            var o = {};
            Object.defineProperty(o, 'a', { value: 1 });
            var d = Object.getOwnPropertyDescriptor(o, 'a');
            [d.value === 1, d.writable, d.enumerable, d.configurable].toString();
        "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"true,false,false,false\"");
}

#[test]
fn test_object_define_property_accessor() {
    let script = r#"
            var o = {};
            Object.defineProperty(o, 'a', { get: function(){ return 7; }, enumerable: true });
            var d = Object.getOwnPropertyDescriptor(o, 'a');
            [typeof d.get === 'function', d.enumerable, d.configurable].toString();
        "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"true,true,false\"");
}

#[test]
fn test_reflect_get_own_property_descriptor() {
    let script = r#"
            var o = {};
            Object.defineProperty(o, 'a', { get: function(){ return 7; }, enumerable: true });
            var d = Reflect.getOwnPropertyDescriptor(o, 'a');
            [typeof d.get === 'function', d.enumerable, d.configurable].toString();
        "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"true,true,false\"");
}

#[test]
fn test_object_define_properties_invalid_getter_throws() {
    let script = r#"
            var o = {};
            try {
                Object.defineProperties(o, { a: { get: 5 } });
                'NO THROW';
            } catch (e) { 'THROW ' + (e.name || e); }
        "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"THROW TypeError\"");
}

#[test]
fn test_reflect_define_property_invalid_getter_returns_false() {
    let script = r#"
            var o = {};
            Reflect.defineProperty(o, 'a', { get: 5 });
        "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "false");
}

#[test]
fn test_object_define_property_mixed_descriptor_throws() {
    let script = r#"
            var o = {};
            try {
                Object.defineProperty(o, 'a', { get: function() {}, value: 1 });
                'NO THROW';
            } catch (e) { 'THROW ' + (e.name || e); }
        "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "\"THROW TypeError\"");
}

#[test]
fn test_reflect_define_property_mixed_descriptor_returns_false() {
    let script = r#"
            var o = {};
            Reflect.defineProperty(o, 'a', { get: function() {}, value: 1 });
        "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "false");
}
