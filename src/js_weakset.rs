use crate::{
    core::{Expr, JSObjectDataPtr, PropertyKey, Value, evaluate_expr},
    error::JSError,
    raise_eval_error,
    unicode::utf8_to_utf16,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::core::JSWeakSet;

/// Handle WeakSet constructor calls
pub(crate) fn handle_weakset_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let weakset = Rc::new(RefCell::new(JSWeakSet { values: Vec::new() }));

    if !args.is_empty() {
        if args.len() == 1 {
            // WeakSet(iterable)
            let iterable = evaluate_expr(env, &args[0])?;
            match iterable {
                Value::Object(obj) => {
                    // Try to iterate over the object
                    let mut i = 0;
                    loop {
                        let key = format!("{}", i);
                        if let Some(value_val) = obj_get_value(&obj, &key.into())? {
                            let value = value_val.borrow().clone();

                            // Check if value is an object
                            if let Value::Object(ref obj) = value {
                                let weak_value = Rc::downgrade(obj);
                                weakset.borrow_mut().values.push(weak_value);
                            } else {
                                return Err(raise_eval_error!("WeakSet values must be objects"));
                            }
                        } else {
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    return Err(raise_eval_error!("WeakSet constructor requires an iterable"));
                }
            }
        } else {
            return Err(raise_eval_error!("WeakSet constructor takes at most one argument"));
        }
    }

    Ok(Value::WeakSet(weakset))
}

/// Handle WeakSet instance method calls
pub(crate) fn handle_weakset_instance_method(
    weakset: &Rc<RefCell<JSWeakSet>>,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    match method {
        "add" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakSet.prototype.add requires exactly one argument"));
            }
            let value = evaluate_expr(env, &args[0])?;

            // Check if value is an object
            let value_obj_rc = match value {
                Value::Object(ref obj) => obj.clone(),
                _ => return Err(raise_eval_error!("WeakSet values must be objects")),
            };

            let weak_value = Rc::downgrade(&value_obj_rc);

            // Remove existing entry with same value (if still alive)
            weakset.borrow_mut().values.retain(|v| {
                if let Some(strong_v) = v.upgrade() {
                    !Rc::ptr_eq(&value_obj_rc, &strong_v)
                } else {
                    false // Remove dead entries
                }
            });

            // Add new entry
            weakset.borrow_mut().values.push(weak_value);

            Ok(Value::WeakSet(weakset.clone()))
        }
        "has" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakSet.prototype.has requires exactly one argument"));
            }
            let value = evaluate_expr(env, &args[0])?;

            let value_obj_rc = match value {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            // Clean up dead entries and check if value exists
            let mut found = false;
            weakset.borrow_mut().values.retain(|v| {
                if let Some(strong_v) = v.upgrade() {
                    if Rc::ptr_eq(value_obj_rc, &strong_v) {
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
                return Err(raise_eval_error!("WeakSet.prototype.delete requires exactly one argument"));
            }
            let value = evaluate_expr(env, &args[0])?;

            let value_obj_rc = match value {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            // Clean up dead entries and remove the value
            let mut deleted = false;
            weakset.borrow_mut().values.retain(|v| {
                if let Some(strong_v) = v.upgrade() {
                    if Rc::ptr_eq(value_obj_rc, &strong_v) {
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
                return Err(raise_eval_error!("WeakSet.prototype.toString takes no arguments"));
            }
            Ok(Value::String(utf8_to_utf16("[object WeakSet]")))
        }
        _ => Err(raise_eval_error!(format!("WeakSet.prototype.{} is not implemented", method))),
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
