use crate::core::JSWeakMap;
use crate::core::{Gc, GcCell, MutationContext, new_gc_cell_ptr};
use crate::{
    core::{JSObjectDataPtr, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value},
    error::JSError,
    unicode::utf8_to_utf16,
};

/// Handle WeakMap constructor calls
pub(crate) fn handle_weakmap_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let weakmap = new_gc_cell_ptr(mc, JSWeakMap { entries: Vec::new() });

    if !args.is_empty() {
        if args.len() == 1 {
            // WeakMap(iterable) - args are already evaluated values
            initialize_weakmap_from_iterable(mc, &weakmap, &args[0])?;
        } else {
            return Err(raise_eval_error!("WeakMap constructor takes at most one argument"));
        }
    }

    // Create a wrapper object for the WeakMap
    let weakmap_obj = new_js_object_data(mc);
    // Store the actual weakmap data
    weakmap_obj
        .borrow_mut(mc)
        .insert("__weakmap__", new_gc_cell_ptr(mc, Value::WeakMap(weakmap)));
    // Internal slot should be non-enumerable so it doesn't show up in `evaluate_script` output
    weakmap_obj.borrow_mut(mc).set_non_enumerable("__weakmap__");

    // Set prototype to WeakMap.prototype if available
    if let Some(weakmap_ctor) = object_get_key_value(env, "WeakMap")
        && let Value::Object(ctor) = &*weakmap_ctor.borrow()
        && let Some(proto) = object_get_key_value(ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto.borrow()
    {
        weakmap_obj.borrow_mut(mc).prototype = Some(*proto_obj);
    }

    Ok(Value::Object(weakmap_obj))
}

/// Initialize WeakMap from an iterable
fn initialize_weakmap_from_iterable<'gc>(
    mc: &MutationContext<'gc>,
    weakmap: &Gc<'gc, GcCell<JSWeakMap<'gc>>>,
    iterable: &Value<'gc>,
) -> Result<(), JSError> {
    match iterable {
        Value::Object(obj) => {
            let mut i = 0_usize;
            while let Some(entry_val) = object_get_key_value(obj, i) {
                let entry = entry_val.borrow().clone();
                if let Value::Object(entry_obj) = entry
                    && let (Some(key_val), Some(value_val)) = (object_get_key_value(&entry_obj, "0"), object_get_key_value(&entry_obj, "1"))
                {
                    let key_obj = key_val.borrow().clone();
                    let value_obj = value_val.borrow().clone();

                    // Check if key is an object
                    if let Value::Object(ref obj) = key_obj {
                        // Note: JSWeakMap currently holds strong references (Gc), so this is effectively a Map.
                        // Real WeakMap behavior requires ephemeron support in gc-arena.
                        weakmap.borrow_mut(mc).entries.push((Gc::downgrade(*obj), value_obj));
                    } else {
                        return Err(raise_eval_error!("WeakMap keys must be objects"));
                    }
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

/// Initialize WeakMap constructor and prototype
pub fn initialize_weakmap<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let weakmap_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &weakmap_ctor, "__is_constructor", &Value::Boolean(true))?;
    object_set_key_value(mc, &weakmap_ctor, "__native_ctor", &Value::String(utf8_to_utf16("WeakMap")))?;

    // Get Object.prototype
    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let weakmap_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        weakmap_proto.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(mc, &weakmap_ctor, "prototype", &Value::Object(weakmap_proto))?;
    object_set_key_value(mc, &weakmap_proto, "constructor", &Value::Object(weakmap_ctor))?;

    // Register instance methods
    let methods = vec!["set", "get", "has", "delete", "toString"];

    for method in methods {
        let val = Value::Function(format!("WeakMap.prototype.{method}"));
        object_set_key_value(mc, &weakmap_proto, method, &val)?;
        weakmap_proto.borrow_mut(mc).set_non_enumerable(method);
    }
    // Mark constructor non-enumerable
    weakmap_proto.borrow_mut(mc).set_non_enumerable("constructor");

    env_set(mc, env, "WeakMap", &Value::Object(weakmap_ctor))?;
    Ok(())
}

/// Check if WeakMap has a key
fn weakmap_has_key<'gc>(mc: &MutationContext<'gc>, weakmap: &Gc<'gc, GcCell<JSWeakMap<'gc>>>, key_obj_rc: &JSObjectDataPtr<'gc>) -> bool {
    let weakmap = weakmap.borrow();
    for (k, _) in &weakmap.entries {
        if k.upgrade(mc).is_some_and(|p| Gc::ptr_eq(p, *key_obj_rc)) {
            return true;
        }
    }
    false
}

/// Delete a key from WeakMap
fn weakmap_delete_key<'gc>(
    mc: &MutationContext<'gc>,
    weakmap: &Gc<'gc, GcCell<JSWeakMap<'gc>>>,
    key_obj_rc: &JSObjectDataPtr<'gc>,
) -> bool {
    let mut weakmap_mut = weakmap.borrow_mut(mc);
    let len_before = weakmap_mut.entries.len();
    weakmap_mut
        .entries
        .retain(|(k, _)| !k.upgrade(mc).is_some_and(|p| Gc::ptr_eq(p, *key_obj_rc)));
    weakmap_mut.entries.len() < len_before
}

/// Handle WeakMap instance method calls
pub(crate) fn handle_weakmap_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    weakmap: &Gc<'gc, GcCell<JSWeakMap<'gc>>>,
    method: &str,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    match method {
        "set" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("WeakMap.prototype.set requires exactly two arguments"));
            }
            let key = &args[0];
            let value = args[1].clone();

            // Check if key is an object
            let key_obj_rc = match key {
                Value::Object(obj) => obj,
                _ => return Err(raise_eval_error!("WeakMap keys must be objects")),
            };

            // Remove existing entry with same key
            weakmap
                .borrow_mut(mc)
                .entries
                .retain(|(k, _)| !k.upgrade(mc).is_some_and(|p| Gc::ptr_eq(p, *key_obj_rc)));

            // Add new entry
            weakmap.borrow_mut(mc).entries.push((Gc::downgrade(*key_obj_rc), value));

            Ok(Value::WeakMap(*weakmap))
        }
        "get" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakMap.prototype.get requires exactly one argument"));
            }
            let key = &args[0];

            let key_obj_rc = match key {
                Value::Object(obj) => obj,
                _ => return Ok(Value::Undefined),
            };

            let weakmap_ref = weakmap.borrow();
            for (k, v) in &weakmap_ref.entries {
                if k.upgrade(mc).is_some_and(|p| Gc::ptr_eq(p, *key_obj_rc)) {
                    return Ok(v.clone());
                }
            }

            Ok(Value::Undefined)
        }
        "has" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakMap.prototype.has requires exactly one argument"));
            }
            let key = args[0].clone();

            let key_obj_rc = match key {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            Ok(Value::Boolean(weakmap_has_key(mc, weakmap, key_obj_rc)))
        }
        "delete" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakMap.prototype.delete requires exactly one argument"));
            }
            let key = args[0].clone();

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

/// Check if a JS object wraps an internal WeakMap
pub fn is_weakmap_object<'gc>(_mc: &MutationContext<'gc>, obj: &crate::core::JSObjectDataPtr<'gc>) -> bool {
    if let Some(val_rc) = object_get_key_value(obj, "__weakmap__") {
        matches!(&*val_rc.borrow(), crate::core::Value::WeakMap(_))
    } else {
        false
    }
}
