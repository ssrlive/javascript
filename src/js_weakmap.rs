use crate::core::EvalError;
use crate::core::JSWeakMap;
use crate::core::{Gc, GcCell, InternalSlot, MutationContext, new_gc_cell_ptr, slot_get_chained, slot_set};
use crate::{
    core::{JSObjectDataPtr, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value},
    error::JSError,
    unicode::utf8_to_utf16,
};

/// Handle WeakMap constructor calls (spec §24.3.1.1)
pub(crate) fn handle_weakmap_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let weakmap = new_gc_cell_ptr(mc, JSWeakMap { entries: Vec::new() });

    // Create a wrapper object for the WeakMap
    let weakmap_obj = new_js_object_data(mc);
    // Store the actual weakmap data
    slot_set(mc, &weakmap_obj, InternalSlot::WeakMap, &Value::WeakMap(weakmap));

    // OrdinaryCreateFromConstructor(NewTarget, "%WeakMap.prototype%")
    let mut proto_set = false;
    if let Some(Value::Object(nt_obj)) = new_target
        && let Some(proto) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "WeakMap")?
    {
        weakmap_obj.borrow_mut(mc).prototype = Some(proto);
        proto_set = true;
    }
    if !proto_set
        && let Some(weakmap_ctor) = object_get_key_value(env, "WeakMap")
        && let Value::Object(ctor) = &*weakmap_ctor.borrow()
        && let Some(proto) = object_get_key_value(ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto.borrow()
    {
        weakmap_obj.borrow_mut(mc).prototype = Some(*proto_obj);
    }

    // Step 3: If iterable is not present, or is undefined/null, return.
    let iterable = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(iterable, Value::Undefined | Value::Null) {
        return Ok(Value::Object(weakmap_obj));
    }

    // Step 4-5: Get "set" method from the map object and check IsCallable.
    let set_fn = crate::core::get_property_with_accessors(mc, env, &weakmap_obj, "set")?;
    let set_is_callable = match &set_fn {
        Value::Object(obj) => {
            obj.borrow().get_closure().is_some()
                || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                || slot_get_chained(obj, &InternalSlot::Callable).is_some()
        }
        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) => true,
        _ => false,
    };
    if !set_is_callable {
        return Err(raise_type_error!("WeakMap constructor: 'set' is not a function").into());
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

        // 7a: If item is not an Object, throw TypeError and close iterator
        if !matches!(item, Value::Object(_)) {
            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
            return Err(raise_type_error!("Iterator value is not an entry object").into());
        }

        let item_obj = if let Value::Object(o) = &item { *o } else { unreachable!() };

        // 7b: Let k = Get(item, "0")
        let k = match crate::core::get_property_with_accessors(mc, env, &item_obj, "0") {
            Ok(v) => v,
            Err(e) => {
                let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                return Err(e);
            }
        };

        // 7c: Let v = Get(item, "1")
        let v = match crate::core::get_property_with_accessors(mc, env, &item_obj, "1") {
            Ok(v) => v,
            Err(e) => {
                let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
                return Err(e);
            }
        };

        // 7d: Call(adder, map, [k, v])
        let call_result = crate::core::evaluate_call_dispatch(mc, env, &set_fn, Some(&Value::Object(weakmap_obj)), &[k, v]);
        if let Err(e) = call_result {
            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
            return Err(e);
        }
    }

    Ok(Value::Object(weakmap_obj))
}

