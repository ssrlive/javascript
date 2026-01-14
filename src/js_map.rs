use crate::core::{Gc, GcCell, GcPtr};
use crate::core::{
    JSMap, JSObjectDataPtr, MutationContext, PropertyKey, Value, env_set, initialize_collection_from_iterable, new_js_object_data,
    obj_get_key_value, obj_set_key_value, values_equal,
};
use crate::error::JSError;
use crate::js_array::{create_array, set_array_length};
use crate::unicode::utf8_to_utf16;

/// Initialize Map constructor and prototype
pub fn initialize_map<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let map_ctor = new_js_object_data(mc);
    obj_set_key_value(mc, &map_ctor, &"__is_constructor".into(), Value::Boolean(true))?;
    obj_set_key_value(mc, &map_ctor, &"__native_ctor".into(), Value::String(utf8_to_utf16("Map")))?;

    // Get Object.prototype
    let object_proto = if let Some(obj_val) = obj_get_key_value(env, &"Object".into())?
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = obj_get_key_value(obj_ctor, &"prototype".into())?
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

    obj_set_key_value(mc, &map_ctor, &"prototype".into(), Value::Object(map_proto))?;
    obj_set_key_value(mc, &map_proto, &"constructor".into(), Value::Object(map_ctor))?;

    // Register instance methods
    let methods = vec!["set", "get", "has", "delete", "clear", "keys", "values", "entries"];

    for method in methods {
        obj_set_key_value(mc, &map_proto, &method.into(), Value::Function(format!("Map.prototype.{}", method)))?;
        map_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from(method));
    }
    // Mark constructor non-enumerable
    map_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("constructor"));

    // Register size getter
    let size_getter = Value::Function("Map.prototype.size".to_string());
    let size_prop = Value::Property {
        value: None,
        getter: Some(Box::new(size_getter)),
        setter: None,
    };
    obj_set_key_value(mc, &map_proto, &"size".into(), size_prop)?;

    // Register Symbols
    if let Some(sym_ctor) = obj_get_key_value(env, &"Symbol".into())?
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.iterator
        if let Some(iter_sym) = obj_get_key_value(sym_obj, &"iterator".into())?
            && let Value::Symbol(s) = &*iter_sym.borrow()
        {
            let val = Value::Function("Map.prototype.entries".to_string());
            obj_set_key_value(mc, &map_proto, &PropertyKey::Symbol(*s), val)?;
        }

        // Symbol.toStringTag
        if let Some(tag_sym) = obj_get_key_value(sym_obj, &"toStringTag".into())?
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            obj_set_key_value(mc, &map_proto, &PropertyKey::Symbol(*s), Value::String(utf8_to_utf16("Map")))?;
        }
    }

    env_set(mc, env, "Map", Value::Object(map_ctor))?;
    Ok(())
}

/// Handle Map constructor calls
pub(crate) fn handle_map_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let map = Gc::new(mc, GcCell::new(JSMap { entries: Vec::new() }));

    initialize_collection_from_iterable(args, "Map", |entry| {
        if let Value::Object(entry_obj) = entry
            && let (Some(key_val), Some(value_val)) = (
                obj_get_key_value(&entry_obj, &"0".into())?,
                obj_get_key_value(&entry_obj, &"1".into())?,
            )
        {
            map.borrow_mut(mc)
                .entries
                .push((key_val.borrow().clone(), value_val.borrow().clone()));
        }
        Ok(())
    })?;

    // Create a wrapper object for the Map
    let map_obj = new_js_object_data(mc);
    // Store the actual map data
    map_obj.borrow_mut(mc).insert(
        PropertyKey::String("__map__".to_string()),
        Gc::new(mc, GcCell::new(Value::Map(map))),
    );

    // Set prototype to Map.prototype
    if let Some(map_ctor) = obj_get_key_value(env, &"Map".into())?
        && let Value::Object(ctor) = &*map_ctor.borrow()
        && let Some(proto) = obj_get_key_value(ctor, &"prototype".into())?
        && let Value::Object(proto_obj) = &*proto.borrow()
    {
        map_obj.borrow_mut(mc).prototype = Some(*proto_obj);
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
) -> Result<Value<'gc>, JSError> {
    match method {
        "set" => {
            if args.len() != 2 {
                return Err(raise_eval_error!("Map.prototype.set requires exactly two arguments"));
            }
            let key = args[0].clone();
            let value = args[1].clone();

            // Remove existing entry with same key
            map.borrow_mut(mc).entries.retain(|(k, _)| !values_equal(mc, k, &key));
            // Add new entry
            map.borrow_mut(mc).entries.push((key, value));

            Ok(Value::Map(*map))
        }
        "get" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Map.prototype.get requires exactly one argument"));
            }
            let key = args[0].clone();

            for (k, v) in &map.borrow().entries {
                if values_equal(mc, k, &key) {
                    return Ok(v.clone());
                }
            }
            Ok(Value::Undefined)
        }
        "has" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Map.prototype.has requires exactly one argument"));
            }
            let key = args[0].clone();

            let has_key = map.borrow().entries.iter().any(|(k, _)| values_equal(mc, k, &key));
            Ok(Value::Boolean(has_key))
        }
        "delete" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Map.prototype.delete requires exactly one argument"));
            }
            let key = args[0].clone();

            let initial_len = map.borrow().entries.len();
            map.borrow_mut(mc).entries.retain(|(k, _)| !values_equal(mc, k, &key));
            let deleted = map.borrow().entries.len() < initial_len;

            Ok(Value::Boolean(deleted))
        }
        "clear" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Map.prototype.clear takes no arguments"));
            }
            map.borrow_mut(mc).entries.clear();
            Ok(Value::Undefined)
        }
        "size" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Map.prototype.size is a getter"));
            }
            Ok(Value::Number(map.borrow().entries.len() as f64))
        }
        "keys" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Map.prototype.keys takes no arguments"));
            }
            create_map_iterator(mc, env, *map, "keys")
        }
        "values" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Map.prototype.values takes no arguments"));
            }
            create_map_iterator(mc, env, *map, "values")
        }
        "entries" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Map.prototype.entries takes no arguments"));
            }
            create_map_iterator(mc, env, *map, "entries")
        }
        _ => Err(raise_eval_error!(format!("Map.prototype.{} is not implemented", method))),
    }
}

