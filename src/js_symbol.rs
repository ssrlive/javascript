use crate::core::{Gc, InternalSlot, MutationContext, SymbolData, slot_get_chained, slot_set};
use crate::core::{JSObjectDataPtr, PropertyKey, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;

pub fn initialize_symbol<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    parent_env: Option<&JSObjectDataPtr<'gc>>,
) -> Result<(), JSError> {
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

    // §20.4.2: Symbol.length = 0 (non-writable, non-enumerable, configurable)
    object_set_key_value(mc, &symbol_ctor, "length", &Value::Number(0.0))?;
    symbol_ctor.borrow_mut(mc).set_non_enumerable("length");
    symbol_ctor.borrow_mut(mc).set_non_writable("length");

    // §20.4.2: Symbol.name = "Symbol" (non-writable, non-enumerable, configurable)
    object_set_key_value(mc, &symbol_ctor, "name", &Value::String(utf8_to_utf16("Symbol")))?;
    symbol_ctor.borrow_mut(mc).set_non_enumerable("name");
    symbol_ctor.borrow_mut(mc).set_non_writable("name");

    // prototype descriptor: non-enumerable, non-writable, non-configurable
    symbol_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    symbol_ctor.borrow_mut(mc).set_non_writable("prototype");
    symbol_ctor.borrow_mut(mc).set_non_configurable("prototype");

    // §20.4.2: Symbol's [[Prototype]] is Function.prototype
    if let Some(func_val) = object_get_key_value(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(func_proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*func_proto_val.borrow()
    {
        symbol_ctor.borrow_mut(mc).prototype = Some(*func_proto);
    }

    // Helper: try to get a well-known symbol from the parent realm's Symbol constructor.
    // Per §6.1.5.1 well-known symbols are shared across all realms.
    let parent_sym_ctor: Option<crate::core::JSObjectDataPtr<'gc>> = parent_env.and_then(|penv| {
        object_get_key_value(penv, "Symbol").and_then(|v| if let Value::Object(obj) = &*v.borrow() { Some(*obj) } else { None })
    });

    macro_rules! wk_symbol {
        ($name:expr, $desc:expr) => {{
            if let Some(ref psc) = parent_sym_ctor {
                if let Some(val) = object_get_key_value(psc, $name) {
                    if let Value::Symbol(_) = &*val.borrow() {
                        val.borrow().clone()
                    } else {
                        Value::Symbol(Gc::new(mc, SymbolData::new(Some($desc))))
                    }
                } else {
                    Value::Symbol(Gc::new(mc, SymbolData::new(Some($desc))))
                }
            } else {
                Value::Symbol(Gc::new(mc, SymbolData::new(Some($desc))))
            }
        }};
    }

    // Symbol.iterator
    let iterator_sym = wk_symbol!("iterator", "Symbol.iterator");
    object_set_key_value(mc, &symbol_ctor, "iterator", &iterator_sym)?;

    // Symbol.asyncIterator
    let async_iterator_sym = wk_symbol!("asyncIterator", "Symbol.asyncIterator");
    object_set_key_value(mc, &symbol_ctor, "asyncIterator", &async_iterator_sym)?;

    // Symbol.toPrimitive
    let to_primitive_sym = wk_symbol!("toPrimitive", "Symbol.toPrimitive");
    object_set_key_value(mc, &symbol_ctor, "toPrimitive", &to_primitive_sym)?;

    // Symbol.toStringTag
    let to_string_tag_sym = wk_symbol!("toStringTag", "Symbol.toStringTag");
    object_set_key_value(mc, &symbol_ctor, "toStringTag", &to_string_tag_sym)?;

    // Symbol.species
    let species_sym = wk_symbol!("species", "Symbol.species");
    object_set_key_value(mc, &symbol_ctor, "species", &species_sym)?;

    // Symbol.match
    let match_sym = wk_symbol!("match", "Symbol.match");
    object_set_key_value(mc, &symbol_ctor, "match", &match_sym)?;

    // Symbol.replace
    let replace_sym = wk_symbol!("replace", "Symbol.replace");
    object_set_key_value(mc, &symbol_ctor, "replace", &replace_sym)?;

    // Symbol.search
    let search_sym = wk_symbol!("search", "Symbol.search");
    object_set_key_value(mc, &symbol_ctor, "search", &search_sym)?;

    // Symbol.split
    let split_sym = wk_symbol!("split", "Symbol.split");
    object_set_key_value(mc, &symbol_ctor, "split", &split_sym)?;

    // Symbol.matchAll
    let match_all_sym = wk_symbol!("matchAll", "Symbol.matchAll");
    object_set_key_value(mc, &symbol_ctor, "matchAll", &match_all_sym)?;

    // Symbol.hasInstance
    let has_instance_sym = wk_symbol!("hasInstance", "Symbol.hasInstance");
    object_set_key_value(mc, &symbol_ctor, "hasInstance", &has_instance_sym)?;

    // Symbol.unscopables
    let unscopables_sym = wk_symbol!("unscopables", "Symbol.unscopables");
    object_set_key_value(mc, &symbol_ctor, "unscopables", &unscopables_sym)?;

    // Symbol.dispose
    let dispose_sym = wk_symbol!("dispose", "Symbol.dispose");
    object_set_key_value(mc, &symbol_ctor, "dispose", &dispose_sym)?;

    // Symbol.asyncDispose
    let async_dispose_sym = wk_symbol!("asyncDispose", "Symbol.asyncDispose");
    object_set_key_value(mc, &symbol_ctor, "asyncDispose", &async_dispose_sym)?;

    // Symbol.isConcatSpreadable
    let is_concat_spreadable_sym = wk_symbol!("isConcatSpreadable", "Symbol.isConcatSpreadable");
    object_set_key_value(mc, &symbol_ctor, "isConcatSpreadable", &is_concat_spreadable_sym)?;

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
        "dispose",
        "asyncDispose",
        "isConcatSpreadable",
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

    // Symbol.for and Symbol.keyFor (static) — wrap in distinct function objects
    // so each realm has its own identity (cross-realm tests require notSameValue).
    {
        use crate::core::new_gc_cell_ptr;
        let for_obj = new_js_object_data(mc);
        for_obj
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function("Symbol.for".to_string()))));
        // length = 1, name = "for" — {writable: false, enumerable: false, configurable: true}
        object_set_key_value(mc, &for_obj, "length", &Value::Number(1.0))?;
        for_obj.borrow_mut(mc).set_non_enumerable("length");
        for_obj.borrow_mut(mc).set_non_writable("length");
        object_set_key_value(mc, &for_obj, "name", &Value::String(crate::unicode::utf8_to_utf16("for")))?;
        for_obj.borrow_mut(mc).set_non_enumerable("name");
        for_obj.borrow_mut(mc).set_non_writable("name");
        object_set_key_value(mc, &symbol_ctor, "for", &Value::Object(for_obj))?;
        symbol_ctor.borrow_mut(mc).set_non_enumerable("for");
    }

    {
        use crate::core::new_gc_cell_ptr;
        let keyfor_obj = new_js_object_data(mc);
        keyfor_obj
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function("Symbol.keyFor".to_string()))));
        // length = 1, name = "keyFor" — {writable: false, enumerable: false, configurable: true}
        object_set_key_value(mc, &keyfor_obj, "length", &Value::Number(1.0))?;
        keyfor_obj.borrow_mut(mc).set_non_enumerable("length");
        keyfor_obj.borrow_mut(mc).set_non_writable("length");
        object_set_key_value(mc, &keyfor_obj, "name", &Value::String(crate::unicode::utf8_to_utf16("keyFor")))?;
        keyfor_obj.borrow_mut(mc).set_non_enumerable("name");
        keyfor_obj.borrow_mut(mc).set_non_writable("name");
        object_set_key_value(mc, &symbol_ctor, "keyFor", &Value::Object(keyfor_obj))?;
        symbol_ctor.borrow_mut(mc).set_non_enumerable("keyFor");
    }

    // Create per-environment symbol registry object used by Symbol.for / Symbol.keyFor
    let registry_obj = new_js_object_data(mc);
    slot_set(mc, env, InternalSlot::SymbolRegistry, &Value::Object(registry_obj));

    // Set Symbol.prototype[@@toStringTag] = "Symbol"
    // §20.4.3.6: { writable: false, enumerable: false, configurable: true }
    if let Some(tag_sym_val) = object_get_key_value(&symbol_ctor, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        object_set_key_value(mc, &symbol_proto, *tag_sym, &Value::String(utf8_to_utf16("Symbol")))?;
        symbol_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(*tag_sym));
        symbol_proto.borrow_mut(mc).set_non_writable(PropertyKey::Symbol(*tag_sym));
    }

    // §20.4.3.5 Symbol.prototype[@@toPrimitive](hint)
    // Returns the primitive Symbol value (thisSymbolValue(this value)).
    // Descriptor: { writable: false, enumerable: false, configurable: true }
    if let Some(tp_sym_val) = object_get_key_value(&symbol_ctor, "toPrimitive")
        && let Value::Symbol(tp_sym) = &*tp_sym_val.borrow()
    {
        let tp_fn = Value::Function("Symbol.prototype.[Symbol.toPrimitive]".to_string());
        object_set_key_value(mc, &symbol_proto, *tp_sym, &tp_fn)?;
        symbol_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(*tp_sym));
        symbol_proto.borrow_mut(mc).set_non_writable(PropertyKey::Symbol(*tp_sym));
        // configurable: true (default) — do NOT set non-configurable
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
            Value::Undefined => None,
            // §20.4.1.1 step 2: If description is not undefined, let descString be ? ToString(description).
            // ToString on a Symbol throws TypeError.
            Value::Symbol(_) => {
                return Err(crate::raise_type_error!("Cannot convert a Symbol value to a string"));
            }
            Value::String(s) => Some(crate::unicode::utf16_to_utf8(s)),
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
    // §20.4.2.2 Symbol.for(key): step 1 — let stringKey be ? ToString(key).
    // ToString on a Symbol throws TypeError. On undefined, produces "undefined".
    let arg = if args.is_empty() { &Value::Undefined } else { &args[0] };
    if let Value::Symbol(_) = arg {
        return Err(crate::raise_type_error!("Cannot convert a Symbol value to a string"));
    }
    let key = crate::core::value_to_string(arg);

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
    let sym = Gc::new(mc, SymbolData::new_registered(Some(&key)));
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

    // §20.4.2.6 step 1: If Type(sym) is not Symbol, throw a TypeError exception.
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
        _ => Err(crate::raise_type_error!(format!(
            "{} is not a symbol",
            crate::core::value_to_string(&args[0])
        ))),
    }
}
