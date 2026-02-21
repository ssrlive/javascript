use crate::{
    JSError,
    core::{
        InternalSlot, JSObjectDataPtr, MutationContext, PropertyKey, Value, create_descriptor_object, env_get, env_set, new_js_object_data,
        object_set_key_value, slot_set, to_primitive, value_to_string,
    },
    object_get_key_value, raise_type_error, utf8_to_utf16, utf16_to_utf8,
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
                let mut mapped_kind = None;
                if let Value::Object(obj) = &v
                    && let Some(name_rc) = object_get_key_value(obj, "name")
                    && let Value::String(name_u16) = &*name_rc.borrow()
                {
                    let name = utf16_to_utf8(name_u16);
                    let message = if let Some(message_rc) = object_get_key_value(obj, "message") {
                        match &*message_rc.borrow() {
                            Value::String(m) => utf16_to_utf8(m),
                            other => value_to_string(other),
                        }
                    } else {
                        msg.clone()
                    };

                    mapped_kind = match name.as_str() {
                        "TypeError" => Some(crate::error::JSErrorKind::TypeError { message }),
                        "RangeError" => Some(crate::error::JSErrorKind::RangeError { message }),
                        "SyntaxError" => Some(crate::error::JSErrorKind::SyntaxError { message }),
                        "ReferenceError" => Some(crate::error::JSErrorKind::ReferenceError { message }),
                        "EvalError" | "URIError" => Some(crate::error::JSErrorKind::EvaluationError { message }),
                        _ => None,
                    };
                }

                let mut e = if let Some(kind) = mapped_kind {
                    crate::make_js_error!(kind)
                } else {
                    crate::make_js_error!(crate::error::JSErrorKind::Throw(msg))
                };
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
    slot_set(mc, &error_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &error_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Error")));
    object_set_key_value(mc, &error_ctor, "name", &Value::String(utf8_to_utf16("Error")))?;

    // Error.length = 1 (non-enumerable, non-writable, configurable)
    let len_desc = create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
    crate::js_object::define_property_internal(mc, &error_ctor, "length", &len_desc)?;
    let name_desc = create_descriptor_object(mc, &Value::String(utf8_to_utf16("Error")), false, false, true)?;
    crate::js_object::define_property_internal(mc, &error_ctor, "name", &name_desc)?;

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

    object_set_key_value(mc, &error_ctor, "prototype", &Value::Object(error_proto))?;
    error_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    error_ctor.borrow_mut(mc).set_non_writable("prototype");
    error_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &error_proto, "constructor", &Value::Object(error_ctor))?;
    error_proto.borrow_mut(mc).set_non_enumerable("constructor");
    object_set_key_value(mc, &error_proto, "name", &Value::String(utf8_to_utf16("Error")))?;
    error_proto.borrow_mut(mc).set_non_enumerable("name");
    object_set_key_value(mc, &error_proto, "message", &Value::String(utf8_to_utf16("")))?;
    error_proto.borrow_mut(mc).set_non_enumerable("message");
    // Provide Error.prototype.toString implementation
    let val = Value::Function("Error.prototype.toString".to_string());
    object_set_key_value(mc, &error_proto, "toString", &val)?;
    error_proto.borrow_mut(mc).set_non_enumerable("toString");

    // Provide Error.isError static method
    let is_error_fn = Value::Function("Error.isError".to_string());
    object_set_key_value(mc, &error_ctor, "isError", &is_error_fn)?;
    error_ctor.borrow_mut(mc).set_non_enumerable("isError");

    env_set(mc, env, "Error", &Value::Object(error_ctor))?;

    let error_ctor_val = Value::Object(error_ctor);
    let error_proto_val = Value::Object(error_proto);

    initialize_native_error(mc, env, "ReferenceError", &error_ctor_val, &error_proto_val)?;
    initialize_native_error(mc, env, "TypeError", &error_ctor_val, &error_proto_val)?;
    initialize_native_error(mc, env, "SyntaxError", &error_ctor_val, &error_proto_val)?;
    initialize_native_error(mc, env, "RangeError", &error_ctor_val, &error_proto_val)?;
    initialize_native_error(mc, env, "EvalError", &error_ctor_val, &error_proto_val)?;
    initialize_native_error(mc, env, "URIError", &error_ctor_val, &error_proto_val)?;
    initialize_native_error(mc, env, "AggregateError", &error_ctor_val, &error_proto_val)?;

    Ok(())
}

