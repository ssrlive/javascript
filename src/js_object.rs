use crate::core::{
    Expr, JSObjectData, JSObjectDataPtr, PropertyKey, Value, evaluate_expr, get_well_known_symbol_rc, obj_get_value, obj_set_value,
};
use crate::error::JSError;
use crate::js_array::{get_array_length, is_array, set_array_length};
use crate::unicode::utf8_to_utf16;
use std::cell::RefCell;
use std::rc::Rc;

pub fn handle_object_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "keys" => {
            if args.is_empty() {
                return Err(make_type_error!("Object.keys requires at least one argument"));
            }
            if args.len() > 1 {
                return Err(make_type_error!("Object.keys accepts only one argument"));
            }
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let mut keys = Vec::new();
                    for key in obj.borrow().keys() {
                        if let PropertyKey::String(s) = key
                            && s != "length"
                        {
                            // Skip array length property
                            keys.push(Value::String(utf8_to_utf16(s)));
                        }
                    }
                    // Create a simple array-like object for keys
                    let result_obj = Rc::new(RefCell::new(JSObjectData::new()));
                    for (i, key) in keys.into_iter().enumerate() {
                        obj_set_value(&result_obj, &i.to_string().into(), key)?;
                    }
                    let len = result_obj.borrow().properties.len();
                    set_array_length(&result_obj, len)?;
                    Ok(Value::Object(result_obj))
                }
                Value::Undefined => Err(make_type_error!("Object.keys called on undefined")),
                _ => {
                    // For primitive values, return empty array (like in JS)
                    let result_obj = Rc::new(RefCell::new(JSObjectData::new()));
                    set_array_length(&result_obj, 0)?;
                    Ok(Value::Object(result_obj))
                }
            }
        }
        "values" => {
            if args.is_empty() {
                return Err(make_type_error!("Object.values requires at least one argument"));
            }
            if args.len() > 1 {
                return Err(make_type_error!("Object.values accepts only one argument"));
            }
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let mut values = Vec::new();
                    for (key, value) in obj.borrow().properties.iter() {
                        if let PropertyKey::String(s) = key
                            && s != "length"
                        {
                            // Skip array length property and only include string keys
                            values.push(value.borrow().clone());
                        }
                    }
                    // Create a simple array-like object for values
                    let result_obj = Rc::new(RefCell::new(JSObjectData::new()));
                    for (i, value) in values.into_iter().enumerate() {
                        obj_set_value(&result_obj, &i.to_string().into(), value)?;
                    }
                    let len = result_obj.borrow().properties.len();
                    set_array_length(&result_obj, len)?;
                    Ok(Value::Object(result_obj))
                }
                Value::Undefined => Err(make_type_error!("Object.values called on undefined")),
                _ => {
                    // For primitive values, return empty array (like in JS)
                    let result_obj = Rc::new(RefCell::new(JSObjectData::new()));
                    set_array_length(&result_obj, 0)?;
                    Ok(Value::Object(result_obj))
                }
            }
        }
        "assign" => {
            if args.is_empty() {
                return Err(make_type_error!("Object.assign requires at least one argument"));
            }
            let target_val = evaluate_expr(env, &args[0])?;
            let target_obj = match target_val {
                Value::Object(obj) => obj,
                _ => {
                    return Err(make_type_error!("Object.assign target must be an object"));
                }
            };

            // Copy properties from source objects to target
            for arg in &args[1..] {
                let source_val = evaluate_expr(env, arg)?;
                if let Value::Object(source_obj) = source_val {
                    for (key, value) in source_obj.borrow().properties.iter() {
                        if *key != "length".into() && *key != "__proto__".into() {
                            // Skip array length and prototype properties
                            obj_set_value(&target_obj, key, value.borrow().clone())?;
                        }
                    }
                }
                // If source is not an object, skip it (like in JS)
            }

            Ok(Value::Object(target_obj))
        }
        "create" => {
            if args.is_empty() {
                return Err(make_type_error!("Object.create requires at least one argument"));
            }
            let proto_val = evaluate_expr(env, &args[0])?;
            let proto_obj = match proto_val {
                Value::Object(obj) => Some(obj),
                Value::Undefined => None,
                _ => {
                    return Err(make_type_error!("Object.create prototype must be an object or undefined"));
                }
            };

            // Create new object
            let new_obj = Rc::new(RefCell::new(JSObjectData::new()));

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
                            let value = if let Some(val) = obj_get_value(desc_obj, &"value".into())? {
                                val.borrow().clone()
                            } else {
                                Value::Undefined
                            };

                            // For now, we just set the value directly
                            // Full property descriptor support would require more complex implementation
                            obj_set_value(&new_obj, key, value)?;
                        }
                    }
                }
            }

            Ok(Value::Object(new_obj))
        }
        "getOwnPropertySymbols" => {
            if args.len() != 1 {
                return Err(make_type_error!("Object.getOwnPropertySymbols requires exactly one argument"));
            }
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let result_obj = Rc::new(RefCell::new(JSObjectData::new()));
                    let mut idx = 0;
                    for (key, _value) in obj.borrow().properties.iter() {
                        if let PropertyKey::Symbol(sym) = key
                            && let Value::Symbol(symbol_data) = &*sym.borrow()
                        {
                            // push symbol primitive into result array
                            obj_set_value(&result_obj, &idx.to_string().into(), Value::Symbol(symbol_data.clone()))?;
                            idx += 1;
                        }
                    }
                    set_array_length(&result_obj, idx)?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(make_type_error!("Object.getOwnPropertySymbols called on non-object")),
            }
        }
        "getOwnPropertyDescriptors" => {
            if args.len() != 1 {
                return Err(make_type_error!("Object.getOwnPropertyDescriptors requires exactly one argument"));
            }
            let obj_val = evaluate_expr(env, &args[0])?;
            match obj_val {
                Value::Object(obj) => {
                    let result_obj = Rc::new(RefCell::new(JSObjectData::new()));

                    for (key, val_rc) in obj.borrow().properties.iter() {
                        // iterate own properties
                        // Build descriptor object
                        let desc_obj = Rc::new(RefCell::new(JSObjectData::new()));

                        match &*val_rc.borrow() {
                            Value::Property { value, getter, setter } => {
                                // Data value
                                if let Some(v) = value {
                                    obj_set_value(&desc_obj, &"value".into(), v.borrow().clone())?;
                                    // writable: treat as true by default for data properties
                                    obj_set_value(&desc_obj, &"writable".into(), Value::Boolean(true))?;
                                }
                                // Accessor
                                if let Some((gbody, genv)) = getter {
                                    // expose getter as function (Closure) on descriptor
                                    obj_set_value(&desc_obj, &"get".into(), Value::Closure(Vec::new(), gbody.clone(), genv.clone()))?;
                                }
                                if let Some((sparams, sbody, senv)) = setter {
                                    // expose setter as function (Closure) on descriptor
                                    obj_set_value(
                                        &desc_obj,
                                        &"set".into(),
                                        Value::Closure(sparams.clone(), sbody.clone(), senv.clone()),
                                    )?;
                                }
                                // default flags
                                obj_set_value(&desc_obj, &"enumerable".into(), Value::Boolean(true))?;
                                obj_set_value(&desc_obj, &"configurable".into(), Value::Boolean(true))?;
                            }
                            other => {
                                // plain value stored directly
                                obj_set_value(&desc_obj, &"value".into(), other.clone())?;
                                obj_set_value(&desc_obj, &"writable".into(), Value::Boolean(true))?;
                                obj_set_value(&desc_obj, &"enumerable".into(), Value::Boolean(true))?;
                                obj_set_value(&desc_obj, &"configurable".into(), Value::Boolean(true))?;
                            }
                        }

                        // debug dump
                        log::trace!("descriptor for key={} created: {:?}", key, desc_obj.borrow().properties);
                        // Put descriptor onto result using the original key (string or symbol)
                        match key {
                            PropertyKey::String(s) => {
                                obj_set_value(&result_obj, &s.clone().into(), Value::Object(desc_obj.clone()))?;
                            }
                            PropertyKey::Symbol(sym_rc) => {
                                // Push symbol-keyed property on returned object with the same symbol key
                                let property_key = PropertyKey::Symbol(sym_rc.clone());
                                obj_set_value(&result_obj, &property_key, Value::Object(desc_obj.clone()))?;
                            }
                        }
                    }

                    Ok(Value::Object(result_obj))
                }
                _ => Err(make_type_error!("Object.getOwnPropertyDescriptors called on non-object")),
            }
        }
        _ => Err(eval_error_here!(format!("Object.{method} is not implemented"))),
    }
}

