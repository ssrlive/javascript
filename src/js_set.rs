use crate::core::JSSet;
use crate::core::{Gc, GcCell, MutationContext};
use crate::{
    core::{
        JSObjectDataPtr, PropertyKey, Value, env_set, initialize_collection_from_iterable, new_js_object_data, obj_set_key_value,
        object_get_key_value, values_equal,
    },
    error::JSError,
    js_array::{create_array, set_array_length},
    unicode::utf8_to_utf16,
};

/// Initialize Set constructor and prototype
pub fn initialize_set<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let set_ctor = new_js_object_data(mc);
    obj_set_key_value(mc, &set_ctor, &"__is_constructor".into(), Value::Boolean(true))?;
    obj_set_key_value(mc, &set_ctor, &"__native_ctor".into(), Value::String(utf8_to_utf16("Set")))?;

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

    obj_set_key_value(mc, &set_ctor, &"prototype".into(), Value::Object(set_proto))?;
    obj_set_key_value(mc, &set_proto, &"constructor".into(), Value::Object(set_ctor))?;

    // Register instance methods
    let methods = vec!["add", "has", "delete", "clear", "keys", "values", "entries", "forEach"];

    for method in methods {
        obj_set_key_value(mc, &set_proto, &method.into(), Value::Function(format!("Set.prototype.{}", method)))?;
        set_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from(method));
    }
    // Mark constructor non-enumerable
    set_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("constructor"));

    // Get Symbol.iterator
    let iterator_sym = if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
    {
        Some(iter_sym.borrow().clone())
    } else {
        None
    };

    if let Some(Value::Symbol(iterator_sym_data)) = iterator_sym {
        let val = Value::Function("Set.prototype.values".to_string());
        obj_set_key_value(mc, &set_proto, &PropertyKey::Symbol(iterator_sym_data), val)?;
    }

    // Symbol.toStringTag
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(s) = &*tag_sym.borrow()
    {
        obj_set_key_value(mc, &set_proto, &PropertyKey::Symbol(*s), Value::String(utf8_to_utf16("Set")))?;
    }

    // Register size getter
    let size_getter = Value::Function("Set.prototype.size".to_string());
    let size_prop = Value::Property {
        value: None,
        getter: Some(Box::new(size_getter)),
        setter: None,
    };
    obj_set_key_value(mc, &set_proto, &"size".into(), size_prop)?;

    env_set(mc, env, "Set", Value::Object(set_ctor))?;
    Ok(())
}

/// Handle Set constructor calls
pub(crate) fn handle_set_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let set = Gc::new(mc, GcCell::new(JSSet { values: Vec::new() }));

    initialize_collection_from_iterable(args, "Set", |value| {
        // Check if value already exists
        let exists = set.borrow().values.iter().any(|v| values_equal(mc, &value, v));
        if !exists {
            set.borrow_mut(mc).values.push(value);
        }
        Ok(())
    })?;

    // Create a wrapper object for the Set
    let set_obj = new_js_object_data(mc);
    // Store the actual set data
    set_obj.borrow_mut(mc).insert(
        PropertyKey::String("__set__".to_string()),
        Gc::new(mc, GcCell::new(Value::Set(set))),
    );

    // Set prototype to Set.prototype
    if let Some(set_ctor) = object_get_key_value(env, "Set")
        && let Value::Object(ctor) = &*set_ctor.borrow()
        && let Some(proto) = object_get_key_value(ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto.borrow()
    {
        set_obj.borrow_mut(mc).prototype = Some(*proto_obj);
    }

    Ok(Value::Object(set_obj))
}

