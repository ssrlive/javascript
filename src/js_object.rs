#![allow(clippy::collapsible_if, clippy::collapsible_match)]

use crate::core::{
    Expr, JSObjectDataPtr, PropertyKey, Statement, Value, evaluate_expr, get_well_known_symbol_rc, new_js_object_data, obj_get_key_value,
    obj_set_key_value, value_to_string,
};
use crate::error::JSError;
use crate::js_array::{get_array_length, is_array, set_array_length};
use crate::js_date::is_date_object;
use crate::unicode::utf8_to_utf16;

pub fn handle_object_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "keys" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.keys requires at least one argument"));
            }
            if args.len() > 1 {
                return Err(raise_type_error!("Object.keys accepts only one argument"));
            }
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let mut keys = Vec::new();
                    for key in obj.borrow().keys() {
                        if !obj.borrow().is_enumerable(key) {
                            continue;
                        }
                        if let PropertyKey::String(s) = key
                            && s != "length"
                        {
                            // Skip array length property
                            keys.push(Value::String(utf8_to_utf16(s)));
                        }
                    }
                    // Create a simple array-like object for keys
                    let result_obj = new_js_object_data();
                    for (i, key) in keys.into_iter().enumerate() {
                        obj_set_key_value(&result_obj, &i.to_string().into(), key)?;
                    }
                    let len = result_obj.borrow().properties.len();
                    set_array_length(&result_obj, len)?;
                    Ok(Value::Object(result_obj))
                }
                Value::Undefined => Err(raise_type_error!("Object.keys called on undefined")),
                _ => {
                    // For primitive values, return empty array (like in JS)
                    let result_obj = new_js_object_data();
                    set_array_length(&result_obj, 0)?;
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
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let mut values = Vec::new();
                    for (key, value) in obj.borrow().properties.iter() {
                        if !obj.borrow().is_enumerable(key) {
                            continue;
                        }
                        if let PropertyKey::String(s) = key
                            && s != "length"
                        {
                            // Skip array length property and only include string keys
                            values.push(value.borrow().clone());
                        }
                    }
                    // Create a simple array-like object for values
                    let result_obj = new_js_object_data();
                    for (i, value) in values.into_iter().enumerate() {
                        obj_set_key_value(&result_obj, &i.to_string().into(), value)?;
                    }
                    let len = result_obj.borrow().properties.len();
                    set_array_length(&result_obj, len)?;
                    Ok(Value::Object(result_obj))
                }
                Value::Undefined => Err(raise_type_error!("Object.values called on undefined")),
                _ => {
                    // For primitive values, return empty array (like in JS)
                    let result_obj = new_js_object_data();
                    set_array_length(&result_obj, 0)?;
                    Ok(Value::Object(result_obj))
                }
            }
        }
        "hasOwn" => {
            if args.len() != 2 {
                return Err(raise_type_error!("Object.hasOwn requires exactly two arguments"));
            }
            let obj_val = evaluate_expr(env, &args[0])?;
            let prop_val = evaluate_expr(env, &args[1])?;

            if matches!(obj_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot convert undefined or null to object"));
            }

            let key = match prop_val {
                Value::Symbol(_) => PropertyKey::Symbol(std::rc::Rc::new(std::cell::RefCell::new(prop_val))),
                _ => PropertyKey::String(value_to_string(&prop_val)),
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
        "create" => {
            if args.is_empty() {
                return Err(raise_type_error!("Object.create requires at least one argument"));
            }
            let proto_val = evaluate_expr(env, &args[0])?;
            let proto_obj = match proto_val {
                Value::Object(obj) => Some(obj),
                Value::Undefined | Value::Null => None,
                _ => {
                    return Err(raise_type_error!("Object.create prototype must be an object, null, or undefined"));
                }
            };

            // Create new object
            let new_obj = new_js_object_data();

            // Set prototype
            if let Some(proto) = proto_obj {
                new_obj.borrow_mut().prototype = Some(proto);
            }

            // If properties descriptor is provided, add properties
            if args.len() > 1 {
                let props_val = evaluate_expr(env, &args[1])?;
                if let Value::Object(props_obj) = props_val {
                    for (key, desc_val) in props_obj.borrow().properties.iter() {
                        if let Value::Object(desc_obj) = &*desc_val.borrow() {
                            // Handle property descriptor
                            let value = if let Some(val) = obj_get_key_value(desc_obj, &"value".into())? {
                                val.borrow().clone()
                            } else {
                                Value::Undefined
                            };

                            // For now, we just set the value directly
                            // Full property descriptor support would require more complex implementation
                            obj_set_key_value(&new_obj, key, value)?;
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
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let result_obj = new_js_object_data();
                    let mut idx = 0;
                    for (key, _value) in obj.borrow().properties.iter() {
                        if let PropertyKey::Symbol(sym) = key
                            && let Value::Symbol(symbol_data) = &*sym.borrow()
                        {
                            // push symbol primitive into result array
                            obj_set_key_value(&result_obj, &idx.to_string().into(), Value::Symbol(symbol_data.clone()))?;
                            idx += 1;
                        }
                    }
                    set_array_length(&result_obj, idx)?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Object.getOwnPropertySymbols called on non-object")),
            }
        }
        "getOwnPropertyNames" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getOwnPropertyNames requires exactly one argument"));
            }
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let result_obj = new_js_object_data();
                    let mut idx = 0;
                    for (key, _value) in obj.borrow().properties.iter() {
                        if let PropertyKey::String(s) = key
                            && s != "length"
                        {
                            obj_set_key_value(&result_obj, &idx.to_string().into(), Value::String(utf8_to_utf16(s)))?;
                            idx += 1;
                        }
                    }
                    set_array_length(&result_obj, idx)?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Object.getOwnPropertyNames called on non-object")),
            }
        }
        "getOwnPropertyDescriptors" => {
            if args.len() != 1 {
                return Err(raise_type_error!("Object.getOwnPropertyDescriptors requires exactly one argument"));
            }
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let result_obj = new_js_object_data();

                    for (key, val_rc) in obj.borrow().properties.iter() {
                        // iterate own properties
                        // Build descriptor object
                        if !obj.borrow().is_enumerable(key) {
                            // Mark the descriptor's enumerable flag appropriately below
                        }
                        let desc_obj = new_js_object_data();

                        match &*val_rc.borrow() {
                            Value::Property { value, getter, setter } => {
                                // Data value
                                if let Some(v) = value {
                                    obj_set_key_value(&desc_obj, &"value".into(), v.borrow().clone())?;
                                    // writable: treat as true by default for data properties
                                    obj_set_key_value(&desc_obj, &"writable".into(), Value::Boolean(true))?;
                                }
                                // Accessor
                                if let Some((gbody, genv, _)) = getter {
                                    // expose getter as function (Closure) on descriptor
                                    obj_set_key_value(
                                        &desc_obj,
                                        &"get".into(),
                                        Value::Closure(Vec::new(), gbody.clone(), genv.clone(), None),
                                    )?;
                                }
                                if let Some((sparams, sbody, senv, _)) = setter {
                                    // expose setter as function (Closure) on descriptor
                                    obj_set_key_value(
                                        &desc_obj,
                                        &"set".into(),
                                        Value::Closure(sparams.clone(), sbody.clone(), senv.clone(), None),
                                    )?;
                                }
                                // flags: enumerable depends on object's non-enumerable set
                                let enum_flag = Value::Boolean(obj.borrow().is_enumerable(key));
                                obj_set_key_value(&desc_obj, &"enumerable".into(), enum_flag)?;
                                let config_flag = Value::Boolean(obj.borrow().is_configurable(key));
                                obj_set_key_value(&desc_obj, &"configurable".into(), config_flag)?;
                            }
                            other => {
                                // plain value stored directly
                                obj_set_key_value(&desc_obj, &"value".into(), other.clone())?;
                                let writable_flag = Value::Boolean(obj.borrow().is_writable(key));
                                obj_set_key_value(&desc_obj, &"writable".into(), writable_flag)?;
                                let enum_flag = Value::Boolean(obj.borrow().is_enumerable(key));
                                obj_set_key_value(&desc_obj, &"enumerable".into(), enum_flag)?;
                                let config_flag = Value::Boolean(obj.borrow().is_configurable(key));
                                obj_set_key_value(&desc_obj, &"configurable".into(), config_flag)?;
                            }
                        }

                        // debug dump
                        log::trace!("descriptor for key={} created: {:?}", key, desc_obj.borrow().properties);
                        // Put descriptor onto result using the original key (string or symbol)
                        match key {
                            PropertyKey::String(s) => {
                                obj_set_key_value(&result_obj, &s.clone().into(), Value::Object(desc_obj.clone()))?;
                            }
                            PropertyKey::Symbol(sym_rc) => {
                                // Push symbol-keyed property on returned object with the same symbol key
                                let property_key = PropertyKey::Symbol(sym_rc.clone());
                                obj_set_key_value(&result_obj, &property_key, Value::Object(desc_obj.clone()))?;
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
            let target_val = evaluate_expr(env, &args[0])?;
            let target_obj = match target_val {
                Value::Object(o) => o,
                Value::Undefined => return Err(raise_type_error!("Object.assign target cannot be undefined or null")),
                Value::Number(n) => {
                    let obj = new_js_object_data();
                    obj_set_key_value(&obj, &"valueOf".into(), Value::Function("Number_valueOf".to_string()))?;
                    obj_set_key_value(&obj, &"toString".into(), Value::Function("Number_toString".to_string()))?;
                    obj_set_key_value(&obj, &"__value__".into(), Value::Number(n))?;
                    // Set prototype to Number.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(&obj, env, "Number");
                    obj
                }
                Value::Boolean(b) => {
                    let obj = new_js_object_data();
                    obj_set_key_value(&obj, &"valueOf".into(), Value::Function("Boolean_valueOf".to_string()))?;
                    obj_set_key_value(&obj, &"toString".into(), Value::Function("Boolean_toString".to_string()))?;
                    obj_set_key_value(&obj, &"__value__".into(), Value::Boolean(b))?;
                    // Set prototype to Boolean.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(&obj, env, "Boolean");
                    obj
                }
                Value::String(s) => {
                    let obj = new_js_object_data();
                    obj_set_key_value(&obj, &"valueOf".into(), Value::Function("String_valueOf".to_string()))?;
                    obj_set_key_value(&obj, &"toString".into(), Value::Function("String_toString".to_string()))?;
                    obj_set_key_value(&obj, &"length".into(), Value::Number(s.len() as f64))?;
                    obj_set_key_value(&obj, &"__value__".into(), Value::String(s.clone()))?;
                    // Set prototype to String.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(&obj, env, "String");
                    obj
                }
                Value::BigInt(h) => {
                    let obj = new_js_object_data();
                    obj_set_key_value(&obj, &"valueOf".into(), Value::Function("BigInt_valueOf".to_string()))?;
                    obj_set_key_value(&obj, &"toString".into(), Value::Function("BigInt_toString".to_string()))?;
                    obj_set_key_value(&obj, &"__value__".into(), Value::BigInt(h.clone()))?;
                    // Set prototype to BigInt.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(&obj, env, "BigInt");
                    obj
                }
                Value::Symbol(sd) => {
                    let obj = new_js_object_data();
                    obj_set_key_value(&obj, &"valueOf".into(), Value::Function("Symbol_valueOf".to_string()))?;
                    obj_set_key_value(&obj, &"toString".into(), Value::Function("Symbol_toString".to_string()))?;
                    obj_set_key_value(&obj, &"__value__".into(), Value::Symbol(sd.clone()))?;
                    // Set prototype to Symbol.prototype if available
                    let _ = crate::core::set_internal_prototype_from_constructor(&obj, env, "Symbol");
                    obj
                }
                // For other types (functions, closures, etc.), create a plain object
                _ => new_js_object_data(),
            };

            // Iterate sources
            for src_expr in args.iter().skip(1) {
                let src_val = evaluate_expr(env, src_expr)?;
                if let Value::Object(source_obj) = src_val {
                    for (key, _val_rc) in source_obj.borrow().properties.iter() {
                        if *key != "length".into() && *key != "__proto__".into() {
                            // Only copy string-keyed own properties
                            if let PropertyKey::String(_) = key
                                && let Some(v_rc) = obj_get_key_value(&source_obj, key)?
                            {
                                obj_set_key_value(&target_obj, key, v_rc.borrow().clone())?;
                            }
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
            let target_val = evaluate_expr(env, &args[0])?;
            let target_obj = match target_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Object.defineProperty called on non-object")),
            };

            let prop_val = evaluate_expr(env, &args[1])?;
            // Determine property key (support strings & numbers for now)
            let prop_key = match prop_val {
                Value::String(s) => PropertyKey::String(String::from_utf16_lossy(&s)),
                Value::Number(n) => PropertyKey::String(n.to_string()),
                _ => return Err(raise_type_error!("Unsupported property key type in Object.defineProperty")),
            };

            let desc_val = evaluate_expr(env, &args[2])?;
            let desc_obj = match desc_val {
                Value::Object(o) => o,
                _ => return Err(raise_type_error!("Property descriptor must be an object")),
            };

            // Extract descriptor fields
            let value_rc_opt = obj_get_key_value(&desc_obj, &"value".into())?;

            // If the property exists and is non-configurable on the target, apply ECMAScript-compatible checks
            if let Some(existing_rc) = obj_get_key_value(&target_obj, &prop_key)? {
                if !target_obj.borrow().is_configurable(&prop_key) {
                    // If descriptor explicitly sets configurable true -> throw
                    if let Some(cfg_rc) = obj_get_key_value(&desc_obj, &"configurable".into())? {
                        if let Value::Boolean(true) = &*cfg_rc.borrow() {
                            return Err(raise_type_error!("Cannot make non-configurable property configurable"));
                        }
                    }

                    // If descriptor explicitly sets enumerable and it's different -> throw
                    if let Some(enum_rc) = obj_get_key_value(&desc_obj, &"enumerable".into())? {
                        if let Value::Boolean(new_enum) = &*enum_rc.borrow() {
                            let existing_enum = target_obj.borrow().is_enumerable(&prop_key);
                            if *new_enum != existing_enum {
                                return Err(raise_type_error!("Cannot change enumerability of non-configurable property"));
                            }
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
                        if obj_get_key_value(&desc_obj, &"get".into())?.is_some() || obj_get_key_value(&desc_obj, &"set".into())?.is_some()
                        {
                            return Err(raise_type_error!("Cannot convert non-configurable data property to an accessor"));
                        }

                        // If writable is being set from false -> true, disallow
                        if let Some(wrc) = obj_get_key_value(&desc_obj, &"writable".into())? {
                            if let Value::Boolean(new_writable) = &*wrc.borrow() {
                                if *new_writable && !target_obj.borrow().is_writable(&prop_key) {
                                    return Err(raise_type_error!("Cannot make non-writable property writable"));
                                }
                            }
                        }

                        // If attempting to change value while not writable and values differ -> throw
                        if let Some(new_val_rc) = value_rc_opt.as_ref() {
                            if !target_obj.borrow().is_writable(&prop_key) {
                                // get existing value for comparison
                                let existing_val = match &*existing_rc.borrow() {
                                    Value::Property { value: Some(v), .. } => v.borrow().clone(),
                                    other => other.clone(),
                                };
                                if !crate::core::values_equal(&existing_val, &new_val_rc.borrow().clone()) {
                                    return Err(raise_type_error!("Cannot change value of non-writable, non-configurable property"));
                                }
                            }
                        }
                    } else {
                        // existing is accessor
                        // Disallow converting to data property
                        if value_rc_opt.is_some() || obj_get_key_value(&desc_obj, &"writable".into())?.is_some() {
                            return Err(raise_type_error!("Cannot convert non-configurable accessor to a data property"));
                        }

                        // Disallow changing getter/setter functions on non-configurable accessor
                        if obj_get_key_value(&desc_obj, &"get".into())?.is_some() || obj_get_key_value(&desc_obj, &"set".into())?.is_some()
                        {
                            return Err(raise_type_error!(
                                "Cannot change getter/setter of non-configurable accessor property"
                            ));
                        }
                    }
                }
            }

            let mut getter_opt: Option<(Vec<crate::core::Statement>, JSObjectDataPtr, Option<JSObjectDataPtr>)> = None;
            if let Some(get_rc) = obj_get_key_value(&desc_obj, &"get".into())? {
                match &*get_rc.borrow() {
                    Value::Closure(_params, body, genv, _) => {
                        getter_opt = Some((body.clone(), genv.clone(), None));
                    }
                    Value::Getter(body, genv, _) => {
                        getter_opt = Some((body.clone(), genv.clone(), None));
                    }
                    _ => {}
                }
            }

            #[allow(clippy::type_complexity)]
            let mut setter_opt: Option<(
                Vec<(String, Option<Box<Expr>>)>,
                Vec<Statement>,
                JSObjectDataPtr,
                Option<JSObjectDataPtr>,
            )> = None;
            if let Some(set_rc) = obj_get_key_value(&desc_obj, &"set".into())? {
                match &*set_rc.borrow() {
                    Value::Closure(params, body, senv, _) => {
                        setter_opt = Some((params.clone(), body.clone(), senv.clone(), None));
                    }
                    Value::Setter(params, body, senv, _) => {
                        setter_opt = Some((params.clone(), body.clone(), senv.clone(), None));
                    }
                    _ => {}
                }
            }

            // Create property descriptor value
            let prop_descriptor = Value::Property {
                value: value_rc_opt.clone(),
                getter: getter_opt,
                setter: setter_opt,
            };

            // Install property on target object
            obj_set_key_value(&target_obj, &prop_key, prop_descriptor)?;
            Ok(Value::Object(target_obj))
        }
        _ => Err(raise_eval_error!(format!("Object.{method} is not implemented"))),
    }
}

pub(crate) fn handle_to_string_method(obj_val: &Value, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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
            },
            args.len()
        )));
    }
    match obj_val {
        Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
        Value::BigInt(h) => Ok(Value::String(utf8_to_utf16(&h.to_string()))),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
        Value::Undefined => Ok(Value::String(utf8_to_utf16("[object Undefined]"))),
        Value::Null => Ok(Value::String(utf8_to_utf16("[object Null]"))),
        Value::Object(obj_map) => {
            // Check if this is a wrapped primitive object
            if let Some(wrapped_val) = obj_get_key_value(obj_map, &"__value__".into())? {
                match &*wrapped_val.borrow() {
                    Value::Number(n) => return Ok(Value::String(utf8_to_utf16(&n.to_string()))),
                    Value::BigInt(h) => return Ok(Value::String(utf8_to_utf16(&h.to_string()))),
                    Value::Boolean(b) => return Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                    Value::String(s) => return Ok(Value::String(s.clone())),
                    _ => {}
                }
            }

            // If this object looks like a Date (has __timestamp), call Date.toString()
            if is_date_object(obj_map) {
                return crate::js_date::handle_date_method(obj_map, "toString", args, env);
            }

            // If this object looks like an array, join elements with comma (Array.prototype.toString overrides Object.prototype)
            if is_array(obj_map) {
                let current_len = get_array_length(obj_map).unwrap_or(0);
                let mut parts = Vec::new();
                for i in 0..current_len {
                    if let Some(val_rc) = obj_get_key_value(obj_map, &i.to_string().into())? {
                        match &*val_rc.borrow() {
                            Value::Undefined | Value::Null => parts.push("".to_string()), // push empty string for null and undefined
                            Value::String(s) => parts.push(String::from_utf16_lossy(s)),
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
            if let Some(tag_sym_rc) = get_well_known_symbol_rc("toStringTag") {
                let key = PropertyKey::Symbol(tag_sym_rc.clone());
                if let Some(tag_val_rc) = obj_get_key_value(obj_map, &key)?
                    && let Value::String(s) = &*tag_val_rc.borrow()
                {
                    return Ok(Value::String(utf8_to_utf16(&format!("[object {}]", String::from_utf16_lossy(s)))));
                }
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

pub(crate) fn handle_error_to_string_method(obj_val: &Value, args: &[Expr]) -> Result<Value, JSError> {
    if !args.is_empty() {
        return Err(raise_type_error!("Error.prototype.toString takes no arguments"));
    }

    // Expect an object receiver
    if let Value::Object(obj_map) = obj_val {
        // name default to "Error"
        let name = if let Some(n_rc) = obj_get_key_value(obj_map, &"name".into())? {
            if let Value::String(s) = &*n_rc.borrow() {
                String::from_utf16_lossy(s)
            } else {
                "Error".to_string()
            }
        } else {
            "Error".to_string()
        };

        // message default to empty
        let message = if let Some(m_rc) = obj_get_key_value(obj_map, &"message".into())? {
            if let Value::String(s) = &*m_rc.borrow() {
                String::from_utf16_lossy(s)
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

pub(crate) fn handle_value_of_method(obj_val: &Value, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
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
        Value::Object(obj_map) => {
            // Check if this is a wrapped primitive object
            if let Some(wrapped_val) = obj_get_key_value(obj_map, &"__value__".into())? {
                return Ok(wrapped_val.borrow().clone());
            }
            // If object defines a user valueOf function, call it and use its
            // primitive result if it returns a primitive.
            if let Some(method_rc) = obj_get_key_value(obj_map, &"valueOf".into())? {
                let method_val = method_rc.borrow().clone();
                match method_val {
                    Value::Closure(_params, body, captured_env, _) | Value::AsyncClosure(_params, body, captured_env, _) => {
                        let func_env = new_js_object_data();
                        func_env.borrow_mut().prototype = Some(captured_env.clone());
                        func_env.borrow_mut().is_function_scope = true;
                        // bind `this` to the object
                        crate::core::env_set(&func_env, "this", Value::Object(obj_map.clone()))?;
                        let result = crate::core::evaluate_statements(&func_env, &body)?;
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
                            return Ok(Value::Object(obj_map.clone()));
                        }
                        if func_name == "Object.prototype.toString" {
                            return crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), args, env);
                        }

                        let func_env = new_js_object_data();
                        func_env.borrow_mut().is_function_scope = true;
                        // bind `this` to the object
                        crate::core::env_set(&func_env, "this", Value::Object(obj_map.clone()))?;
                        let res = crate::js_function::handle_global_function(&func_name, &[], &func_env)?;
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
                if let Value::Object(func_obj_map) = &*method_rc.borrow() {
                    if let Some(cl_rc) = obj_get_key_value(func_obj_map, &"__closure__".into())? {
                        match &*cl_rc.borrow() {
                            Value::Closure(_params, body, captured_env, _) | Value::AsyncClosure(_params, body, captured_env, _) => {
                                let func_env = new_js_object_data();
                                func_env.borrow_mut().prototype = Some(captured_env.clone());
                                func_env.borrow_mut().is_function_scope = true;
                                crate::core::env_set(&func_env, "this", Value::Object(obj_map.clone()))?;
                                let result = crate::core::evaluate_statements(&func_env, body)?;
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
            }
            // If this object looks like a Date (has __timestamp), call Date.valueOf()
            if is_date_object(obj_map) {
                return crate::js_date::handle_date_method(obj_map, "valueOf", args, env);
            }
            // For regular objects, return the object itself
            Ok(Value::Object(obj_map.clone()))
        }
        Value::Function(name) => Ok(Value::Function(name.clone())),
        Value::Closure(params, body, env, _) | Value::AsyncClosure(params, body, env, _) => {
            Ok(Value::Closure(params.clone(), body.clone(), env.clone(), None))
        }
        Value::ClassDefinition(class_def) => Ok(Value::ClassDefinition(class_def.clone())),
        Value::Getter(body, env, _) => Ok(Value::Getter(body.clone(), env.clone(), None)),
        Value::Setter(param, body, env, _) => Ok(Value::Setter(param.clone(), body.clone(), env.clone(), None)),
        Value::Property { value, getter, setter } => Ok(Value::Property {
            value: value.clone(),
            getter: getter.clone(),
            setter: setter.clone(),
        }),
        Value::Promise(promise) => Ok(Value::Promise(promise.clone())),
        Value::Symbol(symbol_data) => Ok(Value::Symbol(symbol_data.clone())),
        Value::Map(map) => Ok(Value::Map(map.clone())),
        Value::Set(set) => Ok(Value::Set(set.clone())),
        Value::WeakMap(weakmap) => Ok(Value::WeakMap(weakmap.clone())),
        Value::WeakSet(weakset) => Ok(Value::WeakSet(weakset.clone())),
        Value::GeneratorFunction(_, params, body, env, _) => {
            Ok(Value::GeneratorFunction(None, params.clone(), body.clone(), env.clone(), None))
        }
        Value::Generator(generator) => Ok(Value::Generator(generator.clone())),
        Value::Proxy(proxy) => Ok(Value::Proxy(proxy.clone())),
        Value::ArrayBuffer(array_buffer) => Ok(Value::ArrayBuffer(array_buffer.clone())),
        Value::DataView(data_view) => Ok(Value::DataView(data_view.clone())),
        Value::TypedArray(typed_array) => Ok(Value::TypedArray(typed_array.clone())),
    }
}
