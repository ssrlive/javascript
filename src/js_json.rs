use crate::core::MutationContext;
use crate::core::{
    EvalError, InternalSlot, JSObjectDataPtr, PropertyKey, Value, env_set, new_js_object_data, object_get_key_value, object_set_key_value,
    slot_get,
};
use crate::error::JSError;
use crate::js_array::{create_array, is_array, set_array_length};
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

// ═══════════════════════════════════════════════════════════════════════════════
// Initialization
// ═══════════════════════════════════════════════════════════════════════════════

pub fn initialize_json<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let json_obj = new_js_object_data(mc);
    let _ = crate::core::set_internal_prototype_from_constructor(mc, &json_obj, env, "Object");

    // JSON.parse  — function with length = 2
    object_set_key_value(mc, &json_obj, "parse", &Value::Function("JSON.parse".to_string()))?;
    json_obj.borrow_mut(mc).set_non_enumerable("parse");

    // JSON.stringify — function with length = 3
    object_set_key_value(mc, &json_obj, "stringify", &Value::Function("JSON.stringify".to_string()))?;
    json_obj.borrow_mut(mc).set_non_enumerable("stringify");

    // Symbol.toStringTag = "JSON" { writable: false, enumerable: false, configurable: true }
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_val.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let tag_desc = crate::core::create_descriptor_object(
            mc,
            &Value::String(utf8_to_utf16("JSON")),
            false, // writable
            false, // enumerable
            true,  // configurable
        )?;
        crate::js_object::define_property_internal(mc, &json_obj, PropertyKey::Symbol(*tag_sym), &tag_desc)?;
    }

    env_set(mc, env, "JSON", &Value::Object(json_obj))?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Dispatch — returns EvalError to preserve thrown JS values
// ═══════════════════════════════════════════════════════════════════════════════

pub fn handle_json_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "parse" => json_parse(mc, args, env),
        "stringify" => json_stringify(mc, args, env),
        _ => Err(raise_eval_error!(format!("JSON.{method} is not implemented")).into()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// JSON.parse (§24.5.1)
// ═══════════════════════════════════════════════════════════════════════════════

fn json_parse<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1: Let jsonString be ? ToString(text).
    let text = args.first().cloned().unwrap_or(Value::Undefined);
    let json_str = to_string_for_json(mc, &text, env)?;

    // Step 2: Parse jsonString as JSON. Throw SyntaxError on failure.
    let json_value: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(_) => return Err(raise_syntax_error!("Unexpected token in JSON").into()),
    };

    // Step 3: Convert JSON AST to JS value.
    let unfiltered = json_value_to_js(mc, json_value, env)?;

    // Step 4: If reviver is a callable function, run InternalizeJSONProperty.
    let reviver = args.get(1).cloned().unwrap_or(Value::Undefined);
    if is_callable(&reviver) {
        // Wrap root in {"": unfiltered}
        let root = new_js_object_data(mc);
        let _ = crate::core::set_internal_prototype_from_constructor(mc, &root, env, "Object");
        object_set_key_value(mc, &root, "", &unfiltered)?;
        internalize_json_property(mc, env, &root, "", &reviver)
    } else {
        Ok(unfiltered)
    }
}

/// ToString coercion for JSON.parse input.
fn to_string_for_json<'gc>(mc: &MutationContext<'gc>, val: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<String, EvalError<'gc>> {
    match val {
        Value::String(s) => Ok(utf16_to_utf8(s)),
        Value::Number(_) => Ok(crate::core::value_to_string(val)),
        Value::Boolean(b) => Ok(if *b { "true".to_string() } else { "false".to_string() }),
        Value::Null => Ok("null".to_string()),
        Value::Undefined => Ok("undefined".to_string()),
        Value::BigInt(_) => Err(raise_type_error!("Cannot convert a BigInt value to a string").into()),
        Value::Symbol(_) => Err(raise_type_error!("Cannot convert a Symbol value to a string").into()),
        Value::Object(_obj) => {
            // ToPrimitive(input, string) then ToString
            let prim = crate::core::to_primitive(mc, val, "string", env)?;
            to_string_for_json(mc, &prim, env)
        }
        _ => Ok(crate::core::value_to_string(val)),
    }
}

