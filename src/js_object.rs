use crate::core::{
    ClosureData, EvalError, JSObjectDataPtr, PropertyDescriptor, PropertyKey, Value, evaluate_call_dispatch, new_js_object_data,
    object_get_key_value, object_set_key_value, prepare_closure_call_env, prepare_function_call_env, value_to_string,
};
use crate::core::{Gc, GcCell, GcPtr, MutationContext, new_gc_cell_ptr};
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
    object_proto.borrow_mut(mc).set_non_enumerable("constructor");

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
        object_proto.borrow_mut(mc).set_non_enumerable(method);
    }

    Ok(())
}

pub(crate) fn define_property_internal<'gc>(
    mc: &MutationContext<'gc>,
    target_obj: &JSObjectDataPtr<'gc>,
    prop_key: impl Into<PropertyKey<'gc>>,
    desc_obj: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    let prop_key = &prop_key.into();
    // Parse descriptor into typed PropertyDescriptor
    let pd = PropertyDescriptor::from_object(desc_obj)?;

    // If the property exists and is non-configurable on the target, apply ECMAScript-compatible checks
    if let Some(existing_rc) = object_get_key_value(target_obj, prop_key)
        && !target_obj.borrow().is_configurable(prop_key)
    {
        // If descriptor explicitly sets configurable true -> throw
        if pd.configurable == Some(true) {
            return Err(raise_type_error!("Cannot make non-configurable property configurable"));
        }

        // If descriptor explicitly sets enumerable and it's different -> throw
        if let Some(new_enum) = pd.enumerable {
            let existing_enum = target_obj.borrow().is_enumerable(prop_key);
            if new_enum != existing_enum {
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
            if pd.get.is_some() || pd.set.is_some() {
                return Err(raise_type_error!("Cannot convert non-configurable data property to an accessor"));
            }

            // If writable is being set from false -> true, disallow
            if pd.writable == Some(true) && !target_obj.borrow().is_writable(prop_key) {
                return Err(raise_type_error!("Cannot make non-writable property writable"));
            }

            // If attempting to change value while not writable and values differ -> throw
            if !target_obj.borrow().is_writable(prop_key)
                && let Some(pd_value) = &pd.value
            {
                // get existing value for comparison
                let existing_val = match &*existing_rc.borrow() {
                    Value::Property { value: Some(v), .. } => v.borrow().clone(),
                    other => other.clone(),
                };
                if !crate::core::values_equal(mc, &existing_val, pd_value) {
                    return Err(raise_type_error!("Cannot change value of non-writable, non-configurable property"));
                }
            }
        } else {
            // existing is accessor
            // Disallow converting to data property
            if pd.value.is_some() || pd.writable.is_some() {
                return Err(raise_type_error!("Cannot convert non-configurable accessor to a data property"));
            }

            // Disallow changing getter/setter functions on non-configurable accessor
            if pd.get.is_some() || pd.set.is_some() {
                return Err(raise_type_error!(
                    "Cannot change getter/setter of non-configurable accessor property"
                ));
            }
        }
    }

    let mut getter_opt: Option<Box<Value>> = None;
    if pd.get.is_some()
        && let Some(get_val) = pd.get.clone()
        && !matches!(get_val, Value::Undefined)
    {
        getter_opt = Some(Box::new(get_val));
    }

    let mut setter_opt: Option<Box<Value>> = None;
    if pd.set.is_some()
        && let Some(set_val) = pd.set.clone()
        && !matches!(set_val, Value::Undefined)
    {
        setter_opt = Some(Box::new(set_val));
    }

    // Create property descriptor value
    let prop_descriptor = Value::Property {
        value: pd.value.clone().map(|v| new_gc_cell_ptr(mc, v)),
        getter: getter_opt,
        setter: setter_opt,
    };

    // DEBUG: Log raw descriptor fields for troubleshooting
    log::debug!("define_property_internal: descriptor writable raw = {:?}", pd.writable);
    log::debug!("define_property_internal: descriptor enumerable raw = {:?}", pd.enumerable);
    if let Some(is_cfg) = pd.configurable {
        if is_cfg {
            log::trace!("define_property_internal: setting configurable=true for {:?}", prop_key);
            target_obj.borrow_mut(mc).set_configurable(prop_key.clone());
        } else {
            log::trace!("define_property_internal: setting configurable=false for {:?}", prop_key);
            target_obj.borrow_mut(mc).set_non_configurable(prop_key.clone());
        }
    }

    // If descriptor is a property descriptor (has value/get/set), unspecified attributes default to false
    let is_property_desc = pd.value.is_some() || pd.get.is_some() || pd.set.is_some();
    if is_property_desc {
        // Only default missing 'writable' to false for data descriptors (value or writable present).
        // For accessor descriptors (get/set) the 'writable' attribute does not apply and
        // must not be defaulted to false here.
        if (pd.value.is_some() || pd.writable.is_some()) && pd.writable.is_none() {
            log::trace!(
                "define_property_internal: writable absent -> default false; setting non-writable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_non_writable(prop_key.clone());
        }
        // Default missing 'enumerable' to false
        if pd.enumerable.is_none() {
            log::trace!(
                "define_property_internal: enumerable absent -> default false; setting non-enumerable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_non_enumerable(prop_key.clone());
        }
        // Default missing 'configurable' to false
        if pd.configurable.is_none() {
            log::trace!(
                "define_property_internal: configurable absent -> default false; setting non-configurable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_non_configurable(prop_key.clone());
        }
    }

    // Only update writable/enumerable if property is configurable or does not exist
    let existed = object_get_key_value(target_obj, prop_key).is_some();
    let is_configurable = existed || target_obj.borrow().is_configurable(prop_key);
    log::debug!(
        "define_property_internal: existed={} is_configurable={} for {:?} on obj_ptr={:p}",
        existed,
        is_configurable,
        prop_key,
        target_obj.as_ptr()
    );
    if is_configurable || !existed {
        if let PropertyKey::String(s) = prop_key
            && s == "length"
        {
            log::debug!(
                "define_property_internal: defining 'length' desc fields: writable_present={} configurable_present={} enumerable_present={}",
                pd.writable.is_some(),
                pd.configurable.is_some(),
                pd.enumerable.is_some()
            );
        }
        // If writable flag explicitly set to false, mark property as non-writable
        if pd.writable == Some(false) {
            log::trace!(
                "define_property_internal: setting non-writable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            log::debug!(
                "define_property_internal: before set_non_writable contains={} list={:?} for obj_ptr={:p} key={:?}",
                target_obj.borrow().non_writable.contains(prop_key),
                target_obj.borrow().non_writable.iter().collect::<Vec<_>>(),
                target_obj.as_ptr(),
                prop_key
            );
            target_obj.borrow_mut(mc).set_non_writable(prop_key.clone());
            log::debug!(
                "define_property_internal: after set_non_writable contains={} list={:?} for obj_ptr={:p} key={:?}",
                target_obj.borrow().non_writable.contains(prop_key),
                target_obj.borrow().non_writable.iter().collect::<Vec<_>>(),
                target_obj.as_ptr(),
                prop_key
            );
        }
        // If writable flag explicitly set to true, clear non-writable
        if pd.writable == Some(true) {
            log::trace!(
                "define_property_internal: setting writable=true for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_writable(prop_key.clone());
        }

        // If enumerable flag explicitly set to false, mark property as non-enumerable
        if pd.enumerable == Some(false) {
            log::trace!(
                "define_property_internal: setting non-enumerable for {:?} on obj_ptr={:p}",
                prop_key,
                target_obj.as_ptr()
            );
            target_obj.borrow_mut(mc).set_non_enumerable(prop_key.clone());
        }
        // If enumerable flag explicitly set to true, clear non-enumerable
        if pd.enumerable == Some(true) {
            target_obj.borrow_mut(mc).set_enumerable(prop_key.clone());
        }
    }

    // DEBUG: print non_writable/non_enumerable state before final set
    log::debug!(
        "define_property_internal: post-flag state for obj_ptr={:p} key={:?} non_writable_contains={} non_writable_list={:?} non_enumerable_contains={} non_enumerable_list={:?}",
        target_obj.as_ptr(),
        prop_key,
        target_obj.borrow().non_writable.contains(prop_key),
        target_obj.borrow().non_writable.iter().collect::<Vec<_>>(),
        target_obj.borrow().non_enumerable.contains(prop_key),
        target_obj.borrow().non_enumerable.iter().collect::<Vec<_>>()
    );

    // Always update value
    object_set_key_value(mc, target_obj, prop_key, prop_descriptor)?;
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
                Value::Object(_) => {
                    // ToPropertyKey semantics: ToPrimitive with hint 'string'
                    let prim = crate::core::to_primitive(mc, &prop_val, "string", env)?;
                    match prim {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        Value::Symbol(s) => PropertyKey::Symbol(s),
                        other => PropertyKey::String(value_to_string(&other)),
                    }
                }
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
                        let func_env = prepare_closure_call_env(mc, Some(&captured_env), Some(&params), &args, Some(env))?;
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
                            let _value = if let Some(val) = object_get_key_value(desc_obj, "value") {
                                val.borrow().clone()
                            } else {
                                Value::Undefined
                            };
                            let mut desc_obj_copy = desc_obj.borrow().clone();
                            let desc_obj_ptr = new_js_object_data(mc);
                            for (k, v) in desc_obj_copy.properties.drain(..) {
                                object_set_key_value(mc, &desc_obj_ptr, k, v.borrow().clone())?;
                            }
                            define_property_internal(mc, &new_obj, key, &desc_obj_ptr)?;
                        }
                    }
                }
            }

            Ok(Value::Object(new_obj))
        }
        "setPrototypeOf" => {
            if args.len() != 2 {
                return Err(raise_type_error!("Object.setPrototypeOf requires exactly two arguments"));
            }
            match &args[0] {
                Value::Object(obj) => match args[1] {
                    Value::Object(proto_obj) => {
                        obj.borrow_mut(mc).prototype = Some(proto_obj);
                        Ok(Value::Object(*obj))
                    }
                    Value::Undefined | Value::Null => {
                        obj.borrow_mut(mc).prototype = None;
                        Ok(Value::Object(*obj))
                    }
                    _ => Err(raise_type_error!("Object.setPrototypeOf prototype must be an object or null")),
                },
                _ => Err(raise_type_error!("Object.setPrototypeOf target must be an object")),
            }
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
                Value::Object(_) => {
                    let prim = crate::core::to_primitive(mc, &prop_val, "string", env)?;
                    match prim {
                        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                        Value::Symbol(s) => PropertyKey::Symbol(s),
                        other => PropertyKey::String(value_to_string(&other)),
                    }
                }
                val => PropertyKey::String(value_to_string(&val)),
            };

            if let Some(_val_rc) = object_get_key_value(&obj, &key) {
                if let Some(pd) = crate::core::build_property_descriptor(mc, &obj, &key) {
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
                        if let Some(pd) = crate::core::build_property_descriptor(mc, obj, key) {
                            let desc_obj = pd.to_object(mc)?; // Put descriptor onto result using the original key (string or symbol)
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
                        // Copy both string and symbol keyed enumerable own properties
                        if (matches!(key, PropertyKey::String(_)) || matches!(key, PropertyKey::Symbol(_)))
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
                Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                _ => return Err(raise_type_error!("Unsupported property key type in Object.defineProperty")),
            };

            let desc_val = args[2].clone();
            let desc_obj = match desc_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Property descriptor must be an object")),
            };

            let pd = PropertyDescriptor::from_object(&desc_obj)?;
            crate::core::validate_descriptor_for_define(mc, &pd)?;
            define_property_internal(mc, &target_obj, &prop_key, &desc_obj)?;
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

                let pd = PropertyDescriptor::from_object(&desc_obj)?;
                crate::core::validate_descriptor_for_define(mc, &pd)?;

                define_property_internal(mc, &target_obj, key, &desc_obj)?;
            }

            Ok(Value::Object(target_obj))
        }
        "is" => {
            // Object.is(x,y) - SameValue comparison
            let a = args.first().cloned().unwrap_or(Value::Undefined);
            let b = args.get(1).cloned().unwrap_or(Value::Undefined);
            let eq = crate::core::values_equal(mc, &a, &b);
            Ok(Value::Boolean(eq))
        }
        _ => Err(raise_eval_error!(format!("Object.{method} is not implemented"))),
    }
}

