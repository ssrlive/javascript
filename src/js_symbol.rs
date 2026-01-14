use crate::core::obj_get_key_value;
use crate::core::{Gc, MutationContext, SymbolData};
use crate::core::{JSObjectDataPtr, PropertyKey, Value, env_set, new_js_object_data, obj_set_key_value};
use crate::error::JSError;
use crate::unicode::utf8_to_utf16;

pub fn initialize_symbol<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let symbol_ctor = new_js_object_data(mc);

    obj_set_key_value(mc, &symbol_ctor, &"__is_constructor".into(), Value::Boolean(true))?;
    obj_set_key_value(mc, &symbol_ctor, &"__native_ctor".into(), Value::String(utf8_to_utf16("Symbol")))?;

    // Symbol() is not a constructor (cannot new Symbol()), but a function. All good `__is_constructor` usually means it is callable as a class/function.
    // Spec says `new Symbol()` throws TypeError, but `Symbol()` works.
    // My engine's `__is_constructor` usually distinguishes between normal objects and functions.
    // I might need to handle `new Symbol()` check inside the handler.

    // Symbol.prototype
    let symbol_proto = new_js_object_data(mc);

    // Get Object.prototype
    if let Some(obj_val) = obj_get_key_value(env, &"Object".into())?
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = obj_get_key_value(obj_ctor, &"prototype".into())?
        && let Value::Object(obj_proto) = &*proto_val.borrow()
    {
        symbol_proto.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    obj_set_key_value(mc, &symbol_ctor, &"prototype".into(), Value::Object(symbol_proto))?;
    obj_set_key_value(mc, &symbol_proto, &"constructor".into(), Value::Object(symbol_ctor))?;

    // Symbol.iterator
    let iterator_sym_data = Gc::new(
        mc,
        SymbolData {
            description: Some("Symbol.iterator".to_string()),
        },
    );
    let iterator_sym = Value::Symbol(iterator_sym_data);
    obj_set_key_value(mc, &symbol_ctor, &"iterator".into(), iterator_sym)?;

    // Symbol.toPrimitive
    let to_primitive_data = Gc::new(
        mc,
        SymbolData {
            description: Some("Symbol.toPrimitive".to_string()),
        },
    );
    let to_primitive_sym = Value::Symbol(to_primitive_data);
    obj_set_key_value(mc, &symbol_ctor, &"toPrimitive".into(), to_primitive_sym)?;

    // Symbol.toStringTag
    let to_string_tag_data = Gc::new(
        mc,
        SymbolData {
            description: Some("Symbol.toStringTag".to_string()),
        },
    );
    let to_string_tag_sym = Value::Symbol(to_string_tag_data);
    obj_set_key_value(mc, &symbol_ctor, &"toStringTag".into(), to_string_tag_sym)?;

    // toString method
    let val = Value::Function("Symbol.prototype.toString".to_string());
    obj_set_key_value(mc, &symbol_proto, &"toString".into(), val)?;
    symbol_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("toString"));

    // valueOf method
    let val_of = Value::Function("Symbol.prototype.valueOf".to_string());
    obj_set_key_value(mc, &symbol_proto, &"valueOf".into(), val_of)?;
    symbol_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("valueOf"));

    symbol_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::from("constructor"));

    // Symbol.for and Symbol.keyFor (static) - register as functions on the constructor
    let for_fn = Value::Function("Symbol.for".to_string());
    obj_set_key_value(mc, &symbol_ctor, &"for".into(), for_fn)?;
    symbol_ctor.borrow_mut(mc).set_non_enumerable(PropertyKey::from("for"));

    let keyfor_fn = Value::Function("Symbol.keyFor".to_string());
    obj_set_key_value(mc, &symbol_ctor, &"keyFor".into(), keyfor_fn)?;
    symbol_ctor.borrow_mut(mc).set_non_enumerable(PropertyKey::from("keyFor"));

    // Create per-environment symbol registry object used by Symbol.for / Symbol.keyFor
    let registry_obj = new_js_object_data(mc);
    obj_set_key_value(mc, env, &"__symbol_registry".into(), Value::Object(registry_obj))?;

    env_set(mc, env, "Symbol", Value::Object(symbol_ctor))?;

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

    let sym = Gc::new(mc, SymbolData { description });
    Ok(Value::Symbol(sym))
}

pub(crate) fn handle_symbol_tostring<'gc>(_mc: &MutationContext<'gc>, this_value: Value<'gc>) -> Result<Value<'gc>, JSError> {
    let sym = match this_value {
        Value::Symbol(s) => s,
        Value::Object(obj) => {
            if let Some(val) = obj_get_key_value(&obj, &"__value__".into())? {
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

    let desc = sym.description.as_deref().unwrap_or("");
    let s = if desc.is_empty() {
        "Symbol()".to_string()
    } else {
        format!("Symbol({desc})")
    };
    Ok(Value::String(utf8_to_utf16(&s)))
}

pub(crate) fn handle_symbol_valueof<'gc>(_mc: &MutationContext<'gc>, this_value: Value<'gc>) -> Result<Value<'gc>, JSError> {
    match this_value {
        Value::Symbol(s) => Ok(Value::Symbol(s)),
        Value::Object(obj) => {
            if let Some(val) = obj_get_key_value(&obj, &"__value__".into())?
                && let Value::Symbol(s) = &*val.borrow()
            {
                return Ok(Value::Symbol(*s));
            }
            Err(crate::raise_type_error!("Symbol.prototype.valueOf called on incompatible receiver"))
        }
        _ => Err(crate::raise_type_error!("Symbol.prototype.valueOf called on incompatible receiver")),
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
    let registry_obj = match obj_get_key_value(env, &"__symbol_registry".into())? {
        Some(val) => {
            if let Value::Object(obj) = &*val.borrow() {
                *obj
            } else {
                let obj = new_js_object_data(mc);
                obj_set_key_value(mc, env, &"__symbol_registry".into(), Value::Object(obj))?;
                obj
            }
        }
        None => {
            let obj = new_js_object_data(mc);
            obj_set_key_value(mc, env, &"__symbol_registry".into(), Value::Object(obj))?;
            obj
        }
    };

    // If an existing symbol is found for this key, return it
    if let Ok(Some(val)) = obj_get_key_value(&registry_obj, &key.clone().into())
        && let Value::Symbol(s) = &*val.borrow()
    {
        return Ok(Value::Symbol(*s));
    }

    // Otherwise create and store a new symbol associated with the key
    let sym = Gc::new(
        mc,
        SymbolData {
            description: Some(key.clone()),
        },
    );
    obj_set_key_value(mc, &registry_obj, &key.into(), Value::Symbol(sym))?;
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
            if let Some(val) = obj_get_key_value(env, &"__symbol_registry".into())?
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
