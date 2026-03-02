use crate::core::js_error::EvalError;
use crate::core::{
    InternalSlot, JSObjectDataPtr, MutationContext, PropertyKey, Value, env_set, evaluate_call_dispatch, get_own_property, new_gc_cell_ptr,
    new_js_object_data, object_get_key_value, object_set_key_value, slot_get, slot_get_chained, slot_set, to_number_with_env, to_primitive,
    value_to_string,
};
use crate::error::JSError;
use crate::js_array::{create_array, set_array_length};
use crate::js_regexp::{
    get_or_compile_regex, handle_regexp_constructor_with_env, handle_regexp_method, internal_get_regex_pattern, is_regex_object,
};
use crate::unicode::{
    utf8_to_utf16, utf16_char_at, utf16_find, utf16_len, utf16_replace, utf16_rfind, utf16_slice, utf16_to_lowercase, utf16_to_uppercase,
    utf16_to_utf8,
};
use std::collections::BTreeMap;

/// Get a well-known symbol method from an object (e.g., @@match, @@search, @@replace, @@split).
/// Returns `Ok(Some(method))` if the symbol property exists and is callable,
/// `Ok(None)` if it's undefined/null, or an error if e.g. a getter throws.
pub fn get_well_known_symbol_method<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &crate::core::JSObjectDataPtr<'gc>,
    symbol_name: &str,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    // Resolve the well-known symbol (e.g., "match" → Symbol.match)
    if let Some(sym_ctor_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_ctor_val.borrow()
        && let Some(sym_val) = object_get_key_value(sym_ctor, symbol_name)
        && let Value::Symbol(sym) = &*sym_val.borrow()
    {
        // Get the property using accessor-aware read (triggers getters)
        let method = crate::core::get_property_with_accessors(mc, env, obj, PropertyKey::Symbol(*sym))?;
        match method {
            Value::Undefined | Value::Null => Ok(None),
            _ => Ok(Some(method)),
        }
    } else {
        Ok(None)
    }
}

/// IsRegExp abstract operation (ES 2024 §7.2.8)
/// Returns true if the argument is an object with a truthy @@match property,
/// or is a RegExp instance.
fn is_regexp_like<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, val: &Value<'gc>) -> Result<bool, EvalError<'gc>> {
    if let Value::Object(obj) = val {
        // First check @@match property
        if let Some(sym_ctor_val) = object_get_key_value(env, "Symbol")
            && let Value::Object(sym_ctor) = &*sym_ctor_val.borrow()
            && let Some(sym_val) = object_get_key_value(sym_ctor, "match")
            && let Value::Symbol(sym) = &*sym_val.borrow()
        {
            let matcher = crate::core::get_property_with_accessors(mc, env, obj, PropertyKey::Symbol(*sym))?;
            if !matches!(matcher, Value::Undefined) {
                // Coerce to boolean — any truthy value means it's regexp-like
                let is_truthy = match &matcher {
                    Value::Boolean(b) => *b,
                    Value::Null => false,
                    Value::Number(n) => *n != 0.0 && !n.is_nan(),
                    Value::String(s) => !s.is_empty(),
                    _ => true,
                };
                return Ok(is_truthy);
            }
        }
        // Fallback: check if it's a regex object
        Ok(is_regex_object(obj))
    } else {
        Ok(false)
    }
}

/// Spec ToIntegerOrZero: ToNumber, then truncate. NaN/±0→0, ±∞→±∞, else trunc
fn spec_to_integer_or_zero<'gc>(mc: &MutationContext<'gc>, val: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<f64, EvalError<'gc>> {
    let n = to_number_with_env(mc, env, val)?;
    if n.is_nan() || n == 0.0 {
        Ok(0.0)
    } else if !n.is_finite() {
        Ok(n) // ±∞
    } else {
        Ok(n.trunc())
    }
}

/// ES spec whitespace check: includes all WhiteSpace + LineTerminator code points
fn is_es_whitespace(ch: u16) -> bool {
    matches!(
        ch,
        0x0009  // TAB
        | 0x000A // LF
        | 0x000B // VT
        | 0x000C // FF
        | 0x000D // CR
        | 0x0020 // SPACE
        | 0x00A0 // NO-BREAK SPACE
        | 0x1680 // OGHAM SPACE MARK
        | 0x2000
            ..=0x200A // EN QUAD..HAIR SPACE
        | 0x2028 // LINE SEPARATOR
        | 0x2029 // PARAGRAPH SEPARATOR
        | 0x202F // NARROW NO-BREAK SPACE
        | 0x205F // MEDIUM MATHEMATICAL SPACE
        | 0x3000 // IDEOGRAPHIC SPACE
        | 0xFEFF // BOM / ZWNBSP
    )
}

/// Trim leading and trailing ES whitespace from UTF-16 string
fn es_trim(s: &[u16]) -> Vec<u16> {
    let start = s.iter().position(|&c| !is_es_whitespace(c)).unwrap_or(s.len());
    let end = s.iter().rposition(|&c| !is_es_whitespace(c)).map(|i| i + 1).unwrap_or(start);
    s[start..end].to_vec()
}

/// Trim leading ES whitespace
fn es_trim_start(s: &[u16]) -> Vec<u16> {
    let start = s.iter().position(|&c| !is_es_whitespace(c)).unwrap_or(s.len());
    s[start..].to_vec()
}

/// Trim trailing ES whitespace
fn es_trim_end(s: &[u16]) -> Vec<u16> {
    let end = s.iter().rposition(|&c| !is_es_whitespace(c)).map(|i| i + 1).unwrap_or(0);
    s[..end].to_vec()
}

pub fn initialize_string<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let string_ctor = new_js_object_data(mc);
    slot_set(mc, &string_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    object_set_key_value(mc, &string_ctor, "name", &Value::String(utf8_to_utf16("String")))?;

    // Mark as native constructor so it can be called as a function (String(...))
    slot_set(mc, &string_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("String")));
    // Hide internal flags/prototype from enumeration
    string_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    string_ctor.borrow_mut(mc).set_non_writable("prototype");
    string_ctor.borrow_mut(mc).set_non_configurable("prototype");

    // String.length = 1 (non-writable, non-enumerable, non-configurable)
    object_set_key_value(mc, &string_ctor, "length", &Value::Number(1.0))?;
    string_ctor.borrow_mut(mc).set_non_enumerable("length");
    string_ctor.borrow_mut(mc).set_non_writable("length");

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

    let string_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        string_proto.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(mc, &string_ctor, "prototype", &Value::Object(string_proto))?;
    object_set_key_value(mc, &string_proto, "constructor", &Value::Object(string_ctor))?;

    // Register Symbol.iterator
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
    {
        if let Some(iter_sym_val) = object_get_key_value(sym_ctor, "iterator")
            && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
        {
            let val = Value::Function("String.prototype.[Symbol.iterator]".to_string());
            object_set_key_value(mc, &string_proto, iter_sym, &val)?;
            string_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(*iter_sym));
        }

        // Symbol.toStringTag default for String.prototype
        if let Some(tag_sym_val) = object_get_key_value(sym_ctor, "toStringTag")
            && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
        {
            let val = Value::String(utf8_to_utf16("String"));
            object_set_key_value(mc, &string_proto, tag_sym, &val)?;
            string_proto.borrow_mut(mc).set_non_enumerable(PropertyKey::Symbol(*tag_sym));
        }
    }

    object_set_key_value(
        mc,
        &string_ctor,
        "fromCharCode",
        &Value::Function("String.fromCharCode".to_string()),
    )?;
    string_ctor.borrow_mut(mc).set_non_enumerable("fromCharCode");
    object_set_key_value(
        mc,
        &string_ctor,
        "fromCodePoint",
        &Value::Function("String.fromCodePoint".to_string()),
    )?;
    string_ctor.borrow_mut(mc).set_non_enumerable("fromCodePoint");
    object_set_key_value(mc, &string_ctor, "raw", &Value::Function("String.raw".to_string()))?;
    string_ctor.borrow_mut(mc).set_non_enumerable("raw");

    // Register instance methods with correct .length
    let methods_len1 = vec![
        "charAt",
        "charCodeAt",
        "codePointAt",
        "concat",
        "endsWith",
        "indexOf",
        "lastIndexOf",
        "localeCompare",
        "match",
        "matchAll",
        "padEnd",
        "padStart",
        "repeat",
        "search",
        "startsWith",
        "at",
        "includes",
    ];
    let methods_len2 = vec!["replace", "replaceAll", "slice", "split", "substring", "substr"];
    let methods_len0 = vec![
        "toUpperCase",
        "toLowerCase",
        "toLocaleLowerCase",
        "toLocaleUpperCase",
        "trim",
        "trimEnd",
        "trimStart",
        "toWellFormed",
        "isWellFormed",
    ];
    // normalize has length 0 per spec
    let methods_normalize = vec!["normalize"];

    // toString and valueOf must be wrapped in Object with OriginGlobal so that
    // cross-realm identity and realm-specific TypeError throwing work correctly.
    for method in &["toString", "valueOf"] {
        let fn_obj = new_js_object_data(mc);
        fn_obj
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function(format!("String.prototype.{method}")))));
        slot_set(mc, &fn_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
        // Set Function.prototype as the internal prototype so the object is callable
        if let Some(func_val_rc) = object_get_key_value(env, "Function")
            && let Value::Object(func_ctor) = &*func_val_rc.borrow()
            && let Some(func_proto_rc) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*func_proto_rc.borrow()
        {
            fn_obj.borrow_mut(mc).prototype = Some(*func_proto);
        }
        let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "length", &len_desc)?;
        let name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16(method)), false, false, true)?;
        crate::js_object::define_property_internal(mc, &fn_obj, "name", &name_desc)?;
        object_set_key_value(mc, &string_proto, *method, &Value::Object(fn_obj))?;
        string_proto.borrow_mut(mc).set_non_enumerable(*method);
    }
    for method in &methods_len0 {
        object_set_key_value(mc, &string_proto, *method, &Value::Function(format!("String.prototype.{method}")))?;
        string_proto.borrow_mut(mc).set_non_enumerable(*method);
    }
    for method in &methods_len1 {
        object_set_key_value(mc, &string_proto, *method, &Value::Function(format!("String.prototype.{method}")))?;
        string_proto.borrow_mut(mc).set_non_enumerable(*method);
    }
    for method in &methods_len2 {
        object_set_key_value(mc, &string_proto, *method, &Value::Function(format!("String.prototype.{method}")))?;
        string_proto.borrow_mut(mc).set_non_enumerable(*method);
    }
    for method in &methods_normalize {
        object_set_key_value(mc, &string_proto, *method, &Value::Function(format!("String.prototype.{method}")))?;
        string_proto.borrow_mut(mc).set_non_enumerable(*method);
    }

    // AnnexB: trimLeft === trimStart, trimRight === trimEnd (reference identity)
    if let Some(trim_start_val) = object_get_key_value(&string_proto, "trimStart") {
        let v = trim_start_val.borrow().clone();
        object_set_key_value(mc, &string_proto, "trimLeft", &v)?;
        string_proto.borrow_mut(mc).set_non_enumerable("trimLeft");
    }
    if let Some(trim_end_val) = object_get_key_value(&string_proto, "trimEnd") {
        let v = trim_end_val.borrow().clone();
        object_set_key_value(mc, &string_proto, "trimRight", &v)?;
        string_proto.borrow_mut(mc).set_non_enumerable("trimRight");
    }

    // AnnexB HTML string wrapper methods (B.2.3.2 – B.2.3.14)
    // Simple wrappers (length 0)
    for method in ["big", "blink", "bold", "fixed", "italics", "small", "strike", "sub", "sup"] {
        object_set_key_value(mc, &string_proto, method, &Value::Function(format!("String.prototype.{method}")))?;
        string_proto.borrow_mut(mc).set_non_enumerable(method);
    }
    // Attribute wrappers (length 1)
    for method in ["anchor", "fontcolor", "fontsize", "link"] {
        object_set_key_value(mc, &string_proto, method, &Value::Function(format!("String.prototype.{method}")))?;
        string_proto.borrow_mut(mc).set_non_enumerable(method);
    }

    // Make constructor non-enumerable on the prototype
    string_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // String.prototype is a String exotic object with [[StringData]] = ""
    slot_set(mc, &string_proto, InternalSlot::PrimitiveValue, &Value::String(Vec::new()));

    // Ensure String.prototype.length exists and is a number (0)
    let proto_len_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, false)?;
    crate::js_object::define_property_internal(mc, &string_proto, "length", &proto_len_desc)?;

    env_set(mc, env, "String", &Value::Object(string_ctor))?;

    Ok(())
}

