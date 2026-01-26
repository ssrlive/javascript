use crate::core::MutationContext;
use crate::core::{
    JSObjectDataPtr, PropertyKey, Value, new_js_object_data, object_get_key_value, object_set_key_value, prepare_function_call_env,
};
use crate::js_array::{get_array_length, set_array_length};
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use crate::{JSError, core::EvalError};

/// Initialize the Reflect object with all reflection methods
pub fn initialize_reflect<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let reflect_obj = new_js_object_data(mc);
    object_set_key_value(mc, &reflect_obj, "apply", Value::Function("Reflect.apply".to_string()))?;
    object_set_key_value(mc, &reflect_obj, "construct", Value::Function("Reflect.construct".to_string()))?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "defineProperty",
        Value::Function("Reflect.defineProperty".to_string()),
    )?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "deleteProperty",
        Value::Function("Reflect.deleteProperty".to_string()),
    )?;
    object_set_key_value(mc, &reflect_obj, "get", Value::Function("Reflect.get".to_string()))?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "getOwnPropertyDescriptor",
        Value::Function("Reflect.getOwnPropertyDescriptor".to_string()),
    )?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "getPrototypeOf",
        Value::Function("Reflect.getPrototypeOf".to_string()),
    )?;
    object_set_key_value(mc, &reflect_obj, "has", Value::Function("Reflect.has".to_string()))?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "isExtensible",
        Value::Function("Reflect.isExtensible".to_string()),
    )?;
    object_set_key_value(mc, &reflect_obj, "ownKeys", Value::Function("Reflect.ownKeys".to_string()))?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "preventExtensions",
        Value::Function("Reflect.preventExtensions".to_string()),
    )?;
    object_set_key_value(mc, &reflect_obj, "set", Value::Function("Reflect.set".to_string()))?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "setPrototypeOf",
        Value::Function("Reflect.setPrototypeOf".to_string()),
    )?;

    crate::core::env_set(mc, env, "Reflect", Value::Object(reflect_obj))?;
    Ok(())
}

