use crate::core::{GcPtr, InternalSlot, new_gc_cell_ptr, slot_get_chained, slot_set};
use crate::core::{
    JSMap, JSObjectDataPtr, MutationContext, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value,
    same_value_zero,
};
use crate::js_array::{create_array, set_array_length};
use crate::unicode::utf8_to_utf16;
use crate::{JSError, core::EvalError};

/// Normalize a key per SameValueZero: -0 becomes +0.
fn normalize_map_key<'gc>(key: Value<'gc>) -> Value<'gc> {
    if let Value::Number(n) = &key
        && *n == 0.0
        && n.is_sign_negative()
    {
        return Value::Number(0.0);
    }
    key
}

/// Initialize Map constructor and prototype
pub fn initialize_map<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let map_ctor = new_js_object_data(mc);
    slot_set(mc, &map_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &map_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Map")));

    // Map.length = 0, Map.name = "Map" (non-enumerable, non-writable, configurable)
    object_set_key_value(mc, &map_ctor, "length", &Value::Number(0.0))?;
    map_ctor.borrow_mut(mc).set_non_enumerable("length");
    map_ctor.borrow_mut(mc).set_non_writable("length");
    object_set_key_value(mc, &map_ctor, "name", &Value::String(utf8_to_utf16("Map")))?;
    map_ctor.borrow_mut(mc).set_non_enumerable("name");
    map_ctor.borrow_mut(mc).set_non_writable("name");

    // Set Map's [[Prototype]] to Function.prototype
    if let Some(func_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        map_ctor.borrow_mut(mc).prototype = Some(*func_proto);
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

    let map_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        map_proto.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(mc, &map_ctor, "prototype", &Value::Object(map_proto))?;
    map_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    map_ctor.borrow_mut(mc).set_non_writable("prototype");
    map_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &map_proto, "constructor", &Value::Object(map_ctor))?;

    // Register instance methods
    let methods = vec!["set", "get", "has", "delete", "clear", "keys", "values", "entries", "forEach"];

    for method in methods {
        object_set_key_value(mc, &map_proto, method, &Value::Function(format!("Map.prototype.{}", method)))?;
        map_proto.borrow_mut(mc).set_non_enumerable(method);
    }
    // Mark constructor non-enumerable
    map_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Register size getter
    let size_getter = Value::Function("Map.prototype.size".to_string());
    let size_prop = Value::Property {
        value: None,
        getter: Some(Box::new(size_getter)),
        setter: None,
    };
    object_set_key_value(mc, &map_proto, "size", &size_prop)?;
    map_proto.borrow_mut(mc).set_non_enumerable("size");

    // Register Symbols
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.iterator -> entries (writable, non-enumerable, configurable)
        if let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
            && let Value::Symbol(s) = &*iter_sym.borrow()
        {
            let val = Value::Function("Map.prototype.entries".to_string());
            // Use a data descriptor: writable=true, enumerable=false, configurable=true
            let iter_desc = {
                let desc_obj = new_js_object_data(mc);
                object_set_key_value(mc, &desc_obj, "value", &val)?;
                object_set_key_value(mc, &desc_obj, "writable", &Value::Boolean(true))?;
                object_set_key_value(mc, &desc_obj, "enumerable", &Value::Boolean(false))?;
                object_set_key_value(mc, &desc_obj, "configurable", &Value::Boolean(true))?;
                desc_obj
            };
            crate::js_object::define_property_internal(mc, &map_proto, crate::core::PropertyKey::Symbol(*s), &iter_desc)?;
        }

        // Symbol.toStringTag
        if let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            let tag_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Map")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &map_proto, crate::core::PropertyKey::Symbol(*s), &tag_desc)?;
        }

        // Symbol.species on Map constructor (getter that returns `this`)
        if let Some(species_sym) = object_get_key_value(sym_obj, "species")
            && let Value::Symbol(s) = &*species_sym.borrow()
        {
            let species_getter = Value::Function("Map[Symbol.species]".to_string());
            // Create accessor descriptor: get=species_getter, set=undefined, enumerable=false, configurable=true
            let species_desc_obj = new_js_object_data(mc);
            object_set_key_value(mc, &species_desc_obj, "get", &species_getter)?;
            object_set_key_value(mc, &species_desc_obj, "set", &Value::Undefined)?;
            object_set_key_value(mc, &species_desc_obj, "enumerable", &Value::Boolean(false))?;
            object_set_key_value(mc, &species_desc_obj, "configurable", &Value::Boolean(true))?;
            crate::js_object::define_property_internal(mc, &map_ctor, crate::core::PropertyKey::Symbol(*s), &species_desc_obj)?;
        }
    }

    env_set(mc, env, "Map", &Value::Object(map_ctor))?;

    // --- %MapIteratorPrototype% ---
    // [[Prototype]] = %IteratorPrototype%
    let map_iter_proto = new_js_object_data(mc);
    if let Some(iter_proto_val) = slot_get_chained(env, &InternalSlot::IteratorPrototype)
        && let Value::Object(iter_proto) = &*iter_proto_val.borrow()
    {
        map_iter_proto.borrow_mut(mc).prototype = Some(*iter_proto);
    }

    // next method (non-enumerable)
    object_set_key_value(
        mc,
        &map_iter_proto,
        "next",
        &Value::Function("MapIterator.prototype.next".to_string()),
    )?;
    map_iter_proto.borrow_mut(mc).set_non_enumerable("next");

    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.toStringTag = "Map Iterator" (non-writable, non-enumerable, configurable)
        if let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
        {
            let tag_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Map Iterator")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &map_iter_proto, crate::core::PropertyKey::Symbol(*tag_sym), &tag_desc)?;
        }
    }

    slot_set(mc, env, InternalSlot::MapIteratorPrototype, &Value::Object(map_iter_proto));

    Ok(())
}

