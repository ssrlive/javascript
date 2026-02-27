use crate::core::EvalError;
use crate::core::JSWeakSet;
use crate::core::{Gc, GcCell, InternalSlot, MutationContext, new_gc_cell_ptr, slot_get_chained, slot_set};
use crate::{
    core::{JSObjectDataPtr, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value},
    error::JSError,
    unicode::utf8_to_utf16,
};

/// Handle WeakSet constructor calls (spec §24.4.1.1)
pub(crate) fn handle_weakset_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let weakset = new_gc_cell_ptr(mc, JSWeakSet { values: Vec::new() });

    // Create a wrapper object for the WeakSet
    let weakset_obj = new_js_object_data(mc);
    // Store the actual weakset data
    slot_set(mc, &weakset_obj, InternalSlot::WeakSet, &Value::WeakSet(weakset));

    // OrdinaryCreateFromConstructor(NewTarget, "%WeakSet.prototype%")
    let mut proto_set = false;
    if let Some(Value::Object(nt_obj)) = new_target
        && let Some(proto) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "WeakSet")?
    {
        weakset_obj.borrow_mut(mc).prototype = Some(proto);
        proto_set = true;
    }
    if !proto_set
        && let Some(weakset_ctor) = object_get_key_value(env, "WeakSet")
        && let Value::Object(ctor) = &*weakset_ctor.borrow()
        && let Some(proto) = object_get_key_value(ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto.borrow()
    {
        weakset_obj.borrow_mut(mc).prototype = Some(*proto_obj);
    }

    // Step 3: If iterable is not present, or is undefined/null, return.
    let iterable = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(iterable, Value::Undefined | Value::Null) {
        return Ok(Value::Object(weakset_obj));
    }

    // Step 4-5: Get "add" method from the set object and check IsCallable.
    let add_fn = crate::core::get_property_with_accessors(mc, env, &weakset_obj, "add")?;
    let add_is_callable = match &add_fn {
        Value::Object(obj) => {
            obj.borrow().get_closure().is_some()
                || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                || slot_get_chained(obj, &InternalSlot::Callable).is_some()
        }
        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) => true,
        _ => false,
    };
    if !add_is_callable {
        return Err(raise_type_error!("WeakSet constructor: 'add' is not a function").into());
    }

    // Step 6: GetIterator.
    let (iter_obj, next_fn) = crate::js_map::get_iterator(mc, env, &iterable)?;

    // Step 7: Iterate
    loop {
        let next_result = crate::js_map::call_iterator_next(mc, env, &iter_obj, &next_fn)?;
        let done = crate::js_map::get_iterator_done(mc, env, &next_result)?;
        if done {
            break;
        }
        let item = match crate::js_map::get_iterator_value(mc, env, &next_result) {
            Ok(v) => v,
            Err(e) => {
                let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                return Err(e);
            }
        };

        // Call(adder, set, [item])
        let call_result = crate::core::evaluate_call_dispatch(mc, env, &add_fn, Some(&Value::Object(weakset_obj)), &[item]);
        if let Err(e) = call_result {
            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
            return Err(e);
        }
    }

    Ok(Value::Object(weakset_obj))
}

/// Initialize WeakSet constructor and prototype
pub fn initialize_weakset<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let weakset_ctor = new_js_object_data(mc);
    slot_set(mc, &weakset_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(
        mc,
        &weakset_ctor,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("WeakSet")),
    );

    // WeakSet.length = 0, WeakSet.name = "WeakSet" (non-enumerable, non-writable, configurable)
    object_set_key_value(mc, &weakset_ctor, "length", &Value::Number(0.0))?;
    weakset_ctor.borrow_mut(mc).set_non_enumerable("length");
    weakset_ctor.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &weakset_ctor, "name", &Value::String(utf8_to_utf16("WeakSet")))?;
    weakset_ctor.borrow_mut(mc).set_non_enumerable("name");
    weakset_ctor.borrow_mut(mc).set_non_writable("name");

    // Set WeakSet's [[Prototype]] to Function.prototype
    if let Some(func_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        weakset_ctor.borrow_mut(mc).prototype = Some(*func_proto);
    }

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

    object_set_key_value(mc, &weakset_ctor, "prototype", &Value::Object(weakset_proto))?;
    weakset_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    weakset_ctor.borrow_mut(mc).set_non_writable("prototype");
    weakset_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &weakset_proto, "constructor", &Value::Object(weakset_ctor))?;

    // Register instance methods
    let methods = vec!["add", "has", "delete", "toString"];

    for method in methods {
        let val = Value::Function(format!("WeakSet.prototype.{}", method));
        object_set_key_value(mc, &weakset_proto, method, &val)?;
        weakset_proto.borrow_mut(mc).set_non_enumerable(method);
    }
    // Mark constructor non-enumerable
    weakset_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Register Symbols
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.toStringTag = "WeakSet"
        if let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            let tag_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("WeakSet")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &weakset_proto, crate::core::PropertyKey::Symbol(*s), &tag_desc)?;
        }
    }

    env_set(mc, env, "WeakSet", &Value::Object(weakset_ctor))?;
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
    this_obj: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "add" => {
            let value = args.first().cloned().unwrap_or(Value::Undefined);

            // Check if value can be held weakly (must be an object)
            let value_obj_rc = match &value {
                Value::Object(obj) => *obj,
                _ => return Err(raise_type_error!("Invalid value used in weak set").into()),
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

            // Return this (the wrapper object)
            Ok(this_obj.clone())
        }
        "has" => {
            let value = args.first().cloned().unwrap_or(Value::Undefined);

            let value_obj_rc = match &value {
                Value::Object(obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            Ok(Value::Boolean(weakset_has_value(mc, weakset, value_obj_rc)))
        }
        "delete" => {
            let value = args.first().cloned().unwrap_or(Value::Undefined);

            let value_obj_rc = match &value {
                Value::Object(obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            Ok(Value::Boolean(weakset_delete_value(mc, weakset, value_obj_rc)))
        }
        "toString" => Ok(Value::String(utf8_to_utf16("[object WeakSet]"))),
        _ => Err(raise_type_error!(format!("WeakSet.prototype.{} is not a function", method)).into()),
    }
}

/// Check if a JS object wraps an internal WeakSet
pub fn is_weakset_object<'gc>(_mc: &MutationContext<'gc>, obj: &crate::core::JSObjectDataPtr<'gc>) -> bool {
    if let Some(val_rc) = slot_get_chained(obj, &InternalSlot::WeakSet) {
        matches!(&*val_rc.borrow(), crate::core::Value::WeakSet(_))
    } else {
        false
    }
}