/// InternalizeJSONProperty (holder, name, reviver) — §24.5.1.1
fn internalize_json_property<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    holder: &JSObjectDataPtr<'gc>,
    name: &str,
    reviver: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1: Let val be ? Get(holder, name).
    let val = get_prop(mc, env, holder, name)?;

    // Step 2: If val is an Object, then …
    if let Value::Object(obj) = &val {
        if is_array_for_json_checked(mc, obj)? {
            // Step 2.b: isArray is true
            // ii. Let len be ? LengthOfArrayLike(val).
            let len = to_length_of_array_like(mc, env, obj)?;
            for i in 0..len {
                let i_str = i.to_string();
                let new_element = internalize_json_property(mc, env, obj, &i_str, reviver)?;
                if matches!(new_element, Value::Undefined) {
                    // 3.a: Perform ? val.[[Delete]](prop).
                    // Note: OrdinaryDelete returns false for non-configurable — no throw.
                    let _ = json_delete_property(mc, obj, &i_str)?;
                } else {
                    // 4.a: Perform ? CreateDataProperty(val, prop, newElement).
                    // Note: CreateDataProperty — returns false for non-configurable, no throw.
                    let _ = json_create_data_property(mc, obj, &i_str, &new_element)?;
                }
            }
        } else {
            // Step 2.c: Else (object)
            // i. Let keys be ? EnumerableOwnProperties(val, key).
            let keys = enumerable_own_property_names(mc, obj)?;
            for key in keys {
                let new_element = internalize_json_property(mc, env, obj, &key, reviver)?;
                if matches!(new_element, Value::Undefined) {
                    let _ = json_delete_property(mc, obj, &key)?;
                } else {
                    let _ = json_create_data_property(mc, obj, key.as_str(), &new_element)?;
                }
            }
        }
    }

    // Step 3: Return ? Call(reviver, holder, « name, val »).
    let name_val = Value::String(utf8_to_utf16(name));
    crate::core::evaluate_call_dispatch(mc, env, reviver, Some(&Value::Object(*holder)), &[name_val, val])
}

/// LengthOfArrayLike(obj) — §7.3.2: Get(obj, "length") then ToLength.
fn to_length_of_array_like<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
) -> Result<usize, EvalError<'gc>> {
    let len_val = get_prop(mc, env, obj, "length")?;
    // ToLength: ToIntegerOrInfinity(argument), then clamp [0, 2^53 - 1]
    // ToIntegerOrInfinity first calls ToNumber which may call valueOf() and throw.
    let n = to_number_for_json(mc, &len_val, env)?;
    if n.is_nan() || n <= 0.0 {
        Ok(0)
    } else if n.is_infinite() {
        Ok(usize::MAX)
    } else {
        Ok(n.min(9007199254740991.0) as usize) // 2^53 - 1
    }
}

/// ToNumber for JSON operations — calls ToPrimitive("number") for objects.
fn to_number_for_json<'gc>(mc: &MutationContext<'gc>, val: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<f64, EvalError<'gc>> {
    match val {
        Value::Number(n) => Ok(*n),
        Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Null => Ok(0.0),
        Value::Undefined => Ok(f64::NAN),
        Value::String(s) => {
            let utf8 = utf16_to_utf8(s);
            Ok(utf8.trim().parse::<f64>().unwrap_or(f64::NAN))
        }
        Value::Object(_) => {
            let prim = crate::core::to_primitive(mc, val, "number", env)?;
            to_number_for_json(mc, &prim, env)
        }
        _ => Ok(f64::NAN),
    }
}

/// [[Delete]](P) — respects proxy deleteProperty trap.
/// Returns false (without throwing) for non-configurable properties per OrdinaryDelete.
fn json_delete_property<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>, key: &str) -> Result<bool, EvalError<'gc>> {
    // Check for Proxy
    if let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
    {
        let pk: PropertyKey = key.into();
        return crate::js_proxy::proxy_delete_property(mc, proxy, &pk);
    }
    // OrdinaryDelete: non-configurable → return false (don't throw).
    let pk: PropertyKey = key.into();
    if obj.borrow().non_configurable.contains(&pk) {
        return Ok(false);
    }
    let _ = obj.borrow_mut(mc).properties.swap_remove(&pk);
    Ok(true)
}