/// Handle Map constructor calls: `new Map()`, `new Map(iterable)`
///
/// Per spec:
/// 1. Let map be OrdinaryCreateFromConstructor(NewTarget, "%Map.prototype%", « [[MapData]] »).
/// 2. Set map.[[MapData]] to a new empty List.
/// 3. If iterable is undefined or null, return map.
/// 4. Let adder be ? Get(map, "set").
/// 5. If IsCallable(adder) is false, throw a TypeError.
/// 6. Let iteratorRecord be ? GetIterator(iterable, sync).
/// 7. For each item from iteratorRecord:
///    a. If item is not an Object, throw a TypeError (and close iterator).
///    b. Let k be Get(item, "0").
///    c. Let v be Get(item, "1").
///    d. Call(adder, map, « k, v »).
///    e. On abrupt: IteratorClose.
pub(crate) fn handle_map_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    new_target: Option<&Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let map = new_gc_cell_ptr(mc, JSMap { entries: Vec::new() });

    // Create a wrapper object for the Map
    let map_obj = new_js_object_data(mc);
    // Store the actual map data
    slot_set(mc, &map_obj, InternalSlot::Map, &Value::Map(map));

    // OrdinaryCreateFromConstructor(NewTarget, "%Map.prototype%")
    // If new_target is provided (Reflect.construct), use GetPrototypeFromConstructor
    let mut proto_set = false;
    if let Some(Value::Object(nt_obj)) = new_target
        && let Some(proto) = crate::js_class::get_prototype_from_constructor(mc, nt_obj, env, "Map")?
    {
        map_obj.borrow_mut(mc).prototype = Some(proto);
        proto_set = true;
    }
    // Default: Set prototype to Map.prototype from current realm
    if !proto_set
        && let Some(map_ctor) = object_get_key_value(env, "Map")
        && let Value::Object(ctor) = &*map_ctor.borrow()
        && let Some(proto) = object_get_key_value(ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto.borrow()
    {
        map_obj.borrow_mut(mc).prototype = Some(*proto_obj);
    }

    // Step 3: If iterable is not present, or is undefined/null, return the empty map.
    let iterable = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(iterable, Value::Undefined | Value::Null) {
        return Ok(Value::Object(map_obj));
    }

    // Step 4-5: Get "set" method from the map object.
    // This must be done before iterating so a poisoned getter is triggered.
    let set_fn = crate::core::get_property_with_accessors(mc, env, &map_obj, "set")?;
    // Validate callable
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
        return Err(raise_type_error!("Map constructor: 'set' is not a function").into());
    }

    // Step 6: GetIterator. Get Symbol.iterator from iterable.
    let iterator_result = get_iterator(mc, env, &iterable)?;
    let (iter_obj, next_fn) = iterator_result;

    // Step 7: Iterate
    loop {
        // Call next
        let next_result = call_iterator_next(mc, env, &iter_obj, &next_fn)?;
        let done = get_iterator_done(mc, env, &next_result)?;
        if done {
            break;
        }
        let item = match get_iterator_value(mc, env, &next_result) {
            Ok(v) => v,
            Err(e) => {
                let _ = close_iterator(mc, env, &iter_obj);
                return Err(e);
            }
        };

        // 7a: If item is not an Object, throw TypeError and close iterator
        if !matches!(item, Value::Object(_)) {
            let _ = close_iterator(mc, env, &iter_obj);
            return Err(raise_type_error!("Iterator value is not an entry object").into());
        }

        let item_obj = if let Value::Object(o) = &item { *o } else { unreachable!() };

        // 7b: Let k = Get(item, "0")
        let k_result = crate::core::get_property_with_accessors(mc, env, &item_obj, "0");
        let k = match k_result {
            Ok(v) => v,
            Err(e) => {
                let _ = close_iterator(mc, env, &iter_obj);
                return Err(e);
            }
        };

        // 7c: Let v = Get(item, "1")
        let v_result = crate::core::get_property_with_accessors(mc, env, &item_obj, "1");
        let v = match v_result {
            Ok(v) => v,
            Err(e) => {
                let _ = close_iterator(mc, env, &iter_obj);
                return Err(e);
            }
        };

        // 7d: Call(adder, map, [k, v])
        let call_result = crate::core::evaluate_call_dispatch(mc, env, &set_fn, Some(&Value::Object(map_obj)), &[k, v]);
        if let Err(e) = call_result {
            let _ = close_iterator(mc, env, &iter_obj);
            return Err(e);
        }
    }

    Ok(Value::Object(map_obj))
}

