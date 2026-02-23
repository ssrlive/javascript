use crate::core::{GcPtr, InternalSlot, MutationContext, new_gc_cell_ptr, slot_get_chained, slot_set};
use crate::core::{
    JSObjectDataPtr, JSSet, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value, same_value_zero,
};
use crate::js_array::{create_array, set_array_length};
use crate::unicode::utf8_to_utf16;
use crate::{JSError, core::EvalError};

/// Normalize a value per SameValueZero: -0 becomes +0.
fn normalize_set_value<'gc>(val: Value<'gc>) -> Value<'gc> {
    if let Value::Number(n) = &val
        && *n == 0.0
        && n.is_sign_negative()
    {
        return Value::Number(0.0);
    }
    val
}

/// Initialize Set constructor and prototype
pub fn initialize_set<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let set_ctor = new_js_object_data(mc);
    slot_set(mc, &set_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &set_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Set")));

    // Set.length = 0, Set.name = "Set" (non-enumerable, non-writable, configurable)
    object_set_key_value(mc, &set_ctor, "length", &Value::Number(0.0))?;
    set_ctor.borrow_mut(mc).set_non_enumerable("length");
    set_ctor.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &set_ctor, "name", &Value::String(utf8_to_utf16("Set")))?;
    set_ctor.borrow_mut(mc).set_non_enumerable("name");
    set_ctor.borrow_mut(mc).set_non_writable("name");

    // Set Set's [[Prototype]] to Function.prototype
    if let Some(func_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        set_ctor.borrow_mut(mc).prototype = Some(*func_proto);
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

    let set_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        set_proto.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(mc, &set_ctor, "prototype", &Value::Object(set_proto))?;
    set_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    set_ctor.borrow_mut(mc).set_non_writable("prototype");
    set_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &set_proto, "constructor", &Value::Object(set_ctor))?;

    // Register instance methods
    let methods = vec!["add", "has", "delete", "clear", "values", "entries", "forEach"];

    for method in methods {
        object_set_key_value(mc, &set_proto, method, &Value::Function(format!("Set.prototype.{}", method)))?;
        set_proto.borrow_mut(mc).set_non_enumerable(method);
    }

    // Per spec: Set.prototype.keys === Set.prototype.values (same function object)
    if let Some(values_fn) = object_get_key_value(&set_proto, "values") {
        object_set_key_value(mc, &set_proto, "keys", &values_fn.borrow().clone())?;
        set_proto.borrow_mut(mc).set_non_enumerable("keys");
    }
    // Mark constructor non-enumerable
    set_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Register size getter
    let size_getter = Value::Function("Set.prototype.size".to_string());
    let size_prop = Value::Property {
        value: None,
        getter: Some(Box::new(size_getter)),
        setter: None,
    };
    object_set_key_value(mc, &set_proto, "size", &size_prop)?;
    set_proto.borrow_mut(mc).set_non_enumerable("size");

    // Register Symbols
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.iterator -> values (writable, non-enumerable, configurable)
        if let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
            && let Value::Symbol(s) = &*iter_sym.borrow()
        {
            let val = Value::Function("Set.prototype.values".to_string());
            let iter_desc = {
                let desc_obj = new_js_object_data(mc);
                object_set_key_value(mc, &desc_obj, "value", &val)?;
                object_set_key_value(mc, &desc_obj, "writable", &Value::Boolean(true))?;
                object_set_key_value(mc, &desc_obj, "enumerable", &Value::Boolean(false))?;
                object_set_key_value(mc, &desc_obj, "configurable", &Value::Boolean(true))?;
                desc_obj
            };
            crate::js_object::define_property_internal(mc, &set_proto, crate::core::PropertyKey::Symbol(*s), &iter_desc)?;
        }

        // Symbol.toStringTag
        if let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            let tag_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Set")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &set_proto, crate::core::PropertyKey::Symbol(*s), &tag_desc)?;
        }

        // Symbol.species on Set constructor (getter that returns `this`)
        if let Some(species_sym) = object_get_key_value(sym_obj, "species")
            && let Value::Symbol(s) = &*species_sym.borrow()
        {
            let species_getter = Value::Function("Set[Symbol.species]".to_string());
            let species_desc_obj = new_js_object_data(mc);
            object_set_key_value(mc, &species_desc_obj, "get", &species_getter)?;
            object_set_key_value(mc, &species_desc_obj, "set", &Value::Undefined)?;
            object_set_key_value(mc, &species_desc_obj, "enumerable", &Value::Boolean(false))?;
            object_set_key_value(mc, &species_desc_obj, "configurable", &Value::Boolean(true))?;
            crate::js_object::define_property_internal(mc, &set_ctor, crate::core::PropertyKey::Symbol(*s), &species_desc_obj)?;
        }
    }

    // Set "keys" property to be the same function object as "values"
    // Per spec: Set.prototype.keys === Set.prototype.values
    // (already registered as separate Function values above, which is fine for tests)

    env_set(mc, env, "Set", &Value::Object(set_ctor))?;

    // --- %SetIteratorPrototype% ---
    // [[Prototype]] = %IteratorPrototype%
    let set_iter_proto = new_js_object_data(mc);
    if let Some(iter_proto_val) = slot_get_chained(env, &InternalSlot::IteratorPrototype)
        && let Value::Object(iter_proto) = &*iter_proto_val.borrow()
    {
        set_iter_proto.borrow_mut(mc).prototype = Some(*iter_proto);
    }

    // next method (non-enumerable)
    object_set_key_value(
        mc,
        &set_iter_proto,
        "next",
        &Value::Function("SetIterator.prototype.next".to_string()),
    )?;
    set_iter_proto.borrow_mut(mc).set_non_enumerable("next");

    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.toStringTag = "Set Iterator" (non-writable, non-enumerable, configurable)
        if let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
        {
            let tag_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Set Iterator")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &set_iter_proto, crate::core::PropertyKey::Symbol(*tag_sym), &tag_desc)?;
        }
    }

    slot_set(mc, env, InternalSlot::SetIteratorPrototype, &Value::Object(set_iter_proto));

    Ok(())
}