/// CreateDataProperty(O, P, V) — non-throwing for non-configurable properties.
/// Proxies still go through defineProperty trap (which may throw).
/// For ordinary objects, returns false silently if non-configurable conflicts.
fn json_create_data_property<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: &str,
    val: &Value<'gc>,
) -> Result<bool, EvalError<'gc>> {
    // If obj is a Proxy, invoke the [[DefineOwnProperty]] trap
    if let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
    {
        let pk: PropertyKey = key.into();
        return crate::js_proxy::proxy_define_data_property(mc, proxy, &pk, val);
    }
    // For ordinary objects: check non-configurable — if existing prop is non-configurable
    // and new desc has configurable:true, OrdinaryDefineOwnProperty returns false.
    let pk: PropertyKey = key.into();
    if obj.borrow().non_configurable.contains(&pk) {
        return Ok(false);
    }
    // Set the property normally
    object_set_key_value(mc, obj, key, val)?;
    Ok(true)
}

/// EnumerableOwnProperties(O, key) — goes through proxy ownKeys + getOwnPropertyDescriptor traps.
/// Uses EvalError throughout to preserve the identity of user-thrown errors.
fn enumerable_own_property_names<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> Result<Vec<String>, EvalError<'gc>> {
    // Get all own keys — for proxies, call proxy_own_keys directly to preserve EvalError::Throw.
    let all_keys: Vec<PropertyKey<'gc>> = if let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
    {
        crate::js_proxy::proxy_own_keys(mc, proxy)?
    } else {
        crate::core::ordinary_own_property_keys_mc(mc, obj)?
    };
    let mut result = Vec::new();
    for k in all_keys {
        if let PropertyKey::String(s) = k {
            // Check enumerability — for proxies goes through getOwnPropertyDescriptor trap
            if let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                let pk = PropertyKey::String(s.clone());
                match crate::js_proxy::proxy_get_own_property_is_enumerable(mc, proxy, &pk)? {
                    Some(true) => result.push(s), // enumerable
                    Some(false) => {}             // non-enumerable
                    None => {}                    // undefined from trap
                }
            } else {
                // Regular object: check own property exists and is enumerable
                if obj.borrow().is_enumerable(&s) {
                    result.push(s);
                }
            }
        }
    }
    Ok(result)
}

/// Get a property from an object, going through accessors.
fn get_prop<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: &str,
) -> Result<Value<'gc>, EvalError<'gc>> {
    crate::core::get_property_with_accessors(mc, env, obj, key)
}

// ═══════════════════════════════════════════════════════════════════════════════
// JSON.stringify (§24.5.2)
// ═══════════════════════════════════════════════════════════════════════════════