/// Create %StringIteratorPrototype%. Must be called AFTER %IteratorPrototype% is available.
pub fn initialize_string_iterator_prototype<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let str_iter_proto = new_js_object_data(mc);
    if let Some(iter_proto_val) = slot_get_chained(env, &InternalSlot::IteratorPrototype)
        && let Value::Object(iter_proto) = &*iter_proto_val.borrow()
    {
        str_iter_proto.borrow_mut(mc).prototype = Some(*iter_proto);
    }

    // next method (non-enumerable)
    object_set_key_value(
        mc,
        &str_iter_proto,
        "next",
        &Value::Function("StringIterator.prototype.next".to_string()),
    )?;
    str_iter_proto.borrow_mut(mc).set_non_enumerable("next");

    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
    {
        // Symbol.toStringTag = "String Iterator" (non-writable, non-enumerable, configurable)
        if let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
            && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
        {
            let tag_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("String Iterator")), false, false, true)?;
            crate::js_object::define_property_internal(mc, &str_iter_proto, PropertyKey::Symbol(*tag_sym), &tag_desc)?;
        }
    }

    slot_set(mc, env, InternalSlot::StringIteratorPrototype, &Value::Object(str_iter_proto));

    Ok(())
}

pub(crate) fn string_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // String() constructor
    if args.len() != 1 {
        return Ok(Value::String(Vec::new())); // String() with no args returns empty string
    }
    let arg_val = args.first().unwrap();
    match arg_val {
        Value::Number(n) => Ok(Value::String(utf8_to_utf16(&crate::core::value_to_string(&Value::Number(*n))))),
        Value::String(s) => Ok(Value::String(s.clone())),
        Value::Boolean(b) => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
        Value::Undefined => Ok(Value::String(utf8_to_utf16("undefined"))),
        Value::Null => Ok(Value::String(utf8_to_utf16("null"))),
        Value::Object(_) => {
            // Attempt ToPrimitive with 'string' hint first (honor [Symbol.toPrimitive] or fallback)
            let prim = to_primitive(mc, arg_val, "string", env)?;
            // Convert the resulting primitive to a string
            Ok(Value::String(spec_to_string(mc, &prim, env)?))
        }
        Value::Function(name) => Ok(Value::String(utf8_to_utf16(&format!("[Function: {name}]")))),
        Value::Closure(_) => Ok(Value::String(utf8_to_utf16("[Function]"))),
        Value::AsyncClosure(_) => Ok(Value::String(utf8_to_utf16("[AsyncFunction]"))),
        Value::ClassDefinition(_) => Ok(Value::String(utf8_to_utf16("[Class]"))),
        Value::Getter(..) => Ok(Value::String(utf8_to_utf16("[Getter]"))),
        Value::Setter(..) => Ok(Value::String(utf8_to_utf16("[Setter]"))),
        Value::Property { .. } => Ok(Value::String(utf8_to_utf16("[property]"))),
        Value::Promise(_) => Ok(Value::String(utf8_to_utf16("[object Promise]"))),
        Value::Symbol(symbol_data) => {
            let desc = symbol_data.description().unwrap_or("");
            Ok(Value::String(utf8_to_utf16(&format!("Symbol({desc})"))))
        }
        Value::BigInt(h) => Ok(Value::String(utf8_to_utf16(&h.to_string()))),
        Value::Map(_) => Ok(Value::String(utf8_to_utf16("[object Map]"))),
        Value::Set(_) => Ok(Value::String(utf8_to_utf16("[object Set]"))),
        Value::WeakMap(_) => Ok(Value::String(utf8_to_utf16("[object WeakMap]"))),
        Value::WeakSet(_) => Ok(Value::String(utf8_to_utf16("[object WeakSet]"))),
        Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..) => Ok(Value::String(utf8_to_utf16("[GeneratorFunction]"))),
        Value::Generator(_) | Value::AsyncGenerator(_) => Ok(Value::String(utf8_to_utf16("[object Generator]"))),
        Value::Proxy(_) => Ok(Value::String(utf8_to_utf16("[object Proxy]"))),
        Value::ArrayBuffer(_) => Ok(Value::String(utf8_to_utf16("[object ArrayBuffer]"))),
        Value::DataView(_) => Ok(Value::String(utf8_to_utf16("[object DataView]"))),
        Value::TypedArray(_) => Ok(Value::String(utf8_to_utf16("[object TypedArray]"))),
        Value::Uninitialized => Ok(Value::String(utf8_to_utf16("undefined"))),
        Value::PrivateName(n, _) => Ok(Value::String(utf8_to_utf16(&format!("#{}", n)))),
    }
}

pub fn handle_string_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "[Symbol.iterator]" => create_string_iterator(mc, s, env),
        "toString" | "valueOf" => Ok(Value::String(s.to_vec())),
        "substring" => string_substring_method(mc, s, args, env),
        "substr" => string_substr_method(mc, s, args, env),
        "slice" => string_slice_method(mc, s, args, env),
        "toUpperCase" | "toLocaleUpperCase" => Ok(Value::String(utf16_to_uppercase(s))),
        "toLowerCase" | "toLocaleLowerCase" => Ok(Value::String(utf16_to_lowercase(s))),
        "indexOf" => string_indexof_method(mc, s, args, env),
        "lastIndexOf" => string_lastindexof_method(mc, s, args, env),
        "replace" => string_replace_method(mc, s, args, env),
        "split" => string_split_method(mc, s, args, env),
        "match" => string_match_method(mc, s, args, env),
        "charAt" => string_charat_method(mc, s, args, env),
        "charCodeAt" => string_char_code_at_method(mc, s, args, env),
        "trim" => Ok(Value::String(es_trim(s))),
        "trimEnd" => Ok(Value::String(es_trim_end(s))),
        "trimStart" => Ok(Value::String(es_trim_start(s))),
        "startsWith" => string_starts_with_method(mc, s, args, env),
        "endsWith" => string_ends_with_method(mc, s, args, env),
        "includes" => string_includes_method(mc, s, args, env),
        "repeat" => string_repeat_method(mc, s, args, env),
        "concat" => string_concat_method(mc, s, args, env),
        "padStart" => string_pad_start_method(mc, s, args, env),
        "padEnd" => string_pad_end_method(mc, s, args, env),
        "at" => string_at_method(mc, s, args, env),
        "codePointAt" => string_code_point_at_method(mc, s, args, env),
        "search" => string_search_method(mc, s, args, env),
        "matchAll" => string_match_all_method(mc, s, args, env),
        "normalize" => string_normalize_method(mc, s, args, env),
        "toWellFormed" => string_to_well_formed_method(mc, s, args, env),
        "isWellFormed" => string_is_well_formed_method(mc, s, args, env),
        "replaceAll" => string_replace_all_method(mc, s, args, env),
        "localeCompare" => string_locale_compare_method(mc, s, args, env),
        // AnnexB aliases
        "trimLeft" => Ok(Value::String(es_trim_start(s))),
        "trimRight" => Ok(Value::String(es_trim_end(s))),
        // AnnexB HTML wrapper methods
        "anchor" => string_html_wrapper(mc, s, "a", Some(("name", args)), env),
        "big" => string_html_wrapper(mc, s, "big", None, env),
        "blink" => string_html_wrapper(mc, s, "blink", None, env),
        "bold" => string_html_wrapper(mc, s, "b", None, env),
        "fixed" => string_html_wrapper(mc, s, "tt", None, env),
        "fontcolor" => string_html_wrapper(mc, s, "font", Some(("color", args)), env),
        "fontsize" => string_html_wrapper(mc, s, "font", Some(("size", args)), env),
        "italics" => string_html_wrapper(mc, s, "i", None, env),
        "link" => string_html_wrapper(mc, s, "a", Some(("href", args)), env),
        "small" => string_html_wrapper(mc, s, "small", None, env),
        "strike" => string_html_wrapper(mc, s, "strike", None, env),
        "sub" => string_html_wrapper(mc, s, "sub", None, env),
        "sup" => string_html_wrapper(mc, s, "sup", None, env),
        _ => Err(raise_eval_error!(format!("Unknown string method: {method}")).into()),
    }
}

