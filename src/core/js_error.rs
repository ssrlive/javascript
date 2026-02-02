use crate::{
    JSError,
    core::{JSObjectDataPtr, MutationContext, PropertyKey, Value, env_set, new_js_object_data, object_set_key_value, value_to_string},
    object_get_key_value, utf8_to_utf16, utf16_to_utf8,
};

#[derive(Debug)]
pub enum EvalError<'gc> {
    Js(JSError),
    Throw(Value<'gc>, Option<usize>, Option<usize>),
}

impl<'gc> From<JSError> for EvalError<'gc> {
    fn from(e: JSError) -> Self {
        EvalError::Js(e)
    }
}

impl<'gc> From<EvalError<'gc>> for JSError {
    fn from(e: EvalError<'gc>) -> Self {
        match e {
            EvalError::Js(j) => j,
            EvalError::Throw(v, line, column) => {
                let msg = value_to_string(&v);
                let mut e = crate::make_js_error!(crate::error::JSErrorKind::Throw(msg));
                e.inner.js_line = line;
                e.inner.js_column = column;
                e
            }
        }
    }
}

impl<'gc> EvalError<'gc> {
    #[allow(dead_code)]
    pub fn message(&self) -> String {
        match self {
            EvalError::Js(e) => e.message(),
            EvalError::Throw(v, ..) => value_to_string(v),
        }
    }
}

/// Initialize the Error constructor and its prototype.
pub fn initialize_error_constructor<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let error_ctor = new_js_object_data(mc);
    // Set the internal prototype of the constructor object so it behaves like a Function
    if let Some(func_val_rc) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val_rc.borrow()
        && let Some(func_proto_rc) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_rc.borrow()
    {
        error_ctor.borrow_mut(mc).prototype = Some(*func_proto);
    }
    object_set_key_value(mc, &error_ctor, "__is_constructor", Value::Boolean(true))?;
    object_set_key_value(mc, &error_ctor, "__native_ctor", Value::String(utf8_to_utf16("Error")))?;

    // We need Object.prototype to set as the prototype of Error.prototype
    // If Object is not yet initialized, we might have an issue, but usually Object is init first.
    // However, in the current core.rs, Object is initialized right before Error.
    // We can try to retrieve Object from env.

    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let error_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        error_proto.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(mc, &error_ctor, "prototype", Value::Object(error_proto))?;
    object_set_key_value(mc, &error_proto, "constructor", Value::Object(error_ctor))?;
    object_set_key_value(mc, &error_proto, "name", Value::String(utf8_to_utf16("Error")))?;
    object_set_key_value(mc, &error_proto, "message", Value::String(utf8_to_utf16("")))?;
    // Provide Error.prototype.toString implementation
    let val = Value::Function("Error.prototype.toString".to_string());
    object_set_key_value(mc, &error_proto, "toString", val)?;
    error_proto.borrow_mut(mc).set_non_enumerable("toString");

    env_set(mc, env, "Error", Value::Object(error_ctor))?;

    let error_ctor_val = Value::Object(error_ctor);
    let error_proto_val = Value::Object(error_proto);

    initialize_native_error(mc, env, "ReferenceError", error_ctor_val.clone(), error_proto_val.clone())?;
    initialize_native_error(mc, env, "TypeError", error_ctor_val.clone(), error_proto_val.clone())?;
    initialize_native_error(mc, env, "SyntaxError", error_ctor_val.clone(), error_proto_val.clone())?;
    initialize_native_error(mc, env, "RangeError", error_ctor_val.clone(), error_proto_val.clone())?;
    initialize_native_error(mc, env, "EvalError", error_ctor_val.clone(), error_proto_val.clone())?;
    initialize_native_error(mc, env, "URIError", error_ctor_val.clone(), error_proto_val.clone())?;

    Ok(())
}

fn initialize_native_error<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    name: &str,
    _parent_ctor: Value<'gc>,
    parent_proto: Value<'gc>,
) -> Result<(), JSError> {
    let ctor = new_js_object_data(mc);
    // Ensure the native ctor object has Function.prototype as its internal prototype
    if let Some(func_val_rc) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val_rc.borrow()
        && let Some(func_proto_rc) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_rc.borrow()
    {
        ctor.borrow_mut(mc).prototype = Some(*func_proto);
    }
    object_set_key_value(mc, &ctor, "__is_constructor", Value::Boolean(true))?;
    object_set_key_value(mc, &ctor, "__native_ctor", Value::String(utf8_to_utf16(name)))?;

    // Set prototype of constructor to parent constructor (Error) so strict inheritance works if checked
    // However, usually Foo.__proto__ === Function.prototype.
    // But in class inheritance: class Ref extends Error {} -> Ref.__proto__ === Error.
    // Native errors behave like subclasses.

    // For simplicity, let's just make sure the prototype property is set up correctly.

    let proto = new_js_object_data(mc);
    // ReferenceError.prototype.__proto__ === Error.prototype
    if let Value::Object(parent_p_obj) = parent_proto {
        proto.borrow_mut(mc).prototype = Some(parent_p_obj);
    }

    object_set_key_value(mc, &ctor, "prototype", Value::Object(proto))?;
    object_set_key_value(mc, &proto, "constructor", Value::Object(ctor))?;
    object_set_key_value(mc, &proto, "name", Value::String(utf8_to_utf16(name)))?;
    object_set_key_value(mc, &proto, "message", Value::String(utf8_to_utf16("")))?;

    env_set(mc, env, name, Value::Object(ctor))?;
    Ok(())
}

/// Create a new Error object with the given message.
pub fn create_error<'gc>(
    mc: &MutationContext<'gc>,
    prototype: Option<JSObjectDataPtr<'gc>>,
    message: Value<'gc>,
) -> Result<Value<'gc>, JSError> {
    let error_obj = new_js_object_data(mc);
    error_obj.borrow_mut(mc).prototype = prototype;

    object_set_key_value(mc, &error_obj, "message", message.clone())?;
    // Make message non-enumerable by default
    error_obj.borrow_mut(mc).set_non_enumerable("message");

    let msg_str = if let Value::String(s) = &message {
        utf16_to_utf8(s)
    } else {
        "Unknown error".to_string()
    };
    let stack_str = format!("Error: {msg_str}");
    object_set_key_value(mc, &error_obj, "stack", Value::String(utf8_to_utf16(&stack_str)))?;
    // Make stack non-enumerable by default
    error_obj.borrow_mut(mc).set_non_enumerable("stack");

    // If a prototype was provided, mirror its constructor onto the instance
    if let Some(proto) = prototype
        && let Some(ctor_val) = object_get_key_value(&proto, "constructor")
    {
        object_set_key_value(mc, &error_obj, "constructor", ctor_val.borrow().clone())?;
    }

    // Internal marker to identify Error objects
    object_set_key_value(mc, &error_obj, "__is_error", Value::Boolean(true))?;
    // Make internal marker non-enumerable so it doesn't show up in enumerations
    error_obj.borrow_mut(mc).set_non_enumerable("__is_error");

    Ok(Value::Object(error_obj))
}

/// Check if a value is an Error object.
pub fn is_error<'gc>(val: &Value<'gc>) -> bool {
    if let Value::Object(obj) = val
        && let Ok(borrowed) = obj.try_borrow()
    {
        return borrowed.properties.contains_key(&PropertyKey::String("__is_error".to_string()));
    }
    false
}
