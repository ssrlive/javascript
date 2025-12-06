use crate::{
    core::{Expr, JSObjectDataPtr, PropertyKey, Value, evaluate_expr},
    error::JSError,
    raise_eval_error,
    unicode::utf8_to_utf16,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::core::JSWeakMap;

/// Handle WeakMap constructor calls
pub(crate) fn handle_weakmap_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let weakmap = Rc::new(RefCell::new(JSWeakMap { entries: Vec::new() }));

    if !args.is_empty() {
        if args.len() == 1 {
            // WeakMap(iterable)
            let iterable = evaluate_expr(env, &args[0])?;
            match iterable {
                Value::Object(obj) => {
                    // Try to iterate over the object
                    let mut i = 0;
                    loop {
                        let key = format!("{}", i);
                        if let Some(entry_val) = obj_get_value(&obj, &key.into())? {
                            let entry = entry_val.borrow().clone();
                            if let Value::Object(entry_obj) = entry
                                && let (Some(key_val), Some(value_val)) =
                                    (obj_get_value(&entry_obj, &"0".into())?, obj_get_value(&entry_obj, &"1".into())?)
                            {
                                let key_obj = key_val.borrow().clone();
                                let value_obj = value_val.borrow().clone();

                                // Check if key is an object
                                if let Value::Object(ref obj) = key_obj {
                                    let weak_key = Rc::downgrade(obj);
                                    weakmap.borrow_mut().entries.push((weak_key, value_obj));
                                } else {
                                    return Err(raise_eval_error!("WeakMap keys must be objects"));
                                }
                            }
                        } else {
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    return Err(raise_eval_error!("WeakMap constructor requires an iterable"));
                }
            }
        } else {
            return Err(raise_eval_error!("WeakMap constructor takes at most one argument"));
        }
    }

    Ok(Value::WeakMap(weakmap))
}

/// Handle WeakMap instance method calls
pub(crate) fn handle_weakmap_instance_method(
    weakmap: &Rc<RefCell<JSWeakMap>>,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    match method {
        "set" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("WeakMap.prototype.set requires exactly two arguments"));
            }
            let key = evaluate_expr(env, &args[0])?;
            let value = evaluate_expr(env, &args[1])?;

            // Check if key is an object
            let key_obj_rc = match key {
                Value::Object(ref obj) => obj.clone(),
                _ => return Err(raise_eval_error!("WeakMap keys must be objects")),
            };

            let weak_key = Rc::downgrade(&key_obj_rc);

            // Remove existing entry with same key (if still alive)
            weakmap.borrow_mut().entries.retain(|(k, _)| {
                if let Some(strong_k) = k.upgrade() {
                    !Rc::ptr_eq(&key_obj_rc, &strong_k)
                } else {
                    false // Remove dead entries
                }
            });

            // Add new entry
            weakmap.borrow_mut().entries.push((weak_key, value));

            Ok(Value::WeakMap(weakmap.clone()))
        }
        "get" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakMap.prototype.get requires exactly one argument"));
            }
            let key = evaluate_expr(env, &args[0])?;

            let key_obj_rc = match key {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Undefined),
            };

            // Clean up dead entries and find the key
            let mut result = None;
            weakmap.borrow_mut().entries.retain(|(k, v)| {
                if let Some(strong_k) = k.upgrade() {
                    if Rc::ptr_eq(key_obj_rc, &strong_k) {
                        result = Some(v.clone());
                    }
                    true // Keep alive entries
                } else {
                    false // Remove dead entries
                }
            });

            Ok(result.unwrap_or(Value::Undefined))
        }
        "has" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakMap.prototype.has requires exactly one argument"));
            }
            let key = evaluate_expr(env, &args[0])?;

            let key_obj_rc = match key {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            // Clean up dead entries and check if key exists
            let mut found = false;
            weakmap.borrow_mut().entries.retain(|(k, _)| {
                if let Some(strong_k) = k.upgrade() {
                    if Rc::ptr_eq(key_obj_rc, &strong_k) {
                        found = true;
                    }
                    true // Keep alive entries
                } else {
                    false // Remove dead entries
                }
            });

            Ok(Value::Boolean(found))
        }
        "delete" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakMap.prototype.delete requires exactly one argument"));
            }
            let key = evaluate_expr(env, &args[0])?;

            let key_obj_rc = match key {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            // Clean up dead entries and remove the key
            let mut deleted = false;
            weakmap.borrow_mut().entries.retain(|(k, _)| {
                if let Some(strong_k) = k.upgrade() {
                    if Rc::ptr_eq(key_obj_rc, &strong_k) {
                        deleted = true;
                        false // Remove this entry
                    } else {
                        true // Keep other alive entries
                    }
                } else {
                    false // Remove dead entries
                }
            });

            Ok(Value::Boolean(deleted))
        }
        "toString" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("WeakMap.prototype.toString takes no arguments"));
            }
            Ok(Value::String(utf8_to_utf16("[object WeakMap]")))
        }
        _ => Err(raise_eval_error!(format!("WeakMap.prototype.{} is not implemented", method))),
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