fn json_stringify<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let replacer_arg = args.get(1).cloned().unwrap_or(Value::Undefined);
    let space_arg = args.get(2).cloned().unwrap_or(Value::Undefined);

    // --- Build ReplacerFunction and PropertyList ---
    let mut replacer_function: Option<Value<'gc>> = None;
    let mut property_list: Option<Vec<String>> = None;

    if let Value::Object(rep_obj) = &replacer_arg {
        if is_callable(&replacer_arg) {
            replacer_function = Some(replacer_arg.clone());
        } else if is_array_for_json_checked(mc, rep_obj)? {
            // Build PropertyList from array
            let len = to_length_of_array_like(mc, env, rep_obj)?;
            let mut list: Vec<String> = Vec::new();
            for i in 0..len {
                let item = get_prop(mc, env, rep_obj, &i.to_string())?;
                let key_str = match &item {
                    Value::String(s) => Some(utf16_to_utf8(s)),
                    Value::Number(_) => Some(crate::core::value_to_string(&item)),
                    Value::Object(o) => {
                        // Spec 24.5.3 step 4.b.iii:
                        // If v has [[StringData]], set item to ? ToString(v).
                        // Else if v has [[NumberData]], set item to ? ToString(v).
                        let has_wrapper = if let Some(pv_rc) = slot_get(o, &InternalSlot::PrimitiveValue) {
                            let pv = pv_rc.borrow();
                            matches!(&*pv, Value::String(_) | Value::Number(_))
                        } else {
                            false
                        };
                        if has_wrapper {
                            // ToString(v) on the object — goes through ToPrimitive(v, "string")
                            Some(to_string_for_json(mc, &item, env)?)
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some(s) = key_str
                    && !list.contains(&s)
                {
                    list.push(s);
                }
            }
            property_list = Some(list);
        }
    }

    // --- Process space argument ---
    let gap = process_space_arg(mc, &space_arg, env)?;

    // --- Create wrapper object { "": value } ---
    let wrapper = new_js_object_data(mc);
    let _ = crate::core::set_internal_prototype_from_constructor(mc, &wrapper, env, "Object");
    object_set_key_value(mc, &wrapper, "", &value)?;

    // --- SerializeJSONProperty ---
    let mut stack: Vec<usize> = Vec::new();
    let result = serialize_json_property(mc, env, &wrapper, "", &replacer_function, &property_list, &gap, "", &mut stack)?;

    match result {
        Some(s) => Ok(Value::String(utf8_to_utf16(&s))),
        None => Ok(Value::Undefined),
    }
}

/// Process the `space` argument (§24.5.2 steps 5-8)
fn process_space_arg<'gc>(mc: &MutationContext<'gc>, space: &Value<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<String, EvalError<'gc>> {
    let space = match space {
        Value::Object(obj) => unwrap_boxed(mc, obj, env)?,
        other => other.clone(),
    };

    Ok(match &space {
        Value::Number(n) => {
            let count = n.clamp(0.0, 10.0) as usize;
            " ".repeat(count)
        }
        Value::String(s) => {
            let utf8 = utf16_to_utf8(s);
            if utf8.len() > 10 { utf8[..10].to_string() } else { utf8 }
        }
        _ => String::new(),
    })
}

/// SerializeJSONProperty(state, key, holder) — §24.5.2.1
#[allow(clippy::too_many_arguments)]
fn serialize_json_property<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    holder: &JSObjectDataPtr<'gc>,
    key: &str,
    replacer_function: &Option<Value<'gc>>,
    property_list: &Option<Vec<String>>,
    gap: &str,
    indent: &str,
    stack: &mut Vec<usize>,
) -> Result<Option<String>, EvalError<'gc>> {
    // Step 1: Let value be ? Get(holder, key).
    let mut value = get_prop(mc, env, holder, key)?;

    // Step 2: If value is an Object or BigInt, check for toJSON.
    if let Value::Object(obj) = &value
        && let Ok(to_json) = get_prop(mc, env, obj, "toJSON")
        && is_callable(&to_json)
    {
        let key_arg = Value::String(utf8_to_utf16(key));
        value = crate::core::evaluate_call_dispatch(mc, env, &to_json, Some(&value), &[key_arg])?;
    }
    if let Value::BigInt(_) = &value {
        // BigInt.prototype may have toJSON — use GetV(value, "toJSON") semantics.
        // GetV: O = ToObject(value), return O.[[Get]]("toJSON", value).
        // The receiver must be the original BigInt so strict-mode getters see the primitive.
        if let Some(bigint_val) = crate::core::env_get(env, "BigInt")
            && let Value::Object(bigint_ctor) = &*bigint_val.borrow()
            && let Some(proto_val) = object_get_key_value(bigint_ctor, "prototype")
            && let Value::Object(proto) = &*proto_val.borrow()
        {
            // Check the raw property — may be a data prop, getter, or Property descriptor
            if let Some(raw) = object_get_key_value(proto, "toJSON") {
                let raw_val = raw.borrow().clone();
                let to_json = match &raw_val {
                    Value::Getter(..) => {
                        // Invoke getter with receiver = value (the BigInt primitive)
                        crate::core::evaluate_call_dispatch(mc, env, &raw_val, Some(&value), &[])?
                    }
                    Value::Property { getter: Some(g), .. } => {
                        // Invoke getter with receiver = value (BigInt primitive)
                        let getter_fn: Value<'gc> = (**g).clone();
                        crate::core::evaluate_call_dispatch(mc, env, &getter_fn, Some(&value), &[])?
                    }
                    other => other.clone(),
                };
                if is_callable(&to_json) {
                    let key_arg = Value::String(utf8_to_utf16(key));
                    value = crate::core::evaluate_call_dispatch(mc, env, &to_json, Some(&value), &[key_arg])?;
                }
            }
        }
    }

    // Step 3: If ReplacerFunction is defined, call it.
    if let Some(replacer) = replacer_function {
        let key_arg = Value::String(utf8_to_utf16(key));
        value = crate::core::evaluate_call_dispatch(mc, env, replacer, Some(&Value::Object(*holder)), &[key_arg, value])?;
    }

    // Step 4: If value is an Object, unwrap boxed primitives.
    if let Value::Object(obj) = &value {
        let unwrapped = unwrap_boxed(mc, obj, env)?;
        if !matches!(unwrapped, Value::Object(_)) {
            value = unwrapped;
        }
    }

    // Step 5-12: Serialize based on type.
    match &value {
        Value::Null => Ok(Some("null".to_string())),
        Value::Boolean(b) => Ok(Some(if *b { "true".to_string() } else { "false".to_string() })),
        Value::String(s) => Ok(Some(quote_json_string_utf16(s))),
        Value::Number(n) => {
            if n.is_finite() {
                Ok(Some(format_number(*n)))
            } else {
                Ok(Some("null".to_string()))
            }
        }
        Value::BigInt(_) => Err(raise_type_error!("Do not know how to serialize a BigInt").into()),
        Value::Object(obj) => {
            // Check callable — functions/closures/etc. are not serializable
            if is_callable(&value) {
                return Ok(None);
            }

            // IsArray check — throws TypeError for revoked proxies
            let is_arr = is_array_for_json_checked(mc, obj)?;

            // Circular check
            let obj_id = gc_ptr_id(obj);
            if stack.contains(&obj_id) {
                return Err(raise_type_error!("Converting circular structure to JSON").into());
            }
            stack.push(obj_id);

            let result = if is_arr {
                serialize_json_array(mc, env, obj, replacer_function, property_list, gap, indent, stack)
            } else {
                serialize_json_object(mc, env, obj, replacer_function, property_list, gap, indent, stack)
            };

            stack.pop();
            result
        }
        // Undefined, Symbol, Function → None (omitted)
        _ => Ok(None),
    }
}