fn string_substring_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let len = utf16_len(s) as f64;
    let int_start = if args.is_empty() {
        0.0
    } else {
        spec_to_integer_or_zero(mc, &args[0], env)?
    };
    let int_end = if args.len() < 2 || matches!(args[1], Value::Undefined) {
        len
    } else {
        spec_to_integer_or_zero(mc, &args[1], env)?
    };
    let final_start = int_start.max(0.0).min(len);
    let final_end = int_end.max(0.0).min(len);
    let (from, to) = if final_start <= final_end {
        (final_start as usize, final_end as usize)
    } else {
        (final_end as usize, final_start as usize)
    };
    Ok(Value::String(utf16_slice(s, from, to)))
}

fn string_substr_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let size = utf16_len(s) as f64;
    let int_start = if args.is_empty() {
        0.0
    } else {
        spec_to_integer_or_zero(mc, &args[0], env)?
    };
    // Step 5: If intStart is -∞ or +∞, clamp accordingly
    let int_start = if int_start < 0.0 {
        (size + int_start).max(0.0)
    } else {
        int_start.min(size)
    };
    let length = if args.len() < 2 || matches!(args[1], Value::Undefined) {
        size - int_start
    } else {
        let n = spec_to_integer_or_zero(mc, &args[1], env)?;
        n.max(0.0).min(size - int_start)
    };
    if length <= 0.0 {
        return Ok(Value::String(Vec::new()));
    }
    let start_idx = int_start as usize;
    let end_idx = (int_start + length) as usize;
    Ok(Value::String(utf16_slice(s, start_idx, end_idx)))
}

fn string_slice_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let len = utf16_len(s) as f64;
    let int_start = if args.is_empty() {
        0.0
    } else {
        spec_to_integer_or_zero(mc, &args[0], env)?
    };
    let int_end = if args.len() < 2 || matches!(args[1], Value::Undefined) {
        len
    } else {
        spec_to_integer_or_zero(mc, &args[1], env)?
    };
    let from = if int_start < 0.0 {
        (len + int_start).max(0.0)
    } else {
        int_start.min(len)
    };
    let to = if int_end < 0.0 {
        (len + int_end).max(0.0)
    } else {
        int_end.min(len)
    };
    if from >= to {
        Ok(Value::String(Vec::new()))
    } else {
        Ok(Value::String(utf16_slice(s, from as usize, to as usize)))
    }
}

fn string_indexof_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let search = if args.is_empty() {
        utf8_to_utf16("undefined")
    } else {
        spec_to_string(mc, &args[0], env)?
    };
    let from_index = if args.len() >= 2 {
        let n = spec_to_integer_or_zero(mc, &args[1], env)?;
        n.max(0.0) as usize
    } else {
        0
    };
    let len = utf16_len(s);
    if search.is_empty() {
        return Ok(Value::Number(from_index.min(len) as f64));
    }
    if from_index >= len {
        return Ok(Value::Number(-1.0));
    }
    if let Some(pos) = utf16_find(&s[from_index..], &search) {
        Ok(Value::Number((from_index + pos) as f64))
    } else {
        Ok(Value::Number(-1.0))
    }
}

fn string_lastindexof_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let search = if args.is_empty() {
        utf8_to_utf16("undefined")
    } else {
        spec_to_string(mc, &args[0], env)?
    };
    let len = utf16_len(s);
    let pos = if args.len() >= 2 {
        let n = to_number_with_env(mc, env, &args[1])?;
        if n.is_nan() {
            len // NaN → search from end
        } else {
            let i = if n == 0.0 {
                0.0
            } else if !n.is_finite() {
                if n > 0.0 { len as f64 } else { 0.0 }
            } else {
                n.trunc()
            };
            i.max(0.0).min(len as f64) as usize
        }
    } else {
        len
    };
    let search_len = utf16_len(&search);
    if search_len == 0 {
        return Ok(Value::Number(pos.min(len) as f64));
    }
    if search_len > len {
        return Ok(Value::Number(-1.0));
    }
    // Search backwards from min(pos + searchLen, len)
    let max_start = (pos + search_len).min(len);
    if let Some(found) = utf16_rfind(&s[..max_start], &search) {
        Ok(Value::Number(found as f64))
    } else {
        Ok(Value::Number(-1.0))
    }
}

// Standalone helper for expand_replacement tokens ($&, $1, $2, $`, $', $$, $<name>)
fn expand_replacement_tokens(
    repl: &str,
    matched: &[u16],
    captures: &[Option<Vec<u16>>],
    named_captures: Option<&BTreeMap<String, Option<Vec<u16>>>>,
    before: &[u16],
    after: &[u16],
) -> Vec<u16> {
    let mut out = String::new();
    let mut chars = repl.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' {
            if let Some(&next) = chars.peek() {
                match next {
                    '&' => {
                        chars.next();
                        out.push_str(&utf16_to_utf8(matched));
                    }
                    '`' => {
                        chars.next();
                        out.push_str(&utf16_to_utf8(before));
                    }
                    '\'' => {
                        chars.next();
                        out.push_str(&utf16_to_utf8(after));
                    }
                    '$' => {
                        chars.next();
                        out.push('$');
                    }
                    '<' => {
                        if named_captures.is_none() {
                            chars.next();
                            out.push('$');
                            out.push('<');
                        } else {
                            chars.next();
                            let mut name = String::new();
                            let mut closed = false;
                            while let Some(&c) = chars.peek() {
                                chars.next();
                                if c == '>' {
                                    closed = true;
                                    break;
                                }
                                name.push(c);
                            }
                            if closed {
                                if let Some(named_captures) = named_captures
                                    && let Some(Some(cap)) = named_captures.get(&name)
                                {
                                    out.push_str(&utf16_to_utf8(cap));
                                }
                            } else {
                                out.push('$');
                                out.push('<');
                                out.push_str(&name);
                            }
                        }
                    }
                    '0'..='9' => {
                        // Spec: $nn (two-digit), $n (single-digit)
                        let d1 = next;
                        chars.next();
                        let n1 = (d1 as u32 - '0' as u32) as usize;

                        if let Some(&d2 @ '0'..='9') = chars.peek() {
                            let nn = n1 * 10 + (d2 as u32 - '0' as u32) as usize;
                            if nn >= 1 && nn <= captures.len() {
                                chars.next();
                                if let Some(ref cap) = captures[nn - 1] {
                                    out.push_str(&utf16_to_utf8(cap));
                                }
                            } else if n1 >= 1 && n1 <= captures.len() {
                                if let Some(ref cap) = captures[n1 - 1] {
                                    out.push_str(&utf16_to_utf8(cap));
                                }
                            } else {
                                out.push('$');
                                out.push(d1);
                            }
                        } else if n1 >= 1 && n1 <= captures.len() {
                            if let Some(ref cap) = captures[n1 - 1] {
                                out.push_str(&utf16_to_utf8(cap));
                            }
                        } else {
                            out.push('$');
                            out.push(d1);
                        }
                    }
                    _ => {
                        out.push('$');
                    }
                }
            } else {
                out.push('$');
            }
        } else {
            out.push(ch);
        }
    }
    utf8_to_utf16(&out)
}

