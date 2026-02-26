use crate::core::{Gc, InternalSlot, MutationContext, SymbolData, slot_get_chained, slot_set};
use crate::core::{JSObjectDataPtr, PropertyKey, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;

pub fn initialize_symbol<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let symbol_ctor = new_js_object_data(mc);

    slot_set(mc, &symbol_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &symbol_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Symbol")));

    // Symbol() is not a constructor (cannot new Symbol()), but a function. All good `__is_constructor` usually means it is callable as a class/function.
    // Spec says `new Symbol()` throws TypeError, but `Symbol()` works.
    // My engine's `__is_constructor` usually distinguishes between normal objects and functions.
    // I might need to handle `new Symbol()` check inside the handler.

    // Symbol.prototype
    let symbol_proto = new_js_object_data(mc);

    // Get Object.prototype
    if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(obj_proto) = &*proto_val.borrow()
    {
        symbol_proto.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    object_set_key_value(mc, &symbol_ctor, "prototype", &Value::Object(symbol_proto))?;
    object_set_key_value(mc, &symbol_proto, "constructor", &Value::Object(symbol_ctor))?;

    // Symbol.iterator
    let iterator_sym_data = Gc::new(mc, SymbolData::new(Some("Symbol.iterator")));
    let iterator_sym = Value::Symbol(iterator_sym_data);
    object_set_key_value(mc, &symbol_ctor, "iterator", &iterator_sym)?;

    // Symbol.asyncIterator
    let async_iterator_sym_data = Gc::new(mc, SymbolData::new(Some("Symbol.asyncIterator")));
    let async_iterator_sym = Value::Symbol(async_iterator_sym_data);
    object_set_key_value(mc, &symbol_ctor, "asyncIterator", &async_iterator_sym)?;

    // Symbol.toPrimitive
    let to_primitive_data = Gc::new(mc, SymbolData::new(Some("Symbol.toPrimitive")));
    let to_primitive_sym = Value::Symbol(to_primitive_data);
    object_set_key_value(mc, &symbol_ctor, "toPrimitive", &to_primitive_sym)?;

    // Symbol.toStringTag
    let to_string_tag_data = Gc::new(mc, SymbolData::new(Some("Symbol.toStringTag")));
    let to_string_tag_sym = Value::Symbol(to_string_tag_data);
    object_set_key_value(mc, &symbol_ctor, "toStringTag", &to_string_tag_sym)?;

    // Symbol.species
    let species_data = Gc::new(mc, SymbolData::new(Some("Symbol.species")));
    let species_sym = Value::Symbol(species_data);
    object_set_key_value(mc, &symbol_ctor, "species", &species_sym)?;

    // Symbol.match
    let match_data = Gc::new(mc, SymbolData::new(Some("Symbol.match")));
    let match_sym = Value::Symbol(match_data);
    object_set_key_value(mc, &symbol_ctor, "match", &match_sym)?;

    // Symbol.replace
    let replace_data = Gc::new(mc, SymbolData::new(Some("Symbol.replace")));
    let replace_sym = Value::Symbol(replace_data);
    object_set_key_value(mc, &symbol_ctor, "replace", &replace_sym)?;

    // Symbol.search
    let search_data = Gc::new(mc, SymbolData::new(Some("Symbol.search")));
    let search_sym = Value::Symbol(search_data);
    object_set_key_value(mc, &symbol_ctor, "search", &search_sym)?;

    // Symbol.split
    let split_data = Gc::new(mc, SymbolData::new(Some("Symbol.split")));
    let split_sym = Value::Symbol(split_data);
    object_set_key_value(mc, &symbol_ctor, "split", &split_sym)?;

    // Symbol.matchAll
    let match_all_data = Gc::new(mc, SymbolData::new(Some("Symbol.matchAll")));
    let match_all_sym = Value::Symbol(match_all_data);
    object_set_key_value(mc, &symbol_ctor, "matchAll", &match_all_sym)?;

    // Symbol.hasInstance
    let has_instance_data = Gc::new(mc, SymbolData::new(Some("Symbol.hasInstance")));
    let has_instance_sym = Value::Symbol(has_instance_data);
    object_set_key_value(mc, &symbol_ctor, "hasInstance", &has_instance_sym)?;

    // Symbol.unscopables
    let unscopables_data = Gc::new(mc, SymbolData::new(Some("Symbol.unscopables")));
    let unscopables_sym = Value::Symbol(unscopables_data);
    object_set_key_value(mc, &symbol_ctor, "unscopables", &unscopables_sym)?;

    // All well-known symbol properties on Symbol are non-writable, non-enumerable, non-configurable
    // per the ECMAScript spec (they are immutable values, not methods).
    for wk in &[
        "iterator",
        "asyncIterator",
        "toPrimitive",
        "toStringTag",
        "species",
        "match",
        "matchAll",
        "replace",
        "search",
        "split",
        "hasInstance",
        "unscopables",
    ] {
        symbol_ctor.borrow_mut(mc).set_non_enumerable(*wk);
        symbol_ctor.borrow_mut(mc).set_non_writable(*wk);
        symbol_ctor.borrow_mut(mc).set_non_configurable(*wk);
    }

    // toString method
    let val = Value::Function("Symbol.prototype.toString".to_string());
    object_set_key_value(mc, &symbol_proto, "toString", &val)?;
    symbol_proto.borrow_mut(mc).set_non_enumerable("toString");

    // valueOf method
    let val_of = Value::Function("Symbol.prototype.valueOf".to_string());
    object_set_key_value(mc, &symbol_proto, "valueOf", &val_of)?;
    symbol_proto.borrow_mut(mc).set_non_enumerable("valueOf");

    // description getter (accessor property)
    let desc_getter = Value::Function("Symbol.prototype.description.get".to_string());
    let desc_prop = Value::Property {
        value: None,
        getter: Some(Box::new(desc_getter)),
        setter: None,
    };
    object_set_key_value(mc, &symbol_proto, "description", &desc_prop)?;
    symbol_proto.borrow_mut(mc).set_non_enumerable("description");

    symbol_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Symbol.for and Symbol.keyFor (static) - register as functions on the constructor
    let for_fn = Value::Function("Symbol.for".to_string());
    object_set_key_value(mc, &symbol_ctor, "for", &for_fn)?;
    symbol_ctor.borrow_mut(mc).set_non_enumerable("for");

    let keyfor_fn = Value::Function("Symbol.keyFor".to_string());
    object_set_key_value(mc, &symbol_ctor, "keyFor", &keyfor_fn)?;
    symbol_ctor.borrow_mut(mc).set_non_enumerable("keyFor");

    // Create per-environment symbol registry object used by Symbol.for / Symbol.keyFor
    let registry_obj = new_js_object_data(mc);
    slot_set(mc, env, InternalSlot::SymbolRegistry, &Value::Object(registry_obj));

    // Set Symbol.prototype[@@toStringTag] = "Symbol"
    // The toStringTag symbol was just created above and stored on symbol_ctor
    if let Some(tag_sym_val) = object_get_key_value(&symbol_ctor, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        object_set_key_value(mc, &symbol_proto, *tag_sym, &Value::String(utf8_to_utf16("Symbol")))?;
        symbol_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(*tag_sym));
    }

    env_set(mc, env, "Symbol", &Value::Object(symbol_ctor))?;

    Ok(())
}

pub(crate) fn handle_symbol_call<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let description = if let Some(arg) = args.first() {
        match arg {
            Value::String(s) => Some(crate::unicode::utf16_to_utf8(s)),
            Value::Undefined => None,
            _ => Some(crate::core::value_to_string(arg)),
        }
    } else {
        None
    };

    let sym = Gc::new(mc, SymbolData::new(description.as_deref()));
    Ok(Value::Symbol(sym))
}