pub(crate) fn handle_to_string_method<'gc>(
    mc: &MutationContext<'gc>,
    obj_val: &Value<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if !args.is_empty() {
        return Err(EvalError::Js(raise_type_error!(format!(
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
                Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..) => "GeneratorFunction",
                Value::Generator(_) | Value::AsyncGenerator(_) => "Generator",
                Value::Proxy(_) => "Proxy",
                Value::ArrayBuffer(_) => "ArrayBuffer",
                Value::DataView(_) => "DataView",
                Value::TypedArray(_) => "TypedArray",
                Value::Uninitialized => "undefined",
            },
            args.len()
        ))));
    }

    if let Value::Object(object) = obj_val {
        // Check if this object defines its own toString method (Get semantics)
        if let Some(method_rc) = object_get_key_value(object, "toString") {
            let method_val = method_rc.borrow().clone();
            log::debug!("DBG handle_to_string_method: found toString property => {:?}", method_val);
            match method_val {
                // If the property is a callable, call it and return the result
                Value::Closure(_) | Value::AsyncClosure(_) | Value::Function(_) | Value::Object(_) => {
                    // If it's an object, it might be a function object (with an internal closure slot)
                    log::debug!("DBG handle_to_string_method: calling toString implementation");
                    let res = evaluate_call_dispatch(mc, env, method_val, Some(obj_val.clone()), Vec::new()).map_err(JSError::from)?;
                    log::debug!("DBG handle_to_string_method: toString returned {:?}", res);
                    return Ok(res);
                }
                // If the property exists but is not callable (e.g. `toString: null`),
                // per ECMAScript ToPrimitive semantics we must not call the prototype's
                // `toString` - instead indicate there's no callable toString by
                // returning `Uninitialized` so the caller can try `valueOf` next.
                _ => {
                    log::debug!("DBG handle_to_string_method: toString not callable -> returning Uninitialized sentinel");
                    // Indicate "no callable toString" by returning `Uninitialized` as
                    // a sentinel that is not a JS primitive and will cause the
                    // caller to proceed to `valueOf`.
                    return Ok(Value::Uninitialized);
                }
            }
        }
    }

    match obj_val {
        Value::Number(n) => Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(*n))))),
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
                    Value::Number(n) => return Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(*n))))),
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
                            Value::Number(n) => parts.push(crate::core::value_to_string(&Value::Number(*n))),
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
        Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..) => Ok(Value::String(utf8_to_utf16("[GeneratorFunction]"))),
        Value::Generator(_) | Value::AsyncGenerator(_) => Ok(Value::String(utf8_to_utf16("[object Generator]"))),
        Value::Proxy(_) => Ok(Value::String(utf8_to_utf16("[object Proxy]"))),
        Value::ArrayBuffer(_) => Ok(Value::String(utf8_to_utf16("[object ArrayBuffer]"))),
        Value::DataView(_) => Ok(Value::String(utf8_to_utf16("[object DataView]"))),
        Value::TypedArray(_) => Ok(Value::String(utf8_to_utf16("[object TypedArray]"))),
    }
}