fn string_replace_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let search_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };
    let replace_val = if args.len() < 2 { Value::Undefined } else { args[1].clone() };

    // Step 1-2: If searchValue is not undefined/null, let replacer = GetMethod(searchValue, @@replace)
    if !matches!(search_val, Value::Undefined | Value::Null)
        && let Value::Object(obj) = &search_val
        && let Some(replacer) = get_well_known_symbol_method(mc, env, obj, "replace")?
    {
        return evaluate_call_dispatch(mc, env, &replacer, Some(&search_val), &[Value::String(s.to_vec()), replace_val]);
    }

    // Legacy path: searchValue has no @@replace
    if let Value::Object(obj) = &search_val
        && is_regex_object(obj)
    {
        // get flags
        let flags = match slot_get(obj, &InternalSlot::Flags) {
            Some(val) => match &*val.borrow() {
                Value::String(s) => utf16_to_utf8(s),
                _ => "".to_string(),
            },
            None => "".to_string(),
        };
        let global = flags.contains('g');

        // Extract pattern
        let pattern_u16 = internal_get_regex_pattern(obj)?;

        let re = get_or_compile_regex(&pattern_u16, &flags).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {e}")))?;

        if let Value::String(repl_u16) = replace_val {
            let repl = utf16_to_utf8(&repl_u16);
            let mut out: Vec<u16> = Vec::new();
            let mut last_pos = 0usize;

            // Only check for custom exec on non-global regex.
            // For global regex, we always use the manual matching loop below,
            // so the expensive get_property_with_accessors call is wasted.
            if !global {
                let exec_prop = crate::core::get_property_with_accessors(mc, env, obj, "exec")?;
                let has_custom_exec = !matches!(&exec_prop, Value::Function(name) if name == "RegExp.prototype.exec");

                if has_custom_exec {
                    let exec_res = evaluate_call_dispatch(mc, env, &exec_prop, Some(&Value::Object(*obj)), &[Value::String(s.to_vec())])?;

                    match exec_res {
                        Value::Null => return Ok(Value::String(s.to_vec())),
                        Value::Object(match_obj) => {
                            let matched_u16 = if let Some(v) = object_get_key_value(&match_obj, "0") {
                                match &*v.borrow() {
                                    Value::String(ms) => ms.clone(),
                                    other => utf8_to_utf16(&value_to_string(other)),
                                }
                            } else {
                                Vec::new()
                            };

                            let start = if let Some(v) = object_get_key_value(&match_obj, "index") {
                                if let Value::Number(n) = *v.borrow() {
                                    (n as isize).max(0) as usize
                                } else {
                                    0
                                }
                            } else {
                                0
                            };

                            let start = start.min(s.len());
                            let end = (start + matched_u16.len()).min(s.len());
                            let before = &s[..start];
                            let after = &s[end..];

                            let mut captures: Vec<Option<Vec<u16>>> = Vec::new();
                            let cap_len = if let Some(v) = object_get_key_value(&match_obj, "length") {
                                if let Value::Number(n) = *v.borrow() {
                                    (n as usize).saturating_sub(1)
                                } else {
                                    0
                                }
                            } else {
                                0
                            };
                            for idx in 1..=cap_len {
                                if let Some(v) = object_get_key_value(&match_obj, idx) {
                                    match &*v.borrow() {
                                        Value::String(cs) => captures.push(Some(cs.clone())),
                                        Value::Undefined => captures.push(None),
                                        other => captures.push(Some(utf8_to_utf16(&value_to_string(other)))),
                                    }
                                } else {
                                    captures.push(None);
                                }
                            }

                            let mut named_captures: BTreeMap<String, Option<Vec<u16>>> = BTreeMap::new();
                            if let Some(groups_rc) = object_get_key_value(&match_obj, "groups")
                                && let Value::Object(groups_obj) = &*groups_rc.borrow()
                            {
                                let mut cur = Some(*groups_obj);
                                while let Some(obj_ptr) = cur {
                                    let mut entries: Vec<(String, Value<'gc>)> = Vec::new();
                                    {
                                        let b = obj_ptr.borrow();
                                        for (k, v) in &b.properties {
                                            if let PropertyKey::String(name) = k {
                                                entries.push((name.clone(), v.borrow().clone()));
                                            }
                                        }
                                        cur = b.prototype;
                                    }
                                    for (name, v) in entries {
                                        if named_captures.contains_key(&name) {
                                            continue;
                                        }
                                        match v {
                                            Value::String(cs) => {
                                                named_captures.insert(name, Some(cs));
                                            }
                                            Value::Undefined => {
                                                named_captures.insert(name, None);
                                            }
                                            other => {
                                                named_captures.insert(name, Some(utf8_to_utf16(&value_to_string(&other))));
                                            }
                                        }
                                    }
                                }
                            }

                            let named_captures_opt = if named_captures.is_empty() { None } else { Some(&named_captures) };
                            let mut out = before.to_vec();
                            out.extend_from_slice(&expand_replacement_tokens(
                                &repl,
                                &matched_u16,
                                &captures,
                                named_captures_opt,
                                before,
                                after,
                            ));
                            out.extend_from_slice(after);
                            return Ok(Value::String(out));
                        }
                        _ => return Ok(Value::String(s.to_vec())),
                    }
                }
            } // end if !global

            let mut offset = 0usize;
            // regress doesn't have an iterator for matches that handles overlap/global automatically in a simple way?
            // It has `find_iter` but that might not handle `lastIndex` updates if we were doing that.
            // But here we just want all matches.
            // `re.find_iter` returns an iterator.

            // Wait, `find_iter` is for `&str`. `find_iter_utf16`?
            // regress 0.4.1 has `find_iter`. Does it support `&[u16]`?
            // `Regex::find_iter` takes `text`.
            // Let's check if `regress` has `find_iter` for utf16.
            // The README says `*_utf16` family.
            // I'll assume `find_iter_utf16` exists or I loop manually.
            // Manual loop is safer.

            while let Some(m) = re.find_from_utf16(s, offset).next() {
                let start = m.range.start;
                let end = m.range.end;

                // If global and zero-length match, we must advance by 1 to avoid infinite loop
                // But we must also include the zero-length match in replacement.

                let before = &s[..start];
                let after = &s[end..];
                let matched = &s[start..end];

                let mut captures = Vec::new();
                for cap in m.captures.iter() {
                    if let Some(range) = cap {
                        captures.push(Some(s[range.start..range.end].to_vec()));
                    } else {
                        captures.push(None);
                    }
                }

                let mut named_captures: BTreeMap<String, Option<Vec<u16>>> = BTreeMap::new();
                for (name, range_opt) in m.named_groups() {
                    let val = range_opt.map(|range| s[range.start..range.end].to_vec());
                    named_captures.insert(name.to_string(), val);
                }
                let named_captures_opt = if named_captures.is_empty() { None } else { Some(&named_captures) };

                out.extend_from_slice(&s[last_pos..start]);
                out.extend_from_slice(&expand_replacement_tokens(
                    &repl,
                    matched,
                    &captures,
                    named_captures_opt,
                    before,
                    after,
                ));
                last_pos = end;

                if !global {
                    break;
                }

                if start == end {
                    offset = end + 1;
                } else {
                    offset = end;
                }
                if offset > s.len() {
                    break;
                }
            }

            out.extend_from_slice(&s[last_pos..]);
            return Ok(Value::String(out));
        } else if matches!(
            replace_val,
            Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..)
        ) || matches!(&replace_val, Value::Object(o) if o.borrow().get_closure().is_some())
        {
            let mut out: Vec<u16> = Vec::new();
            let mut last_pos = 0usize;
            let mut offset = 0usize;

            while let Some(m) = re.find_from_utf16(s, offset).next() {
                let start = m.range.start;
                let end = m.range.end;

                let matched = Value::String(s[start..end].to_vec());
                let mut call_args: Vec<Value<'gc>> = vec![matched];

                for cap in m.captures.iter() {
                    if let Some(range) = cap {
                        call_args.push(Value::String(s[range.start..range.end].to_vec()));
                    } else {
                        call_args.push(Value::Undefined);
                    }
                }

                call_args.push(Value::Number(start as f64));
                call_args.push(Value::String(s.to_vec()));

                let mut named_captures: BTreeMap<String, Option<Vec<u16>>> = BTreeMap::new();
                for (name, range_opt) in m.named_groups() {
                    let val = range_opt.map(|range| s[range.start..range.end].to_vec());
                    named_captures.insert(name.to_string(), val);
                }

                if !named_captures.is_empty() {
                    let groups_obj = new_js_object_data(mc);
                    for (name, val_opt) in named_captures {
                        if let Some(v) = val_opt {
                            object_set_key_value(mc, &groups_obj, name.as_str(), &Value::String(v))?;
                        } else {
                            object_set_key_value(mc, &groups_obj, name.as_str(), &Value::Undefined)?;
                        }
                    }
                    call_args.push(Value::Object(groups_obj));
                }

                let repl_val = evaluate_call_dispatch(mc, env, &replace_val, Some(&Value::Undefined), &call_args)?;
                let repl_u16 = utf8_to_utf16(&value_to_string(&repl_val));

                out.extend_from_slice(&s[last_pos..start]);
                out.extend_from_slice(&repl_u16);
                last_pos = end;

                if !global {
                    break;
                }

                if start == end {
                    offset = end + 1;
                } else {
                    offset = end;
                }
                if offset > s.len() {
                    break;
                }
            }

            out.extend_from_slice(&s[last_pos..]);
            return Ok(Value::String(out));
        } else {
            // Non-callable replaceValue (undefined, null, objects, etc.) → coerce to string
            let repl_u16 = spec_to_string(mc, &replace_val, env)?;
            let repl = utf16_to_utf8(&repl_u16);
            let mut out: Vec<u16> = Vec::new();
            let mut last_pos = 0usize;
            let mut offset = 0usize;

            while let Some(m) = re.find_from_utf16(s, offset).next() {
                let start = m.range.start;
                let end = m.range.end;
                let before = &s[..start];
                let after = &s[end..];
                let matched = &s[start..end];

                let mut captures = Vec::new();
                for cap in m.captures.iter() {
                    if let Some(range) = cap {
                        captures.push(Some(s[range.start..range.end].to_vec()));
                    } else {
                        captures.push(None);
                    }
                }

                let mut named_captures: BTreeMap<String, Option<Vec<u16>>> = BTreeMap::new();
                for (name, range_opt) in m.named_groups() {
                    let val = range_opt.map(|range| s[range.start..range.end].to_vec());
                    named_captures.insert(name.to_string(), val);
                }
                let named_captures_opt = if named_captures.is_empty() { None } else { Some(&named_captures) };

                out.extend_from_slice(&s[last_pos..start]);
                out.extend_from_slice(&expand_replacement_tokens(
                    &repl,
                    matched,
                    &captures,
                    named_captures_opt,
                    before,
                    after,
                ));
                last_pos = end;

                if !global {
                    break;
                }

                if start == end {
                    offset = end + 1;
                } else {
                    offset = end;
                }
                if offset > s.len() {
                    break;
                }
            }

            out.extend_from_slice(&s[last_pos..]);
            return Ok(Value::String(out));
        }
    } // end Value::Object

    // String replacement path (for non-regex search values)
    let search = spec_to_string(mc, &search_val, env)?;
    let is_callable = matches!(
        &replace_val,
        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..)
    ) || matches!(&replace_val, Value::Object(o) if o.borrow().get_closure().is_some());
    if is_callable {
        if search.is_empty() {
            let matched = Value::String(Vec::new());
            let position = Value::Number(0.0);
            let input = Value::String(s.to_vec());
            let repl = evaluate_call_dispatch(mc, env, &replace_val, Some(&Value::Undefined), &[matched, position, input])?;
            let repl_s = value_to_string(&repl);
            let mut out = utf8_to_utf16(&repl_s);
            out.extend_from_slice(s);
            return Ok(Value::String(out));
        }

        if let Some(pos) = utf16_find(s, &search) {
            let before = &s[..pos];
            let after = &s[pos + search.len()..];
            let matched = Value::String(search.clone());
            let position = Value::Number(pos as f64);
            let input = Value::String(s.to_vec());
            let repl = evaluate_call_dispatch(mc, env, &replace_val, Some(&Value::Undefined), &[matched, position, input])?;
            let repl_s = value_to_string(&repl);

            let mut out = before.to_vec();
            out.extend_from_slice(&utf8_to_utf16(&repl_s));
            out.extend_from_slice(after);
            Ok(Value::String(out))
        } else {
            Ok(Value::String(s.to_vec()))
        }
    } else {
        // Coerce replacement to string
        let replace = spec_to_string(mc, &replace_val, env)?;
        Ok(Value::String(utf16_replace(s, &search, &replace)))
    }
}

