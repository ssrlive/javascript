use crate::core::{
    ClosureData, JSObjectDataPtr, PropertyKey, Value, evaluate_call_dispatch, new_js_object_data, object_get_key_value,
    object_set_key_value, prepare_closure_call_env, prepare_function_call_env, value_to_string,
};
use crate::core::{Gc, GcCell, GcPtr, MutationContext};
use crate::error::JSError;
use crate::js_array::{get_array_length, is_array, set_array_length};
use crate::js_date::is_date_object;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

pub fn initialize_object_module<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    // 1. Create Object constructor
    let object_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &object_ctor, "__is_constructor", Value::Boolean(true))?;
    object_set_key_value(mc, &object_ctor, "__native_ctor", Value::String(utf8_to_utf16("Object")))?;

    // Register Object in the environment
    crate::core::env_set(mc, env, "Object", Value::Object(object_ctor))?;

    // 2. Create Object.prototype
    let object_proto = new_js_object_data(mc);
    // Link prototype and constructor
    object_set_key_value(mc, &object_ctor, "prototype", Value::Object(object_proto))?;
    object_set_key_value(mc, &object_proto, "constructor", Value::Object(object_ctor))?;
    // Make constructor non-enumerable
    object_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("constructor"));

    // 3. Register static methods
    let static_methods = vec![
        "assign",
        "create",
        "defineProperties",
        "defineProperty",
        "entries",
        "freeze",
        "fromEntries",
        "getOwnPropertyDescriptor",
        "getOwnPropertyDescriptors",
        "getOwnPropertyNames",
        "getOwnPropertySymbols",
        "getPrototypeOf",
        "groupBy",
        "hasOwn",
        "is",
        "isExtensible",
        "isFrozen",
        "isSealed",
        "keys",
        "preventExtensions",
        "seal",
        "setPrototypeOf",
        "values",
    ];

    for method in static_methods {
        object_set_key_value(mc, &object_ctor, method, Value::Function(format!("Object.{method}")))?;
    }

    // 4. Register prototype methods
    let proto_methods = vec![
        "hasOwnProperty",
        "isPrototypeOf",
        "propertyIsEnumerable",
        "toLocaleString",
        "toString",
        "valueOf",
    ];

    for method in proto_methods {
        object_set_key_value(mc, &object_proto, method, Value::Function(format!("Object.prototype.{method}")))?;
        // Methods on prototypes should be non-enumerable so for..in doesn't list them
        object_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from(method));
    }

    Ok(())
}

fn define_property_internal<'gc>(
    mc: &MutationContext<'gc>,
    target_obj: &JSObjectDataPtr<'gc>,
    prop_key: PropertyKey<'gc>,
    desc_obj: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    // Extract descriptor fields
    let value_rc_opt = object_get_key_value(desc_obj, "value");

    // If the property exists and is non-configurable on the target, apply ECMAScript-compatible checks
    if let Some(existing_rc) = object_get_key_value(target_obj, &prop_key)
        && !target_obj.borrow().is_configurable(&prop_key)
    {
        // If descriptor explicitly sets configurable true -> throw
        if let Some(cfg_rc) = object_get_key_value(desc_obj, "configurable")
            && let Value::Boolean(true) = &*cfg_rc.borrow()
        {
            return Err(raise_type_error!("Cannot make non-configurable property configurable"));
        }

        // If descriptor explicitly sets enumerable and it's different -> throw
        if let Some(enum_rc) = object_get_key_value(desc_obj, "enumerable")
            && let Value::Boolean(new_enum) = &*enum_rc.borrow()
        {
            let existing_enum = target_obj.borrow().is_enumerable(&prop_key);
            if *new_enum != existing_enum {
                return Err(raise_type_error!("Cannot change enumerability of non-configurable property"));
            }
        }

        // Determine whether existing property is a data property or accessor
        let existing_is_accessor = match &*existing_rc.borrow() {
            Value::Property { value: _, getter, setter } => getter.is_some() || setter.is_some(),
            Value::Getter(..) | Value::Setter(..) => true,
            _ => false,
        };

        // If existing is data property
        if !existing_is_accessor {
            // Disallow converting to accessor
            if object_get_key_value(desc_obj, "get").is_some() || object_get_key_value(desc_obj, "set").is_some() {
                return Err(raise_type_error!("Cannot convert non-configurable data property to an accessor"));
            }

            // If writable is being set from false -> true, disallow
            if let Some(wrc) = object_get_key_value(desc_obj, "writable")
                && let Value::Boolean(new_writable) = &*wrc.borrow()
                && *new_writable
                && !target_obj.borrow().is_writable(&prop_key)
            {
                return Err(raise_type_error!("Cannot make non-writable property writable"));
            }

            // If attempting to change value while not writable and values differ -> throw
            if let Some(new_val_rc) = value_rc_opt.as_ref()
                && !target_obj.borrow().is_writable(&prop_key)
            {
                // get existing value for comparison
                let existing_val = match &*existing_rc.borrow() {
                    Value::Property { value: Some(v), .. } => v.borrow().clone(),
                    other => other.clone(),
                };
                if !crate::core::values_equal(mc, &existing_val, &new_val_rc.borrow().clone()) {
                    return Err(raise_type_error!("Cannot change value of non-writable, non-configurable property"));
                }
            }
        } else {
            // existing is accessor
            // Disallow converting to data property
            if value_rc_opt.is_some() || object_get_key_value(desc_obj, "writable").is_some() {
                return Err(raise_type_error!("Cannot convert non-configurable accessor to a data property"));
            }

            // Disallow changing getter/setter functions on non-configurable accessor
            if object_get_key_value(desc_obj, "get").is_some() || object_get_key_value(desc_obj, "set").is_some() {
                return Err(raise_type_error!(
                    "Cannot change getter/setter of non-configurable accessor property"
                ));
            }
        }
    }

    let mut getter_opt: Option<Box<Value>> = None;
    if let Some(get_rc) = object_get_key_value(desc_obj, "get") {
        let get_val = get_rc.borrow();
        if !matches!(&*get_val, Value::Undefined) {
            getter_opt = Some(Box::new(get_val.clone()));
        }
    }

    let mut setter_opt: Option<Box<Value>> = None;
    if let Some(set_rc) = object_get_key_value(desc_obj, "set") {
        let set_val = set_rc.borrow();
        if !matches!(&*set_val, Value::Undefined) {
            setter_opt = Some(Box::new(set_val.clone()));
        }
    }

    // Create property descriptor value
    let prop_descriptor = Value::Property {
        value: value_rc_opt,
        getter: getter_opt,
        setter: setter_opt,
    };

    // If writable flag explicitly set to false, mark property as non-writable
    if let Some(wrc) = object_get_key_value(desc_obj, "writable")
        && let Value::Boolean(is_writable) = &*wrc.borrow()
        && !*is_writable
    {
        target_obj.borrow_mut(mc).set_non_writable(prop_key.clone());
    }

    // Install property on target object
    object_set_key_value(mc, target_obj, &prop_key, prop_descriptor)?;
    Ok(())
}