/// Handle Map instance method calls
pub(crate) fn handle_map_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    map: &GcPtr<'gc, JSMap<'gc>>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    this_obj: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "set" => {
            let key = normalize_map_key(args.first().cloned().unwrap_or(Value::Undefined));
            let value = args.get(1).cloned().unwrap_or(Value::Undefined);

            // Update existing entry in-place if key exists (preserves insertion order)
            let mut found = false;
            for (k, v) in map.borrow_mut(mc).entries.iter_mut().flatten() {
                if same_value_zero(k, &key) {
                    *k = key.clone(); // normalize key (e.g. -0 → +0)
                    *v = value.clone();
                    found = true;
                    break;
                }
            }
            if !found {
                map.borrow_mut(mc).entries.push(Some((key, value)));
            }

            // Return the Map object itself (not the raw Map data)
            Ok(this_obj.clone())
        }
        "get" => {
            let key = args.first().cloned().unwrap_or(Value::Undefined);

            for (k, v) in map.borrow().entries.iter().flatten() {
                if same_value_zero(k, &key) {
                    return Ok(v.clone());
                }
            }
            Ok(Value::Undefined)
        }
        "has" => {
            let key = args.first().cloned().unwrap_or(Value::Undefined);

            let has_key = map
                .borrow()
                .entries
                .iter()
                .any(|entry| entry.as_ref().is_some_and(|(k, _)| same_value_zero(k, &key)));
            Ok(Value::Boolean(has_key))
        }
        "delete" => {
            let key = args.first().cloned().unwrap_or(Value::Undefined);

            // Tombstone deletion: set entry to None (preserves indices for iteration)
            let mut deleted = false;
            for entry in map.borrow_mut(mc).entries.iter_mut() {
                if let Some((k, _)) = entry
                    && same_value_zero(k, &key)
                {
                    *entry = None;
                    deleted = true;
                    break;
                }
            }

            Ok(Value::Boolean(deleted))
        }
        "clear" => {
            map.borrow_mut(mc).entries.clear();
            Ok(Value::Undefined)
        }
        "size" => Ok(Value::Number(map.borrow().entries.iter().filter(|e| e.is_some()).count() as f64)),
        "keys" => Ok(create_map_iterator(mc, env, *map, "keys")?),
        "values" => Ok(create_map_iterator(mc, env, *map, "values")?),
        "entries" => Ok(create_map_iterator(mc, env, *map, "entries")?),
        "forEach" => {
            if args.is_empty() {
                return Err(raise_type_error!("Map.prototype.forEach requires a callback function").into());
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
                return Err(raise_type_error!("Map.prototype.forEach callback is not a function").into());
            }

            // Iterate with index-based approach to handle mutations during iteration.
            // Per spec: tombstoned (deleted) entries are skipped, and entries added
            // during iteration are visited (because we re-check len each loop).
            let mut i = 0usize;
            loop {
                let len = map.borrow().entries.len();
                if i >= len {
                    break;
                }
                let entry = map.borrow().entries[i].clone();
                if let Some((k, v)) = entry {
                    let call_args = vec![v, k, this_obj.clone()];
                    crate::core::evaluate_call_dispatch(mc, env, callback, this_arg.as_ref(), &call_args)?;
                }
                i += 1;
            }
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!(format!("Map.prototype.{} is not a function", method)).into()),
    }
}