/// Initialize WeakMap constructor and prototype
pub fn initialize_weakmap<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let weakmap_ctor = new_js_object_data(mc);
    slot_set(mc, &weakmap_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(
        mc,
        &weakmap_ctor,
        InternalSlot::NativeCtor,
        &Value::String(utf8_to_utf16("WeakMap")),
    );

    // WeakMap.length = 0, WeakMap.name = "WeakMap" (non-enumerable, non-writable, configurable)
    object_set_key_value(mc, &weakmap_ctor, "length", &Value::Number(0.0))?;
    weakmap_ctor.borrow_mut(mc).set_non_enumerable("length");
    weakmap_ctor.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &weakmap_ctor, "name", &Value::String(utf8_to_utf16("WeakMap")))?;
    weakmap_ctor.borrow_mut(mc).set_non_enumerable("name");
    weakmap_ctor.borrow_mut(mc).set_non_writable("name");

    // Set WeakMap's [[Prototype]] to Function.prototype
    if let Some(func_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        weakmap_ctor.borrow_mut(mc).prototype = Some(*func_proto);
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

    let weakmap_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        weakmap_proto.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(mc, &weakmap_ctor, "prototype", &Value::Object(weakmap_proto))?;
    weakmap_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    weakmap_ctor.borrow_mut(mc).set_non_writable("prototype");
    weakmap_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &weakmap_proto, "constructor", &Value::Object(weakmap_ctor))?;

    // Register instance methods
    let methods = vec!["set", "get", "has", "delete", "toString", "getOrInsert", "getOrInsertComputed"];

    for method in methods {
        let val = Value::Function(format!("WeakMap.prototype.{method}"));
        object_set_key_value(mc, &weakmap_proto, method, &val)?;
        weakmap_proto.borrow_mut(mc).set_non_enumerable(method);
    }
    // Mark constructor non-enumerable
    weakmap_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Register Symbols
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.toStringTag = "WeakMap"
        if let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            let tag_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("WeakMap")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &weakmap_proto, crate::core::PropertyKey::Symbol(*s), &tag_desc)?;
        }
    }

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
    this_obj: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "set" => {
            let key = args.first().cloned().unwrap_or(Value::Undefined);
            let value = args.get(1).cloned().unwrap_or(Value::Undefined);

            // Check if key can be held weakly (must be an object or non-registered symbol)
            let key_obj_rc = match &key {
                Value::Object(obj) => *obj,
                _ => return Err(raise_type_error!("Invalid value used as weak map key").into()),
            };

            // Remove existing entry with same key
            weakmap
                .borrow_mut(mc)
                .entries
                .retain(|(k, _)| !k.upgrade(mc).is_some_and(|p| Gc::ptr_eq(p, key_obj_rc)));

            // Add new entry
            weakmap.borrow_mut(mc).entries.push((Gc::downgrade(key_obj_rc), value));

            // Return this (the wrapper object)
            Ok(this_obj.clone())
        }
        "get" => {
            let key = args.first().cloned().unwrap_or(Value::Undefined);

            let key_obj_rc = match &key {
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
            let key = args.first().cloned().unwrap_or(Value::Undefined);

            let key_obj_rc = match &key {
                Value::Object(obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            Ok(Value::Boolean(weakmap_has_key(mc, weakmap, key_obj_rc)))
        }
        "delete" => {
            let key = args.first().cloned().unwrap_or(Value::Undefined);

            let key_obj_rc = match &key {
                Value::Object(obj) => obj,
                _ => return Ok(Value::Boolean(false)),
            };

            Ok(Value::Boolean(weakmap_delete_key(mc, weakmap, key_obj_rc)))
        }
        "toString" => Ok(Value::String(utf8_to_utf16("[object WeakMap]"))),
        "getOrInsert" => {
            // WeakMap.prototype.getOrInsert(key, value)
            let key = args.first().cloned().unwrap_or(Value::Undefined);
            let default_value = args.get(1).cloned().unwrap_or(Value::Undefined);

            let key_obj_rc = match &key {
                Value::Object(obj) => *obj,
                _ => return Err(raise_type_error!("Invalid value used as weak map key").into()),
            };

            // If key already exists, return existing value
            for (k, v) in &weakmap.borrow().entries {
                if k.upgrade(mc).is_some_and(|p| Gc::ptr_eq(p, key_obj_rc)) {
                    return Ok(v.clone());
                }
            }
            // Insert and return the default
            weakmap
                .borrow_mut(mc)
                .entries
                .push((Gc::downgrade(key_obj_rc), default_value.clone()));
            Ok(default_value)
        }
        "getOrInsertComputed" => {
            // WeakMap.prototype.getOrInsertComputed(key, callbackFn)
            let key = args.first().cloned().unwrap_or(Value::Undefined);
            let callback = args.get(1).cloned().unwrap_or(Value::Undefined);

            let key_obj_rc = match &key {
                Value::Object(obj) => *obj,
                _ => return Err(raise_type_error!("Invalid value used as weak map key").into()),
            };

            // Validate callback is callable
            let is_callable = match &callback {
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::Callable).is_some()
                        || slot_get_chained(obj, &InternalSlot::BoundTarget).is_some()
                }
                Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) => true,
                _ => false,
            };
            if !is_callable {
                return Err(raise_type_error!("WeakMap.prototype.getOrInsertComputed: callback is not a function").into());
            }

            // If key already exists, return existing value
            for (k, v) in &weakmap.borrow().entries {
                if k.upgrade(mc).is_some_and(|p| Gc::ptr_eq(p, key_obj_rc)) {
                    return Ok(v.clone());
                }
            }
            // Call callback(key) to compute the value
            let computed = crate::core::evaluate_call_dispatch(mc, _env, &callback, None, std::slice::from_ref(&key))?;
            // Re-check: if callback caused insertion of same key, OVERWRITE with computed value
            for (k, v) in &mut weakmap.borrow_mut(mc).entries {
                if k.upgrade(mc).is_some_and(|p| Gc::ptr_eq(p, key_obj_rc)) {
                    *v = computed.clone();
                    return Ok(computed);
                }
            }
            weakmap.borrow_mut(mc).entries.push((Gc::downgrade(key_obj_rc), computed.clone()));
            Ok(computed)
        }
        _ => Err(raise_type_error!(format!("WeakMap.prototype.{} is not a function", method)).into()),
    }
}

/// Check if a JS object wraps an internal WeakMap
pub fn is_weakmap_object<'gc>(_mc: &MutationContext<'gc>, obj: &crate::core::JSObjectDataPtr<'gc>) -> bool {
    if let Some(val_rc) = slot_get_chained(obj, &InternalSlot::WeakMap) {
        matches!(&*val_rc.borrow(), crate::core::Value::WeakMap(_))
    } else {
        false
    }
}