pub(crate) fn handle_symbol_tostring<'gc>(_mc: &MutationContext<'gc>, this_value: &Value<'gc>) -> Result<Value<'gc>, JSError> {
    let sym = match this_value {
        Value::Symbol(s) => *s,
        Value::Object(obj) => {
            if let Some(val) = slot_get_chained(obj, &InternalSlot::PrimitiveValue) {
                if let Value::Symbol(s) = &*val.borrow() {
                    *s
                } else {
                    return Err(crate::raise_type_error!(
                        "Symbol.prototype.toString called on incompatible receiver"
                    ));
                }
            } else {
                return Err(crate::raise_type_error!(
                    "Symbol.prototype.toString called on incompatible receiver"
                ));
            }
        }
        _ => {
            return Err(crate::raise_type_error!(
                "Symbol.prototype.toString called on incompatible receiver"
            ));
        }
    };

    let desc = sym.description().unwrap_or("");
    let s = if desc.is_empty() {
        "Symbol()".to_string()
    } else {
        format!("Symbol({desc})")
    };
    Ok(Value::String(utf8_to_utf16(&s)))
}

pub(crate) fn handle_symbol_valueof<'gc>(_mc: &MutationContext<'gc>, this_value: &Value<'gc>) -> Result<Value<'gc>, JSError> {
    match this_value {
        Value::Symbol(s) => Ok(Value::Symbol(*s)),
        Value::Object(obj) => {
            if let Some(val) = slot_get_chained(obj, &InternalSlot::PrimitiveValue)
                && let Value::Symbol(s) = &*val.borrow()
            {
                return Ok(Value::Symbol(*s));
            }
            Err(crate::raise_type_error!("Symbol.prototype.valueOf called on incompatible receiver"))
        }
        _ => Err(crate::raise_type_error!("Symbol.prototype.valueOf called on incompatible receiver")),
    }
}