fn string_split_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let sep_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };
    let limit_val = if args.len() >= 2 { args[1].clone() } else { Value::Undefined };

    // Step 2: If separator is not undefined/null, let splitter = GetMethod(separator, @@split)
    if !matches!(sep_val, Value::Undefined | Value::Null)
        && let Value::Object(obj) = &sep_val
        && let Some(splitter) = get_well_known_symbol_method(mc, env, obj, "split")?
    {
        return evaluate_call_dispatch(mc, env, &splitter, Some(&sep_val), &[Value::String(s.to_vec()), limit_val.clone()]);
    }

    let limit = if matches!(limit_val, Value::Undefined) {
        0xFFFFFFFF_usize // 2^32 - 1
    } else {
        crate::core::to_uint32_value_with_env(mc, env, &limit_val)? as usize
    };

    // If separator is a RegExp object, use regex split
    if let Value::Object(object) = &sep_val
        && is_regex_object(object)
    {
        if limit == 0 {
            let arr = create_array(mc, env)?;
            set_array_length(mc, &arr, 0)?;
            return Ok(Value::Object(arr));
        }
        let pattern_u16 = internal_get_regex_pattern(object)?;

        let flags_opt = slot_get(object, &InternalSlot::Flags);
        let flags = match flags_opt {
            Some(val_rc) => match &*val_rc.borrow() {
                Value::String(s) => utf16_to_utf8(s),
                _ => String::new(),
            },
            None => String::new(),
        };

        let re = get_or_compile_regex(&pattern_u16, &flags).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {e}")))?;

        let mut parts: Vec<Value> = Vec::new();
        let mut start = 0usize;
        let mut offset = 0usize;

        // @@split-compatible loop: `offset` is `q`, `start` is `p`
        // Loop while q < size (spec uses strict <)
        loop {
            if parts.len() >= limit || offset >= s.len() {
                break;
            }

            match re.find_from_utf16(s, offset).next() {
                Some(m) => {
                    let match_start = m.range.start;
                    let match_end = m.range.end;

                    // Zero-length match NOT at current offset → sticky skip
                    if match_start == match_end && match_start != offset {
                        offset += 1;
                        continue;
                    }

                    // Zero-length match at p (e == p) → advance q
                    if match_end == start {
                        offset += 1;
                        continue;
                    }

                    // Push T = S[p..q] where q = match_start
                    parts.push(Value::String(s[start..match_start].to_vec()));
                    if parts.len() >= limit {
                        break;
                    }

                    // Capturing groups
                    for cap in m.captures.iter() {
                        if let Some(range) = cap {
                            parts.push(Value::String(s[range.start..range.end].to_vec()));
                        } else {
                            parts.push(Value::Undefined);
                        }
                        if parts.len() >= limit {
                            break;
                        }
                    }

                    start = match_end;
                    offset = match_end;
                }
                None => {
                    // No match at all — advance offset by 1
                    offset += 1;
                }
            }
        }

        // Push remaining: T = S[p..size]
        if parts.len() < limit {
            parts.push(Value::String(s[start..].to_vec()));
        }

        let arr = create_array(mc, env)?;
        for (i, part) in parts.iter().enumerate() {
            object_set_key_value(mc, &arr, i, &part.clone())?;
        }
        set_array_length(mc, &arr, parts.len())?;
        return Ok(Value::Object(arr));
    } // end Value::Object

    // String split path (for non-regex separators)
    if matches!(sep_val, Value::Undefined) {
        if limit == 0 {
            let arr = create_array(mc, env)?;
            set_array_length(mc, &arr, 0)?;
            return Ok(Value::Object(arr));
        }
        let arr = create_array(mc, env)?;
        object_set_key_value(mc, &arr, 0, &Value::String(s.to_vec()))?;
        set_array_length(mc, &arr, 1)?;
        return Ok(Value::Object(arr));
    }
    // Step 7: ToString(separator) — may throw
    let sep = spec_to_string(mc, &sep_val, env)?;
    // Step 8: if limit == 0, return empty array
    if limit == 0 {
        let arr = create_array(mc, env)?;
        set_array_length(mc, &arr, 0)?;
        return Ok(Value::Object(arr));
    }
    let mut parts: Vec<Vec<u16>> = Vec::new();
    if sep.is_empty() {
        let len = utf16_len(s).min(limit);
        for i in 0..len {
            if let Some(ch) = utf16_char_at(s, i) {
                parts.push(vec![ch]);
            }
        }
    } else {
        let mut start = 0usize;
        while parts.len() < limit {
            if let Some(pos) = utf16_find(&s[start..], &sep) {
                parts.push(utf16_slice(s, start, start + pos));
                start += pos + utf16_len(&sep);
            } else {
                parts.push(utf16_slice(s, start, utf16_len(s)));
                break;
            }
        }
    }
    let arr = create_array(mc, env)?;
    for (i, part) in parts.iter().enumerate() {
        object_set_key_value(mc, &arr, i, &Value::String(part.clone()))?;
    }
    set_array_length(mc, &arr, parts.len())?;
    Ok(Value::Object(arr))
}

fn string_match_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // String.prototype.match(search)
    let search_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };

    // Step 2: If regexp is not undefined/null, let matcher = GetMethod(regexp, @@match)
    if !matches!(search_val, Value::Undefined | Value::Null)
        && let Value::Object(obj) = &search_val
        && let Some(matcher) = get_well_known_symbol_method(mc, env, obj, "match")?
    {
        // Call matcher with regexp as this, passing the string
        return evaluate_call_dispatch(mc, env, &matcher, Some(&search_val), &[Value::String(s.to_vec())]);
    }

    // Step 6: Let rx = RegExpCreate(regexp, undefined)
    // Step 8: Return Invoke(rx, @@match, «S»)
    let regexp_obj = if matches!(search_val, Value::Undefined) {
        match handle_regexp_constructor_with_env(mc, Some(env), &[])? {
            Value::Object(o) => o,
            _ => return Err(raise_eval_error!("failed to construct default RegExp").into()),
        }
    } else {
        let pattern = spec_to_string(mc, &search_val, env)?;
        match handle_regexp_constructor_with_env(mc, Some(env), &[Value::String(pattern)])? {
            Value::Object(o) => o,
            _ => return Err(raise_eval_error!("failed to construct RegExp from arg").into()),
        }
    };

    // Look up @@match on the newly created RegExp and invoke it
    if let Some(matcher) = get_well_known_symbol_method(mc, env, &regexp_obj, "match")? {
        return evaluate_call_dispatch(mc, env, &matcher, Some(&Value::Object(regexp_obj)), &[Value::String(s.to_vec())]);
    }

    // Fallback: manual match (should not normally reach here)
    let flags = match slot_get(&regexp_obj, &InternalSlot::Flags) {
        Some(val) => match &*val.borrow() {
            Value::String(s) => utf16_to_utf8(s),
            _ => String::new(),
        },
        None => String::new(),
    };

    let global = flags.contains('g');
    let exec_arg = Value::String(s.to_vec());
    let exec_args = vec![exec_arg.clone()];

    if global {
        let prev_last_index = get_own_property(&regexp_obj, "lastIndex");
        object_set_key_value(mc, &regexp_obj, "lastIndex", &Value::Number(0.0))?;

        let mut matches: Vec<String> = Vec::new();
        loop {
            match handle_regexp_method(mc, &regexp_obj, "exec", &exec_args, env)? {
                Value::Object(arr) => {
                    if let Some(val_rc) = object_get_key_value(&arr, "0") {
                        match &*val_rc.borrow() {
                            Value::String(u16s) => matches.push(utf16_to_utf8(u16s)),
                            _ => matches.push("".to_string()),
                        }
                    } else {
                        break;
                    }
                }
                Value::Null => break,
                _ => break,
            }
        }

        if let Some(val) = prev_last_index {
            object_set_key_value(mc, &regexp_obj, "lastIndex", &val.borrow().clone())?;
        } else {
            object_set_key_value(mc, &regexp_obj, "lastIndex", &Value::Number(0.0))?;
        }

        if matches.is_empty() {
            return Ok(Value::Null);
        }

        let arr = create_array(mc, env)?;
        for (i, m) in matches.iter().enumerate() {
            object_set_key_value(mc, &arr, i, &Value::String(utf8_to_utf16(m)))?;
        }
        set_array_length(mc, &arr, matches.len())?;
        Ok(Value::Object(arr))
    } else {
        let res = handle_regexp_method(mc, &regexp_obj, "exec", &exec_args, env)?;
        Ok(res)
    }
}

fn string_charat_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let pos = if args.is_empty() {
        0.0
    } else {
        spec_to_integer_or_zero(mc, &args[0], env)?
    };
    if pos < 0.0 || pos >= utf16_len(s) as f64 {
        Ok(Value::String(Vec::new()))
    } else if let Some(ch) = utf16_char_at(s, pos as usize) {
        Ok(Value::String(vec![ch]))
    } else {
        Ok(Value::String(Vec::new()))
    }
}

fn string_char_code_at_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let pos = if args.is_empty() {
        0.0
    } else {
        spec_to_integer_or_zero(mc, &args[0], env)?
    };
    if pos < 0.0 || pos >= s.len() as f64 {
        Ok(Value::Number(f64::NAN))
    } else {
        Ok(Value::Number(s[pos as usize] as f64))
    }
}

fn string_starts_with_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 4: If IsRegExp(searchString) is true, throw TypeError
    if let Some(arg) = args.first()
        && is_regexp_like(mc, env, arg)?
    {
        return Err(crate::raise_type_error!("First argument to String.prototype.startsWith must not be a regular expression").into());
    }
    let search = if args.is_empty() {
        utf8_to_utf16("undefined")
    } else {
        spec_to_string(mc, &args[0], env)?
    };
    let len = utf16_len(s);
    let pos = if args.len() >= 2 && !matches!(args[1], Value::Undefined) {
        let p = spec_to_integer_or_zero(mc, &args[1], env)?;
        p.max(0.0).min(len as f64) as usize
    } else {
        0
    };
    let search_len = utf16_len(&search);
    if pos + search_len > len {
        Ok(Value::Boolean(false))
    } else {
        Ok(Value::Boolean(utf16_slice(s, pos, pos + search_len) == search))
    }
}