/// SerializeJSONObject (§24.5.2.2)
#[allow(clippy::too_many_arguments)]
fn serialize_json_object<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    replacer_function: &Option<Value<'gc>>,
    property_list: &Option<Vec<String>>,
    gap: &str,
    indent: &str,
    stack: &mut Vec<usize>,
) -> Result<Option<String>, EvalError<'gc>> {
    let new_indent = format!("{}{}", indent, gap);

    let keys: Vec<String> = if let Some(pl) = property_list {
        pl.clone()
    } else {
        // EnumerableOwnPropertyNames(value, "key") — goes through proxy ownKeys + getOwnPropertyDescriptor
        enumerable_own_property_names(mc, obj)?
    };

    let mut partial: Vec<String> = Vec::new();
    for key in &keys {
        let str_p = serialize_json_property(mc, env, obj, key, replacer_function, property_list, gap, &new_indent, stack)?;
        if let Some(s) = str_p {
            let member = format!("{}:{}{}", quote_json_string(key), if gap.is_empty() { "" } else { " " }, s);
            partial.push(member);
        }
    }

    if partial.is_empty() {
        Ok(Some("{}".to_string()))
    } else if gap.is_empty() {
        Ok(Some(format!("{{{}}}", partial.join(","))))
    } else {
        let sep = format!(",\n{}", new_indent);
        Ok(Some(format!("{{\n{}{}\n{}}}", new_indent, partial.join(&sep), indent)))
    }
}