pub(crate) fn handle_error_to_string_method<'gc>(
    _mc: &MutationContext<'gc>,
    obj_val: &Value<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError<'gc>> {
    if !args.is_empty() {
        return Err(EvalError::Js(raise_type_error!("Error.prototype.toString takes no arguments")));
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
        Err(EvalError::Js(raise_type_error!("Error.prototype.toString called on non-object")))
    }
}

pub(crate) fn handle_value_of_method<'gc>(
    mc: &MutationContext<'gc>,
    obj_val: &Value<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if !args.is_empty() {
        return Err(EvalError::Js(raise_type_error!(format!(
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
                &Value::GeneratorFunction(..) | &Value::AsyncGeneratorFunction(..) => "GeneratorFunction",
                &Value::Generator(_) | &Value::AsyncGenerator(_) => "Generator",
                &Value::Proxy(_) => "Proxy",
                &Value::ArrayBuffer(_) => "ArrayBuffer",
                &Value::DataView(_) => "DataView",
                &Value::TypedArray(_) => "TypedArray",
                Value::Uninitialized => "undefined",
            },
            args.len()
        ))));
    }
    match obj_val {
        Value::Number(n) => Ok(Value::Number(*n)),
        Value::BigInt(s) => Ok(Value::BigInt(s.clone())),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::Boolean(*b)),
        Value::Undefined => Err(EvalError::Js(raise_type_error!("Cannot convert undefined to object"))),
        Value::Null => Err(EvalError::Js(raise_type_error!("Cannot convert null to object"))),
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
                        let func_env = prepare_function_call_env(
                            mc,
                            Some(captured_env.as_ref().unwrap()),
                            Some(Value::Object(*obj)),
                            None,
                            &[],
                            None,
                            Some(env),
                        )?;
                        let result = crate::core::evaluate_statements(mc, &func_env, &body)?;
                        if matches!(
                            result,
                            Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::BigInt(_) | Value::Symbol(_)
                        ) {
                            return Ok(result);
                        }
                    }
                    Value::Function(func_name) => {
                        log::debug!("DBG handle_value_of_method: found function name='{}'", func_name);
                        // If the function is one of the Object.prototype builtins,
                        // handle it directly here to avoid recursion back into
                        // `handle_global_function` which would call us again.
                        if func_name == "Object.prototype.valueOf" {
                            return Ok(Value::Object(*obj));
                        }
                        if func_name == "Object.prototype.toString" {
                            return crate::js_object::handle_to_string_method(mc, &Value::Object(*obj), args, env);
                        }
                        // Handle Date prototype functions specially when invoking as a method
                        if func_name.ends_with(".valueOf") && crate::js_date::is_date_object(obj) {
                            return crate::js_date::handle_date_method(mc, &Value::Object(*obj), "valueOf", args, env);
                        }
                        if func_name.ends_with(".toString") && crate::js_date::is_date_object(obj) {
                            return crate::js_date::handle_date_method(mc, &Value::Object(*obj), "toString", args, env);
                        }
                        if let Some(method) = func_name.strip_prefix("Date.prototype.") {
                            return crate::js_date::handle_date_method(mc, &Value::Object(*obj), method, args, env);
                        }

                        let func_env = prepare_function_call_env(mc, None, Some(Value::Object(*obj)), None, &[], None, Some(env))?;
                        // Use the central `evaluate_call_dispatch` helper to perform the call so
                        // builtins like `Date.prototype.*` are routed correctly when represented
                        // as `Value::Function` names. This avoids falling back to `handle_global_function`
                        // which does not perform the same prefix-based dispatching.
                        let eval_args: Vec<Value<'gc>> = Vec::new();
                        let res = crate::core::evaluate_call_dispatch(
                            mc,
                            &func_env,
                            Value::Function(func_name.clone()),
                            Some(Value::Object(*obj)),
                            eval_args,
                        )?;
                        if matches!(
                            res,
                            Value::Number(_) | Value::String(_) | Value::Boolean(_) | Value::BigInt(_) | Value::Symbol(_)
                        ) {
                            return Ok(res);
                        }
                    }
                    _ => {}
                }
                // Support method stored as a function-object (object wrapping a closure)
                if let Value::Object(func_obj_map) = &*method_rc.borrow()
                    && let Some(cl_rc) = func_obj_map.borrow().get_closure()
                {
                    match &*cl_rc.borrow() {
                        Value::Closure(data) | Value::AsyncClosure(data) => {
                            let _params = &data.params;
                            let body = data.body.clone();
                            let captured_env = &data.env;
                            let func_env = prepare_function_call_env(
                                mc,
                                Some(captured_env.as_ref().unwrap()),
                                Some(Value::Object(*obj)),
                                None,
                                &[],
                                None,
                                Some(env),
                            )?;
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
            let closure_data = ClosureData::new(&data.params, &data.body, data.env, None);
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
            let closure_data = ClosureData::new(&data.params, &data.body, data.env, None);
            Ok(Value::GeneratorFunction(None, Gc::new(mc, closure_data)))
        }
        Value::AsyncGeneratorFunction(_, data) => {
            let closure_data = ClosureData::new(&data.params, &data.body, data.env, None);
            Ok(Value::AsyncGeneratorFunction(None, Gc::new(mc, closure_data)))
        }
        Value::Generator(generator) => Ok(Value::Generator(*generator)),
        Value::AsyncGenerator(generator) => Ok(Value::AsyncGenerator(*generator)),
        Value::Proxy(proxy) => Ok(Value::Proxy(*proxy)),
        Value::ArrayBuffer(array_buffer) => Ok(Value::ArrayBuffer(*array_buffer)),
        Value::DataView(data_view) => Ok(Value::DataView(*data_view)),
        Value::TypedArray(typed_array) => Ok(Value::TypedArray(*typed_array)),
        Value::Uninitialized => Err(EvalError::Js(raise_type_error!("Cannot convert uninitialized to object"))),
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
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    match func_name {
        "Object.prototype.hasOwnProperty" => {
            if args.len() != 1 {
                return Err(EvalError::Js(raise_eval_error!("hasOwnProperty requires one argument")));
            }
            let key_val = args[0].clone();
            let exists = crate::core::has_own_property_value(object, &key_val);
            Ok(Some(Value::Boolean(exists)))
        }
        "Object.prototype.isPrototypeOf" => {
            if args.len() != 1 {
                return Err(EvalError::Js(raise_eval_error!("isPrototypeOf requires one argument")));
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
                return Err(EvalError::Js(raise_eval_error!("propertyIsEnumerable requires one argument")));
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