fn string_ends_with_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 4: If IsRegExp(searchString) is true, throw TypeError
    if let Some(arg) = args.first()
        && is_regexp_like(mc, env, arg)?
    {
        return Err(crate::raise_type_error!("First argument to String.prototype.endsWith must not be a regular expression").into());
    }
    let search = if args.is_empty() {
        utf8_to_utf16("undefined")
    } else {
        spec_to_string(mc, &args[0], env)?
    };
    let len = utf16_len(s);
    let end_pos = if args.len() >= 2 && !matches!(args[1], Value::Undefined) {
        let p = spec_to_integer_or_zero(mc, &args[1], env)?;
        p.max(0.0).min(len as f64) as usize
    } else {
        len
    };
    let search_len = utf16_len(&search);
    if search_len > end_pos {
        Ok(Value::Boolean(false))
    } else {
        let start = end_pos - search_len;
        Ok(Value::Boolean(utf16_slice(s, start, end_pos) == search))
    }
}

fn string_includes_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 4: If IsRegExp(searchString) is true, throw TypeError
    if let Some(arg) = args.first()
        && is_regexp_like(mc, env, arg)?
    {
        return Err(crate::raise_type_error!("First argument to String.prototype.includes must not be a regular expression").into());
    }
    let search = if args.is_empty() {
        utf8_to_utf16("undefined")
    } else {
        spec_to_string(mc, &args[0], env)?
    };
    let position = if args.len() >= 2 {
        let p = spec_to_integer_or_zero(mc, &args[1], env)?;
        p.max(0.0) as usize
    } else {
        0
    };
    if position >= s.len() && !search.is_empty() {
        return Ok(Value::Boolean(false));
    }
    let start = position.min(s.len());
    Ok(Value::Boolean(utf16_find(&s[start..], &search).is_some()))
}

fn string_repeat_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let n = if args.is_empty() {
        0.0
    } else {
        spec_to_integer_or_zero(mc, &args[0], env)?
    };
    if n < 0.0 || n == f64::INFINITY {
        return Err(crate::raise_range_error!("Invalid count value").into());
    }
    if s.is_empty() {
        return Ok(Value::String(Vec::new()));
    }
    let count = n as usize;
    let mut repeated = Vec::with_capacity(s.len() * count);
    for _ in 0..count {
        repeated.extend_from_slice(s);
    }
    Ok(Value::String(repeated))
}

fn string_concat_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut result = s.to_vec();
    for arg in args {
        let str_val = spec_to_string(mc, arg, env)?;
        result.extend(str_val);
    }
    Ok(Value::String(result))
}

/// Spec-compliant ToString: for objects, calls ToPrimitive(hint: "string") first,
/// then converts the resulting primitive to a string.
pub(crate) fn spec_to_string<'gc>(
    mc: &MutationContext<'gc>,
    val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Vec<u16>, EvalError<'gc>> {
    match val {
        Value::String(s) => Ok(s.clone()),
        Value::Number(_n) => Ok(utf8_to_utf16(&value_to_string(val))),
        Value::BigInt(b) => Ok(utf8_to_utf16(&b.to_string())),
        Value::Boolean(b) => Ok(utf8_to_utf16(&b.to_string())),
        Value::Undefined => Ok(utf8_to_utf16("undefined")),
        Value::Null => Ok(utf8_to_utf16("null")),
        Value::Symbol(_) => Err(crate::raise_type_error!("Cannot convert a Symbol value to a string").into()),
        Value::Object(_) => {
            let prim = to_primitive(mc, val, "string", env)?;
            spec_to_string(mc, &prim, env)
        }
        _ => Ok(utf8_to_utf16(&value_to_string(val))),
    }
}

fn string_pad_start_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let max_length = if args.is_empty() {
        0.0
    } else {
        spec_to_integer_or_zero(mc, &args[0], env)?
    };
    let string_length = utf16_len(s);
    if max_length as usize <= string_length {
        return Ok(Value::String(s.to_vec()));
    }
    let fill_string = if args.len() >= 2 && !matches!(args[1], Value::Undefined) {
        spec_to_string(mc, &args[1], env)?
    } else {
        vec![0x0020] // space
    };
    if fill_string.is_empty() {
        return Ok(Value::String(s.to_vec()));
    }
    let fill_len = max_length as usize - string_length;
    let mut filler = Vec::with_capacity(fill_len);
    while filler.len() < fill_len {
        filler.extend_from_slice(&fill_string);
    }
    filler.truncate(fill_len);
    filler.extend_from_slice(s);
    Ok(Value::String(filler))
}

fn string_pad_end_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let max_length = if args.is_empty() {
        0.0
    } else {
        spec_to_integer_or_zero(mc, &args[0], env)?
    };
    let string_length = utf16_len(s);
    if max_length as usize <= string_length {
        return Ok(Value::String(s.to_vec()));
    }
    let fill_string = if args.len() >= 2 && !matches!(args[1], Value::Undefined) {
        spec_to_string(mc, &args[1], env)?
    } else {
        vec![0x0020] // space
    };
    if fill_string.is_empty() {
        return Ok(Value::String(s.to_vec()));
    }
    let fill_len = max_length as usize - string_length;
    let mut result = s.to_vec();
    let mut filler = Vec::with_capacity(fill_len);
    while filler.len() < fill_len {
        filler.extend_from_slice(&fill_string);
    }
    filler.truncate(fill_len);
    result.extend_from_slice(&filler);
    Ok(Value::String(result))
}

fn string_at_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let n = if args.is_empty() {
        0.0
    } else {
        spec_to_integer_or_zero(mc, &args[0], env)?
    };
    let len = s.len() as i64;
    let k = if n >= 0.0 { n as i64 } else { len + n as i64 };
    if k < 0 || k >= len {
        Ok(Value::Undefined)
    } else {
        Ok(Value::String(vec![s[k as usize]]))
    }
}

fn string_code_point_at_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let pos = if args.is_empty() {
        0.0
    } else {
        spec_to_integer_or_zero(mc, &args[0], env)?
    };
    if pos < 0.0 || pos >= s.len() as f64 {
        return Ok(Value::Undefined);
    }
    let idx = pos as usize;
    let first = s[idx];
    if (0xD800..=0xDBFF).contains(&first) && idx + 1 < s.len() {
        let second = s[idx + 1];
        if (0xDC00..=0xDFFF).contains(&second) {
            let code_point = 0x10000 + ((first as u32 - 0xD800) << 10) + (second as u32 - 0xDC00);
            return Ok(Value::Number(code_point as f64));
        }
    }
    Ok(Value::Number(first as f64))
}

fn string_search_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let search_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };

    // Step 2: If regexp is not undefined/null, let searcher = GetMethod(regexp, @@search)
    if !matches!(search_val, Value::Undefined | Value::Null)
        && let Value::Object(obj) = &search_val
        && let Some(searcher) = get_well_known_symbol_method(mc, env, obj, "search")?
    {
        return evaluate_call_dispatch(mc, env, &searcher, Some(&search_val), &[Value::String(s.to_vec())]);
    }

    // Step 6: Let rx = RegExpCreate(regexp, undefined)
    // Step 8: Return Invoke(rx, @@search, «S»)
    let regexp_obj = if matches!(search_val, Value::Undefined) {
        match handle_regexp_constructor_with_env(mc, Some(env), &[Value::String(Vec::new())])? {
            Value::Object(o) => o,
            _ => return Err(raise_eval_error!("Failed to create RegExp").into()),
        }
    } else {
        let p = spec_to_string(mc, &search_val, env)?;
        match handle_regexp_constructor_with_env(mc, Some(env), &[Value::String(p)])? {
            Value::Object(o) => o,
            _ => return Err(raise_eval_error!("Failed to create RegExp").into()),
        }
    };

    // Look up @@search on the newly created RegExp and invoke it
    if let Some(searcher) = get_well_known_symbol_method(mc, env, &regexp_obj, "search")? {
        return evaluate_call_dispatch(mc, env, &searcher, Some(&Value::Object(regexp_obj)), &[Value::String(s.to_vec())]);
    }

    // Fallback: manual search
    object_set_key_value(mc, &regexp_obj, "lastIndex", &Value::Number(0.0))?;
    let exec_args = vec![Value::String(s.to_vec())];
    let res = handle_regexp_method(mc, &regexp_obj, "exec", &exec_args, env)?;

    match res {
        Value::Object(match_obj) => {
            if let Some(idx_val) = object_get_key_value(&match_obj, "index") {
                Ok(idx_val.borrow().clone())
            } else {
                Ok(Value::Number(-1.0))
            }
        }
        _ => Ok(Value::Number(-1.0)),
    }
}

fn string_match_all_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // §22.1.3.13 String.prototype.matchAll(regexp)
    let regexp_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };

    // Step 2: If regexp is neither undefined nor null
    if !matches!(regexp_val, Value::Undefined | Value::Null) {
        // Step 2.a: Let isRegExp = ? IsRegExp(regexp)
        if is_regexp_like(mc, env, &regexp_val)? {
            // Step 2.b: If isRegExp is true, then
            // Let flags = ? Get(regexp, "flags")
            if let Value::Object(obj) = &regexp_val {
                let flags_val = crate::core::get_property_with_accessors(mc, env, obj, "flags")?;
                // RequireObjectCoercible(flags) — if flags is undefined/null, throw TypeError
                if matches!(flags_val, Value::Undefined | Value::Null) {
                    return Err(raise_type_error!("String.prototype.matchAll called with a non-global RegExp argument").into());
                }
                let flags_str = utf16_to_utf8(&spec_to_string(mc, &flags_val, env)?);
                if !flags_str.contains('g') {
                    return Err(raise_type_error!("String.prototype.matchAll called with a non-global RegExp argument").into());
                }
            }
        }

        // Step 2.c: Let matcher = ? GetMethod(regexp, @@matchAll)
        // Step 2.d: If matcher is not undefined, return ? Call(matcher, regexp, « O »)
        if let Value::Object(obj) = &regexp_val
            && let Some(matcher) = get_well_known_symbol_method(mc, env, obj, "matchAll")?
        {
            return evaluate_call_dispatch(mc, env, &matcher, Some(&regexp_val), &[Value::String(s.to_vec())]);
        }
    }

    // Step 3: Let S = ? ToString(O) — already done (s parameter)
    // Step 4: Let rx = ? RegExpCreate(regexp, "g")
    // Pass the original value directly so RegExp constructor handles undefined → empty pattern
    let pattern_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };
    let re_args = vec![pattern_val, Value::String(utf8_to_utf16("g"))];
    let val = handle_regexp_constructor_with_env(mc, Some(env), &re_args)?;
    if let Value::Object(obj) = val {
        // Step 5: Return ? Invoke(rx, @@matchAll, « S »)
        if let Some(matcher) = get_well_known_symbol_method(mc, env, &obj, "matchAll")? {
            return evaluate_call_dispatch(mc, env, &matcher, Some(&Value::Object(obj)), &[Value::String(s.to_vec())]);
        }
        // If @@matchAll is not found (e.g., deleted from RegExp.prototype), Invoke throws TypeError
        Err(raise_type_error!("[Symbol.matchAll] is not a function").into())
    } else {
        Err(raise_eval_error!("Failed to create RegExp").into())
    }
}

