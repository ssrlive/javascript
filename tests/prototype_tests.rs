use javascript::evaluate_script;

#[test]
fn test_prototype_assignment() {
    // Test __proto__ assignment
    let script = r#"
        var proto = { inheritedProp: "inherited value" };
        var obj = { ownProp: "own value" };
        obj.__proto__ = proto;
        obj
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "{\"ownProp\":\"own value\",\"inheritedProp\":\"inherited value\"}");
}

#[test]
fn test_prototype_chain_lookup() {
    // Test prototype chain property lookup
    let script = r#"
        var proto = { inheritedProp: "inherited value" };
        var obj = { ownProp: "own value" };
        obj.__proto__ = proto;
        [obj.ownProp, obj.inheritedProp]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[\"own value\",\"inherited value\"]");
}

#[test]
fn test_multi_level_prototype_chain() {
    // Test multi-level prototype chain
    let script = r#"
        var grandparent = { grandparentProp: "grandparent value" };
        var parent = { parentProp: "parent value" };
        parent.__proto__ = grandparent;
        var child = { childProp: "child value" };
        child.__proto__ = parent;
        [child.childProp, child.parentProp, child.grandparentProp]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[\"child value\",\"parent value\",\"grandparent value\"]");
}

#[test]
fn test_has_own_property_symbol_and_inherited() {
    let script = r#"
        var s = Symbol('x');
        var proto = { inherited: 'yes' };
        var obj = { own: 'ok' };
        obj.__proto__ = proto;
        obj[s] = 42;
        [ obj.hasOwnProperty('own'), obj.hasOwnProperty('inherited'), obj.hasOwnProperty(s) ]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[true,false,true]");
}

#[test]
fn test_is_prototype_of_and_property_is_enumerable() {
    let script = r#"
        var proto = { p: 1 };
        var obj = Object.create(proto);
        obj.q = 2;
        [ proto.isPrototypeOf(obj), obj.isPrototypeOf(proto), obj.propertyIsEnumerable('q'), obj.propertyIsEnumerable('p') ]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[true,false,true,false]");
}

#[test]
fn test_override_has_own_property() {
    let script = r#"
        var proto = { inherited: 'yes' };
        var obj = { own: 'ok' };
        obj.__proto__ = proto;
        obj.hasOwnProperty = function(k) { return 'override-' + k; };
        var keys = Object.keys(obj);
        var descs = Object.getOwnPropertyDescriptors(obj);
        [obj.hasOwnProperty === obj.__proto__.hasOwnProperty, keys, descs.hasOwnProperty ? 'own' : 'none', obj.hasOwnProperty('own')]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[false,[\"own\",\"hasOwnProperty\"],\"own\",\"override-own\"]");
}

#[test]
fn test_to_string_default_and_tag() {
    let script = r#"
        var o1 = {};
        var tag = Symbol.toStringTag;
        var o2 = {};
        o2[tag] = 'Custom';
        [ o1.toString(), o2.toString() ]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[\"[object Object]\",\"[object Custom]\"]");
}

#[test]
fn test_to_locale_string_defaults_and_override() {
    let script = r#"
        var o1 = {};
        var tag = Symbol.toStringTag;
        var o2 = {};
        o2[tag] = 'Custom';
        var o3 = {};
        o3.toLocaleString = function() { return 'my-locale'; };
        [ o1.toLocaleString(), o2.toLocaleString(), o3.toLocaleString() ]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[\"[object Object]\",\"[object Custom]\",\"my-locale\"]");
}

#[test]
fn test_to_string_override_and_valueof() {
    let script = r#"
        var proto = {};
        var o = { own: 1 };
        o.__proto__ = proto;
        o.toString = function() { return 'my-toString'; };
        var v = o.valueOf();
        [ o.toString(), proto.toString(), o === v ]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[\"my-toString\",\"[object Object]\",true]");
}

#[test]
fn test_class_instance_to_string_inherits_object_prototype() {
    let script = r#"
        class C {}
        [ (new C()).toString(), Object.prototype.toString.call(new C()) ]
    "#;
    let result = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(result, "[\"[object Object]\",\"[object Object]\"]");
}