pub fn handle_object_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    match method {
        "keys" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.keys requires at least one argument"));
            }
            if args.len() > 1 {
                return Err(raise_type_error!("Object.keys accepts only one argument"));
            }
            match args[0] {
                Value::Object(obj) => {
                    let mut keys = Vec::new();
                    let ordered = crate::core::ordinary_own_property_keys(&obj);
                    for key in ordered {
                        if !obj.borrow().is_enumerable(&key) {
                            continue;
                        }
                        if let PropertyKey::String(s) = key {
                            // Only include string keys (array indices and others). Skip 'length' because it's non-enumerable
                            keys.push(Value::String(utf8_to_utf16(&s)));
                        }
                    }
                    // Create a proper Array for keys
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    let len = keys.len();
                    for (i, key) in keys.into_iter().enumerate() {
                        object_set_key_value(mc, &result_obj, i, key)?;
                    }
                    set_array_length(mc, &result_obj, len)?;
                    Ok(Value::Object(result_obj))
                }
                Value::Undefined => Err(raise_type_error!("Object.keys called on undefined")),
                _ => {
                    // For primitive values, return empty array (like in JS)
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    set_array_length(mc, &result_obj, 0)?;
                    Ok(Value::Object(result_obj))
                }
            }
        }
        "values" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.values requires at least one argument"));
            }
            if args.len() > 1 {
                return Err(raise_type_error!("Object.values accepts only one argument"));
            }
            match args[0] {
                Value::Object(obj) => {
                    let mut values = Vec::new();
                    let ordered = crate::core::ordinary_own_property_keys(&obj);
                    for key in ordered {
                        if !obj.borrow().is_enumerable(&key) {
                            continue;
                        }
                        if let PropertyKey::String(_s) = &key {
                            // Only include string keys (array indices and others); 'length' is non-enumerable so won't appear
                            if let Some(v_rc) = object_get_key_value(&obj, &key) {
                                values.push(v_rc.borrow().clone());
                            }
                        }
                    }
                    // Create a proper Array for values
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    let len = values.len();
                    for (i, value) in values.into_iter().enumerate() {
                        object_set_key_value(mc, &result_obj, i, value)?;
                    }
                    set_array_length(mc, &result_obj, len)?;
                    Ok(Value::Object(result_obj))
                }
                Value::Undefined => Err(raise_type_error!("Object.values called on undefined")),
                _ => {
                    // For primitive values, return empty array (like in JS)
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    set_array_length(mc, &result_obj, 0)?;
                    Ok(Value::Object(result_obj))
                }
            }
        }
        "hasOwn" => {
            if args.len() != 2 {
                return Err(raise_type_error!("Object.hasOwn requires exactly two arguments"));
            }
            let obj_val = args[0].clone();
            let prop_val = args[1].clone();

            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot convert undefined or null to object"));
            }

            let key = match prop_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::BigInt(b) => PropertyKey::String(b.to_string()),
                Value::Symbol(sd) => PropertyKey::Symbol(sd),
                val => PropertyKey::String(value_to_string(&val)),
            };

            let has_own = match obj_val {
                Value::Object(obj) => obj.borrow().properties.contains_key(&key),
                Value::String(s) => {
                    if let PropertyKey::String(k) = key {
                        if k == "length" {
                            true
                        } else if let Ok(idx) = k.parse::<usize>() {
                            idx < s.len()
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                _ => false,
            };

            Ok(Value::Boolean(has_own))
        }
        "getPrototypeOf" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getPrototypeOf requires exactly one argument"));
            }
            let obj_val = args[0].clone();
            match obj_val {
                Value::Object(obj) => {
                    if let Some(proto_rc) = obj.borrow().prototype {
                        Ok(Value::Object(proto_rc))
                    } else {
                        Ok(Value::Null)
                    }
                }
                Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object")),
                _ => {
                    // For primitives, we should ideally return the prototype of the wrapper.
                    // For now, returning Null is a safe fallback if wrappers aren't fully supported.
                    Ok(Value::Null)
                }
            }
        }
        "isExtensible" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.isExtensible requires exactly one argument"));
            }
            let obj_val = args[0].clone();
            match obj_val {
                Value::Object(obj) => Ok(Value::Boolean(obj.borrow().is_extensible())),
                Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object")),
                _ => {
                    // Primitives are considered wrapped objects; treat as extensible for now
                    Ok(Value::Boolean(true))
                }
            }
        }
        "preventExtensions" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.preventExtensions requires exactly one argument"));
            }
            match &args[0] {
                Value::Object(obj) => {
                    obj.borrow_mut(mc).prevent_extensions();
                    Ok(Value::Object(*obj))
                }
                _ => Err(raise_type_error!("Object.preventExtensions called on non-object")),
            }
        }
        "groupBy" => {
            if args.len() != 2 {
                return Err(raise_type_error!("Object.groupBy requires exactly two arguments"));
            }
            let items_val = args[0].clone();
            let callback_val = args[1].clone();

            let items_obj = match items_val {
                Value::Object(obj) => obj,
                _ => return Err(raise_type_error!("Object.groupBy expects an object as first argument")),
            };

            let result_obj = new_js_object_data(mc);
            // Object.groupBy returns a null-prototype object
            result_obj.borrow_mut(mc).prototype = None;

            let len = get_array_length(mc, &items_obj).unwrap_or(0);

            for i in 0..len {
                if let Some(val_rc) = object_get_key_value(&items_obj, i) {
                    let val = val_rc.borrow().clone();

                    let key_val = if let Some((params, body, captured_env)) = crate::core::extract_closure_from_value(&callback_val) {
                        let args = vec![val.clone(), Value::Number(i as f64)];
                        let func_env = prepare_closure_call_env(mc, &captured_env, Some(&params), &args, Some(env))?;
                        crate::core::evaluate_statements(mc, &func_env, &body)?
                    } else {
                        return Err(raise_type_error!("Object.groupBy expects a function as second argument"));
                    };

                    let key = match key_val {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::BigInt(b) => PropertyKey::String(b.to_string()),
                        Value::Symbol(sd) => PropertyKey::Symbol(sd),
                        _ => PropertyKey::String(value_to_string(&key_val)),
                    };

                    let group_arr = if let Some(arr_rc) = object_get_key_value(&result_obj, &key) {
                        if let Value::Object(arr) = &*arr_rc.borrow() {
                            *arr
                        } else {
                            crate::js_array::create_array(mc, env)?
                        }
                    } else {
                        let arr = crate::js_array::create_array(mc, env)?;
                        object_set_key_value(mc, &result_obj, &key, Value::Object(arr))?;
                        arr
                    };

                    let current_len = get_array_length(mc, &group_arr).unwrap_or(0);
                    object_set_key_value(mc, &group_arr, current_len, val)?;
                    crate::js_array::set_array_length(mc, &group_arr, current_len + 1)?;
                }
            }

            Ok(Value::Object(result_obj))
        }
        "create" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.create requires at least one argument"));
            }
            let proto_val = args[0].clone();
            let proto_obj = match proto_val {
                Value::Object(obj) => Some(obj),
                Value::Undefined | Value::Null => None,
                _ => {
                    return Err(raise_type_error!("Object.create prototype must be an object, null, or undefined"));
                }
            };

            // Create new object
            let new_obj = new_js_object_data(mc);

            // Set prototype
            if let Some(proto) = proto_obj {
                new_obj.borrow_mut(mc).prototype = Some(proto);
            }

            // If properties descriptor is provided, add properties
            if args.len() > 1 {
                let props_val = args[1].clone();
                if let Value::Object(props_obj) = props_val {
                    for (key, desc_val) in props_obj.borrow().properties.iter() {
                        if let Value::Object(desc_obj) = &*desc_val.borrow() {
                            // Handle property descriptor
                            let value = if let Some(val) = object_get_key_value(desc_obj, "value") {
                                val.borrow().clone()
                            } else {
                                Value::Undefined
                            };

                            // For now, we just set the value directly
                            // Full property descriptor support would require more complex implementation
                            object_set_key_value(mc, &new_obj, key, value)?;
                        }
                    }
                }
            }

            Ok(Value::Object(new_obj))
        }
        "getOwnPropertySymbols" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getOwnPropertySymbols requires exactly one argument"));
            }
            match args[0] {
                Value::Object(obj) => {
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    let mut idx = 0;
                    let ordered = crate::core::ordinary_own_property_keys(&obj);
                    for key in ordered {
                        if let PropertyKey::Symbol(sym) = key {
                            object_set_key_value(mc, &result_obj, idx, Value::Symbol(sym))?;
                            idx += 1;
                        }
                    }
                    set_array_length(mc, &result_obj, idx)?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Object.getOwnPropertySymbols called on non-object")),
            }
        }
        "getOwnPropertyNames" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getOwnPropertyNames requires exactly one argument"));
            }
            match args[0] {
                Value::Object(obj) => {
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    let mut idx = 0;
                    let ordered = crate::core::ordinary_own_property_keys(&obj);
                    for key in ordered {
                        if let PropertyKey::String(s) = key {
                            object_set_key_value(mc, &result_obj, idx, Value::String(utf8_to_utf16(&s)))?;
                            idx += 1;
                        }
                    }
                    set_array_length(mc, &result_obj, idx)?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Object.getOwnPropertyNames called on non-object")),
            }
        }
        "getOwnPropertyDescriptor" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Object.getOwnPropertyDescriptor requires at least two arguments"));
            }
            let obj_val = args[0].clone();
            let obj = match obj_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.getOwnPropertyDescriptor called on non-object")),
            };

            let prop_val = args[1].clone();
            let key = match prop_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::BigInt(b) => PropertyKey::String(b.to_string()),
                Value::Symbol(sd) => PropertyKey::Symbol(sd),
                val => PropertyKey::String(value_to_string(&val)),
            };

            if let Some(val_rc) = object_get_key_value(&obj, &key) {
                let desc_obj = new_js_object_data(mc);
                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;

                match &*val_rc.borrow() {
                    Value::Property { value, getter, setter } => {
                        if let Some(v) = value {
                            object_set_key_value(mc, &desc_obj, "value", v.borrow().clone())?;
                            object_set_key_value(mc, &desc_obj, "writable", Value::Boolean(true))?;
                        }
                        if let Some(g) = getter {
                            match &*g.clone() {
                                Value::Getter(body, captured_env, _home) => {
                                    let func_obj = crate::core::new_js_object_data(mc);
                                    let closure_data = crate::core::ClosureData {
                                        params: Vec::new(),
                                        body: body.clone(),
                                        env: *captured_env,
                                        home_object: crate::core::GcCell::new(None),
                                        captured_envs: Vec::new(),
                                        bound_this: None,
                                        is_arrow: false,
                                        is_strict: false,
                                    };
                                    let closure_val = crate::core::Value::Closure(crate::core::Gc::new(mc, closure_data));
                                    object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
                                    object_set_key_value(mc, &desc_obj, "get", Value::Object(func_obj))?;
                                }
                                other => {
                                    object_set_key_value(mc, &desc_obj, "get", other.clone())?;
                                }
                            }
                        }
                        if let Some(s) = setter {
                            match &*s.clone() {
                                Value::Setter(params, body, captured_env, _home) => {
                                    let func_obj = crate::core::new_js_object_data(mc);
                                    let closure_data = crate::core::ClosureData {
                                        params: params.clone(),
                                        body: body.clone(),
                                        env: *captured_env,
                                        home_object: crate::core::GcCell::new(None),
                                        captured_envs: Vec::new(),
                                        bound_this: None,
                                        is_arrow: false,
                                        is_strict: false,
                                    };
                                    let closure_val = crate::core::Value::Closure(crate::core::Gc::new(mc, closure_data));
                                    object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
                                    object_set_key_value(mc, &desc_obj, "set", Value::Object(func_obj))?;
                                }
                                other => {
                                    object_set_key_value(mc, &desc_obj, "set", other.clone())?;
                                }
                            }
                        }

                        let enum_flag = Value::Boolean(obj.borrow().is_enumerable(&key));
                        object_set_key_value(mc, &desc_obj, "enumerable", enum_flag)?;
                        let config_flag = Value::Boolean(obj.borrow().is_configurable(&key));
                        object_set_key_value(mc, &desc_obj, "configurable", config_flag)?;
                    }
                    Value::Getter(body, captured_env, _home_opt) => {
                        let func_obj = crate::core::new_js_object_data(mc);
                        let closure_data = crate::core::ClosureData {
                            params: Vec::new(),
                            body: body.clone(),
                            env: *captured_env,
                            home_object: crate::core::GcCell::new(None),
                            captured_envs: Vec::new(),
                            bound_this: None,
                            is_arrow: false,
                            is_strict: false,
                        };
                        let closure_val = crate::core::Value::Closure(crate::core::Gc::new(mc, closure_data));
                        object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
                        object_set_key_value(mc, &desc_obj, "get", Value::Object(func_obj))?;

                        let enum_flag = Value::Boolean(obj.borrow().is_enumerable(&key));
                        object_set_key_value(mc, &desc_obj, "enumerable", enum_flag)?;
                        let config_flag = Value::Boolean(obj.borrow().is_configurable(&key));
                        object_set_key_value(mc, &desc_obj, "configurable", config_flag)?;
                    }
                    Value::Setter(params, body, captured_env, _home_opt) => {
                        let func_obj = crate::core::new_js_object_data(mc);
                        let closure_data = crate::core::ClosureData {
                            params: params.clone(),
                            body: body.clone(),
                            env: *captured_env,
                            home_object: crate::core::GcCell::new(None),
                            captured_envs: Vec::new(),
                            bound_this: None,
                            is_arrow: false,
                            is_strict: false,
                        };
                        let closure_val = crate::core::Value::Closure(crate::core::Gc::new(mc, closure_data));
                        object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
                        object_set_key_value(mc, &desc_obj, "set", Value::Object(func_obj))?;

                        let enum_flag = Value::Boolean(obj.borrow().is_enumerable(&key));
                        object_set_key_value(mc, &desc_obj, "enumerable", enum_flag)?;
                        let config_flag = Value::Boolean(obj.borrow().is_configurable(&key));
                        object_set_key_value(mc, &desc_obj, "configurable", config_flag)?;
                    }
                    other => {
                        object_set_key_value(mc, &desc_obj, "value", other.clone())?;
                        let writable_flag = Value::Boolean(obj.borrow().is_writable(&key));
                        object_set_key_value(mc, &desc_obj, "writable", writable_flag)?;
                        let enum_flag = Value::Boolean(obj.borrow().is_enumerable(&key));
                        object_set_key_value(mc, &desc_obj, "enumerable", enum_flag)?;
                        let config_flag = Value::Boolean(obj.borrow().is_configurable(&key));
                        object_set_key_value(mc, &desc_obj, "configurable", config_flag)?;
                    }
                }

                Ok(Value::Object(desc_obj))
            } else {
                Ok(Value::Undefined)
            }
        }
        "getOwnPropertyDescriptors" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getOwnPropertyDescriptors requires exactly one argument"));
            }
            let obj_val = args[0].clone();
            match obj_val {
                Value::Object(ref obj) => {
                    let result_obj = new_js_object_data(mc);

                    let ordered = crate::core::ordinary_own_property_keys(obj);
                    for key in &ordered {
                        // iterate own properties in spec order
                        if let Some(val_rc) = object_get_key_value(obj, key) {
                            let desc_obj = new_js_object_data(mc);

                            match &*val_rc.borrow() {
                                Value::Property { value, getter, setter } => {
                                    // Data value
                                    if let Some(v) = value {
                                        object_set_key_value(mc, &desc_obj, "value", v.borrow().clone())?;
                                        // writable: treat as true by default for data properties (simplification)
                                        // Real implementation tracks writable separate from Value::Property in Value::Property?
                                        // Current Value::Property struct doesn't have 'writable' field!
                                        // It assumes checking 'setter' existence calls for writable? No.
                                        // This engine seems to use Value::Property only for Accessors or special internal slots,
                                        // but regular properties are just values.
                                        // If we are here, it's likely an accessor.
                                        // But if it has 'value', it's data?
                                        // The Value::Property variant in core/value.rs has: value, getter, setter. Make sense.

                                        object_set_key_value(mc, &desc_obj, "writable", Value::Boolean(true))?;
                                    }
                                    // Accessor
                                    if let Some(g) = getter {
                                        match &*g.clone() {
                                            Value::Getter(body, captured_env, _home) => {
                                                let func_obj = crate::core::new_js_object_data(mc);
                                                let closure_data = crate::core::ClosureData {
                                                    params: Vec::new(),
                                                    body: body.clone(),
                                                    env: *captured_env,
                                                    home_object: crate::core::GcCell::new(None),
                                                    captured_envs: Vec::new(),
                                                    bound_this: None,
                                                    is_arrow: false,
                                                    is_strict: false,
                                                };
                                                let closure_val = crate::core::Value::Closure(crate::core::Gc::new(mc, closure_data));
                                                object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
                                                object_set_key_value(mc, &desc_obj, "get", Value::Object(func_obj))?;
                                            }
                                            other => {
                                                object_set_key_value(mc, &desc_obj, "get", other.clone())?;
                                            }
                                        }
                                    }
                                    if let Some(s) = setter {
                                        match &*s.clone() {
                                            Value::Setter(params, body, captured_env, _home) => {
                                                let func_obj = crate::core::new_js_object_data(mc);
                                                let closure_data = crate::core::ClosureData {
                                                    params: params.clone(),
                                                    body: body.clone(),
                                                    env: *captured_env,
                                                    home_object: crate::core::GcCell::new(None),
                                                    captured_envs: Vec::new(),
                                                    bound_this: None,
                                                    is_arrow: false,
                                                    is_strict: false,
                                                };
                                                let closure_val = crate::core::Value::Closure(crate::core::Gc::new(mc, closure_data));
                                                object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
                                                object_set_key_value(mc, &desc_obj, "set", Value::Object(func_obj))?;
                                            }
                                            other => {
                                                object_set_key_value(mc, &desc_obj, "set", other.clone())?;
                                            }
                                        }
                                    }
                                    // flags: enumerable depends on object's non-enumerable set
                                    let enum_flag = Value::Boolean(obj.borrow().is_enumerable(key));
                                    object_set_key_value(mc, &desc_obj, "enumerable", enum_flag)?;
                                    let config_flag = Value::Boolean(obj.borrow().is_configurable(key));
                                    object_set_key_value(mc, &desc_obj, "configurable", config_flag)?;
                                }
                                // Handle raw Getter/Setter values stored directly on objects (from object literal shorthand)
                                Value::Getter(body, captured_env, _home_opt) => {
                                    // Create a function object to expose as the 'get' value on the descriptor
                                    let func_obj = crate::core::new_js_object_data(mc);
                                    let closure_data = crate::core::ClosureData {
                                        params: Vec::new(),
                                        body: body.clone(),
                                        env: *captured_env,
                                        home_object: crate::core::GcCell::new(None),
                                        captured_envs: Vec::new(),
                                        bound_this: None,
                                        is_arrow: false,
                                        is_strict: false,
                                    };
                                    let closure_val = crate::core::Value::Closure(crate::core::Gc::new(mc, closure_data));
                                    object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
                                    object_set_key_value(mc, &desc_obj, "get", Value::Object(func_obj))?;

                                    let enum_flag = Value::Boolean(obj.borrow().is_enumerable(key));
                                    object_set_key_value(mc, &desc_obj, "enumerable", enum_flag)?;
                                    let config_flag = Value::Boolean(obj.borrow().is_configurable(key));
                                    object_set_key_value(mc, &desc_obj, "configurable", config_flag)?;
                                }
                                Value::Setter(params, body, captured_env, _home_opt) => {
                                    // Create a function object to expose as the 'set' value on the descriptor
                                    let func_obj = crate::core::new_js_object_data(mc);
                                    let closure_data = crate::core::ClosureData {
                                        params: params.clone(),
                                        body: body.clone(),
                                        env: *captured_env,
                                        home_object: crate::core::GcCell::new(None),
                                        captured_envs: Vec::new(),
                                        bound_this: None,
                                        is_arrow: false,
                                        is_strict: false,
                                    };
                                    let closure_val = crate::core::Value::Closure(crate::core::Gc::new(mc, closure_data));
                                    object_set_key_value(mc, &func_obj, "__closure__", closure_val)?;
                                    object_set_key_value(mc, &desc_obj, "set", Value::Object(func_obj))?;

                                    let enum_flag = Value::Boolean(obj.borrow().is_enumerable(key));
                                    object_set_key_value(mc, &desc_obj, "enumerable", enum_flag)?;
                                    let config_flag = Value::Boolean(obj.borrow().is_configurable(key));
                                    object_set_key_value(mc, &desc_obj, "configurable", config_flag)?;
                                }
                                other => {
                                    // plain value stored directly
                                    object_set_key_value(mc, &desc_obj, "value", other.clone())?;
                                    let writable_flag = Value::Boolean(obj.borrow().is_writable(key));
                                    object_set_key_value(mc, &desc_obj, "writable", writable_flag)?;
                                    let enum_flag = Value::Boolean(obj.borrow().is_enumerable(key));
                                    object_set_key_value(mc, &desc_obj, "enumerable", enum_flag)?;
                                    let config_flag = Value::Boolean(obj.borrow().is_configurable(key));
                                    object_set_key_value(mc, &desc_obj, "configurable", config_flag)?;
                                }
                            }

                            // debug dump
                            log::trace!("descriptor for key={} created: {:?}", key, desc_obj.borrow().properties);
                            // Put descriptor onto result using the original key (string or symbol)
                            match key {
                                PropertyKey::String(s) => {
                                    object_set_key_value(mc, &result_obj, s, Value::Object(desc_obj))?;
                                }
                                PropertyKey::Symbol(sym_rc) => {
                                    // Push symbol-keyed property on returned object with the same symbol key
                                    let property_key = PropertyKey::Symbol(*sym_rc);
                                    object_set_key_value(mc, &result_obj, &property_key, Value::Object(desc_obj))?;
                                }
                            }
                        }
                    }

                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Object.getOwnPropertyDescriptors called on non-object")),
            }
        }
        "assign" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.assign requires at least one argument"));
            }
            // Evaluate target and apply ToObject semantics: throw on undefined,
            // box primitives into corresponding object wrappers, or use the
            // object directly.
            let target_val = args[0].clone();
            let target_obj = match target_val {
                Value::Object(o) => o,
                Value::Undefined => return Err(raise_type_error!("Object.assign target cannot be undefined or null")),
                Value::Number(n) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", Value::Function("Number_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", Value::Function("Number_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "__value__", Value::Number(n))?;
                    // Set prototype to Number.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Number");
                    obj
                }
                Value::Boolean(b) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", Value::Function("Boolean_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", Value::Function("Boolean_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "__value__", Value::Boolean(b))?;
                    // Set prototype to Boolean.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Boolean");
                    obj
                }
                Value::String(s) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "valueOf", Value::Function("String_valueOf".to_string()))?;
                    object_set_key_value(mc, &obj, "toString", Value::Function("String_toString".to_string()))?;
                    object_set_key_value(mc, &obj, "length", Value::Number(s.len() as f64))?;
                    object_set_key_value(mc, &obj, "__value__", Value::String(s.clone()))?;
                    // Set prototype to String.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "String");
                    obj
                }
                Value::BigInt(h) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "__value__", Value::BigInt(h.clone()))?;
                    // Set prototype to BigInt.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "BigInt");
                    obj
                }
                Value::Symbol(sd) => {
                    let obj = new_js_object_data(mc);
                    object_set_key_value(mc, &obj, "__value__", Value::Symbol(sd))?;

                    // Set prototype to Symbol.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, env, "Symbol");
                    obj
                }
                // For other types (functions, closures, etc.), create a plain object
                _ => new_js_object_data(mc),
            };

            // Iterate sources
            for src_expr in args.iter().skip(1) {
                let src_val = src_expr.clone();
                if let Value::Object(source_obj) = src_val {
                    let ordered = crate::core::ordinary_own_property_keys(&source_obj);
                    for key in ordered {
                        if key == "__proto__".into() {
                            continue;
                        }
                        if let PropertyKey::String(_) = &key
                            && source_obj.borrow().is_enumerable(&key)
                            && let Some(v_rc) = object_get_key_value(&source_obj, &key)
                        {
                            object_set_key_value(mc, &target_obj, &key, v_rc.borrow().clone())?;
                        }
                    }
                }
                // non-objects are skipped, like in JS
            }

            Ok(Value::Object(target_obj))
        }
        "defineProperty" => {
            // Minimal implementation: Object.defineProperty(target, prop, descriptor)
            if args.len() < 3 {
                return Err(raise_type_error!("Object.defineProperty requires three arguments"));
            }
            let target_val = args[0].clone();
            let target_obj = match target_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.defineProperty called on non-object")),
            };

            let prop_val = args[1].clone();
            // Determine property key (support strings & numbers for now)
            let prop_key = match prop_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(n.to_string()),
                _ => return Err(raise_type_error!("Unsupported property key type in Object.defineProperty")),
            };

            let desc_val = args[2].clone();
            let desc_obj = match desc_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Property descriptor must be an object")),
            };

            define_property_internal(mc, &target_obj, prop_key, &desc_obj)?;
            Ok(Value::Object(target_obj))
        }
        "defineProperties" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Object.defineProperties requires two arguments"));
            }
            let target_val = args[0].clone();
            let target_obj = match target_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.defineProperties called on non-object")),
            };

            let props_val = args[1].clone();
            let props_obj = match props_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.defineProperties requires an object as second argument")),
            };

            // Iterate over own properties of props_obj
            for (key, val_rc) in props_obj.borrow().properties.iter() {
                // Only process own properties (already handled by properties map)
                // In JS, it also checks enumerability, but for now we iterate all.
                // Actually, Object.defineProperties only uses own enumerable properties.
                // Let's check enumerability.
                if !props_obj.borrow().is_enumerable(key) {
                    continue;
                }

                let desc_val = val_rc.borrow().clone();
                let desc_obj = match desc_val {
                    Value::Object(o) => o,
                    _ => return Err(raise_type_error!("Property descriptor must be an object")),
                };

                define_property_internal(mc, &target_obj, key.clone(), &desc_obj)?;
            }

            Ok(Value::Object(target_obj))
        }
        _ => Err(raise_eval_error!(format!("Object.{method} is not implemented"))),
    }
}

