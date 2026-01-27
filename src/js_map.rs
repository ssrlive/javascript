use crate::core::{GcPtr, new_gc_cell_ptr};
use crate::core::{
    JSMap, JSObjectDataPtr, MutationContext, PropertyKey, Value, env_set, initialize_collection_from_iterable, new_js_object_data,
    object_get_key_value, object_set_key_value, values_equal,
};
use crate::js_array::{create_array, set_array_length};
use crate::unicode::utf8_to_utf16;
use crate::{JSError, core::EvalError};

/// Initialize Map constructor and prototype
pub fn initialize_map<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let map_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &map_ctor, "__is_constructor", Value::Boolean(true))?;
    object_set_key_value(mc, &map_ctor, "__native_ctor", Value::String(utf8_to_utf16("Map")))?;

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

    object_set_key_value(mc, &map_ctor, "prototype", Value::Object(map_proto))?;
    object_set_key_value(mc, &map_proto, "constructor", Value::Object(map_ctor))?;

    // Register instance methods
    let methods = vec!["set", "get", "has", "delete", "clear", "keys", "values", "entries"];

    for method in methods {
        object_set_key_value(mc, &map_proto, method, Value::Function(format!("Map.prototype.{}", method)))?;
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
    object_set_key_value(mc, &map_proto, "size", size_prop)?;

    // Register Symbols
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.iterator
        if let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
            && let Value::Symbol(s) = &*iter_sym.borrow()
        {
            let val = Value::Function("Map.prototype.entries".to_string());
            object_set_key_value(mc, &map_proto, s, val)?;
        }

        // Symbol.toStringTag
        if let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            object_set_key_value(mc, &map_proto, s, Value::String(utf8_to_utf16("Map")))?;
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
    let map = new_gc_cell_ptr(mc, JSMap { entries: Vec::new() });

    initialize_collection_from_iterable(mc, env, args, "Map", |entry| {
        if let Value::Object(entry_obj) = entry {
            let key_val_opt = crate::core::get_property_with_accessors(mc, env, &entry_obj, &PropertyKey::from("0".to_string()));
            let value_val_opt = crate::core::get_property_with_accessors(mc, env, &entry_obj, &PropertyKey::from("1".to_string()));
            match (key_val_opt, value_val_opt) {
                (Ok(key_val), Ok(value_val)) => {
                    map.borrow_mut(mc).entries.push((key_val, value_val));
                }
                (Err(e), _) | (_, Err(e)) => return Err(e.into()),
            }
        }
        Ok(())
    })?;

    // Create a wrapper object for the Map
    let map_obj = new_js_object_data(mc);
    // Store the actual map data
    object_set_key_value(mc, &map_obj, "__map__", Value::Map(map))?;

    // Set prototype to Map.prototype
    if let Some(map_ctor) = object_get_key_value(env, "Map")
        && let Value::Object(ctor) = &*map_ctor.borrow()
        && let Some(proto) = object_get_key_value(ctor, "prototype")
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
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "set" => {
            if args.len() != 2 {
                return Err(EvalError::Js(raise_eval_error!("Map.prototype.set requires exactly two arguments")));
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
                return Err(EvalError::Js(raise_eval_error!("Map.prototype.get requires exactly one argument")));
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
                return Err(EvalError::Js(raise_eval_error!("Map.prototype.has requires exactly one argument")));
            }
            let key = args[0].clone();

            let has_key = map.borrow().entries.iter().any(|(k, _)| values_equal(mc, k, &key));
            Ok(Value::Boolean(has_key))
        }
        "delete" => {
            if args.len() != 1 {
                return Err(EvalError::Js(raise_eval_error!(
                    "Map.prototype.delete requires exactly one argument"
                )));
            }
            let key = args[0].clone();

            let initial_len = map.borrow().entries.len();
            map.borrow_mut(mc).entries.retain(|(k, _)| !values_equal(mc, k, &key));
            let deleted = map.borrow().entries.len() < initial_len;

            Ok(Value::Boolean(deleted))
        }
        "clear" => {
            if !args.is_empty() {
                return Err(EvalError::Js(raise_eval_error!("Map.prototype.clear takes no arguments")));
            }
            map.borrow_mut(mc).entries.clear();
            Ok(Value::Undefined)
        }
        "size" => {
            if !args.is_empty() {
                return Err(EvalError::Js(raise_eval_error!("Map.prototype.size is a getter")));
            }
            Ok(Value::Number(map.borrow().entries.len() as f64))
        }
        "keys" => {
            if !args.is_empty() {
                return Err(EvalError::Js(raise_eval_error!("Map.prototype.keys takes no arguments")));
            }
            create_map_iterator(mc, env, *map, "keys").map_err(EvalError::Js)
        }
        "values" => {
            if !args.is_empty() {
                return Err(EvalError::Js(raise_eval_error!("Map.prototype.values takes no arguments")));
            }
            create_map_iterator(mc, env, *map, "values").map_err(EvalError::Js)
        }
        "entries" => {
            if !args.is_empty() {
                return Err(EvalError::Js(raise_eval_error!("Map.prototype.entries takes no arguments")));
            }
            create_map_iterator(mc, env, *map, "entries").map_err(EvalError::Js)
        }
        _ => Err(EvalError::Js(raise_eval_error!(format!(
            "Map.prototype.{} is not implemented",
            method
        )))),
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
    object_set_key_value(mc, &iterator, "__iterator_map__", Value::Map(map))?;
    // Store index
    object_set_key_value(mc, &iterator, "__iterator_index__", Value::Number(0.0))?;
    // Store kind
    object_set_key_value(mc, &iterator, "__iterator_kind__", Value::String(utf8_to_utf16(kind)))?;

    // next method
    object_set_key_value(mc, &iterator, "next", Value::Function("MapIterator.prototype.next".to_string()))?;

    // Register Symbols
    if let Some(sym_ctor) = object_get_key_value(_env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.iterator
        if let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
            && let Value::Symbol(s) = &*iter_sym.borrow()
        {
            object_set_key_value(mc, &iterator, s, Value::Function("IteratorSelf".to_string()))?;
        }

        // Symbol.toStringTag
        if let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            object_set_key_value(mc, &iterator, s, Value::String(utf8_to_utf16("Map Iterator")))?;
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
    let map_val = object_get_key_value(iterator, "__iterator_map__").ok_or(raise_eval_error!("Iterator has no map"))?;
    let map_ptr = if let Value::Map(m) = &*map_val.borrow() {
        *m
    } else {
        return Err(raise_eval_error!("Iterator map is invalid"));
    };

    // Get index
    let index_val = object_get_key_value(iterator, "__iterator_index__").ok_or(raise_eval_error!("Iterator has no index"))?;
    let mut index = if let Value::Number(n) = &*index_val.borrow() {
        *n as usize
    } else {
        return Err(raise_eval_error!("Iterator index is invalid"));
    };

    // Get kind
    let kind_val = object_get_key_value(iterator, "__iterator_kind__").ok_or(raise_eval_error!("Iterator has no kind"))?;
    let kind = if let Value::String(s) = &*kind_val.borrow() {
        crate::unicode::utf16_to_utf8(s)
    } else {
        return Err(raise_eval_error!("Iterator kind is invalid"));
    };

    let entries = &map_ptr.borrow().entries;

    if index >= entries.len() {
        let result_obj = new_js_object_data(mc);
        object_set_key_value(mc, &result_obj, "value", Value::Undefined)?;
        object_set_key_value(mc, &result_obj, "done", Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    }

    let (key, value) = &entries[index];
    let result_value = match kind.as_str() {
        "keys" => key.clone(),
        "values" => value.clone(),
        "entries" => {
            let entry_array = create_array(mc, env)?;
            object_set_key_value(mc, &entry_array, "0", key.clone())?;
            object_set_key_value(mc, &entry_array, "1", value.clone())?;
            set_array_length(mc, &entry_array, 2)?;
            Value::Object(entry_array)
        }
        _ => return Err(raise_eval_error!("Unknown iterator kind")),
    };

    // Update index
    index += 1;
    object_set_key_value(mc, iterator, "__iterator_index__", Value::Number(index as f64))?;

    let result_obj = new_js_object_data(mc);
    object_set_key_value(mc, &result_obj, "value", result_value)?;
    object_set_key_value(mc, &result_obj, "done", Value::Boolean(false))?;

    Ok(Value::Object(result_obj))
}
