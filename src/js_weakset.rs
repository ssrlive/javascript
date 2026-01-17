use crate::core::{Gc, GcCell, MutationContext};
use crate::core::{JSWeakSet, PropertyKey};
use crate::{
    core::{JSObjectDataPtr, Value, env_set, new_js_object_data, obj_set_key_value, object_get_key_value},
    error::JSError,
    unicode::utf8_to_utf16,
};

/// Handle WeakSet constructor calls
pub(crate) fn handle_weakset_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let weakset = Gc::new(mc, GcCell::new(JSWeakSet { values: Vec::new() }));

    if !args.is_empty() {
        if args.len() == 1 {
            // WeakSet(iterable) - args are already evaluated values
            initialize_weakset_from_iterable(mc, &weakset, &args[0])?;
        } else {
            return Err(raise_eval_error!("WeakSet constructor takes at most one argument"));
        }
    }

    // Create a wrapper object for the WeakSet
    let weakset_obj = new_js_object_data(mc);
    // Store the actual weakset data
    weakset_obj.borrow_mut(mc).insert(
        PropertyKey::String("__weakset__".to_string()),
        Gc::new(mc, GcCell::new(Value::WeakSet(weakset))),
    );
    // Internal slot should be non-enumerable so it doesn't show up in `evaluate_script` output
    weakset_obj.borrow_mut(mc).set_non_enumerable(PropertyKey::from("__weakset__"));

    // Set prototype to WeakSet.prototype if available
    if let Some(weakset_ctor) = object_get_key_value(env, "WeakSet")
        && let Value::Object(ctor) = &*weakset_ctor.borrow()
        && let Some(proto) = object_get_key_value(ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto.borrow()
    {
        weakset_obj.borrow_mut(mc).prototype = Some(*proto_obj);
    }

    Ok(Value::Object(weakset_obj))
}

/// Initialize WeakSet from an iterable
fn initialize_weakset_from_iterable<'gc>(
    mc: &MutationContext<'gc>,
    weakset: &Gc<'gc, GcCell<JSWeakSet<'gc>>>,
    iterable: &Value<'gc>,
) -> Result<(), JSError> {
    match iterable {
        Value::Object(obj) => {
            let mut i = 0_usize;
            while let Some(value_val) = object_get_key_value(obj, i) {
                let value = value_val.borrow().clone();

                // Check if value is an object
                if let Value::Object(ref obj) = value {
                    let weak_value = Gc::downgrade(*obj);
                    weakset.borrow_mut(mc).values.push(weak_value);
                } else {
                    return Err(raise_eval_error!("WeakSet values must be objects"));
                }
                i += 1;
            }
        }
        _ => {
            return Err(raise_eval_error!("WeakSet constructor requires an iterable"));
        }
    }
    Ok(())
}

/// Initialize WeakSet constructor and prototype
pub fn initialize_weakset<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let weakset_ctor = new_js_object_data(mc);
    obj_set_key_value(mc, &weakset_ctor, &"__is_constructor".into(), Value::Boolean(true))?;
    obj_set_key_value(mc, &weakset_ctor, &"__native_ctor".into(), Value::String(utf8_to_utf16("WeakSet")))?;

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

    let weakset_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        weakset_proto.borrow_mut(mc).prototype = Some(proto);
    }

    obj_set_key_value(mc, &weakset_ctor, &"prototype".into(), Value::Object(weakset_proto))?;
    obj_set_key_value(mc, &weakset_proto, &"constructor".into(), Value::Object(weakset_ctor))?;

    // Register instance methods
    let methods = vec!["add", "has", "delete", "toString"];

    for method in methods {
        let val = Value::Function(format!("WeakSet.prototype.{}", method));
        obj_set_key_value(mc, &weakset_proto, &method.into(), val)?;
        weakset_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from(method));
    }
    // Mark constructor non-enumerable
    weakset_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("constructor"));

    env_set(mc, env, "WeakSet", Value::Object(weakset_ctor))?;
    Ok(())
}

/// Check if WeakSet has a value and clean up dead entries
fn weakset_has_value<'gc>(
    mc: &MutationContext<'gc>,
    weakset: &Gc<'gc, GcCell<JSWeakSet<'gc>>>,
    value_obj_rc: &JSObjectDataPtr<'gc>,
) -> bool {
    let mut found = false;
    weakset.borrow_mut(mc).values.retain(|v| {
        if let Some(strong_v) = v.upgrade(mc) {
            if Gc::ptr_eq(*value_obj_rc, strong_v) {
                found = true;
            }
            true // Keep alive entries
        } else {
            false // Remove dead entries
        }
    });
    found
}

/// Delete a value from WeakSet and clean up dead entries
fn weakset_delete_value<'gc>(
    mc: &MutationContext<'gc>,
    weakset: &Gc<'gc, GcCell<JSWeakSet<'gc>>>,
    value_obj_rc: &JSObjectDataPtr<'gc>,
) -> bool {
    let mut deleted = false;
    weakset.borrow_mut(mc).values.retain(|v| {
        if let Some(strong_v) = v.upgrade(mc) {
            if Gc::ptr_eq(*value_obj_rc, strong_v) {
                deleted = true;
                false // Remove this entry
            } else {
                true // Keep other alive entries
            }
        } else {
            false // Remove dead entries
        }
    });
    deleted
}

/// Handle WeakSet instance method calls
pub(crate) fn handle_weakset_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    weakset: &Gc<'gc, GcCell<JSWeakSet<'gc>>>,
    method: &str,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, JSError> {
    match method {
        "add" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakSet.prototype.add requires exactly one argument"));
            }

            // Check if value is an object
            let value_obj_rc = match args[0] {
                Value::Object(obj) => obj,
                _ => return Err(raise_eval_error!("WeakSet values must be objects")),
            };

            let weak_value = Gc::downgrade(value_obj_rc);

            // Remove existing entry with same value (if still alive)
            weakset.borrow_mut(mc).values.retain(|v| {
                if let Some(strong_v) = v.upgrade(mc) {
                    !Gc::ptr_eq(value_obj_rc, strong_v)
                } else {
                    false // Remove dead entries
                }
            });

            // Add new entry
            weakset.borrow_mut(mc).values.push(weak_value);

            Ok(Value::WeakSet(*weakset))
        }
        "has" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakSet.prototype.has requires exactly one argument"));
            }
            let value = args[0].clone();

            let value_obj_rc = match value {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            Ok(Value::Boolean(weakset_has_value(mc, weakset, value_obj_rc)))
        }
        "delete" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("WeakSet.prototype.delete requires exactly one argument"));
            }
            let value = args[0].clone();

            let value_obj_rc = match value {
                Value::Object(ref obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            Ok(Value::Boolean(weakset_delete_value(mc, weakset, value_obj_rc)))
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

/// Check if a JS object wraps an internal WeakSet
pub fn is_weakset_object<'gc>(_mc: &MutationContext<'gc>, obj: &crate::core::JSObjectDataPtr<'gc>) -> bool {
    if let Some(val_rc) = object_get_key_value(obj, "__weakset__") {
        matches!(&*val_rc.borrow(), crate::core::Value::WeakSet(_))
    } else {
        false
    }
}