pub(crate) fn handle_to_string_method<'gc>(
    mc: &MutationContext<'gc>,
    obj_val: &Value<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if !args.is_empty() {
        return Err(raise_type_error!(format!(
            "{}.toString() takes no arguments, but {} were provided",
            match obj_val {
                Value::Number(_) => "Number",
                Value::BigInt(_) => "BigInt",
                Value::String(_) => "String",
                Value::Boolean(_) => "Boolean",
                Value::Object(_) => "Object",
                Value::Function(_) => "Function",
                Value::Closure(..) | Value::AsyncClosure(..) => "Function",
                Value::Undefined => "undefined",
                Value::Null => "null",
                Value::ClassDefinition(_) => "Class",
                Value::Getter(..) => "Getter",
                Value::Setter(..) => "Setter",
                Value::Property { .. } => "Property",
                Value::Promise(_) => "Promise",
                Value::Symbol(_) => "Symbol",
                Value::Map(_) => "Map",
                Value::Set(_) => "Set",
                Value::WeakMap(_) => "WeakMap",
                Value::WeakSet(_) => "WeakSet",
                Value::GeneratorFunction(..) => "GeneratorFunction",
                Value::Generator(_) => "Generator",
                Value::Proxy(_) => "Proxy",
                Value::ArrayBuffer(_) => "ArrayBuffer",
                Value::DataView(_) => "DataView",
                Value::TypedArray(_) => "TypedArray",
                Value::Uninitialized => "undefined",
            },
            args.len()
        )));
    }

    if let Value::Object(object) = obj_val {
        // Check if this object defines its own toString method
        if let Some(method_rc) = object_get_key_value(object, "toString") {
            let method_val = method_rc.borrow().clone();
            match method_val {
                Value::Function(ref name) if name == "Object.prototype.toString" => {
                    // This is the default prototype method, skip calling it to avoid recursion
                    // and proceed to the default implementation below.
                }
                Value::Closure(_) | Value::AsyncClosure(_) | Value::Function(_) | Value::Object(_) => {
                    // If it's an object, it might be a function object (with a __closure__ property)
                    let res = evaluate_call_dispatch(mc, env, method_val, Some(obj_val.clone()), Vec::new()).map_err(JSError::from)?;
                    return Ok(res);
                }
                _ => {}
            }
        }
    }

    match obj_val {
        Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
        Value::BigInt(h) => Ok(Value::String(utf8_to_utf16(&h.to_string()))),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
        Value::Undefined => Ok(Value::String(utf8_to_utf16("[object Undefined]"))),
        Value::Null => Ok(Value::String(utf8_to_utf16("[object Null]"))),
        Value::Uninitialized => Ok(Value::String(utf8_to_utf16("[object Undefined]"))),
        Value::Object(object) => {
            // If this is an Error object, use Error.prototype.toString behavior
            if crate::core::js_error::is_error(&Value::Object(*object)) {
                return handle_error_to_string_method(mc, &Value::Object(*object), args);
            }

            // Check if this is a wrapped primitive object
            if let Some(wrapped_val) = object_get_key_value(object, "__value__") {
                match &*wrapped_val.borrow() {
                    Value::Number(n) => return Ok(Value::String(utf8_to_utf16(&n.to_string()))),
                    Value::BigInt(h) => return Ok(Value::String(utf8_to_utf16(&h.to_string()))),
                    Value::Boolean(b) => return Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                    Value::String(s) => return Ok(Value::String(s.clone())),
                    _ => {}
                }
            }

            // If this object looks like a Date (has __timestamp), call Date.toString()
            if is_date_object(object) {
                return crate::js_date::handle_date_method(mc, &Value::Object(*object), "toString", &[], env);
            }

            // If this object looks like an array, join elements with comma (Array.prototype.toString overrides Object.prototype)
            if is_array(mc, object) {
                let current_len = get_array_length(mc, object).unwrap_or(0);
                let mut parts = Vec::new();
                for i in 0..current_len {
                    if let Some(val_rc) = object_get_key_value(object, i) {
                        match &*val_rc.borrow() {
                            Value::Undefined | Value::Null => parts.push("".to_string()), // push empty string for null and undefined
                            Value::String(s) => parts.push(utf16_to_utf8(s)),
                            Value::Number(n) => parts.push(n.to_string()),
                            Value::Boolean(b) => parts.push(b.to_string()),
                            Value::BigInt(b) => parts.push(format!("{}n", b)),
                            _ => parts.push("[object Object]".to_string()),
                        }
                    } else {
                        parts.push("".to_string())
                    }
                }
                return Ok(Value::String(utf8_to_utf16(&parts.join(","))));
            }

            // If this object contains a Symbol.toStringTag property, honor it
            if let Some(tag_sym_rc) = get_well_known_symbol(mc, env, "toStringTag")
                && let Value::Symbol(sd) = &*tag_sym_rc.borrow()
                && let Some(tag_val_rc) = object_get_key_value(object, sd)
                && let Value::String(s) = &*tag_val_rc.borrow()
            {
                return Ok(Value::String(utf8_to_utf16(&format!("[object {}]", utf16_to_utf8(s)))));
            }

            // Default object tag
            Ok(Value::String(utf8_to_utf16("[object Object]")))
        }
        Value::Function(name) => Ok(Value::String(utf8_to_utf16(&format!("[Function: {}]", name)))),
        Value::Closure(..) | Value::AsyncClosure(..) => Ok(Value::String(utf8_to_utf16("[Function]"))),
        Value::ClassDefinition(_) => Ok(Value::String(utf8_to_utf16("[Class]"))),
        Value::Getter(..) => Ok(Value::String(utf8_to_utf16("[Getter]"))),
        Value::Setter(..) => Ok(Value::String(utf8_to_utf16("[Setter]"))),
        Value::Property { .. } => Ok(Value::String(utf8_to_utf16("[Property]"))),
        Value::Promise(_) => Ok(Value::String(utf8_to_utf16("[object Promise]"))),
        Value::Symbol(symbol_data) => {
            let desc_str = symbol_data.description.as_deref().unwrap_or("");
            Ok(Value::String(utf8_to_utf16(&format!("Symbol({})", desc_str))))
        }
        Value::Map(_) => Ok(Value::String(utf8_to_utf16("[object Map]"))),
        Value::Set(_) => Ok(Value::String(utf8_to_utf16("[object Set]"))),
        Value::WeakMap(_) => Ok(Value::String(utf8_to_utf16("[object WeakMap]"))),
        Value::WeakSet(_) => Ok(Value::String(utf8_to_utf16("[object WeakSet]"))),
        Value::GeneratorFunction(..) => Ok(Value::String(utf8_to_utf16("[GeneratorFunction]"))),
        Value::Generator(_) => Ok(Value::String(utf8_to_utf16("[object Generator]"))),
        Value::Proxy(_) => Ok(Value::String(utf8_to_utf16("[object Proxy]"))),
        Value::ArrayBuffer(_) => Ok(Value::String(utf8_to_utf16("[object ArrayBuffer]"))),
        Value::DataView(_) => Ok(Value::String(utf8_to_utf16("[object DataView]"))),
        Value::TypedArray(_) => Ok(Value::String(utf8_to_utf16("[object TypedArray]"))),
    }
}