pub(crate) fn handle_symbol_description_get<'gc>(_mc: &MutationContext<'gc>, this_value: &Value<'gc>) -> Result<Value<'gc>, JSError> {
    let sym = match this_value {
        Value::Symbol(s) => *s,
        Value::Object(obj) => {
            if let Some(val) = slot_get_chained(obj, &InternalSlot::PrimitiveValue)
                && let Value::Symbol(s) = &*val.borrow()
            {
                *s
            } else {
                return Err(crate::raise_type_error!(
                    "Symbol.prototype.description getter called on incompatible receiver"
                ));
            }
        }
        _ => {
            return Err(crate::raise_type_error!(
                "Symbol.prototype.description getter called on incompatible receiver"
            ));
        }
    };
    match sym.description() {
        Some(desc) => Ok(Value::String(utf8_to_utf16(desc))),
        None => Ok(Value::Undefined),
    }
}

pub(crate) fn handle_symbol_for<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if args.is_empty() {
        return Err(crate::raise_type_error!("Symbol.for requires one argument"));
    }

    let key = crate::core::value_to_string(&args[0]);

    // Retrieve or create registry object on the environment
    let registry_obj = match slot_get_chained(env, &InternalSlot::SymbolRegistry) {
        Some(val) => {
            if let Value::Object(obj) = &*val.borrow() {
                *obj
            } else {
                let obj = new_js_object_data(mc);
                slot_set(mc, env, InternalSlot::SymbolRegistry, &Value::Object(obj));
                obj
            }
        }
        None => {
            let obj = new_js_object_data(mc);
            slot_set(mc, env, InternalSlot::SymbolRegistry, &Value::Object(obj));
            obj
        }
    };

    // If an existing symbol is found for this key, return it
    if let Some(val) = object_get_key_value(&registry_obj, &key)
        && let Value::Symbol(s) = &*val.borrow()
    {
        return Ok(Value::Symbol(*s));
    }

    // Otherwise create and store a new symbol associated with the key
    let sym = Gc::new(mc, SymbolData::new(Some(&key)));
    object_set_key_value(mc, &registry_obj, &key, &Value::Symbol(sym))?;
    Ok(Value::Symbol(sym))
}

pub(crate) fn handle_symbol_keyfor<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if args.is_empty() {
        return Err(crate::raise_type_error!("Symbol.keyFor requires one argument"));
    }

    match &args[0] {
        Value::Symbol(s) => {
            // Lookup registry object and iterate properties to find matching symbol
            if let Some(val) = slot_get_chained(env, &InternalSlot::SymbolRegistry)
                && let Value::Object(obj) = &*val.borrow()
            {
                for (k, v) in &obj.borrow().properties {
                    if let Value::Symbol(s2) = &*v.borrow()
                        && Gc::ptr_eq(*s, *s2)
                    {
                        // Found the key; return it as a JS string
                        if let PropertyKey::String(utf8_key) = k {
                            return Ok(Value::String(crate::unicode::utf8_to_utf16(utf8_key)));
                        }
                    }
                }
            }
            Ok(Value::Undefined)
        }
        _ => Ok(Value::Undefined),
    }
}