/// Create a new Map Iterator
pub(crate) fn create_map_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    map: GcPtr<'gc, JSMap<'gc>>,
    kind: &str,
) -> Result<Value<'gc>, JSError> {
    let iterator = new_js_object_data(mc);

    // Set [[Prototype]] to %MapIteratorPrototype%
    if let Some(proto_val) = slot_get_chained(env, &InternalSlot::MapIteratorPrototype)
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        iterator.borrow_mut(mc).prototype = Some(*proto);
    }

    // Store map
    slot_set(mc, &iterator, InternalSlot::IteratorMap, &Value::Map(map));
    // Store index
    slot_set(mc, &iterator, InternalSlot::IteratorIndex, &Value::Number(0.0));
    // Store kind
    slot_set(mc, &iterator, InternalSlot::IteratorKind, &Value::String(utf8_to_utf16(kind)));

    Ok(Value::Object(iterator))
}

pub(crate) fn handle_map_iterator_next<'gc>(
    mc: &MutationContext<'gc>,
    iterator: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Step 3: If O does not have [[Map]], [[MapNextIndex]], [[MapIterationKind]], throw TypeError
    let map_val = slot_get_chained(iterator, &InternalSlot::IteratorMap)
        .ok_or_else(|| -> JSError { raise_type_error!("next called on incompatible receiver") })?;

    // Step 8: If map is undefined, iterator is exhausted → return {value: undefined, done: true}
    if let Value::Undefined = &*map_val.borrow() {
        let result_obj = new_js_object_data(mc);
        object_set_key_value(mc, &result_obj, "value", &Value::Undefined)?;
        object_set_key_value(mc, &result_obj, "done", &Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    }

    let map_ptr = if let Value::Map(m) = &*map_val.borrow() {
        *m
    } else {
        return Err(raise_type_error!("next called on incompatible receiver"));
    };

    // Get index
    let index_val = slot_get_chained(iterator, &InternalSlot::IteratorIndex)
        .ok_or_else(|| -> JSError { raise_type_error!("next called on incompatible receiver") })?;
    let mut index = if let Value::Number(n) = &*index_val.borrow() {
        *n as usize
    } else {
        return Err(raise_eval_error!("Iterator index is invalid"));
    };

    // Get kind
    let kind_val = slot_get_chained(iterator, &InternalSlot::IteratorKind)
        .ok_or_else(|| -> JSError { raise_type_error!("next called on incompatible receiver") })?;
    let kind = if let Value::String(s) = &*kind_val.borrow() {
        crate::unicode::utf16_to_utf8(s)
    } else {
        return Err(raise_eval_error!("Iterator kind is invalid"));
    };

    let entries = &map_ptr.borrow().entries;

    // Skip tombstoned (None) entries
    while index < entries.len() && entries[index].is_none() {
        index += 1;
    }

    if index >= entries.len() {
        // Per spec: set [[Map]] to undefined so iterator stays exhausted
        slot_set(mc, iterator, InternalSlot::IteratorMap, &Value::Undefined);
        let result_obj = new_js_object_data(mc);
        object_set_key_value(mc, &result_obj, "value", &Value::Undefined)?;
        object_set_key_value(mc, &result_obj, "done", &Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    }

    let (key, value) = entries[index].as_ref().unwrap();
    let result_value = match kind.as_str() {
        "keys" => key.clone(),
        "values" => value.clone(),
        "entries" => {
            let entry_array = create_array(mc, env)?;
            object_set_key_value(mc, &entry_array, "0", &key.clone())?;
            object_set_key_value(mc, &entry_array, "1", &value.clone())?;
            set_array_length(mc, &entry_array, 2)?;
            Value::Object(entry_array)
        }
        _ => return Err(raise_eval_error!("Unknown iterator kind")),
    };

    // Update index
    index += 1;
    slot_set(mc, iterator, InternalSlot::IteratorIndex, &Value::Number(index as f64));

    let result_obj = new_js_object_data(mc);
    object_set_key_value(mc, &result_obj, "value", &result_value)?;
    object_set_key_value(mc, &result_obj, "done", &Value::Boolean(false))?;

    Ok(Value::Object(result_obj))
}

// ---------------------------------------------------------------------------
// Iterator helpers
// ---------------------------------------------------------------------------

/// Get an iterator from an iterable object (GetIterator).
fn get_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iterable: &Value<'gc>,
) -> Result<(JSObjectDataPtr<'gc>, Value<'gc>), EvalError<'gc>> {
    let obj = match iterable {
        Value::Object(o) => *o,
        _ => return Err(raise_type_error!("Value is not iterable").into()),
    };

    // Look up Symbol.iterator
    let iter_fn = {
        let mut found = None;
        if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
            && let Value::Object(sym_obj) = &*sym_ctor.borrow()
            && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
            && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
            && let Some(val) = object_get_key_value(&obj, *iter_sym)
        {
            let resolved = match &*val.borrow() {
                Value::Property { getter: Some(g), .. } => crate::core::call_accessor(mc, env, &obj, g)?,
                Value::Property { value: Some(v), .. } => v.borrow().clone(),
                other => other.clone(),
            };
            found = Some(resolved);
        }
        found
    };

    let iter_fn = iter_fn.ok_or_else(|| -> EvalError<'gc> {
        raise_type_error!("Value is not iterable (cannot read property Symbol(Symbol.iterator))").into()
    })?;

    // Call the iterator function
    if matches!(iter_fn, Value::Undefined) {
        return Err(raise_type_error!("Value is not iterable (iterator method is undefined)").into());
    }

    let iter_result = crate::core::evaluate_call_dispatch(mc, env, &iter_fn, Some(iterable), &[])?;

    let iter_obj = match iter_result {
        Value::Object(o) => o,
        _ => return Err(raise_type_error!("Result of the Symbol.iterator method is not an object").into()),
    };

    // Get "next" from iterator
    let next_fn = crate::core::get_property_with_accessors(mc, env, &iter_obj, "next")?;

    Ok((iter_obj, next_fn))
}