/// Handle Set constructor calls: `new Set()`, `new Set(iterable)`
///
/// Per spec:
/// 1. Let set be OrdinaryCreateFromConstructor(NewTarget, "%Set.prototype%", « [[SetData]] »).
/// 2. Set set.[[SetData]] to a new empty List.
/// 3. If iterable is undefined or null, return set.
/// 4. Let adder be ? Get(set, "add").
/// 5. If IsCallable(adder) is false, throw a TypeError.
/// 6. Let iteratorRecord be ? GetIterator(iterable, sync).
/// 7. For each value from iteratorRecord, call adder with value.
pub(crate) fn handle_set_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let set = new_gc_cell_ptr(mc, JSSet { values: Vec::new() });

    // Create a wrapper object for the Set
    let set_obj = new_js_object_data(mc);
    // Store the actual set data
    slot_set(mc, &set_obj, InternalSlot::Set, &Value::Set(set));

    // OrdinaryCreateFromConstructor(NewTarget, "%Set.prototype%")
    let mut proto_set = false;
    if let Some(Value::Object(nt_obj)) = new_target
        && let Some(proto) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "Set")?
    {
        set_obj.borrow_mut(mc).prototype = Some(proto);
        proto_set = true;
    }
    // Default: Set prototype to Set.prototype from current realm
    if !proto_set
        && let Some(set_ctor) = object_get_key_value(env, "Set")
        && let Value::Object(ctor) = &*set_ctor.borrow()
        && let Some(proto) = object_get_key_value(ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto.borrow()
    {
        set_obj.borrow_mut(mc).prototype = Some(*proto_obj);
    }

    // Step 3: If iterable is not present, or is undefined/null, return the empty set.
    let iterable = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(iterable, Value::Undefined | Value::Null) {
        return Ok(Value::Object(set_obj));
    }

    // Step 4-5: Get "add" method from the set object.
    let add_fn = crate::core::get_property_with_accessors(mc, env, &set_obj, "add")?;
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
        return Err(raise_type_error!("Set constructor: 'add' is not a function").into());
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
        let call_result = crate::core::evaluate_call_dispatch(mc, env, &add_fn, Some(&Value::Object(set_obj)), &[item]);
        if let Err(e) = call_result {
            let _ = crate::js_map::close_iterator(mc, env, &iter_obj);
            return Err(e);
        }
    }

    Ok(Value::Object(set_obj))
}