fn initialize_native_error<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    name: &str,
    parent_ctor: &Value<'gc>,
    parent_proto: &Value<'gc>,
) -> Result<(), JSError> {
    let ctor = new_js_object_data(mc);
    if let Value::Object(parent_ctor_obj) = parent_ctor {
        ctor.borrow_mut(mc).prototype = Some(*parent_ctor_obj);
    } else if let Some(func_val_rc) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val_rc.borrow()
        && let Some(func_proto_rc) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_rc.borrow()
    {
        ctor.borrow_mut(mc).prototype = Some(*func_proto);
    }
    slot_set(mc, &ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16(name)));
    object_set_key_value(mc, &ctor, "name", &Value::String(utf8_to_utf16(name)))?;

    // Set prototype of constructor to parent constructor (Error) so strict inheritance works if checked
    // However, usually Foo.__proto__ === Function.prototype.
    // But in class inheritance: class Ref extends Error {} -> Ref.__proto__ === Error.
    // Native errors behave like subclasses.

    // For simplicity, let's just make sure the prototype property is set up correctly.

    let proto = new_js_object_data(mc);
    // ReferenceError.prototype.__proto__ === Error.prototype
    if let Value::Object(parent_p_obj) = parent_proto {
        proto.borrow_mut(mc).prototype = Some(*parent_p_obj);
    }

    object_set_key_value(mc, &ctor, "prototype", &Value::Object(proto))?;
    ctor.borrow_mut(mc).set_non_enumerable("prototype");
    ctor.borrow_mut(mc).set_non_writable("prototype");
    ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &proto, "constructor", &Value::Object(ctor))?;
    object_set_key_value(mc, &proto, "name", &Value::String(utf8_to_utf16(name)))?;
    object_set_key_value(mc, &proto, "message", &Value::String(utf8_to_utf16("")))?;

    proto.borrow_mut(mc).set_non_enumerable("constructor");
    proto.borrow_mut(mc).set_non_enumerable("name");
    proto.borrow_mut(mc).set_non_enumerable("message");

    // Set length, name as non-enumerable/non-writable/configurable data properties
    {
        let length_val = if name == "AggregateError" { 2.0 } else { 1.0 };
        let len_desc = create_descriptor_object(mc, &Value::Number(length_val), false, false, true)?;
        crate::js_object::define_property_internal(mc, &ctor, "length", &len_desc)?;

        let name_desc = create_descriptor_object(mc, &Value::String(utf8_to_utf16(name)), false, false, true)?;
        crate::js_object::define_property_internal(mc, &ctor, "name", &name_desc)?;
    }

    env_set(mc, env, name, &Value::Object(ctor))?;
    Ok(())
}