/// SerializeJSONArray (§24.5.2.3)
#[allow(clippy::too_many_arguments)]
fn serialize_json_array<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    replacer_function: &Option<Value<'gc>>,
    property_list: &Option<Vec<String>>,
    gap: &str,
    indent: &str,
    stack: &mut Vec<usize>,
) -> Result<Option<String>, EvalError<'gc>> {
    let new_indent = format!("{}{}", indent, gap);
    // ? LengthOfArrayLike(value) — go through proxy get trap for "length", then ToLength
    let len = to_length_of_array_like(mc, env, obj)?;

    let mut partial: Vec<String> = Vec::new();
    for i in 0..len {
        let i_str = i.to_string();
        let str_p = serialize_json_property(mc, env, obj, &i_str, replacer_function, property_list, gap, &new_indent, stack)?;
        match str_p {
            Some(s) => partial.push(s),
            None => partial.push("null".to_string()),
        }
    }

    if partial.is_empty() {
        Ok(Some("[]".to_string()))
    } else if gap.is_empty() {
        Ok(Some(format!("[{}]", partial.join(","))))
    } else {
        let sep = format!(",\n{}", new_indent);
        Ok(Some(format!("[\n{}{}\n{}]", new_indent, partial.join(&sep), indent)))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// JSON value → JS value conversion (for JSON.parse)
// ═══════════════════════════════════════════════════════════════════════════════

fn json_value_to_js<'gc>(
    mc: &MutationContext<'gc>,
    json_value: serde_json::Value,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match json_value {
        serde_json::Value::Null => Ok(Value::Null),
        serde_json::Value::Bool(b) => Ok(Value::Boolean(b)),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Ok(Value::Number(f))
            } else {
                Ok(Value::Number(0.0))
            }
        }
        serde_json::Value::String(s) => Ok(Value::String(utf8_to_utf16(&s))),
        serde_json::Value::Array(arr) => {
            let len = arr.len();
            let obj = create_array(mc, env)?;
            for (i, item) in arr.into_iter().enumerate() {
                let js_val = json_value_to_js(mc, item, env)?;
                object_set_key_value(mc, &obj, i, &js_val)?;
            }
            set_array_length(mc, &obj, len)?;
            Ok(Value::Object(obj))
        }
        serde_json::Value::Object(map) => {
            let js_obj = new_js_object_data(mc);
            let _ = crate::core::set_internal_prototype_from_constructor(mc, &js_obj, env, "Object");
            for (key, value) in map.into_iter() {
                let js_val = json_value_to_js(mc, value, env)?;
                object_set_key_value(mc, &js_obj, &key, &js_val)?;
            }
            Ok(Value::Object(js_obj))
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Utility functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Quote a JSON string per spec (§24.5.2.4)
fn quote_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x08' => out.push_str("\\b"),
            '\x0C' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Quote a JSON string from UTF-16 code units — well-formed-json-stringify (§24.5.2.4)
/// Lone surrogates (U+D800..U+DFFF) are escaped as \uXXXX per spec.
fn quote_json_string_utf16(s: &[u16]) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut i = 0;
    while i < s.len() {
        let cu = s[i];
        match cu {
            0x0022 => out.push_str("\\\""), // "
            0x005C => out.push_str("\\\\"), // \
            0x0008 => out.push_str("\\b"),
            0x000C => out.push_str("\\f"),
            0x000A => out.push_str("\\n"),
            0x000D => out.push_str("\\r"),
            0x0009 => out.push_str("\\t"),
            c if c < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c));
            }
            // Lone surrogates → escape as \uXXXX (well-formed-json-stringify)
            c @ 0xD800..=0xDBFF => {
                // High surrogate — check if followed by low surrogate
                if i + 1 < s.len() && (0xDC00..=0xDFFF).contains(&s[i + 1]) {
                    // Valid surrogate pair, decode and emit as UTF-8
                    let hi = c as u32;
                    let lo = s[i + 1] as u32;
                    let cp = ((hi - 0xD800) << 10) + (lo - 0xDC00) + 0x10000;
                    if let Some(ch) = char::from_u32(cp) {
                        out.push(ch);
                    }
                    i += 2;
                    continue;
                } else {
                    // Lone high surrogate
                    out.push_str(&format!("\\ud{:03x}", c & 0xFFF));
                }
            }
            c @ 0xDC00..=0xDFFF => {
                // Lone low surrogate
                out.push_str(&format!("\\u{:04x}", c));
            }
            c => {
                if let Some(ch) = char::from_u32(c as u32) {
                    out.push(ch);
                }
            }
        }
        i += 1;
    }
    out.push('"');
    out
}

