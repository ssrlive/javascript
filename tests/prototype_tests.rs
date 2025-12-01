use javascript::{Value, evaluate_script};

#[test]
fn test_prototype_assignment() {
    // Test __proto__ assignment
    let script = r#"
        var proto = { inheritedProp: "inherited value" };
        var obj = { ownProp: "own value" };
        obj.__proto__ = proto;
        obj
    "#;
    let result = evaluate_script(script).unwrap();
    println!("Object after __proto__ assignment: {:?}", result);
    match result {
        Value::Object(obj) => {
            let obj = obj.borrow();
            // Check if prototype was set
            assert!(obj.prototype.is_some());
            // Check if we can access the prototype's properties
            if let Some(proto_rc) = &obj.prototype {
                let proto = proto_rc.borrow();
                assert!(proto.contains_key(&"inheritedProp".into()));
            }
        }
        _ => panic!("Expected object"),
    }
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
    let result = evaluate_script(script).unwrap();
    match result {
        Value::Object(arr) => {
            // Check own property
            let own_prop = arr.borrow().get(&"0".into()).unwrap().borrow().clone();
            match own_prop {
                Value::String(s) => {
                    let expected = "own value".encode_utf16().collect::<Vec<u16>>();
                    assert_eq!(s, expected);
                }
                _ => panic!("Expected string for own property"),
            }

            // Check inherited property
            let inherited_prop = arr.borrow().get(&"1".into()).unwrap().borrow().clone();
            match inherited_prop {
                Value::String(s) => {
                    let expected = "inherited value".encode_utf16().collect::<Vec<u16>>();
                    assert_eq!(s, expected);
                }
                _ => panic!("Expected string for inherited property"),
            }
        }
        _ => panic!("Expected array"),
    }
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
    let result = evaluate_script(script).unwrap();
    match result {
        Value::Object(arr) => {
            // Check child property
            let child_prop = arr.borrow().get(&"0".into()).unwrap().borrow().clone();
            match child_prop {
                Value::String(s) => {
                    let expected = "child value".encode_utf16().collect::<Vec<u16>>();
                    assert_eq!(s, expected);
                }
                _ => panic!("Expected string for child property"),
            }

            // Check parent property
            let parent_prop = arr.borrow().get(&"1".into()).unwrap().borrow().clone();
            match parent_prop {
                Value::String(s) => {
                    let expected = "parent value".encode_utf16().collect::<Vec<u16>>();
                    assert_eq!(s, expected);
                }
                _ => panic!("Expected string for parent property"),
            }

            // Check grandparent property
            let grandparent_prop = arr.borrow().get(&"2".into()).unwrap().borrow().clone();
            match grandparent_prop {
                Value::String(s) => {
                    let expected = "grandparent value".encode_utf16().collect::<Vec<u16>>();
                    assert_eq!(s, expected);
                }
                _ => panic!("Expected string for grandparent property"),
            }
        }
        _ => panic!("Expected array"),
    }
}