pub fn create_aggregate_error<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    prototype: Option<JSObjectDataPtr<'gc>>,
    errors: Value<'gc>,
    message: Option<Value<'gc>>,
    options: Option<Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let message_value = if let Some(msg_val) = message {
        if matches!(msg_val, Value::Undefined) {
            Value::Undefined
        } else {
            let prim = to_primitive(mc, &msg_val, "string", env)?;
            if matches!(prim, Value::Symbol(_)) {
                return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
            }
            Value::String(utf8_to_utf16(&value_to_string(&prim)))
        }
    } else {
        Value::Undefined
    };

    let err_obj_val = create_error(mc, prototype, message_value).map_err(EvalError::from)?;
    let err_obj = match &err_obj_val {
        Value::Object(o) => *o,
        _ => return Ok(err_obj_val),
    };

    let mut collected_errors: Vec<Value<'gc>> = Vec::new();
    let iterator = if let Some(sym_ctor) = env_get(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
        && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
    {
        match &errors {
            Value::Object(obj) => {
                let method = crate::core::eval::get_property_with_accessors(mc, env, obj, iter_sym_data)?;
                if matches!(method, Value::Undefined | Value::Null) {
                    return Err(raise_type_error!("Object is not iterable").into());
                }
                let iter = crate::core::eval::evaluate_call_dispatch(mc, env, &method, Some(&errors), &[])?;
                match iter {
                    Value::Object(iter_obj) => iter_obj,
                    _ => return Err(raise_type_error!("Iterator is not an object").into()),
                }
            }
            _ => return Err(raise_type_error!("Object is not iterable").into()),
        }
    } else {
        return Err(raise_type_error!("Object is not iterable").into());
    };

    loop {
        let next_method = crate::core::eval::get_property_with_accessors(mc, env, &iterator, "next")?;
        if matches!(next_method, Value::Undefined | Value::Null) {
            return Err(raise_type_error!("Iterator has no next method").into());
        }
        let next_result = crate::core::eval::evaluate_call_dispatch(mc, env, &next_method, Some(&Value::Object(iterator)), &[])?;
        let next_obj = match next_result {
            Value::Object(obj) => obj,
            _ => return Err(raise_type_error!("Iterator result is not an object").into()),
        };

        let done = crate::core::eval::get_property_with_accessors(mc, env, &next_obj, "done")?;
        if done.to_truthy() {
            break;
        }

        let value = crate::core::eval::get_property_with_accessors(mc, env, &next_obj, "value")?;
        collected_errors.push(value);
    }

    let errors_array = crate::js_array::create_array(mc, env).map_err(EvalError::from)?;
    for (i, value) in collected_errors.iter().enumerate() {
        object_set_key_value(mc, &errors_array, i, value).map_err(EvalError::from)?;
    }
    crate::core::object_set_length(mc, &errors_array, collected_errors.len()).map_err(EvalError::from)?;

    object_set_key_value(mc, &err_obj, "errors", &Value::Object(errors_array)).map_err(EvalError::from)?;
    err_obj.borrow_mut(mc).set_non_enumerable("errors");

    if let Some(options_val) = options
        && let Value::Object(options_obj) = options_val
        && object_get_key_value(&options_obj, "cause").is_some()
    {
        let cause_val = crate::core::eval::get_property_with_accessors(mc, env, &options_obj, "cause")?;
        object_set_key_value(mc, &err_obj, "cause", &cause_val).map_err(EvalError::from)?;
        err_obj.borrow_mut(mc).set_non_enumerable("cause");
    }

    Ok(Value::Object(err_obj))
}

/// Create a new Error object with the given message.
pub fn create_error<'gc>(
    mc: &MutationContext<'gc>,
    prototype: Option<JSObjectDataPtr<'gc>>,
    message: Value<'gc>,
) -> Result<Value<'gc>, JSError> {
    let error_obj = new_js_object_data(mc);
    error_obj.borrow_mut(mc).prototype = prototype;

    let msg_str = match message {
        Value::Undefined => String::new(),
        Value::String(s) => {
            object_set_key_value(mc, &error_obj, "message", &Value::String(s.clone()))?;
            error_obj.borrow_mut(mc).set_non_enumerable("message");
            utf16_to_utf8(&s)
        }
        other => {
            let s = utf8_to_utf16(&value_to_string(&other));
            object_set_key_value(mc, &error_obj, "message", &Value::String(s.clone()))?;
            error_obj.borrow_mut(mc).set_non_enumerable("message");
            utf16_to_utf8(&s)
        }
    };

    let stack_str = format!("Error: {msg_str}");
    object_set_key_value(mc, &error_obj, "stack", &Value::String(utf8_to_utf16(&stack_str)))?;
    // Make stack non-enumerable by default
    error_obj.borrow_mut(mc).set_non_enumerable("stack");

    // If a prototype was provided, mirror its constructor onto the instance
    if let Some(proto) = prototype
        && let Some(ctor_val) = object_get_key_value(&proto, "constructor")
    {
        object_set_key_value(mc, &error_obj, "constructor", &ctor_val.borrow())?;
    }

    // Internal marker to identify Error objects
    slot_set(mc, &error_obj, InternalSlot::IsError, &Value::Boolean(true));
    // Make internal marker non-enumerable so it doesn't show up in enumerations

    Ok(Value::Object(error_obj))
}

/// Check if a value is an Error object.
pub fn is_error<'gc>(val: &Value<'gc>) -> bool {
    if let Value::Object(obj) = val
        && let Ok(borrowed) = obj.try_borrow()
    {
        return borrowed.properties.contains_key(&PropertyKey::Internal(InternalSlot::IsError));
    }
    false
}