/// Create a new Map Iterator
pub(crate) fn create_map_iterator<'gc>(
    mc: &MutationContext<'gc>,
    _env: &JSObjectDataPtr<'gc>,
    map: GcPtr<'gc, JSMap<'gc>>,
    kind: &str,
) -> Result<Value<'gc>, JSError> {
    let iterator = new_js_object_data(mc);

    // Store map
    obj_set_key_value(mc, &iterator, &"__iterator_map__".into(), Value::Map(map))?;
    // Store index
    obj_set_key_value(mc, &iterator, &"__iterator_index__".into(), Value::Number(0.0))?;
    // Store kind
    obj_set_key_value(mc, &iterator, &"__iterator_kind__".into(), Value::String(utf8_to_utf16(kind)))?;

    // next method
    obj_set_key_value(
        mc,
        &iterator,
        &"next".into(),
        Value::Function("MapIterator.prototype.next".to_string()),
    )?;

    // Register Symbols
    if let Some(sym_ctor) = obj_get_key_value(_env, &"Symbol".into())?
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.iterator
        if let Some(iter_sym) = obj_get_key_value(sym_obj, &"iterator".into())?
            && let Value::Symbol(s) = &*iter_sym.borrow()
        {
            let val = Value::Function("IteratorSelf".to_string());
            obj_set_key_value(mc, &iterator, &PropertyKey::Symbol(*s), val)?;
        }

        // Symbol.toStringTag
        if let Some(tag_sym) = obj_get_key_value(sym_obj, &"toStringTag".into())?
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            let val = Value::String(utf8_to_utf16("Map Iterator"));
            obj_set_key_value(mc, &iterator, &PropertyKey::Symbol(*s), val)?;
        }
    }

    Ok(Value::Object(iterator))
}

pub(crate) fn handle_map_iterator_next<'gc>(
    mc: &MutationContext<'gc>,
    iterator: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Get map
    let map_val = obj_get_key_value(iterator, &"__iterator_map__".into())?.ok_or(raise_eval_error!("Iterator has no map"))?;
    let map_ptr = if let Value::Map(m) = &*map_val.borrow() {
        *m
    } else {
        return Err(raise_eval_error!("Iterator map is invalid"));
    };

    // Get index
    let index_val = obj_get_key_value(iterator, &"__iterator_index__".into())?.ok_or(raise_eval_error!("Iterator has no index"))?;
    let mut index = if let Value::Number(n) = &*index_val.borrow() {
        *n as usize
    } else {
        return Err(raise_eval_error!("Iterator index is invalid"));
    };

    // Get kind
    let kind_val = obj_get_key_value(iterator, &"__iterator_kind__".into())?.ok_or(raise_eval_error!("Iterator has no kind"))?;
    let kind = if let Value::String(s) = &*kind_val.borrow() {
        crate::unicode::utf16_to_utf8(s)
    } else {
        return Err(raise_eval_error!("Iterator kind is invalid"));
    };

    let entries = &map_ptr.borrow().entries;

    if index >= entries.len() {
        let result_obj = new_js_object_data(mc);
        obj_set_key_value(mc, &result_obj, &"value".into(), Value::Undefined)?;
        obj_set_key_value(mc, &result_obj, &"done".into(), Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    }

    let (key, value) = &entries[index];
    let result_value = match kind.as_str() {
        "keys" => key.clone(),
        "values" => value.clone(),
        "entries" => {
            let entry_array = create_array(mc, env)?;
            obj_set_key_value(mc, &entry_array, &"0".into(), key.clone())?;
            obj_set_key_value(mc, &entry_array, &"1".into(), value.clone())?;
            set_array_length(mc, &entry_array, 2)?;
            Value::Object(entry_array)
        }
        _ => return Err(raise_eval_error!("Unknown iterator kind")),
    };

    // Update index
    index += 1;
    obj_set_key_value(mc, iterator, &"__iterator_index__".into(), Value::Number(index as f64))?;

    let result_obj = new_js_object_data(mc);
    obj_set_key_value(mc, &result_obj, &"value".into(), result_value)?;
    obj_set_key_value(mc, &result_obj, &"done".into(), Value::Boolean(false))?;

    Ok(Value::Object(result_obj))
}