#[allow(dead_code)]
pub(crate) fn handle_error_to_string_method<'gc>(
    _mc: &MutationContext<'gc>,
    obj_val: &Value<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, JSError> {
    if !args.is_empty() {
        return Err(raise_type_error!("Error.prototype.toString takes no arguments"));
    }

    // Expect an object receiver
    if let Value::Object(object) = obj_val {
        // name default to "Error"
        let name = if let Some(n_rc) = object_get_key_value(object, "name") {
            if let Value::String(s) = &*n_rc.borrow() {
                utf16_to_utf8(s)
            } else {
                "Error".to_string()
            }
        } else {
            "Error".to_string()
        };

        // message default to empty
        let message = if let Some(m_rc) = object_get_key_value(object, "message") {
            if let Value::String(s) = &*m_rc.borrow() {
                utf16_to_utf8(s)
            } else {
                "".to_string()
            }
        } else {
            "".to_string()
        };

        if message.is_empty() {
            Ok(Value::String(utf8_to_utf16(&name)))
        } else {
            Ok(Value::String(utf8_to_utf16(&format!("{}: {}", name, message))))
        }
    } else {
        Err(raise_type_error!("Error.prototype.toString called on non-object"))
    }
}

pub(crate) fn handle_value_of_method<'gc>(
    mc: &MutationContext<'gc>,
    obj_val: &Value<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if !args.is_empty() {
        return Err(raise_type_error!(format!(
            "{}.valueOf() takes no arguments, but {} were provided",
            match obj_val {
                Value::Number(_) => "Number",
                Value::String(_) => "String",
                Value::Boolean(_) => "Boolean",
                Value::Object(_) => "Object",
                Value::Function(_) => "Function",
                Value::Closure(..) | Value::AsyncClosure(..) => "Function",
                Value::Undefined => "undefined",
                Value::Null => "null",
                Value::ClassDefinition(_) => "Class",
                &Value::Getter(..) => "Getter",
                &Value::Setter(..) => "Setter",
                &Value::Property { .. } => "Property",
                &Value::Promise(_) => "Promise",
                Value::BigInt(_) => "BigInt",
                Value::Symbol(_) => "Symbol",
                Value::Map(_) => "Map",
                Value::Set(_) => "Set",
                Value::WeakMap(_) => "WeakMap",
                Value::WeakSet(_) => "WeakSet",
                &Value::GeneratorFunction(..) => "GeneratorFunction",
                &Value::Generator(_) => "Generator",
                &Value::Proxy(_) => "Proxy",
                &Value::ArrayBuffer(_) => "ArrayBuffer",
                &Value::DataView(_) => "DataView",
                &Value::TypedArray(_) => "TypedArray",
                Value::Uninitialized => "undefined",
            },
            args.len()
        )));
    }
    match obj_val {
        Value::Number(n) => Ok(Value::Number(*n)),
        Value::BigInt(s) => Ok(Value::BigInt(s.clone())),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::Boolean(*b)),
        Value::Undefined => Err(raise_type_error!("Cannot convert undefined to object")),
        Value::Null => Err(raise_type_error!("Cannot convert null to object")),
        Value::Object(obj) => {
            // Check if this is a wrapped primitive object
            if let Some(wrapped_val) = object_get_key_value(obj, "__value__") {
                return Ok(wrapped_val.borrow().clone());
            }
            // If object defines a user valueOf function, call it and use its
            // primitive result if it returns a primitive.
            if let Some(method_rc) = object_get_key_value(obj, "valueOf") {
                let method_val = method_rc.borrow().clone();
                match method_val {
                    Value::Closure(data) | Value::AsyncClosure(data) => {
                        let _params = &data.params;
                        let body = data.body.clone();
                        let captured_env = &data.env;
                        let func_env =
                            prepare_function_call_env(mc, Some(captured_env), Some(Value::Object(*obj)), None, &[], None, Some(env))?;
                        let result = crate::core::evaluate_statements(mc, &func_env, &body)?;
                        if matches!(
                            result,
                            Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::BigInt(_) | Value::Symbol(_)
                        ) {
                            return Ok(result);
                        }
                    }
                    Value::Function(func_name) => {
                        // If the function is one of the Object.prototype builtins,
                        // handle it directly here to avoid recursion back into
                        // `handle_global_function` which would call us again.
                        if func_name == "Object.prototype.valueOf" {
                            return Ok(Value::Object(*obj));
                        }
                        if func_name == "Object.prototype.toString" {
                            return crate::js_object::handle_to_string_method(mc, &Value::Object(*obj), args, env);
                        }

                        // let func_env = prepare_function_call_env(mc, None, Some(Value::Object(obj)), None, &[], None, Some(env))?;
                        // let res = crate::js_function::handle_global_function(mc, &func_name, &[], &func_env)?;
                        // if matches!(
                        //     res,
                        //     Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::BigInt(_) | Value::Symbol(_)
                        // ) {
                        //     return Ok(res);
                        // }
                        todo!("Handle built-in function calls in valueOf");
                    }
                    _ => {}
                }
                // Support method stored as a function-object (object wrapping a closure)
                if let Value::Object(func_obj_map) = &*method_rc.borrow()
                    && let Some(cl_rc) = object_get_key_value(func_obj_map, "__closure__")
                {
                    match &*cl_rc.borrow() {
                        Value::Closure(data) | Value::AsyncClosure(data) => {
                            let _params = &data.params;
                            let body = data.body.clone();
                            let captured_env = &data.env;
                            let func_env =
                                prepare_function_call_env(mc, Some(captured_env), Some(Value::Object(*obj)), None, &[], None, Some(env))?;
                            let result = crate::core::evaluate_statements(mc, &func_env, &body)?;
                            if matches!(
                                result,
                                Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::BigInt(_) | Value::Symbol(_)
                            ) {
                                return Ok(result);
                            }
                        }
                        _ => {}
                    }
                }
            }
            // If this object looks like a Date (has __timestamp), call Date.valueOf()
            if is_date_object(obj) {
                return crate::js_date::handle_date_method(mc, obj_val, "valueOf", &[], env);
            }
            // For regular objects, return the object itself
            Ok(Value::Object(*obj))
        }
        Value::Function(name) => Ok(Value::Function(name.clone())),
        Value::Closure(data) | Value::AsyncClosure(data) => {
            let closure_data = ClosureData::new(&data.params, &data.body, &data.env, None);
            Ok(Value::Closure(Gc::new(mc, closure_data)))
        }
        Value::ClassDefinition(class_def) => Ok(Value::ClassDefinition(*class_def)),
        Value::Getter(body, env, _) => Ok(Value::Getter(body.clone(), *env, None)),
        Value::Setter(param, body, env, _) => Ok(Value::Setter(param.clone(), body.clone(), *env, None)),
        Value::Property { value, getter, setter } => Ok(Value::Property {
            value: *value,
            getter: getter.clone(),
            setter: setter.clone(),
        }),
        Value::Promise(promise) => Ok(Value::Promise(*promise)),
        Value::Symbol(symbol_data) => Ok(Value::Symbol(*symbol_data)),
        Value::Map(map) => Ok(Value::Map(*map)),
        Value::Set(set) => Ok(Value::Set(*set)),
        Value::WeakMap(weakmap) => Ok(Value::WeakMap(*weakmap)),
        Value::WeakSet(weakset) => Ok(Value::WeakSet(*weakset)),
        Value::GeneratorFunction(_, data) => {
            let closure_data = ClosureData::new(&data.params, &data.body, &data.env, None);
            Ok(Value::GeneratorFunction(None, Gc::new(mc, closure_data)))
        }
        Value::Generator(generator) => Ok(Value::Generator(*generator)),
        Value::Proxy(proxy) => Ok(Value::Proxy(*proxy)),
        Value::ArrayBuffer(array_buffer) => Ok(Value::ArrayBuffer(*array_buffer)),
        Value::DataView(data_view) => Ok(Value::DataView(*data_view)),
        Value::TypedArray(typed_array) => Ok(Value::TypedArray(*typed_array)),
        Value::Uninitialized => Err(raise_type_error!("Cannot convert uninitialized to object")),
    }
}

