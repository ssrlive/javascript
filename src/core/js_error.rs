use crate::{
    JSError,
    core::{JSObjectDataPtr, MutationContext, PropertyKey, Value, env_set, new_js_object_data, obj_set_key_value, value_to_string},
    obj_get_key_value, utf8_to_utf16, utf16_to_utf8,
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

    let object_proto = if let Some(obj_val) = obj_get_key_value(mc, env, &"Object".into())?
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = obj_get_key_value(mc, obj_ctor, &"prototype".into())?
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
    if let Value::Object(obj) = val {
        if let Ok(borrowed) = obj.try_borrow() {
            return borrowed.properties.contains_key(&PropertyKey::String("__is_error".to_string()));
        }
    }
    false
}
