use crate::{
    core::{Expr, JSObjectData, JSObjectDataPtr, PropertyKey, Value, evaluate_expr, obj_set_value},
    error::JSError,
    raise_eval_error,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::core::JSSet;

/// Handle Set constructor calls
pub(crate) fn handle_set_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let set = Rc::new(RefCell::new(JSSet { values: Vec::new() }));

    if !args.is_empty() {
        if args.len() == 1 {
            // Set(iterable)
            let iterable = evaluate_expr(env, &args[0])?;
            match iterable {
                Value::Object(obj) => {
                    // Try to iterate over the object
                    // For now, assume it's an array-like
                    // TODO: Implement proper iteration protocol
                    let mut i = 0;
                    loop {
                        let key = format!("{}", i);
                        if let Some(value_val) = obj_get_value(&obj, &key.into())? {
                            let value = value_val.borrow().clone();
                            // Check if value already exists
                            let exists = set.borrow().values.iter().any(|v| values_equal(v, &value));
                            if !exists {
                                set.borrow_mut().values.push(value);
                            }
                        } else {
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    return Err(raise_eval_error!("Set constructor requires an iterable"));
                }
            }
        } else {
            return Err(raise_eval_error!("Set constructor takes at most one argument"));
        }
    }

    // Create a wrapper object for the Set
    let set_obj = Rc::new(RefCell::new(JSObjectData::new()));
    // Store the actual set data
    set_obj
        .borrow_mut()
        .insert(PropertyKey::String("__set__".to_string()), Rc::new(RefCell::new(Value::Set(set))));

    Ok(Value::Object(set_obj))
}

/// Handle Set instance method calls
pub(crate) fn handle_set_instance_method(
    set: &Rc<RefCell<JSSet>>,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    match method {
        "add" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Set.prototype.add requires exactly one argument"));
            }
            let value = evaluate_expr(env, &args[0])?;

            // Check if value already exists
            let exists = set.borrow().values.iter().any(|v| values_equal(v, &value));
            if !exists {
                set.borrow_mut().values.push(value);
            }

            Ok(Value::Set(set.clone()))
        }
        "has" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Set.prototype.has requires exactly one argument"));
            }
            let value = evaluate_expr(env, &args[0])?;

            let has_value = set.borrow().values.iter().any(|v| values_equal(v, &value));
            Ok(Value::Boolean(has_value))
        }
        "delete" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Set.prototype.delete requires exactly one argument"));
            }
            let value = evaluate_expr(env, &args[0])?;

            let initial_len = set.borrow().values.len();
            set.borrow_mut().values.retain(|v| !values_equal(v, &value));
            let deleted = set.borrow().values.len() < initial_len;

            Ok(Value::Boolean(deleted))
        }
        "clear" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Set.prototype.clear takes no arguments"));
            }
            set.borrow_mut().values.clear();
            Ok(Value::Undefined)
        }
        "size" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Set.prototype.size is a getter"));
            }
            Ok(Value::Number(set.borrow().values.len() as f64))
        }
        "values" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Set.prototype.values takes no arguments"));
            }
            // Create an array of values
            let values_array = Rc::new(RefCell::new(JSObjectData::new()));
            for (i, value) in set.borrow().values.iter().enumerate() {
                obj_set_value(&values_array, &i.to_string().into(), value.clone())?;
            }
            // Set length
            obj_set_value(&values_array, &"length".into(), Value::Number(set.borrow().values.len() as f64))?;
            Ok(Value::Object(values_array))
        }
        "keys" => {
            // For Set, keys() is the same as values()
            handle_set_instance_method(set, "values", args, env)
        }
        "entries" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Set.prototype.entries takes no arguments"));
            }
            // Create an array of [value, value] pairs
            let entries_array = Rc::new(RefCell::new(JSObjectData::new()));
            for (i, value) in set.borrow().values.iter().enumerate() {
                let entry_array = Rc::new(RefCell::new(JSObjectData::new()));
                obj_set_value(&entry_array, &"0".into(), value.clone())?;
                obj_set_value(&entry_array, &"1".into(), value.clone())?;
                obj_set_value(&entry_array, &"length".into(), Value::Number(2.0))?;
                obj_set_value(&entries_array, &i.to_string().into(), Value::Object(entry_array))?;
            }
            // Set length
            obj_set_value(&entries_array, &"length".into(), Value::Number(set.borrow().values.len() as f64))?;
            Ok(Value::Object(entries_array))
        }
        _ => Err(raise_eval_error!(format!("Set.prototype.{} is not implemented", method))),
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