/// Handle Set instance method calls
pub(crate) fn handle_set_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    set: &Gc<'gc, GcCell<JSSet<'gc>>>,
    this_val: Value<'gc>,
    method: &str,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    match method {
        "add" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Set.prototype.add requires exactly one argument"));
            }
            let value = args[0].clone();

            // Check if value already exists
            let exists = set.borrow().values.iter().any(|v| values_equal(mc, &value, v));
            if !exists {
                set.borrow_mut(mc).values.push(value);
            }

            Ok(Value::Set(*set))
        }
        "has" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Set.prototype.has requires exactly one argument"));
            }
            let value = args[0].clone();

            let has_value = set.borrow().values.iter().any(|v| values_equal(mc, &value, v));
            Ok(Value::Boolean(has_value))
        }
        "delete" => {
            if args.len() != 1 {
                return Err(raise_eval_error!("Set.prototype.delete requires exactly one argument"));
            }
            let value = args[0].clone();

            let initial_len = set.borrow().values.len();
            set.borrow_mut(mc).values.retain(|v| !values_equal(mc, &value, v));
            let deleted = set.borrow().values.len() < initial_len;

            Ok(Value::Boolean(deleted))
        }
        "clear" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Set.prototype.clear takes no arguments"));
            }
            set.borrow_mut(mc).values.clear();
            Ok(Value::Undefined)
        }
        "size" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Set.prototype.size is a getter"));
            }
            Ok(Value::Number(set.borrow().values.len() as f64))
        }
        "values" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Set.prototype.values takes no arguments"));
            }
            create_set_iterator(mc, _env, *set, "values")
        }
        "keys" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Set.prototype.keys takes no arguments"));
            }
            create_set_iterator(mc, _env, *set, "values") // Set keys are same as values
        }
        "entries" => {
            if !args.is_empty() {
                return Err(raise_eval_error!("Set.prototype.entries takes no arguments"));
            }
            create_set_iterator(mc, _env, *set, "entries")
        }
        "forEach" => {
            if args.is_empty() {
                return Err(raise_eval_error!("Set.prototype.forEach requires at least one argument"));
            }
            let callback = args[0].clone();
            let this_arg = args.get(1).cloned();

            let values = set.borrow().values.clone();

            // Helper to execute closure
            let execute = |cl: &crate::core::ClosureData<'gc>| -> Result<(), JSError> {
                for value in &values {
                    let call_args = vec![value.clone(), value.clone(), this_val.clone()];
                    match crate::core::call_closure(mc, cl, this_arg.clone(), &call_args, _env, None) {
                        Ok(_) => {}
                        Err(e) => {
                            return Err(match e {
                                crate::core::EvalError::Js(err) => err,
                                crate::core::EvalError::Throw(val, _, _) => crate::raise_throw_error!(val),
                            });
                        }
                    }
                }
                Ok(())
            };

            match callback {
                Value::Object(obj) => {
                    if let Some(cl_val) = object_get_key_value(&obj, "__closure__") {
                        match &*cl_val.borrow() {
                            Value::Closure(cl) => execute(cl)?,
                            _ => return Err(crate::raise_type_error!("Set.prototype.forEach callback is not a closure")),
                        }
                    } else if let Some(_native_ctor) = object_get_key_value(&obj, "__native_ctor") {
                        // Native function object
                        return Err(raise_eval_error!("Native functions in forEach not supported yet"));
                    } else {
                        return Err(crate::raise_type_error!("Set.prototype.forEach callback is not a function"));
                    }
                }
                Value::Closure(cl) => execute(&cl)?,
                _ => return Err(crate::raise_type_error!("Set.prototype.forEach callback must be a function")),
            }
            Ok(Value::Undefined)
        }
        _ => Err(raise_eval_error!(format!("Set.prototype.{} is not implemented", method))),
    }
}

fn create_set_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    set: Gc<'gc, GcCell<JSSet<'gc>>>,
    kind: &str,
) -> Result<Value<'gc>, JSError> {
    let iterator = new_js_object_data(mc);

    // Store set weak reference or strong? JS iterators usually keep the collection alive.
    // However, cycle collection might be tricky if we use strong ref here and set has ref to iterator?
    // Usually iterators are created from set, set doesn't hold iterators. So strong ref matches spec.
    // Use Value::Set to store it.
    obj_set_key_value(mc, &iterator, &"__iterator_set__".into(), Value::Set(set))?;

    // Store index
    obj_set_key_value(mc, &iterator, &"__iterator_index__".into(), Value::Number(0.0))?;
    // Store kind
    obj_set_key_value(mc, &iterator, &"__iterator_kind__".into(), Value::String(utf8_to_utf16(kind)))?;

    // next method - shared native function name, handled in eval.rs
    obj_set_key_value(
        mc,
        &iterator,
        &"next".into(),
        Value::Function("SetIterator.prototype.next".to_string()),
    )?;

    // Register Symbols
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.iterator
        if let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
            && let Value::Symbol(s) = &*iter_sym.borrow()
        {
            obj_set_key_value(mc, &iterator, &PropertyKey::Symbol(*s), Value::Function("IteratorSelf".to_string()))?;
        }

        // Symbol.toStringTag
        if let Some(tag_sym) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(s) = &*tag_sym.borrow()
        {
            obj_set_key_value(
                mc,
                &iterator,
                &PropertyKey::Symbol(*s),
                Value::String(utf8_to_utf16("Set Iterator")),
            )?;
        }
    }

    Ok(Value::Object(iterator))
}

pub(crate) fn handle_set_iterator_next<'gc>(
    mc: &MutationContext<'gc>,
    iterator: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Get set
    let set_val = object_get_key_value(iterator, "__iterator_set__").ok_or(raise_eval_error!("Iterator has no set"))?;
    let set_ptr = if let Value::Set(s) = &*set_val.borrow() {
        *s
    } else {
        return Err(raise_eval_error!("Iterator set is invalid"));
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

    let values = &set_ptr.borrow().values;

    if index >= values.len() {
        let result_obj = new_js_object_data(mc);
        obj_set_key_value(mc, &result_obj, &"value".into(), Value::Undefined)?;
        obj_set_key_value(mc, &result_obj, &"done".into(), Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    }

    let value = &values[index];
    let result_value = match kind.as_str() {
        "values" => value.clone(),
        "entries" => {
            let entry_array = create_array(mc, env)?;
            obj_set_key_value(mc, &entry_array, &"0".into(), value.clone())?;
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