/// Handle Reflect object method calls
pub fn handle_reflect_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "apply" => {
            if args.len() < 2 {
                return Err(EvalError::Js(raise_type_error!("Reflect.apply requires at least 2 arguments")));
            }
            let target = args[0].clone();
            let this_arg = args[1].clone();
            let arguments_list = if args.len() > 2 { args[2].clone() } else { Value::Undefined };

            // Build argument Value list from array-like arguments_list
            let mut arg_values: Vec<Value> = Vec::new();
            match arguments_list {
                Value::Object(arr_obj) => {
                    // Expect an array-like object
                    if crate::js_array::is_array(mc, &arr_obj) {
                        if let Some(len) = get_array_length(mc, &arr_obj) {
                            for i in 0..len {
                                if let Some(val_rc) = object_get_key_value(&arr_obj, i) {
                                    arg_values.push(val_rc.borrow().clone());
                                } else {
                                    arg_values.push(Value::Undefined);
                                }
                            }
                        }
                    } else {
                        return Err(EvalError::Js(raise_type_error!(
                            "Reflect.apply argumentsList must be an array-like object"
                        )));
                    }
                }
                Value::Undefined => {}
                _ => {
                    return Err(EvalError::Js(raise_type_error!(
                        "Reflect.apply argumentsList must be an array-like object"
                    )));
                }
            }

            // If target is a native constructor object (e.g., String), call its native handler
            if let Value::Object(obj) = &target
                && let Some(native_rc) = object_get_key_value(obj, "__native_ctor")
                && let Value::String(name_utf16) = &*native_rc.borrow()
            {
                let name = utf16_to_utf8(name_utf16);
                return crate::js_function::handle_global_function(mc, &name, &arg_values, env);
            }

            // If target is a closure (sync or async) or an object wrapping a closure, invoke appropriately
            if let Some((_params, _body, _captured_env)) = crate::core::extract_closure_from_value(&target) {
                // Detect async closure (unused here; dispatcher handles it internally)
                let _is_async = matches!(target, Value::AsyncClosure(_))
                    || (if let Value::Object(obj) = &target {
                        if let Some(cl_ptr) = object_get_key_value(obj, "__closure__") {
                            matches!(&*cl_ptr.borrow(), Value::AsyncClosure(_))
                        } else {
                            false
                        }
                    } else {
                        false
                    });

                // Delegate invocation to existing call dispatcher which handles sync/async/native functions
                return crate::core::evaluate_call_dispatch(mc, env, target.clone(), Some(this_arg.clone()), arg_values);
            }

            match target {
                Value::Function(func_name) => Ok(crate::js_function::handle_global_function(mc, &func_name, &arg_values, env)?),
                Value::Object(object) => {
                    // If this object wraps an internal closure (function-object), invoke it
                    if let Some(cl_rc) = object_get_key_value(&object, "__closure__") {
                        let cl_val = cl_rc.borrow().clone();
                        if let Some((params, body, captured_env)) = crate::core::extract_closure_from_value(&cl_val) {
                            let func_env = prepare_function_call_env(
                                mc,
                                Some(&captured_env),
                                Some(this_arg.clone()),
                                Some(&params),
                                &arg_values,
                                None,
                                Some(env),
                            )?;
                            return crate::core::evaluate_statements(mc, &func_env, &body);
                        }
                    }
                    Err(EvalError::Js(raise_type_error!("Reflect.apply target is not callable")))
                }
                _ => Err(EvalError::Js(raise_type_error!("Reflect.apply target is not callable"))),
            }
        }
        "construct" => {
            if args.is_empty() {
                return Err(EvalError::Js(raise_type_error!("Reflect.construct requires at least 1 argument")));
            }
            let target = args[0].clone();
            let arguments_list = if args.len() > 1 { args[1].clone() } else { Value::Undefined };
            let _new_target = if args.len() > 2 { args[2].clone() } else { target.clone() };

            // Build argument list from array-like arguments_list
            let mut arg_values: Vec<Value> = Vec::new();
            match arguments_list {
                Value::Object(arr_obj) => {
                    if crate::js_array::is_array(mc, &arr_obj) {
                        if let Some(len) = get_array_length(mc, &arr_obj) {
                            for i in 0..len {
                                if let Some(val_rc) = object_get_key_value(&arr_obj, i) {
                                    arg_values.push(val_rc.borrow().clone());
                                } else {
                                    arg_values.push(Value::Undefined);
                                }
                            }
                        }
                    } else {
                        return Err(EvalError::Js(raise_type_error!(
                            "Reflect.construct argumentsList must be an array-like object"
                        )));
                    }
                }
                Value::Undefined => {}
                _ => {
                    return Err(EvalError::Js(raise_type_error!(
                        "Reflect.construct argumentsList must be an array-like object"
                    )));
                }
            }

            crate::js_class::evaluate_new(mc, env, target, &arg_values)
        }
        "defineProperty" => {
            if args.len() < 3 {
                return Err(EvalError::Js(raise_type_error!("Reflect.defineProperty requires 3 arguments")));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();
            let attributes = args[2].clone();

            match target {
                Value::Object(obj) => {
                    // For now, just set the property with the value from attributes
                    // This is a simplified implementation
                    if let Value::Object(attr_obj) = &attributes {
                        if let Some(value_rc) = object_get_key_value(attr_obj, "value") {
                            let prop_key = match property_key {
                                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                                Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                                _ => return Err(EvalError::Js(raise_type_error!("Invalid property key"))),
                            };
                            object_set_key_value(mc, &obj, &prop_key, value_rc.borrow().clone())?;
                            Ok(Value::Boolean(true))
                        } else {
                            Ok(Value::Boolean(false))
                        }
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                _ => Err(EvalError::Js(raise_type_error!("Reflect.defineProperty target must be an object"))),
            }
        }
        "deleteProperty" => {
            if args.len() < 2 {
                return Err(EvalError::Js(raise_type_error!("Reflect.deleteProperty requires 2 arguments")));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        _ => return Err(EvalError::Js(raise_type_error!("Invalid property key"))),
                    };
                    // For now, always return true as we don't have configurable properties
                    let _ = obj.borrow_mut(mc).properties.shift_remove(&prop_key);
                    Ok(Value::Boolean(true))
                }
                _ => Err(EvalError::Js(raise_type_error!("Reflect.deleteProperty target must be an object"))),
            }
        }
        "get" => {
            if args.len() < 2 {
                return Err(EvalError::Js(raise_type_error!("Reflect.get requires at least 2 arguments")));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();
            let _receiver = if args.len() > 2 { args[2].clone() } else { target.clone() };

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        _ => return Err(EvalError::Js(raise_type_error!("Invalid property key"))),
                    };
                    if let Some(value_rc) = object_get_key_value(&obj, &prop_key) {
                        Ok(value_rc.borrow().clone())
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(EvalError::Js(raise_type_error!("Reflect.get target must be an object"))),
            }
        }
        "getOwnPropertyDescriptor" => {
            if args.len() < 2 {
                return Err(EvalError::Js(raise_type_error!(
                    "Reflect.getOwnPropertyDescriptor requires 2 arguments"
                )));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        _ => return Err(EvalError::Js(raise_type_error!("Invalid property key"))),
                    };
                    if let Some(value_rc) = object_get_key_value(&obj, &prop_key) {
                        // Create a descriptor object
                        let descriptor = new_js_object_data(mc);
                        object_set_key_value(mc, &descriptor, "value", value_rc.borrow().clone())?;
                        object_set_key_value(mc, &descriptor, "writable", Value::Boolean(true))?;
                        object_set_key_value(mc, &descriptor, "enumerable", Value::Boolean(true))?;
                        object_set_key_value(mc, &descriptor, "configurable", Value::Boolean(true))?;
                        Ok(Value::Object(descriptor))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(EvalError::Js(raise_type_error!(
                    "Reflect.getOwnPropertyDescriptor target must be an object"
                ))),
            }
        }
        "getPrototypeOf" => {
            if args.is_empty() {
                return Err(EvalError::Js(raise_type_error!("Reflect.getPrototypeOf requires 1 argument")));
            }
            match &args[0] {
                Value::Object(obj) => {
                    if let Some(proto_rc) = obj.borrow().prototype {
                        Ok(Value::Object(proto_rc))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(EvalError::Js(raise_type_error!("Reflect.getPrototypeOf target must be an object"))),
            }
        }
        "has" => {
            if args.len() < 2 {
                return Err(EvalError::Js(raise_type_error!("Reflect.has requires 2 arguments")));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        _ => return Err(EvalError::Js(raise_type_error!("Invalid property key"))),
                    };
                    let has_prop = object_get_key_value(&obj, &prop_key).is_some();
                    Ok(Value::Boolean(has_prop))
                }
                _ => Err(EvalError::Js(raise_type_error!("Reflect.has target must be an object"))),
            }
        }
        "isExtensible" => {
            if args.is_empty() {
                return Err(EvalError::Js(raise_type_error!("Reflect.isExtensible requires 1 argument")));
            }
            let target = args[0].clone();

            match target {
                Value::Object(obj) => Ok(Value::Boolean(obj.borrow().is_extensible())),
                _ => Err(EvalError::Js(raise_type_error!("Reflect.isExtensible target must be an object"))),
            }
        }
        "ownKeys" => {
            if args.is_empty() {
                return Err(EvalError::Js(raise_type_error!("Reflect.ownKeys requires 1 argument")));
            }
            match args[0] {
                Value::Object(obj) => {
                    let mut keys = Vec::new();
                    for key in obj.borrow().properties.keys() {
                        if let PropertyKey::String(s) = key {
                            keys.push(Value::String(utf8_to_utf16(s)));
                        }
                    }
                    let keys_len = keys.len();
                    // Create an array-like object for keys
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    for (i, key) in keys.into_iter().enumerate() {
                        object_set_key_value(mc, &result_obj, i, key)?;
                    }
                    // Set length property
                    set_array_length(mc, &result_obj, keys_len)?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(EvalError::Js(raise_type_error!("Reflect.ownKeys target must be an object"))),
            }
        }
        "preventExtensions" => {
            if args.is_empty() {
                return Err(EvalError::Js(raise_type_error!("Reflect.preventExtensions requires 1 argument")));
            }
            let target = args[0].clone();

            match target {
                Value::Object(obj) => {
                    obj.borrow_mut(mc).prevent_extensions();
                    Ok(Value::Boolean(true))
                }
                _ => Err(EvalError::Js(raise_type_error!(
                    "Reflect.preventExtensions target must be an object"
                ))),
            }
        }
        "set" => {
            if args.len() < 3 {
                return Err(EvalError::Js(raise_type_error!("Reflect.set requires at least 3 arguments")));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();
            let value = args[2].clone();
            let _receiver = if args.len() > 3 { args[3].clone() } else { target.clone() };

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        _ => return Err(EvalError::Js(raise_type_error!("Invalid property key"))),
                    };
                    object_set_key_value(mc, &obj, &prop_key, value)?;
                    Ok(Value::Boolean(true))
                }
                _ => Err(EvalError::Js(raise_type_error!("Reflect.set target must be an object"))),
            }
        }
        "setPrototypeOf" => {
            if args.len() < 2 {
                return Err(EvalError::Js(raise_type_error!("Reflect.setPrototypeOf requires 2 arguments")));
            }
            match &args[0] {
                Value::Object(obj) => match args[1] {
                    Value::Object(proto_obj) => {
                        obj.borrow_mut(mc).prototype = Some(proto_obj);
                        Ok(Value::Boolean(true))
                    }
                    Value::Undefined => {
                        obj.borrow_mut(mc).prototype = None;
                        Ok(Value::Boolean(true))
                    }
                    _ => Err(EvalError::Js(raise_type_error!(
                        "Reflect.setPrototypeOf prototype must be an object or null"
                    ))),
                },
                _ => Err(EvalError::Js(raise_type_error!("Reflect.setPrototypeOf target must be an object"))),
            }
        }
        _ => Err(EvalError::Js(raise_eval_error!("Unknown Reflect method"))),
    }
}
