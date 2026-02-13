use crate::core::MutationContext;
use crate::core::{
    JSObjectDataPtr, PropertyDescriptor, PropertyKey, Value, new_js_object_data, object_get_key_value, object_set_key_value,
    prepare_function_call_env,
};
use crate::js_array::{get_array_length, set_array_length};
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use crate::{JSError, core::EvalError};

/// Initialize the Reflect object with all reflection methods
pub fn initialize_reflect<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let reflect_obj = new_js_object_data(mc);
    object_set_key_value(mc, &reflect_obj, "apply", &Value::Function("Reflect.apply".to_string()))?;
    object_set_key_value(mc, &reflect_obj, "construct", &Value::Function("Reflect.construct".to_string()))?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "defineProperty",
        &Value::Function("Reflect.defineProperty".to_string()),
    )?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "deleteProperty",
        &Value::Function("Reflect.deleteProperty".to_string()),
    )?;
    object_set_key_value(mc, &reflect_obj, "get", &Value::Function("Reflect.get".to_string()))?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "getOwnPropertyDescriptor",
        &Value::Function("Reflect.getOwnPropertyDescriptor".to_string()),
    )?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "getPrototypeOf",
        &Value::Function("Reflect.getPrototypeOf".to_string()),
    )?;
    object_set_key_value(mc, &reflect_obj, "has", &Value::Function("Reflect.has".to_string()))?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "isExtensible",
        &Value::Function("Reflect.isExtensible".to_string()),
    )?;
    object_set_key_value(mc, &reflect_obj, "ownKeys", &Value::Function("Reflect.ownKeys".to_string()))?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "preventExtensions",
        &Value::Function("Reflect.preventExtensions".to_string()),
    )?;
    object_set_key_value(mc, &reflect_obj, "set", &Value::Function("Reflect.set".to_string()))?;
    object_set_key_value(
        mc,
        &reflect_obj,
        "setPrototypeOf",
        &Value::Function("Reflect.setPrototypeOf".to_string()),
    )?;

    crate::core::env_set(mc, env, "Reflect", &Value::Object(reflect_obj))?;
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
                return Err(raise_type_error!("Reflect.apply requires at least 2 arguments").into());
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
                        return Err(raise_type_error!("Reflect.apply argumentsList must be an array-like object").into());
                    }
                }
                Value::Undefined => {}
                _ => {
                    return Err(raise_type_error!("Reflect.apply argumentsList must be an array-like object").into());
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
                        if let Some(cl_ptr) = obj.borrow().get_closure() {
                            matches!(&*cl_ptr.borrow(), Value::AsyncClosure(_))
                        } else {
                            false
                        }
                    } else {
                        false
                    });

                // Delegate invocation to existing call dispatcher which handles sync/async/native functions
                return crate::core::evaluate_call_dispatch(mc, env, &target, Some(&this_arg), &arg_values);
            }

            match target {
                Value::Function(func_name) => Ok(crate::js_function::handle_global_function(mc, &func_name, &arg_values, env)?),
                Value::Object(object) => {
                    // If this object wraps an internal closure (function-object), invoke it
                    if let Some(cl_rc) = object.borrow().get_closure() {
                        let cl_val = cl_rc.borrow().clone();
                        if let Some((params, body, captured_env)) = crate::core::extract_closure_from_value(&cl_val) {
                            let func_env = prepare_function_call_env(
                                mc,
                                Some(&captured_env),
                                Some(&this_arg),
                                Some(&params),
                                &arg_values,
                                None,
                                Some(env),
                            )?;
                            return crate::core::evaluate_statements(mc, &func_env, &body);
                        }
                    }
                    Err(raise_type_error!("Reflect.apply target is not callable").into())
                }
                _ => Err(raise_type_error!("Reflect.apply target is not callable").into()),
            }
        }
        "construct" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.construct requires at least 1 argument").into());
            }
            let target = args[0].clone();
            let arguments_list = if args.len() > 1 { args[1].clone() } else { Value::Undefined };
            let new_target = if args.len() > 2 { args[2].clone() } else { target.clone() };

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
                        return Err(raise_type_error!("Reflect.construct argumentsList must be an array-like object").into());
                    }
                }
                Value::Undefined => {}
                _ => {
                    return Err(raise_type_error!("Reflect.construct argumentsList must be an array-like object").into());
                }
            }

            crate::js_class::evaluate_new(mc, env, &target, &arg_values, Some(&new_target))
        }
        "defineProperty" => {
            if args.len() < 3 {
                return Err(raise_type_error!("Reflect.defineProperty requires 3 arguments").into());
            }
            let target = args[0].clone();
            let property_key = args[1].clone();
            let attributes = args[2].clone();

            match target {
                Value::Object(obj) => {
                    if let Value::Object(attr_obj) = &attributes {
                        let prop_key = match property_key {
                            Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                            Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                            _ => return Err(raise_type_error!("Invalid property key").into()),
                        };
                        if let PropertyKey::String(s) = &prop_key {
                            crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                        }
                        match PropertyDescriptor::from_object(attr_obj) {
                            Ok(pd) => {
                                if crate::core::validate_descriptor_for_define(mc, &pd).is_err() {
                                    return Ok(Value::Boolean(false));
                                }
                            }
                            Err(_) => return Ok(Value::Boolean(false)),
                        }

                        match crate::js_object::define_property_internal(mc, &obj, &prop_key, attr_obj) {
                            Ok(()) => Ok(Value::Boolean(true)),
                            Err(_e) => Ok(Value::Boolean(false)),
                        }
                    } else {
                        Ok(Value::Boolean(false))
                    }
                }
                _ => Err(raise_type_error!("Reflect.defineProperty target must be an object").into()),
            }
        }
        "deleteProperty" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.deleteProperty requires 2 arguments").into());
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        _ => return Err(raise_type_error!("Invalid property key").into()),
                    };
                    if let PropertyKey::String(s) = &prop_key {
                        crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                    }
                    // For now, always return true as we don't have configurable properties
                    let _ = obj.borrow_mut(mc).properties.shift_remove(&prop_key);
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.deleteProperty target must be an object").into()),
            }
        }
        "get" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.get requires at least 2 arguments").into());
            }
            let target = args[0].clone();
            let property_key = args[1].clone();
            let _receiver = if args.len() > 2 { args[2].clone() } else { target.clone() };

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        _ => return Err(raise_type_error!("Invalid property key").into()),
                    };
                    if let PropertyKey::String(s) = &prop_key {
                        crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                    }
                    if let Some(value_rc) = object_get_key_value(&obj, &prop_key) {
                        Ok(value_rc.borrow().clone())
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(raise_type_error!("Reflect.get target must be an object").into()),
            }
        }
        "getOwnPropertyDescriptor" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.getOwnPropertyDescriptor requires 2 arguments").into());
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        _ => return Err(raise_type_error!("Invalid property key").into()),
                    };
                    if let PropertyKey::String(s) = &prop_key {
                        crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                    }
                    if let Some(_value_rc) = object_get_key_value(&obj, &prop_key) {
                        if let Some(mut pd) = crate::core::build_property_descriptor(mc, &obj, &prop_key) {
                            let is_deferred_namespace = obj.borrow().deferred_module_path.is_some();
                            let is_accessor_descriptor = pd.get.is_some() || pd.set.is_some();
                            let needs_hydration = (is_deferred_namespace || !is_accessor_descriptor)
                                && (pd.value.is_none() || matches!(pd.value, Some(Value::Undefined)));
                            if needs_hydration && let PropertyKey::String(s) = &prop_key {
                                let hydrated = crate::core::get_property_with_accessors(mc, env, &obj, s.as_str())?;
                                if !matches!(hydrated, Value::Undefined) {
                                    pd.value = Some(hydrated);
                                    pd.get = None;
                                    pd.set = None;
                                    if pd.writable.is_none() {
                                        pd.writable = Some(true);
                                    }
                                } else {
                                    let (module_path, cache_env) = {
                                        let b = obj.borrow();
                                        (b.deferred_module_path.clone(), b.deferred_cache_env)
                                    };
                                    if let (Some(module_path), Some(cache_env)) = (module_path, cache_env)
                                        && let Ok(Value::Object(exports_obj)) =
                                            crate::js_module::load_module(mc, module_path.as_str(), None, Some(cache_env))
                                        && let Some(v) = object_get_key_value(&exports_obj, s)
                                    {
                                        pd.value = Some(v.borrow().clone());
                                        pd.get = None;
                                        pd.set = None;
                                        if pd.writable.is_none() {
                                            pd.writable = Some(true);
                                        }
                                    }
                                }
                            }
                            let desc_obj = pd.to_object(mc)?;
                            crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                            Ok(Value::Object(desc_obj))
                        } else {
                            Ok(Value::Undefined)
                        }
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(raise_type_error!("Reflect.getOwnPropertyDescriptor target must be an object").into()),
            }
        }
        "getPrototypeOf" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.getPrototypeOf requires 1 argument").into());
            }
            match &args[0] {
                Value::Object(obj) => {
                    if let Some(proto_rc) = obj.borrow().prototype {
                        Ok(Value::Object(proto_rc))
                    } else {
                        Ok(Value::Null)
                    }
                }
                _ => Err(raise_type_error!("Reflect.getPrototypeOf target must be an object").into()),
            }
        }
        "has" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.has requires 2 arguments").into());
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = match property_key {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        _ => return Err(raise_type_error!("Invalid property key").into()),
                    };
                    if let PropertyKey::String(s) = &prop_key {
                        crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                    }
                    let has_prop = object_get_key_value(&obj, &prop_key).is_some();
                    Ok(Value::Boolean(has_prop))
                }
                _ => Err(raise_type_error!("Reflect.has target must be an object").into()),
            }
        }
        "isExtensible" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.isExtensible requires 1 argument").into());
            }
            let target = args[0].clone();

            match target {
                Value::Object(obj) => Ok(Value::Boolean(obj.borrow().is_extensible())),
                _ => Err(raise_type_error!("Reflect.isExtensible target must be an object").into()),
            }
        }
        "ownKeys" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.ownKeys requires 1 argument").into());
            }
            match args[0] {
                Value::Object(obj) => {
                    crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, None)?;
                    // Diagnostic trace to ensure proxy wrapper is visible here
                    let obj_ptr = obj.as_ptr();
                    let has_proxy = obj.borrow().properties.get(&PropertyKey::String("__proxy__".to_string())).is_some();
                    log::trace!("Reflect.ownKeys: obj_ptr={:p} has_proxy={}", obj_ptr, has_proxy);
                    // Use proxy-aware ownKeys so Proxy handlers are observed
                    let keys_vec: Vec<crate::core::PropertyKey> = crate::core::ordinary_own_property_keys_mc(mc, &obj)?;
                    let mut keys: Vec<Value> = Vec::new();
                    for key in keys_vec.iter() {
                        match key {
                            crate::core::PropertyKey::String(s) => keys.push(Value::String(utf8_to_utf16(s))),
                            crate::core::PropertyKey::Symbol(sd) => keys.push(Value::Symbol(*sd)),
                            _ => {}
                        }
                    }
                    let keys_len = keys.len();
                    // Create an array-like object for keys
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    for (i, key) in keys.into_iter().enumerate() {
                        object_set_key_value(mc, &result_obj, i, &key)?;
                    }
                    // Set length property
                    set_array_length(mc, &result_obj, keys_len)?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Reflect.ownKeys target must be an object").into()),
            }
        }
        "preventExtensions" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.preventExtensions requires 1 argument").into());
            }
            let target = args[0].clone();

            match target {
                Value::Object(obj) => {
                    obj.borrow_mut(mc).prevent_extensions();
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.preventExtensions target must be an object").into()),
            }
        }
        "set" => {
            if args.len() < 3 {
                return Err(raise_type_error!("Reflect.set requires at least 3 arguments").into());
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
                        _ => return Err(raise_type_error!("Invalid property key").into()),
                    };
                    object_set_key_value(mc, &obj, &prop_key, &value)?;
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.set target must be an object").into()),
            }
        }
        "setPrototypeOf" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.setPrototypeOf requires 2 arguments").into());
            }
            match &args[0] {
                Value::Object(obj) => match &args[1] {
                    Value::Object(proto_obj) => {
                        let current_proto = obj.borrow().prototype;
                        let is_extensible = obj.borrow().is_extensible();
                        let same_proto = current_proto.is_some_and(|p| crate::core::Gc::ptr_eq(p, *proto_obj));
                        if !is_extensible && !same_proto {
                            return Ok(Value::Boolean(false));
                        }
                        obj.borrow_mut(mc).prototype = Some(*proto_obj);
                        Ok(Value::Boolean(true))
                    }
                    Value::Undefined | Value::Null => {
                        let current_proto = obj.borrow().prototype;
                        let is_extensible = obj.borrow().is_extensible();
                        if !is_extensible && current_proto.is_some() {
                            return Ok(Value::Boolean(false));
                        }
                        obj.borrow_mut(mc).prototype = None;
                        Ok(Value::Boolean(true))
                    }
                    Value::Function(func_name) => {
                        if !obj.borrow().is_extensible() {
                            return Ok(Value::Boolean(false));
                        }
                        // Functions are objects in JS. Our engine represents some built-ins as Value::Function,
                        // so wrap it in an object shell that behaves like a function object for prototype chains.
                        let fn_obj = new_js_object_data(mc);
                        if let Some(func_ctor_val) = object_get_key_value(env, "Function")
                            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                            && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                            && let Value::Object(func_proto) = &*proto_val.borrow()
                        {
                            fn_obj.borrow_mut(mc).prototype = Some(*func_proto);
                        }
                        fn_obj
                            .borrow_mut(mc)
                            .set_closure(Some(crate::core::new_gc_cell_ptr(mc, Value::Function(func_name.clone()))));
                        obj.borrow_mut(mc).prototype = Some(fn_obj);
                        Ok(Value::Boolean(true))
                    }
                    _ => Err(raise_type_error!("Reflect.setPrototypeOf prototype must be an object or null").into()),
                },
                _ => Err(raise_type_error!("Reflect.setPrototypeOf target must be an object").into()),
            }
        }
        _ => Err(raise_eval_error!("Unknown Reflect method").into()),
    }
}
