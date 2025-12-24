use javascript::*;
use std::rc::Rc;

#[test]
fn prototype_chain_lookup_demo() {
    // parent object with own property 'foo' = "bar"
    let parent = new_js_object_data();
    obj_set_key_value(
        &parent,
        &PropertyKey::String("foo".to_string()),
        Value::String(utf8_to_utf16("bar")),
    )
    .unwrap();

    // child object whose prototype is parent
    let child = new_js_object_data();
    child.borrow_mut().prototype = Some(Rc::downgrade(&parent));

    // lookup on child should find parent's 'foo' via prototype chain
    let found = obj_get_key_value(&child, &PropertyKey::String("foo".to_string())).unwrap();
    assert!(found.is_some(), "Expected to find 'foo' via prototype chain");
    if let Some(rcv) = found {
        match &*rcv.borrow() {
            Value::String(s) => assert_eq!(utf16_to_utf8(s), "bar"),
            other => panic!("Unexpected value type: {:?}", other),
        }
    }
}