pub(crate) fn handle_to_string_method(obj_val: &Value, args: &[Expr]) -> Result<Value, JSError> {
    if !args.is_empty() {
        return Err(make_type_error!(format!(
            "{}.toString() takes no arguments, but {} were provided",
            match obj_val {
                Value::Number(_) => "Number",
                Value::BigInt(_) => "BigInt",
                Value::String(_) => "String",
                Value::Boolean(_) => "Boolean",
                Value::Object(_) => "Object",
                Value::Function(_) => "Function",
                Value::Closure(_, _, _) => "Function",
                Value::Undefined => "undefined",
                Value::ClassDefinition(_) => "Class",
                Value::Getter(_, _) => "Getter",
                Value::Setter(_, _, _) => "Setter",
                Value::Property { .. } => "Property",
                Value::Promise(_) => "Promise",
                Value::Symbol(_) => "Symbol",
            },
            args.len()
        )));
    }
    match obj_val {
        Value::Number(n) => Ok(Value::String(utf8_to_utf16(&n.to_string()))),
        Value::BigInt(s) => Ok(Value::String(utf8_to_utf16(s))),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
        Value::Undefined => Err(make_type_error!("Cannot convert undefined to object")),
        Value::Object(obj_map) => {
            // Check if this is a wrapped primitive object
            if let Some(wrapped_val) = obj_get_value(obj_map, &"__value__".into())? {
                match &*wrapped_val.borrow() {
                    Value::Number(n) => return Ok(Value::String(utf8_to_utf16(&n.to_string()))),
                    Value::BigInt(s) => return Ok(Value::String(utf8_to_utf16(s))),
                    Value::Boolean(b) => return Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                    Value::String(s) => return Ok(Value::String(s.clone())),
                    _ => {}
                }
            }

            // If this object looks like a Date (has __timestamp), call Date.toString()
            if obj_map.borrow().contains_key(&"__timestamp".into()) {
                return crate::js_date::handle_date_method(obj_map, "toString", args);
            }

            // If this object looks like an array, join elements with comma (Array.prototype.toString overrides Object.prototype)
            if is_array(obj_map) {
                let current_len = get_array_length(obj_map).unwrap_or(0);
                let mut parts = Vec::new();
                for i in 0..current_len {
                    if let Some(val_rc) = obj_get_value(obj_map, &i.to_string().into())? {
                        match &*val_rc.borrow() {
                            Value::String(s) => parts.push(String::from_utf16_lossy(s)),
                            Value::Number(n) => parts.push(n.to_string()),
                            Value::Boolean(b) => parts.push(b.to_string()),
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
                if let Some(tag_val_rc) = obj_get_value(obj_map, &key)?
                    && let Value::String(s) = &*tag_val_rc.borrow()
                {
                    return Ok(Value::String(utf8_to_utf16(&format!("[object {}]", String::from_utf16_lossy(s)))));
                }
            }

            // Default object tag
            Ok(Value::String(utf8_to_utf16("[object Object]")))
        }
        Value::Function(name) => Ok(Value::String(utf8_to_utf16(&format!("[Function: {}]", name)))),
        Value::Closure(_, _, _) => Ok(Value::String(utf8_to_utf16("[Function]"))),
        Value::ClassDefinition(_) => Ok(Value::String(utf8_to_utf16("[Class]"))),
        Value::Getter(_, _) => Ok(Value::String(utf8_to_utf16("[Getter]"))),
        Value::Setter(_, _, _) => Ok(Value::String(utf8_to_utf16("[Setter]"))),
        Value::Property { .. } => Ok(Value::String(utf8_to_utf16("[Property]"))),
        Value::Promise(_) => Ok(Value::String(utf8_to_utf16("[object Promise]"))),
        Value::Symbol(symbol_data) => {
            let desc_str = symbol_data.description.as_deref().unwrap_or("");
            Ok(Value::String(utf8_to_utf16(&format!("Symbol({})", desc_str))))
        }
    }
}

pub(crate) fn handle_value_of_method(obj_val: &Value, args: &[Expr]) -> Result<Value, JSError> {
    if !args.is_empty() {
        return Err(make_type_error!(format!(
            "{}.valueOf() takes no arguments, but {} were provided",
            match obj_val {
                Value::Number(_) => "Number",
                Value::String(_) => "String",
                Value::Boolean(_) => "Boolean",
                Value::Object(_) => "Object",
                Value::Function(_) => "Function",
                Value::Closure(_, _, _) => "Function",
                Value::Undefined => "undefined",
                Value::ClassDefinition(_) => "Class",
                &Value::Getter(_, _) => "Getter",
                &Value::Setter(_, _, _) => "Setter",
                &Value::Property { .. } => "Property",
                &Value::Promise(_) => "Promise",
                Value::BigInt(_) => "BigInt",
                Value::Symbol(_) => "Symbol",
            },
            args.len()
        )));
    }
    match obj_val {
        Value::Number(n) => Ok(Value::Number(*n)),
        Value::BigInt(s) => Ok(Value::BigInt(s.clone())),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::Boolean(*b)),
        Value::Undefined => Err(make_type_error!("Cannot convert undefined to object")),
        Value::Object(obj_map) => {
            // Check if this is a wrapped primitive object
            if let Some(wrapped_val) = obj_get_value(obj_map, &"__value__".into())? {
                return Ok(wrapped_val.borrow().clone());
            }
            // If this object looks like a Date (has __timestamp), call Date.valueOf()
            if obj_map.borrow().contains_key(&"__timestamp".into()) {
                return crate::js_date::handle_date_method(obj_map, "valueOf", args);
            }
            // For regular objects, return the object itself
            Ok(Value::Object(obj_map.clone()))
        }
        Value::Function(name) => Ok(Value::Function(name.clone())),
        Value::Closure(params, body, env) => Ok(Value::Closure(params.clone(), body.clone(), env.clone())),
        Value::ClassDefinition(class_def) => Ok(Value::ClassDefinition(class_def.clone())),
        Value::Getter(body, env) => Ok(Value::Getter(body.clone(), env.clone())),
        Value::Setter(param, body, env) => Ok(Value::Setter(param.clone(), body.clone(), env.clone())),
        Value::Property { value, getter, setter } => Ok(Value::Property {
            value: value.clone(),
            getter: getter.clone(),
            setter: setter.clone(),
        }),
        Value::Promise(promise) => Ok(Value::Promise(promise.clone())),
        Value::Symbol(symbol_data) => Ok(Value::Symbol(symbol_data.clone())),
    }
}