fn string_locale_compare_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let that = if args.is_empty() {
        utf8_to_utf16("undefined")
    } else {
        spec_to_string(mc, &args[0], env)?
    };
    // NFC-normalize both operands so that canonically equivalent sequences
    // (e.g. U+006F U+0308 vs U+00F6) compare as equal, as required by the
    // spec for localeCompare (Unicode canonical equivalence).
    use unicode_normalization::UnicodeNormalization;
    let s_str: String = String::from_utf16_lossy(s).nfc().collect();
    let t_str: String = String::from_utf16_lossy(&that).nfc().collect();
    let result = match s_str.cmp(&t_str) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    };
    Ok(Value::Number(result))
}

fn string_normalize_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let form = if args.is_empty() || matches!(args[0], Value::Undefined) {
        "NFC".to_string()
    } else {
        let f = spec_to_string(mc, &args[0], env)?;
        utf16_to_utf8(&f)
    };
    match form.as_str() {
        "NFC" | "NFD" | "NFKC" | "NFKD" => {}
        _ => {
            return Err(
                crate::raise_range_error!(format!("The normalization form should be one of NFC, NFD, NFKC, NFKD. Got: {form}")).into(),
            );
        }
    }
    // Convert UTF-16 to String, apply normalization, convert back
    let input = String::from_utf16_lossy(s);
    use unicode_normalization::UnicodeNormalization;
    let normalized: String = match form.as_str() {
        "NFC" => input.nfc().collect(),
        "NFD" => input.nfd().collect(),
        "NFKC" => input.nfkc().collect(),
        "NFKD" => input.nfkd().collect(),
        _ => unreachable!(),
    };
    Ok(Value::String(utf8_to_utf16(&normalized)))
}

/// §22.1.3.8 String.prototype.isWellFormed()
/// Returns true if the string contains no lone surrogates.
fn string_is_well_formed_method<'gc>(
    _mc: &MutationContext<'gc>,
    s: &[u16],
    _args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if (0xD800..=0xDBFF).contains(&c) {
            // High surrogate: must be followed by a low surrogate
            if i + 1 < s.len() && (0xDC00..=0xDFFF).contains(&s[i + 1]) {
                i += 2;
                continue;
            }
            return Ok(Value::Boolean(false));
        } else if (0xDC00..=0xDFFF).contains(&c) {
            // Lone low surrogate
            return Ok(Value::Boolean(false));
        }
        i += 1;
    }
    Ok(Value::Boolean(true))
}

fn string_to_well_formed_method<'gc>(
    _mc: &MutationContext<'gc>,
    s: &[u16],
    _args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut res = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if (0xD800..=0xDBFF).contains(&c) {
            if i + 1 < s.len() {
                let next = s[i + 1];
                if (0xDC00..=0xDFFF).contains(&next) {
                    res.push(c);
                    res.push(next);
                    i += 2;
                    continue;
                }
            }
            res.push(0xFFFD);
            i += 1;
        } else if (0xDC00..=0xDFFF).contains(&c) {
            res.push(0xFFFD);
            i += 1;
        } else {
            res.push(c);
            i += 1;
        }
    }
    Ok(Value::String(res))
}

#[allow(dead_code)]
fn make_array_from_values<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    values: Vec<Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let len = values.len();
    let arr = create_array(mc, env)?;
    for (i, v) in values.into_iter().enumerate() {
        object_set_key_value(mc, &arr, i, &v)?;
    }
    set_array_length(mc, &arr, len)?;
    Ok(Value::Object(arr))
}

/// Entry point for replaceAll when called with a raw (not-yet-stringified) `this` value.
/// This ensures the correct spec ordering: steps 2a-2d (IsRegExp, flags check, @@replace)
/// happen BEFORE ToString(this) (step 3).
pub fn string_replace_all_raw<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Value<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // §22.1.3.19 String.prototype.replaceAll ( searchValue, replaceValue )
    let search_val = args.first().cloned().unwrap_or(Value::Undefined);
    let replace_val = args.get(1).cloned().unwrap_or(Value::Undefined);

    // Step 2: If searchValue is neither undefined nor null
    if !matches!(search_val, Value::Undefined | Value::Null)
        && let Value::Object(obj) = &search_val
    {
        // Step 2a: Let isRegExp be ? IsRegExp(searchValue)
        let is_regexp = is_regexp_like(mc, env, &search_val)?;

        // Step 2b: If isRegExp is true
        if is_regexp {
            // i. Let flags be ? Get(searchValue, "flags")
            let flags_val = crate::core::get_property_with_accessors(mc, env, obj, "flags")?;
            // ii. Perform ? RequireObjectCoercible(flags)
            if matches!(flags_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("String.prototype.replaceAll called with a non-global RegExp argument").into());
            }
            // iii. If ? ToString(flags) does not contain "g", throw a TypeError
            let flags_str = utf16_to_utf8(&spec_to_string(mc, &flags_val, env)?);
            if !flags_str.contains('g') {
                return Err(raise_type_error!("String.prototype.replaceAll called with a non-global RegExp argument").into());
            }
        }

        // Step 2c: Let replacer be ? GetMethod(searchValue, @@replace)
        // Step 2d: If replacer is not undefined, return ? Call(replacer, searchValue, « O, replaceValue »)
        if let Some(replacer) = get_well_known_symbol_method(mc, env, obj, "replace")? {
            return evaluate_call_dispatch(mc, env, &replacer, Some(&search_val), &[this_val, replace_val]);
        }
    }

    // Step 3: Let string be ? ToString(O)
    let s = spec_to_string(mc, &this_val, env)?;

    // Delegate to string path
    string_replace_all_string_path(mc, &s, &search_val, &replace_val, env)
}

fn string_replace_all_method<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // §22.1.3.19 String.prototype.replaceAll ( searchValue, replaceValue )
    let search_val = args.first().cloned().unwrap_or(Value::Undefined);
    let replace_val = args.get(1).cloned().unwrap_or(Value::Undefined);

    // Step 2: If searchValue is neither undefined nor null
    if !matches!(search_val, Value::Undefined | Value::Null)
        && let Value::Object(obj) = &search_val
    {
        // Step 2a: Let isRegExp be ? IsRegExp(searchValue)
        let is_regexp = is_regexp_like(mc, env, &search_val)?;

        // Step 2b: If isRegExp is true
        if is_regexp {
            // i. Let flags be ? Get(searchValue, "flags")
            let flags_val = crate::core::get_property_with_accessors(mc, env, obj, "flags")?;
            // ii. Perform ? RequireObjectCoercible(flags)
            if matches!(flags_val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("String.prototype.replaceAll called with a non-global RegExp argument").into());
            }
            // iii. If ? ToString(flags) does not contain "g", throw a TypeError
            let flags_str = utf16_to_utf8(&spec_to_string(mc, &flags_val, env)?);
            if !flags_str.contains('g') {
                return Err(raise_type_error!("String.prototype.replaceAll called with a non-global RegExp argument").into());
            }
        }

        // Step 2c: Let replacer be ? GetMethod(searchValue, @@replace)
        // Step 2d: If replacer is not undefined, return ? Call(replacer, searchValue, « O, replaceValue »)
        if let Some(replacer) = get_well_known_symbol_method(mc, env, obj, "replace")? {
            return evaluate_call_dispatch(mc, env, &replacer, Some(&search_val), &[Value::String(s.to_vec()), replace_val]);
        }
    }

    // Step 3: Let string be ? ToString(O) — already done (s parameter)
    // Delegate to string path
    string_replace_all_string_path(mc, s, &search_val, &replace_val, env)
}

/// Steps 4-16 of §22.1.3.19: the string-based replacement path.
fn string_replace_all_string_path<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    search_val: &Value<'gc>,
    replace_val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 4: Let searchString be ? ToString(searchValue)
    let search = spec_to_string(mc, search_val, env)?;

    // Step 5: Let functionalReplace be IsCallable(replaceValue)
    let functional_replace = matches!(
        &replace_val,
        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..)
    ) || matches!(&replace_val, Value::Object(o) if o.borrow().get_closure().is_some()
        || o.borrow().class_def.is_some()
        || crate::core::slot_get(o, &InternalSlot::BoundTarget).is_some()
        || crate::core::slot_get(o, &InternalSlot::Callable).is_some());

    // Step 6: If functionalReplace is false, let replaceValue be ? ToString(replaceValue)
    let replace_str = if !functional_replace {
        Some(spec_to_string(mc, replace_val, env)?)
    } else {
        None
    };

    // Step 7: Let searchLength be the length of searchString
    let search_length = search.len();

    // Step 8: Let advanceBy be max(1, searchLength)
    let advance_by = std::cmp::max(1, search_length);

    // Steps 9-11: Collect all match positions using StringIndexOf
    let mut match_positions: Vec<usize> = Vec::new();
    let s_len = s.len();
    {
        let mut pos: Option<usize> = string_index_of_u16(s, &search, 0, s_len);
        while let Some(p) = pos {
            match_positions.push(p);
            let next_from = p + advance_by;
            pos = string_index_of_u16(s, &search, next_from, s_len);
        }
    }

    // Steps 12-13
    let mut end_of_last_match: usize = 0;
    let mut result: Vec<u16> = Vec::new();

    // Step 14: For each position in matchPositions
    for &pos in &match_positions {
        // Step 14c: Let stringSlice be the substring of string from endOfLastMatch to position
        result.extend_from_slice(&s[end_of_last_match..pos]);

        if functional_replace {
            // Step 14a: functionalReplace is true
            // i. Let replacement be ? ToString(? Call(replaceValue, undefined, « searchString, 𝔽(position), string »))
            let call_args = vec![Value::String(search.clone()), Value::Number(pos as f64), Value::String(s.to_vec())];
            let repl_val = evaluate_call_dispatch(mc, env, replace_val, Some(&Value::Undefined), &call_args)?;
            let repl_u16 = spec_to_string(mc, &repl_val, env)?;
            result.extend_from_slice(&repl_u16);
        } else {
            // Step 14b: functionalReplace is false
            // ii. Let captures be a new empty List
            // iii. Let replacement be GetSubstitution(searchString, string, position, captures, undefined, replaceValue)
            let replace_s = replace_str.as_ref().unwrap();
            let repl_utf8 = utf16_to_utf8(replace_s);
            let matched = &s[pos..pos + search_length];
            let before = &s[..pos];
            let after = &s[pos + search_length..];
            let empty_captures: Vec<Option<Vec<u16>>> = Vec::new();
            result.extend_from_slice(&expand_replacement_tokens(
                &repl_utf8,
                matched,
                &empty_captures,
                None,
                before,
                after,
            ));
        }

        // Step 14e: Let endOfLastMatch be position + searchLength
        end_of_last_match = pos + search_length;
    }

    // Step 15: If endOfLastMatch < length of string
    if end_of_last_match < s_len {
        result.extend_from_slice(&s[end_of_last_match..]);
    }

    // Step 16: Return result
    Ok(Value::String(result))
}

