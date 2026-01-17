use crate::{
    JSError,
    core::{JSObjectDataPtr, MutationContext, PropertyKey, Value, env_set, new_js_object_data, obj_set_key_value, value_to_string},
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
            EvalError::Throw(v, l, _c) => {
                let msg = value_to_string(&v);
                let line = l.unwrap_or(0);
                crate::JSError::new(
                    crate::error::JSErrorKind::Throw(msg),
                    "unknown".to_string(),
                    line,
                    "unknown".to_string(),
                )
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
    obj_set_key_value(mc, &error_ctor, &"__is_constructor".into(), Value::Boolean(true))?;
    obj_set_key_value(mc, &error_ctor, &"__native_ctor".into(), Value::String(utf8_to_utf16("Error")))?;

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

    obj_set_key_value(mc, &error_ctor, &"prototype".into(), Value::Object(error_proto))?;
    obj_set_key_value(mc, &error_proto, &"constructor".into(), Value::Object(error_ctor))?;
    obj_set_key_value(mc, &error_proto, &"name".into(), Value::String(utf8_to_utf16("Error")))?;
    obj_set_key_value(mc, &error_proto, &"message".into(), Value::String(utf8_to_utf16("")))?;

    env_set(mc, env, "Error", Value::Object(error_ctor))?;

    let error_ctor_val = Value::Object(error_ctor);
    let error_proto_val = Value::Object(error_proto);

    initialize_native_error(mc, env, "ReferenceError", error_ctor_val.clone(), error_proto_val.clone())?;
    initialize_native_error(mc, env, "TypeError", error_ctor_val.clone(), error_proto_val.clone())?;
    initialize_native_error(mc, env, "SyntaxError", error_ctor_val.clone(), error_proto_val.clone())?;
    initialize_native_error(mc, env, "RangeError", error_ctor_val.clone(), error_proto_val.clone())?;

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
    obj_set_key_value(mc, &ctor, &"__is_constructor".into(), Value::Boolean(true))?;
    obj_set_key_value(mc, &ctor, &"__native_ctor".into(), Value::String(utf8_to_utf16(name)))?;

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

    obj_set_key_value(mc, &ctor, &"prototype".into(), Value::Object(proto))?;
    obj_set_key_value(mc, &proto, &"constructor".into(), Value::Object(ctor))?;
    obj_set_key_value(mc, &proto, &"name".into(), Value::String(utf8_to_utf16(name)))?;
    obj_set_key_value(mc, &proto, &"message".into(), Value::String(utf8_to_utf16("")))?;

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

    obj_set_key_value(mc, &error_obj, &"name".into(), Value::String(utf8_to_utf16("Error")))?;
    obj_set_key_value(mc, &error_obj, &"message".into(), message.clone())?;

    let msg_str = if let Value::String(s) = &message {
        utf16_to_utf8(s)
    } else {
        "Unknown error".to_string()
    };
    let stack_str = format!("Error: {msg_str}");
    obj_set_key_value(mc, &error_obj, &"stack".into(), Value::String(utf8_to_utf16(&stack_str)))?;

    // Internal marker to identify Error objects
    obj_set_key_value(mc, &error_obj, &"__is_error".into(), Value::Boolean(true))?;

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