/// Call iterator.next()
fn call_iterator_next<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iter_obj: &JSObjectDataPtr<'gc>,
    next_fn: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    crate::core::evaluate_call_dispatch(mc, env, next_fn, Some(&Value::Object(*iter_obj)), &[])
}

/// Get "done" from iterator result
fn get_iterator_done<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, result: &Value<'gc>) -> Result<bool, EvalError<'gc>> {
    if let Value::Object(obj) = result {
        let done_val = crate::core::get_property_with_accessors(mc, env, obj, "done")?;
        Ok(done_val.to_truthy())
    } else {
        Err(raise_type_error!("Iterator result is not an object").into())
    }
}

/// Get "value" from iterator result (accessor-aware)
fn get_iterator_value<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    result: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if let Value::Object(obj) = result {
        Ok(crate::core::get_property_with_accessors(mc, env, obj, "value")?)
    } else {
        Err(raise_type_error!("Iterator result is not an object").into())
    }
}

/// Close an iterator (call iterator.return() if present)
fn close_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iter_obj: &JSObjectDataPtr<'gc>,
) -> Result<(), EvalError<'gc>> {
    if let Some(return_val) = object_get_key_value(iter_obj, "return") {
        let return_fn = return_val.borrow().clone();
        if !matches!(return_fn, Value::Undefined | Value::Null) {
            let _ = crate::core::evaluate_call_dispatch(mc, env, &return_fn, Some(&Value::Object(*iter_obj)), &[]);
        }
    }
    Ok(())
}