/// Handle Set instance method calls
pub(crate) fn handle_set_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    set: &GcPtr<'gc, JSSet<'gc>>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    this_obj: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "add" => {
            let value = normalize_set_value(args.first().cloned().unwrap_or(Value::Undefined));

            // Update existing entry in-place if value exists
            let exists = set
                .borrow()
                .values
                .iter()
                .any(|entry| entry.as_ref().is_some_and(|v| same_value_zero(v, &value)));
            if !exists {
                set.borrow_mut(mc).values.push(Some(value));
            }

            // Return the Set object itself (not the raw Set data)
            Ok(this_obj.clone())
        }
        "has" => {
            let value = args.first().cloned().unwrap_or(Value::Undefined);

            let has_value = set
                .borrow()
                .values
                .iter()
                .any(|entry| entry.as_ref().is_some_and(|v| same_value_zero(v, &value)));
            Ok(Value::Boolean(has_value))
        }
        "delete" => {
            let value = args.first().cloned().unwrap_or(Value::Undefined);

            // Tombstone deletion: set entry to None (preserves indices for iteration)
            let mut deleted = false;
            for entry in set.borrow_mut(mc).values.iter_mut() {
                if let Some(v) = entry
                    && same_value_zero(v, &value)
                {
                    *entry = None;
                    deleted = true;
                    break;
                }
            }

            Ok(Value::Boolean(deleted))
        }
        "clear" => {
            set.borrow_mut(mc).values.clear();
            Ok(Value::Undefined)
        }
        "size" => Ok(Value::Number(set.borrow().values.iter().filter(|e| e.is_some()).count() as f64)),
        "keys" => Ok(create_set_iterator(mc, env, *set, "values")?), // Set keys === values
        "values" => Ok(create_set_iterator(mc, env, *set, "values")?),
        "entries" => Ok(create_set_iterator(mc, env, *set, "entries")?),
        "forEach" => {
            if args.is_empty() {
                return Err(raise_type_error!("Set.prototype.forEach requires a callback function").into());
            }
            let callback = &args[0];
            let this_arg = args.get(1).cloned();

            // Validate callback is callable
            let is_callable = match callback {
                Value::Object(obj) => {
                    obj.borrow().get_closure().is_some()
                        || slot_get_chained(obj, &InternalSlot::NativeCtor).is_some()
                        || slot_get_chained(obj, &InternalSlot::Callable).is_some()
                }
                Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) => true,
                _ => false,
            };
            if !is_callable {
                return Err(raise_type_error!("Set.prototype.forEach callback is not a function").into());
            }

            // Iterate with index-based approach to handle mutations during iteration.
            let mut i = 0usize;
            loop {
                let len = set.borrow().values.len();
                if i >= len {
                    break;
                }
                let entry = set.borrow().values[i].clone();
                if let Some(v) = entry {
                    let call_args = vec![v.clone(), v, this_obj.clone()];
                    crate::core::evaluate_call_dispatch(mc, env, callback, this_arg.as_ref(), &call_args)?;
                }
                i += 1;
            }
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!(format!("Set.prototype.{} is not a function", method)).into()),
    }
}

