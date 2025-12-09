use javascript::{JSErrorKind, evaluate_script};

#[test]
fn symbol_to_number_in_relational_should_throw() {
    // Try an explicit comparison that triggers ToNumber coercion path
    let script = "Symbol() < 5";
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Err(err) => match err.kind() {
            JSErrorKind::TypeError { message, .. } => assert!(message.contains("Cannot convert Symbol")),
            _ => panic!("Expected TypeError for Symbol to number coercion, got {:?}", err),
        },
        Ok(v) => panic!("expected TypeError, got {:?}", v),
    }
}

#[test]
fn symbol_to_number_in_add_should_throw() {
    // '+' with number attempts ToPrimitive then numeric coercion; Symbol should cause TypeError
    let script = "1 + Symbol()";
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Err(err) => match err.kind() {
            JSErrorKind::TypeError { message, .. } => assert!(message.contains("Cannot convert Symbol")),
            _ => panic!("Expected TypeError for Symbol to number coercion, got {:?}", err),
        },
        Ok(v) => panic!("expected TypeError, got {:?}", v),
    }
}

#[test]
fn symbol_to_primitive_method_must_return_primitive() {
    // If Symbol.toPrimitive returns non-primitive, ToPrimitive should throw TypeError
    let script = r#"
        let o = { [Symbol.toPrimitive]() { return {x:1}; } };
        1 + o
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Err(err) => match err.kind() {
            JSErrorKind::TypeError { message, .. } => {
                assert!(message.contains("must return a primitive") || message.contains("Cannot convert"))
            }
            _ => panic!("Expected TypeError for Symbol.toPrimitive returning non-primitive, got {:?}", err),
        },
        Ok(v) => panic!("expected TypeError, got {:?}", v),
    }
}