/// StringIndexOf spec helper: find `search` in `s` starting at `from_index`.
/// Returns Some(position) or None if not found.
fn string_index_of_u16(s: &[u16], search: &[u16], from_index: usize, s_len: usize) -> Option<usize> {
    if from_index > s_len {
        return None;
    }
    let search_len = search.len();
    if search_len == 0 {
        // Empty search string matches at from_index if from_index <= s_len
        return if from_index <= s_len { Some(from_index) } else { None };
    }
    if from_index + search_len > s_len {
        return None;
    }
    for k in from_index..=s_len - search_len {
        if s[k..k + search_len] == *search {
            return Some(k);
        }
    }
    None
}

pub fn string_from_char_code<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut chars = Vec::new();
    for arg in args {
        let num = crate::core::to_number_with_env(mc, env, arg)?;
        // ToUint16
        let u = if num.is_nan() || num == 0.0 || num.is_infinite() {
            0u16
        } else {
            let int_val = num.signum() * num.abs().floor();
            let m = int_val % 65536.0;
            let m = if m < 0.0 { m + 65536.0 } else { m };
            m as u16
        };
        chars.push(u);
    }
    Ok(Value::String(chars))
}

pub fn string_from_code_point<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut chars = Vec::new();
    for arg in args {
        let num = crate::core::to_number_with_env(mc, env, arg)?;
        // Step 3c: If nextCP is not an integer, throw a RangeError.
        if num.is_nan() || num.is_infinite() || num.fract() != 0.0 {
            return Err(raise_range_error!("Invalid code point").into());
        }
        let cp = num as i64;
        // Step 3d: If nextCP < 0 or nextCP > 0x10FFFF, throw a RangeError.
        if !(0..=0x10FFFF).contains(&cp) {
            return Err(raise_range_error!("Invalid code point").into());
        }
        let cp = cp as u32;
        if cp <= 0xFFFF {
            chars.push(cp as u16);
        } else if let Some(c) = std::char::from_u32(cp) {
            let mut buf = [0; 2];
            let encoded = c.encode_utf16(&mut buf);
            chars.extend_from_slice(encoded);
        } else {
            return Err(raise_range_error!("Invalid code point").into());
        }
    }
    Ok(Value::String(chars))
}

pub fn string_raw<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    // §22.1.2.4 String.raw(template, ...substitutions)
    if args.is_empty() {
        return Err(crate::raise_type_error!("Cannot convert undefined or null to object").into());
    }
    let template = &args[0];
    // Step 2: Let cooked = ToObject(template)
    let cooked_obj = match template {
        Value::Object(obj) => *obj,
        Value::Undefined | Value::Null => {
            return Err(crate::raise_type_error!("Cannot convert undefined or null to object").into());
        }
        _ => {
            return Err(crate::raise_type_error!("String.raw requires an object as first argument").into());
        }
    };
    // Step 3: Let literals = ToObject(Get(cooked, "raw"))
    // Use get_property_with_accessors to trigger getters
    let raw_val = crate::core::get_property_with_accessors(mc, env, &cooked_obj, "raw")?;
    let literals_obj = match raw_val {
        Value::Object(obj) => obj,
        Value::Undefined | Value::Null => {
            return Err(crate::raise_type_error!("Cannot convert undefined or null to object").into());
        }
        _ => {
            return Err(crate::raise_type_error!("String.raw requires raw property to be an object").into());
        }
    };
    // Step 4: Let literalCount = ToLength(Get(raw, "length"))
    // Use get_property_with_accessors to trigger getters on length
    let len_val = crate::core::get_property_with_accessors(mc, env, &literals_obj, "length")?;
    let n = to_number_with_env(mc, env, &len_val)?;
    let literal_count = if n.is_nan() || n <= 0.0 {
        0usize
    } else {
        n.min(9007199254740991.0) as usize // 2^53 - 1
    };
    if literal_count == 0 {
        return Ok(Value::String(Vec::new()));
    }
    let substitutions = &args[1..];
    let mut result: Vec<u16> = Vec::new();
    for next_index in 0..literal_count {
        // Get the next literal segment — use get_property_with_accessors to trigger getters
        let next_seg_val = crate::core::get_property_with_accessors(mc, env, &literals_obj, next_index)?;
        let next_seg = spec_to_string(mc, &next_seg_val, env)?;
        result.extend_from_slice(&next_seg);
        if next_index + 1 == literal_count {
            break;
        }
        // Get substitution if available
        if next_index < substitutions.len() {
            let next_sub_str = spec_to_string(mc, &substitutions[next_index], env)?;
            result.extend_from_slice(&next_sub_str);
        }
    }
    Ok(Value::String(result))
}

/// Create a new String Iterator
pub(crate) fn create_string_iterator<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let iterator = new_js_object_data(mc);

    // Set [[Prototype]] to %StringIteratorPrototype%
    if let Some(proto_val) = slot_get_chained(env, &InternalSlot::StringIteratorPrototype)
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        iterator.borrow_mut(mc).prototype = Some(*proto);
    }

    // Store string data
    slot_set(mc, &iterator, InternalSlot::IteratorString, &Value::String(s.to_vec()));
    slot_set(mc, &iterator, InternalSlot::IteratorIndex, &Value::Number(0.0));

    Ok(Value::Object(iterator))
}

pub(crate) fn handle_string_iterator_next<'gc>(
    mc: &MutationContext<'gc>,
    iterator: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Spec §22.1.5.2.1: If O does not have all of the internal slots of a
    // String Iterator Instance, throw a TypeError.  Use `slot_get` (own-only)
    // so that `Object.create(realIterator)` does NOT inherit these slots.
    let str_val = slot_get(iterator, &InternalSlot::IteratorString).ok_or_else(|| {
        EvalError::from(crate::raise_type_error!(
            "StringIterator.prototype.next requires that 'this' be a String Iterator"
        ))
    })?;
    let s = if let Value::String(utf16) = &*str_val.borrow() {
        utf16.clone()
    } else {
        return Err(crate::raise_type_error!("StringIterator.prototype.next requires that 'this' be a String Iterator").into());
    };

    // Get index (own slot only)
    let index_val = slot_get(iterator, &InternalSlot::IteratorIndex).ok_or_else(|| {
        EvalError::from(crate::raise_type_error!(
            "StringIterator.prototype.next requires that 'this' be a String Iterator"
        ))
    })?;
    let mut index = if let Value::Number(n) = &*index_val.borrow() {
        if *n < 0.0 { 0 } else { *n as usize }
    } else {
        return Err(raise_eval_error!("Iterator index is invalid").into());
    };

    let len = s.len();
    if index >= len {
        let result_obj = new_js_object_data(mc);
        object_set_key_value(mc, &result_obj, "value", &Value::Undefined)?;
        object_set_key_value(mc, &result_obj, "done", &Value::Boolean(true))?;
        return Ok(Value::Object(result_obj));
    }

    // Identify code point (handles surrogate pairs)
    let c1 = s[index];
    let mut code_unit_count = 1;
    let mut ch_vec = vec![c1];

    if (0xD800..=0xDBFF).contains(&c1) && index + 1 < len {
        let c2 = s[index + 1];
        if (0xDC00..=0xDFFF).contains(&c2) {
            ch_vec.push(c2);
            code_unit_count = 2;
        }
    }

    index += code_unit_count;
    slot_set(mc, iterator, InternalSlot::IteratorIndex, &Value::Number(index as f64));

    let result_obj = new_js_object_data(mc);
    object_set_key_value(mc, &result_obj, "value", &Value::String(ch_vec))?;
    object_set_key_value(mc, &result_obj, "done", &Value::Boolean(false))?;
    Ok(Value::Object(result_obj))
}

/// AnnexB CreateHTML(string, tag, attribute, value) — B.2.3.2–B.2.3.14
fn string_html_wrapper<'gc>(
    mc: &MutationContext<'gc>,
    s: &[u16],
    tag: &str,
    attr: Option<(&str, &[Value<'gc>])>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let s_str = utf16_to_utf8(s);
    let result = if let Some((attr_name, args)) = attr {
        let attr_val = if args.is_empty() {
            "undefined".to_string()
        } else {
            let v = spec_to_string(mc, &args[0], env)?;
            utf16_to_utf8(&v)
        };
        // Escape '"' as '&quot;' in attribute value
        let escaped = attr_val.replace('"', "&quot;");
        format!("<{tag} {attr_name}=\"{escaped}\">{s_str}</{tag}>")
    } else {
        format!("<{tag}>{s_str}</{tag}>")
    };
    Ok(Value::String(utf8_to_utf16(&result)))
}