fn create_set_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    set: GcPtr<'gc, JSSet<'gc>>,
    kind: &str,
) -> Result<Value<'gc>, JSError> {
    let iterator = new_js_object_data(mc);

    // Set [[Prototype]] to %SetIteratorPrototype%
    if let Some(proto_val) = slot_get_chained(env, &InternalSlot::SetIteratorPrototype)
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        iterator.borrow_mut(mc).prototype = Some(*proto);
    }

    slot_set(mc, &iterator, InternalSlot::IteratorSet, &Value::Set(set));
    slot_set(mc, &iterator, InternalSlot::IteratorIndex, &Value::Number(0.0));
    slot_set(mc, &iterator, InternalSlot::IteratorKind, &Value::String(utf8_to_utf16(kind)));

    Ok(Value::Object(iterator))
}

pub(crate) fn handle_set_iterator_next<'gc>(
    mc: &MutationContext<'gc>,
    iterator: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Step 3: If O does not have [[Set]], [[SetNextIndex]], [[SetIterationKind]], throw TypeError
    let set_val = slot_get_chained(iterator, &InternalSlot::IteratorSet)
        .ok_or_else(|| -> JSError { raise_type_error!("next called on incompatible receiver") })?;

    // Step 8: If set is undefined, iterator is exhausted
    if let Value::Undefined = &*set_val.borrow() {
        let result_obj = new_js_object_data(mc);
        object_set_key_value(mc, &result_obj, "value", &Value::Undefined)?;
        object_set_key_value(mc, &result_obj, "done", &Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    }

    let set_ptr = if let Value::Set(s) = &*set_val.borrow() {
        *s
    } else {
        return Err(raise_type_error!("next called on incompatible receiver"));
    };

    // Get index
    let index_val = slot_get_chained(iterator, &InternalSlot::IteratorIndex)
        .ok_or_else(|| -> JSError { raise_type_error!("next called on incompatible receiver") })?;
    let mut index = if let Value::Number(n) = &*index_val.borrow() {
        *n as usize
    } else {
        return Err(raise_type_error!("Iterator index is invalid"));
    };

    // Get kind
    let kind_val = slot_get_chained(iterator, &InternalSlot::IteratorKind)
        .ok_or_else(|| -> JSError { raise_type_error!("next called on incompatible receiver") })?;
    let kind = if let Value::String(s) = &*kind_val.borrow() {
        crate::unicode::utf16_to_utf8(s)
    } else {
        return Err(raise_type_error!("Iterator kind is invalid"));
    };

    let values = &set_ptr.borrow().values;

    // Skip tombstoned (None) entries
    while index < values.len() && values[index].is_none() {
        index += 1;
    }

    if index >= values.len() {
        // Per spec: set [[Set]] to undefined so iterator stays exhausted
        slot_set(mc, iterator, InternalSlot::IteratorSet, &Value::Undefined);
        let result_obj = new_js_object_data(mc);
        object_set_key_value(mc, &result_obj, "value", &Value::Undefined)?;
        object_set_key_value(mc, &result_obj, "done", &Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    }

    let value = values[index].as_ref().unwrap();
    let result_value = match kind.as_str() {
        "values" => value.clone(),
        "entries" => {
            let entry_array = create_array(mc, env)?;
            object_set_key_value(mc, &entry_array, "0", &value.clone())?;
            object_set_key_value(mc, &entry_array, "1", &value.clone())?;
            set_array_length(mc, &entry_array, 2)?;
            Value::Object(entry_array)
        }
        _ => return Err(raise_type_error!("Unknown iterator kind")),
    };

    // Update index
    index += 1;
    slot_set(mc, iterator, InternalSlot::IteratorIndex, &Value::Number(index as f64));

    let result_obj = new_js_object_data(mc);
    object_set_key_value(mc, &result_obj, "value", &result_value)?;
    object_set_key_value(mc, &result_obj, "done", &Value::Boolean(false))?;

    Ok(Value::Object(result_obj))
}