/// Format a finite number for JSON output.
fn format_number(n: f64) -> String {
    if n == 0.0 {
        return "0".to_string();
    }
    crate::core::value_to_string(&Value::Number(n))
}

/// Check if a value is callable (Function, Closure, etc.)
fn is_callable<'gc>(val: &Value<'gc>) -> bool {
    match val {
        Value::Function(_)
        | Value::Closure(_)
        | Value::AsyncClosure(_)
        | Value::Getter(..)
        | Value::Setter(..)
        | Value::GeneratorFunction(..)
        | Value::AsyncGeneratorFunction(..) => true,
        Value::Object(obj) => {
            if obj.borrow().get_closure().is_some() {
                return true;
            }
            if slot_get(obj, &InternalSlot::Callable).is_some() {
                return true;
            }
            false
        }
        _ => false,
    }
}

/// Check if an object is an array (including proxied arrays).
/// For revoked proxies, throws TypeError per spec IsArray.
fn is_array_for_json_checked<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> Result<bool, EvalError<'gc>> {
    if is_array(mc, obj) {
        return Ok(true);
    }
    if let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
    {
        if proxy.revoked {
            return Err(raise_type_error!("Cannot perform 'IsArray' on a proxy that has been revoked").into());
        }
        if let Value::Object(target) = &*proxy.target {
            return is_array_for_json_checked(mc, target);
        }
    }
    Ok(false)
}

/// Non-throwing version for contexts where TypeError propagation isn't needed.
#[allow(dead_code)]
fn is_array_for_json<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>) -> bool {
    is_array_for_json_checked(mc, obj).unwrap_or(false)
}

/// Unwrap a boxed primitive (new String, new Number, new Boolean, Object(bigint)).
/// Uses ToNumber for Number wrappers and ToString for String wrappers per spec.
fn unwrap_boxed<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if let Some(pv_rc) = slot_get(obj, &InternalSlot::PrimitiveValue) {
        let pv = pv_rc.borrow().clone();
        match &pv {
            Value::Boolean(_) => return Ok(pv),
            Value::BigInt(_) => return Ok(pv),
            Value::Number(_) => {
                // Spec says ToNumber(value) — which triggers valueOf()
                let obj_val = Value::Object(*obj);
                let prim = crate::core::to_primitive(mc, &obj_val, "number", env)?;
                match &prim {
                    Value::Number(n) => return Ok(Value::Number(*n)),
                    _ => {
                        let n = match &prim {
                            Value::Boolean(true) => 1.0,
                            Value::Boolean(false) | Value::Null => 0.0,
                            Value::Undefined => f64::NAN,
                            Value::String(s) => {
                                let s = utf16_to_utf8(s).trim().to_string();
                                if s.is_empty() { 0.0 } else { s.parse::<f64>().unwrap_or(f64::NAN) }
                            }
                            _ => f64::NAN,
                        };
                        return Ok(Value::Number(n));
                    }
                }
            }
            Value::String(_) => {
                // Spec says ToString(value) — which triggers toString()
                let obj_val = Value::Object(*obj);
                let prim = crate::core::to_primitive(mc, &obj_val, "string", env)?;
                let s = crate::core::value_to_string(&prim);
                return Ok(Value::String(utf8_to_utf16(&s)));
            }
            _ => {}
        }
    }
    Ok(Value::Object(*obj))
}

/// Get a stable identity for an object pointer (for circular detection).
fn gc_ptr_id<'gc>(obj: &JSObjectDataPtr<'gc>) -> usize {
    use gc_arena::Gc;
    Gc::as_ptr(*obj) as *const _ as usize
}
