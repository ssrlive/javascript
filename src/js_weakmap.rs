use crate::{
    core::{Expr, JSObjectDataPtr, Value, evaluate_expr, obj_get_key_value},
    error::JSError,
    unicode::utf8_to_utf16,
};
use gc_arena::Mutation as MutationContext;

use crate::core::JSWeakMap;

/// Handle WeakMap constructor calls
pub(crate) fn handle_weakmap_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Expr],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let weakmap = gc_arena::Gc::new(mc, gc_arena::lock::RefLock::new(JSWeakMap { entries: Vec::new() }));

    if !args.is_empty() {
        if args.len() == 1 {
            // WeakMap(iterable)
            initialize_weakmap_from_iterable(mc, &weakmap, args, env)?;
        } else {
            return Err(raise_eval_error!("WeakMap constructor takes at most one argument"));
        }
    }

    Ok(Value::WeakMap(weakmap))
}

/// Initialize WeakMap from an iterable
fn initialize_weakmap_from_iterable<'gc>(
    mc: &MutationContext<'gc>,
    weakmap: &gc_arena::Gc<'gc, gc_arena::lock::RefLock<JSWeakMap<'gc>>>,
    args: &[Expr],
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    let iterable = evaluate_expr(mc, env, &args[0])?;
    match iterable {
        Value::Object(obj) => {
            let mut i = 0;
            loop {
                let key = format!("{}", i);
                if let Some(entry_val) = obj_get_key_value(&obj, &key.into())? {
                    let entry = entry_val.borrow().clone();
                    if let Value::Object(entry_obj) = entry
                        && let (Some(key_val), Some(value_val)) = (
                            obj_get_key_value(&entry_obj, &"0".into())?,
                            obj_get_key_value(&entry_obj, &"1".into())?,
                        )
                    {
                        let key_obj = key_val.borrow().clone();
                        let value_obj = value_val.borrow().clone();

                        // Check if key is an object
                        if let Value::Object(ref obj) = key_obj {
                            // Note: JSWeakMap currently holds strong references (Gc), so this is effectively a Map.
                            // Real WeakMap behavior requires ephemeron support in gc-arena.
                            weakmap.borrow_mut(mc).entries.push((obj.clone(), value_obj));
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
    Ok(())
}

/// Check if WeakMap has a key
fn weakmap_has_key<'gc>(weakmap: &gc_arena::Gc<'gc, gc_arena::lock::RefLock<JSWeakMap<'gc>>>, key_obj_rc: &JSObjectDataPtr<'gc>) -> bool {
    let weakmap = weakmap.borrow();
    for (k, _) in &weakmap.entries {
        if gc_arena::Gc::ptr_eq(key_obj_rc, k) {
            return true;
        }
    }
    false
}

/// Delete a key from WeakMap
fn weakmap_delete_key<'gc>(
    mc: &MutationContext<'gc>,
    weakmap: &gc_arena::Gc<'gc, gc_arena::lock::RefLock<JSWeakMap<'gc>>>,
    key_obj_rc: &JSObjectDataPtr<'gc>,
) -> bool {
    let mut weakmap_mut = weakmap.borrow_mut(mc);
    let len_before = weakmap_mut.entries.len();
    weakmap_mut.entries.retain(|(k, _)| !gc_arena::Gc::ptr_eq(key_obj_rc, k));
    weakmap_mut.entries.len() < len_before
}

/// Handle WeakMap instance method calls
pub(crate) fn handle_weakmap_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    weakmap: &gc_arena::Gc<'gc, gc_arena::lock::RefLock<JSWeakMap<'gc>>>,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    match method {
        "set" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("WeakMap.prototype.set requires exactly two arguments"));
            }
            let key = evaluate_expr(mc, env, &args[0])?;
            let value = evaluate_expr(mc, env, &args[1])?;

            // Check if key is an object
            let key_obj_rc = match key {
                Value::Object(ref obj) => obj.clone(),
                _ => return Err(raise_eval_error!("WeakMap keys must be objects")),
            };

            // Remove existing entry with same key
            weakmap
                .borrow_mut(mc)
                .entries
                .retain(|(k, _)| !gc_arena::Gc::ptr_eq(&key_obj_rc, k));

            // Add new entry
            weakmap.borrow_mut(mc).entries.push((key_obj_rc, value));

            Ok(Value::WeakMap(weakmap.clone()))
        }
        "get" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakMap.prototype.get requires exactly one argument"));
            }
            let key = evaluate_expr(mc, env, &args[0])?;

            let key_obj_rc = match key {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Undefined),
            };

            let weakmap_ref = weakmap.borrow();
            for (k, v) in &weakmap_ref.entries {
                if gc_arena::Gc::ptr_eq(key_obj_rc, k) {
                    return Ok(v.clone());
                }
            }

            Ok(Value::Undefined)
        }
        "has" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakMap.prototype.has requires exactly one argument"));
            }
            let key = evaluate_expr(mc, env, &args[0])?;

            let key_obj_rc = match key {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            Ok(Value::Boolean(weakmap_has_key(weakmap, key_obj_rc)))
        }
        "delete" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakMap.prototype.delete requires exactly one argument"));
            }
            let key = evaluate_expr(mc, env, &args[0])?;

            let key_obj_rc = match key {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            Ok(Value::Boolean(weakmap_delete_key(mc, weakmap, key_obj_rc)))
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
