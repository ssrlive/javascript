use crate::core::MutationContext;
use crate::core::{
    JSObjectDataPtr, PropertyKey, Value, new_js_object_data, obj_get_key_value, obj_set_key_value, prepare_function_call_env,
};
use crate::error::JSError;
use crate::js_array::{get_array_length, set_array_length};
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

/// Initialize the Reflect object with all reflection methods
pub fn initialize_reflect<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let reflect_obj = new_js_object_data(mc);
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("apply".to_string()),
        Value::Function("Reflect.apply".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("construct".to_string()),
        Value::Function("Reflect.construct".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("defineProperty".to_string()),
        Value::Function("Reflect.defineProperty".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("deleteProperty".to_string()),
        Value::Function("Reflect.deleteProperty".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("get".to_string()),
        Value::Function("Reflect.get".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("getOwnPropertyDescriptor".to_string()),
        Value::Function("Reflect.getOwnPropertyDescriptor".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("getPrototypeOf".to_string()),
        Value::Function("Reflect.getPrototypeOf".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("has".to_string()),
        Value::Function("Reflect.has".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("isExtensible".to_string()),
        Value::Function("Reflect.isExtensible".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("ownKeys".to_string()),
        Value::Function("Reflect.ownKeys".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("preventExtensions".to_string()),
        Value::Function("Reflect.preventExtensions".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("set".to_string()),
        Value::Function("Reflect.set".to_string()),
    )?;
    obj_set_key_value(
        mc,
        &reflect_obj,
        &crate::core::PropertyKey::String("setPrototypeOf".to_string()),
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
) -> Result<Value<'gc>, JSError> {
    match method {
        "apply" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.apply requires at least 2 arguments"));
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
                                if let Some(val_rc) = obj_get_key_value(&arr_obj, &i.to_string().into())? {
                                    arg_values.push(val_rc.borrow().clone());
                                } else {
                                    arg_values.push(Value::Undefined);
                                }
                            }
                        }
                    } else {
                        return Err(raise_type_error!("Reflect.apply argumentsList must be an array-like object"));
                    }
                }
                Value::Undefined => {}
                _ => return Err(raise_type_error!("Reflect.apply argumentsList must be an array-like object")),
            }

            // If target is a native constructor object (e.g., String), call its native handler
            if let Value::Object(obj) = &target
                && let Some(native_rc) = obj_get_key_value(obj, &"__native_ctor".into())?
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
                        if let Ok(Some(cl_ptr)) = obj_get_key_value(obj, &"__closure__".into()) {
                            matches!(&*cl_ptr.borrow(), Value::AsyncClosure(_))
                        } else {
                            false
                        }
                    } else {
                        false
                    });

                // Delegate invocation to existing call dispatcher which handles sync/async/native functions
                return crate::core::evaluate_call_dispatch(mc, env, target.clone(), Some(this_arg.clone()), arg_values)
                    .map_err(|e| e.into());
            }

            match target {
                Value::Function(func_name) => crate::js_function::handle_global_function(mc, &func_name, &arg_values, env),
                Value::Object(object) => {
                    // If this object wraps an internal closure (function-object), invoke it
                    if let Some(cl_rc) = obj_get_key_value(&object, &"__closure__".into())? {
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
                            return Ok(crate::core::evaluate_statements(mc, &func_env, &body)?);
                        }
                    }
                    Err(raise_type_error!("Reflect.apply target is not callable"))
                }
                _ => Err(raise_type_error!("Reflect.apply target is not callable")),
            }
        }
        "construct" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.construct requires at least 1 argument"));
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
                                if let Some(val_rc) = obj_get_key_value(&arr_obj, &i.to_string().into())? {
                                    arg_values.push(val_rc.borrow().clone());
                                } else {
                                    arg_values.push(Value::Undefined);
                                }
                            }
                        }
                    } else {
                        return Err(raise_type_error!("Reflect.construct argumentsList must be an array-like object"));
                    }
                }
                Value::Undefined => {}
                _ => return Err(raise_type_error!("Reflect.construct argumentsList must be an array-like object")),
            }

            crate::js_class::evaluate_new(mc, env, target, &arg_values)
        }
        "defineProperty" => {
            if args.len() < 3 {
                return Err(raise_type_error!("Reflect.defineProperty requires 3 arguments"));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();
            let attributes = args[2].clone();

            match target {
                Value::Object(obj) => {
                    // For now, just set the property with the value from attributes
                    // This is a simplified implementation
                    if let Value::Object(attr_obj) = &attributes {
                        if let Some(value_rc) = obj_get_key_value(attr_obj, &crate::core::PropertyKey::String("value".to_string()))? {
                            let prop_key = match property_key {
                                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                                Value::Number(n) => PropertyKey::String(n.to_string()),
                                _ => return Err(raise_type_error!("Invalid property key")),
                            };
                            obj_set_key_value(mc, &obj, &prop_key, value_rc.borrow().clone())?;
                            Ok(Value::Boolean(true))
                        } else {
                            Ok(Value::Boolean(false))
                        }
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                _ => Err(raise_type_error!("Reflect.defineProperty target must be an object")),
            }
        }
        "deleteProperty" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.deleteProperty requires 2 arguments"));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(n.to_string()),
                        _ => return Err(raise_type_error!("Invalid property key")),
                    };
                    // For now, always return true as we don't have configurable properties
                    let _ = obj.borrow_mut(mc).properties.shift_remove(&prop_key);
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.deleteProperty target must be an object")),
            }
        }
        "get" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.get requires at least 2 arguments"));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();
            let _receiver = if args.len() > 2 { args[2].clone() } else { target.clone() };

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(n.to_string()),
                        _ => return Err(raise_type_error!("Invalid property key")),
                    };
                    if let Some(value_rc) = obj_get_key_value(&obj, &prop_key)? {
                        Ok(value_rc.borrow().clone())
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(raise_type_error!("Reflect.get target must be an object")),
            }
        }
        "getOwnPropertyDescriptor" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.getOwnPropertyDescriptor requires 2 arguments"));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(n.to_string()),
                        _ => return Err(raise_type_error!("Invalid property key")),
                    };
                    if let Some(value_rc) = obj_get_key_value(&obj, &prop_key)? {
                        // Create a descriptor object
                        let descriptor = new_js_object_data(mc);
                        obj_set_key_value(
                            mc,
                            &descriptor,
                            &crate::core::PropertyKey::String("value".to_string()),
                            value_rc.borrow().clone(),
                        )?;
                        obj_set_key_value(
                            mc,
                            &descriptor,
                            &crate::core::PropertyKey::String("writable".to_string()),
                            Value::Boolean(true),
                        )?;
                        obj_set_key_value(
                            mc,
                            &descriptor,
                            &crate::core::PropertyKey::String("enumerable".to_string()),
                            Value::Boolean(true),
                        )?;
                        obj_set_key_value(
                            mc,
                            &descriptor,
                            &crate::core::PropertyKey::String("configurable".to_string()),
                            Value::Boolean(true),
                        )?;
                        Ok(Value::Object(descriptor))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(raise_type_error!("Reflect.getOwnPropertyDescriptor target must be an object")),
            }
        }
        "getPrototypeOf" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.getPrototypeOf requires 1 argument"));
            }
            match &args[0] {
                Value::Object(obj) => {
                    if let Some(proto_rc) = obj.borrow().prototype {
                        Ok(Value::Object(proto_rc))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(raise_type_error!("Reflect.getPrototypeOf target must be an object")),
            }
        }
        "has" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.has requires 2 arguments"));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(n.to_string()),
                        _ => return Err(raise_type_error!("Invalid property key")),
                    };
                    let has_prop = obj_get_key_value(&obj, &prop_key)?.is_some();
                    Ok(Value::Boolean(has_prop))
                }
                _ => Err(raise_type_error!("Reflect.has target must be an object")),
            }
        }
        "isExtensible" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.isExtensible requires 1 argument"));
            }
            let target = args[0].clone();

            match target {
                Value::Object(_) => {
                    // For now, all objects are extensible
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.isExtensible target must be an object")),
            }
        }
        "ownKeys" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.ownKeys requires 1 argument"));
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
                        obj_set_key_value(mc, &result_obj, &i.to_string().into(), key)?;
                    }
                    // Set length property
                    set_array_length(mc, &result_obj, keys_len)?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Reflect.ownKeys target must be an object")),
            }
        }
        "preventExtensions" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.preventExtensions requires 1 argument"));
            }
            let target = args[0].clone();

            match target {
                Value::Object(_) => {
                    // For now, just return true (we don't implement extensibility control yet)
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.preventExtensions target must be an object")),
            }
        }
        "set" => {
            if args.len() < 3 {
                return Err(raise_type_error!("Reflect.set requires at least 3 arguments"));
            }
            let target = args[0].clone();
            let property_key = args[1].clone();
            let value = args[2].clone();
            let _receiver = if args.len() > 3 { args[3].clone() } else { target.clone() };

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(n.to_string()),
                        _ => return Err(raise_type_error!("Invalid property key")),
                    };
                    obj_set_key_value(mc, &obj, &prop_key, value)?;
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.set target must be an object")),
            }
        }
        "setPrototypeOf" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.setPrototypeOf requires 2 arguments"));
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
                    _ => Err(raise_type_error!("Reflect.setPrototypeOf prototype must be an object or null")),
                },
                _ => Err(raise_type_error!("Reflect.setPrototypeOf target must be an object")),
            }
        }
        _ => Err(raise_eval_error!("Unknown Reflect method")),
    }
}