/// Handle built-in Object.prototype.* functions in one place so callers don't
/// have to duplicate matching logic. If `func_name` matches a supported
/// Object.prototype builtin, the function will execute it (evaluating args
/// where needed) and return `Ok(Some(Value))`. If it does not match, returns
/// `Ok(None)` so callers can fall back to other dispatch logic.
pub(crate) fn handle_object_prototype_builtin<'gc>(
    mc: &MutationContext<'gc>,
    func_name: &str,
    object: &JSObjectDataPtr<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, JSError> {
    match func_name {
        "Object.prototype.hasOwnProperty" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("hasOwnProperty requires one argument"));
            }
            let key_val = args[0].clone();
            let exists = crate::core::has_own_property_value(object, &key_val);
            Ok(Some(Value::Boolean(exists)))
        }
        "Object.prototype.isPrototypeOf" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("isPrototypeOf requires one argument"));
            }
            let target_val = args[0].clone();
            match target_val {
                Value::Object(target_map) => {
                    let mut current_opt: Option<Gc<'gc, GcCell<crate::core::JSObjectData<'gc>>>> = target_map.borrow().prototype;
                    while let Some(parent) = current_opt {
                        if Gc::ptr_eq(parent, *object) {
                            return Ok(Some(Value::Boolean(true)));
                        }
                        current_opt = parent.borrow().prototype;
                    }
                    Ok(Some(Value::Boolean(false)))
                }
                _ => Ok(Some(Value::Boolean(false))),
            }
        }
        "Object.prototype.propertyIsEnumerable" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("propertyIsEnumerable requires one argument"));
            }
            let key_val = args[0].clone();
            let exists = crate::core::has_own_property_value(object, &key_val);
            Ok(Some(Value::Boolean(exists)))
        }
        "Object.prototype.toString" => Ok(Some(crate::js_object::handle_to_string_method(
            mc,
            &Value::Object(*object),
            args,
            env,
        )?)),
        "Object.prototype.valueOf" => Ok(Some(crate::js_object::handle_value_of_method(
            mc,
            &Value::Object(*object),
            args,
            env,
        )?)),
        "Object.prototype.toLocaleString" => Ok(Some(crate::js_object::handle_to_string_method(
            mc,
            &Value::Object(*object),
            args,
            env,
        )?)),
        _ => Ok(None),
    }
}

fn get_well_known_symbol<'gc>(_mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str) -> Option<GcPtr<'gc, Value<'gc>>> {
    if let Some(sym_ctor_val) = crate::core::env_get(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_ctor_val.borrow()
        && let Some(sym_val) = object_get_key_value(sym_ctor, name)
        && let Value::Symbol(_) = &*sym_val.borrow()
    {
        return Some(sym_val);
    }
    None
}
