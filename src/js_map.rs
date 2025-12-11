use crate::{
    core::{Expr, JSObjectDataPtr, PropertyKey, Value, evaluate_expr, new_js_object_data, obj_set_value},
    error::JSError,
    raise_eval_error,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::core::JSMap;

/// Handle Map constructor calls
pub(crate) fn handle_map_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let map = Rc::new(RefCell::new(JSMap { entries: Vec::new() }));

    if !args.is_empty() {
        if args.len() == 1 {
            // Map(iterable)
            let iterable = evaluate_expr(env, &args[0])?;
            match iterable {
                Value::Object(obj) => {
                    // Try to iterate over the object
                    // For now, assume it's an array-like or has entries
                    // TODO: Implement proper iteration protocol
                    let mut i = 0;
                    loop {
                        let key = format!("{}", i);
                        if let Some(entry_val) = obj_get_value(&obj, &key.into())? {
                            let entry = entry_val.borrow().clone();
                            if let Value::Object(entry_obj) = entry
                                && let (Some(key_val), Some(value_val)) =
                                    (obj_get_value(&entry_obj, &"0".into())?, obj_get_value(&entry_obj, &"1".into())?)
                            {
                                map.borrow_mut()
                                    .entries
                                    .push((key_val.borrow().clone(), value_val.borrow().clone()));
                            }
                        } else {
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    return Err(raise_eval_error!("Map constructor requires an iterable"));
                }
            }
        } else {
            return Err(raise_eval_error!("Map constructor takes at most one argument"));
        }
    }

    // Create a wrapper object for the Map
    let map_obj = new_js_object_data();
    // Store the actual map data
    map_obj
        .borrow_mut()
        .insert(PropertyKey::String("__map__".to_string()), Rc::new(RefCell::new(Value::Map(map))));

    Ok(Value::Object(map_obj))
}

/// Handle Map instance method calls
pub(crate) fn handle_map_instance_method(
    map: &Rc<RefCell<JSMap>>,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    match method {
        "set" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("Map.prototype.set requires exactly two arguments"));
            }
            let key = evaluate_expr(env, &args[0])?;
            let value = evaluate_expr(env, &args[1])?;

            // Remove existing entry with same key
            map.borrow_mut().entries.retain(|(k, _)| !values_equal(k, &key));
            // Add new entry
            map.borrow_mut().entries.push((key, value));

            Ok(Value::Map(map.clone()))
        }
        "get" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Map.prototype.get requires exactly one argument"));
            }
            let key = evaluate_expr(env, &args[0])?;

            for (k, v) in &map.borrow().entries {
                if values_equal(k, &key) {
                    return Ok(v.clone());
                }
            }
            Ok(Value::Undefined)
        }
        "has" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Map.prototype.has requires exactly one argument"));
            }
            let key = evaluate_expr(env, &args[0])?;

            let has_key = map.borrow().entries.iter().any(|(k, _)| values_equal(k, &key));
            Ok(Value::Boolean(has_key))
        }
        "delete" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Map.prototype.delete requires exactly one argument"));
            }
            let key = evaluate_expr(env, &args[0])?;

            let initial_len = map.borrow().entries.len();
            map.borrow_mut().entries.retain(|(k, _)| !values_equal(k, &key));
            let deleted = map.borrow().entries.len() < initial_len;

            Ok(Value::Boolean(deleted))
        }
        "clear" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Map.prototype.clear takes no arguments"));
            }
            map.borrow_mut().entries.clear();
            Ok(Value::Undefined)
        }
        "size" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Map.prototype.size is a getter"));
            }
            Ok(Value::Number(map.borrow().entries.len() as f64))
        }
        "keys" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Map.prototype.keys takes no arguments"));
            }
            // Create an array of keys
            let keys_array = new_js_object_data();
            for (i, (key, _)) in map.borrow().entries.iter().enumerate() {
                obj_set_value(&keys_array, &i.to_string().into(), key.clone())?;
            }
            // Set length
            obj_set_value(&keys_array, &"length".into(), Value::Number(map.borrow().entries.len() as f64))?;
            Ok(Value::Object(keys_array))
        }
        "values" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Map.prototype.values takes no arguments"));
            }
            // Create an array of values
            let values_array = new_js_object_data();
            for (i, (_, value)) in map.borrow().entries.iter().enumerate() {
                obj_set_value(&values_array, &i.to_string().into(), value.clone())?;
            }
            // Set length
            obj_set_value(&values_array, &"length".into(), Value::Number(map.borrow().entries.len() as f64))?;
            Ok(Value::Object(values_array))
        }
        "entries" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Map.prototype.entries takes no arguments"));
            }
            // Create an array of [key, value] pairs
            let entries_array = new_js_object_data();
            for (i, (key, value)) in map.borrow().entries.iter().enumerate() {
                let entry_array = new_js_object_data();
                obj_set_value(&entry_array, &"0".into(), key.clone())?;
                obj_set_value(&entry_array, &"1".into(), value.clone())?;
                obj_set_value(&entry_array, &"length".into(), Value::Number(2.0))?;
                obj_set_value(&entries_array, &i.to_string().into(), Value::Object(entry_array))?;
            }
            // Set length
            obj_set_value(&entries_array, &"length".into(), Value::Number(map.borrow().entries.len() as f64))?;
            Ok(Value::Object(entries_array))
        }
        _ => Err(raise_eval_error!(format!("Map.prototype.{} is not implemented", method))),
    }
}

// Helper function to compare two values for equality (simplified version)
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(na), Value::Number(nb)) => na == nb,
        (Value::String(sa), Value::String(sb)) => sa == sb,
        (Value::Boolean(ba), Value::Boolean(bb)) => ba == bb,
        (Value::Undefined, Value::Undefined) => true,
        (Value::Symbol(sa), Value::Symbol(sb)) => Rc::ptr_eq(sa, sb),
        _ => false, // For objects, we use reference equality
    }
}

// Helper function to get object property value
fn obj_get_value(js_obj: &JSObjectDataPtr, key: &PropertyKey) -> Result<Option<Rc<RefCell<Value>>>, JSError> {
    let mut current: Option<JSObjectDataPtr> = Some(js_obj.clone());
    while let Some(cur) = current {
        if let Some(val) = cur.borrow().properties.get(key) {
            return Ok(Some(val.clone()));
        }
        current = cur.borrow().prototype.clone();
    }
    Ok(None)
}
