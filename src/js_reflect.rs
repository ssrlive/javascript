use crate::core::MutationContext;
use crate::core::{
    InternalSlot, JSObjectDataPtr, PropertyDescriptor, PropertyKey, Value, new_js_object_data, object_get_key_value, object_set_key_value,
    prepare_function_call_env, slot_get, slot_get_chained,
};
use crate::js_array::{get_array_length, set_array_length};
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};
use crate::{JSError, core::EvalError};

/// Initialize the Reflect object with all reflection methods
pub fn initialize_reflect<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let reflect_obj = new_js_object_data(mc);

    // Set [[Prototype]] to Object.prototype (spec §26.1)
    let _ = crate::core::set_internal_prototype_from_constructor(mc, &reflect_obj, env, "Object");

    // Register all methods (writable: true, enumerable: false, configurable: true)
    let methods: &[(&str, &str)] = &[
        ("apply", "Reflect.apply"),
        ("construct", "Reflect.construct"),
        ("defineProperty", "Reflect.defineProperty"),
        ("deleteProperty", "Reflect.deleteProperty"),
        ("get", "Reflect.get"),
        ("getOwnPropertyDescriptor", "Reflect.getOwnPropertyDescriptor"),
        ("getPrototypeOf", "Reflect.getPrototypeOf"),
        ("has", "Reflect.has"),
        ("isExtensible", "Reflect.isExtensible"),
        ("ownKeys", "Reflect.ownKeys"),
        ("preventExtensions", "Reflect.preventExtensions"),
        ("set", "Reflect.set"),
        ("setPrototypeOf", "Reflect.setPrototypeOf"),
    ];
    for &(name, func_name) in methods {
        object_set_key_value(mc, &reflect_obj, name, &Value::Function(func_name.to_string()))?;
        reflect_obj.borrow_mut(mc).set_non_enumerable(name);
    }

    // Symbol.toStringTag = "Reflect" { writable: false, enumerable: false, configurable: true }
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_val.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let tag_desc = crate::core::create_descriptor_object(
            mc,
            &Value::String(utf8_to_utf16("Reflect")),
            false, // writable
            false, // enumerable
            true,  // configurable
        )?;
        crate::js_object::define_property_internal(mc, &reflect_obj, PropertyKey::Symbol(*tag_sym), &tag_desc)?;
    }

    crate::core::env_set(mc, env, "Reflect", &Value::Object(reflect_obj))?;
    Ok(())
}

fn to_property_key<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: Value<'gc>,
) -> Result<PropertyKey<'gc>, EvalError<'gc>> {
    let key = match value {
        Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
        Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
        Value::BigInt(b) => PropertyKey::String(b.to_string()),
        Value::Symbol(s) => PropertyKey::Symbol(s),
        Value::Object(_) => {
            let prim = crate::core::to_primitive(mc, &value, "string", env)?;
            match prim {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(crate::core::value_to_string(&Value::Number(n))),
                Value::BigInt(b) => PropertyKey::String(b.to_string()),
                Value::Symbol(s) => PropertyKey::Symbol(s),
                other => PropertyKey::String(crate::core::value_to_string(&other)),
            }
        }
        other => PropertyKey::String(crate::core::value_to_string(&other)),
    };
    Ok(key)
}

/// OrdinaryGet with a receiver: walk the prototype chain starting from `obj`,
/// and when an accessor getter is found, call it with `receiver` as `this`.
fn reflect_get_with_receiver<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: &PropertyKey<'gc>,
    receiver: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    use crate::core::{Gc, get_own_property};

    // TypedArray [[Get]]: intercept CanonicalNumericIndexString before prototype walk
    if let PropertyKey::String(s) = key
        && let Some(ta_cell) = slot_get(obj, &InternalSlot::TypedArray)
        && let Value::TypedArray(ta) = &*ta_cell.borrow()
        && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
    {
        if !crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
            return Ok(Value::Undefined);
        }
        let idx = num_idx as usize;
        match ta.kind {
            crate::core::TypedArrayKind::BigInt64 | crate::core::TypedArrayKind::BigUint64 => {
                let size = ta.element_size();
                let byte_offset = ta.byte_offset + idx * size;
                let buffer = ta.buffer.borrow();
                let data = buffer.data.lock().unwrap();
                if byte_offset + size <= data.len() {
                    let bytes = &data[byte_offset..byte_offset + size];
                    let big_int = if matches!(ta.kind, crate::core::TypedArrayKind::BigInt64) {
                        let mut b = [0u8; 8];
                        b.copy_from_slice(bytes);
                        num_bigint::BigInt::from(i64::from_le_bytes(b))
                    } else {
                        let mut b = [0u8; 8];
                        b.copy_from_slice(bytes);
                        num_bigint::BigInt::from(u64::from_le_bytes(b))
                    };
                    return Ok(Value::BigInt(Box::new(big_int)));
                }
                return Ok(Value::Undefined);
            }
            _ => {
                let n = ta.get(idx)?;
                return Ok(Value::Number(n));
            }
        }
    }

    let mut cur = Some(*obj);
    while let Some(cur_obj) = cur {
        if let Some(val_ptr) = get_own_property(&cur_obj, key) {
            let val = val_ptr.borrow().clone();
            return match val {
                Value::Property { getter, value, .. } => {
                    if let Some(g) = getter {
                        // Call accessor getter with receiver as `this`
                        let receiver_obj = match receiver {
                            Value::Object(o) => *o,
                            _ => cur_obj,
                        };
                        crate::core::call_accessor(mc, env, &receiver_obj, &g)
                    } else if let Some(v) = value {
                        Ok(v.borrow().clone())
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                Value::Getter(body, captured_env, home_opt) => {
                    let receiver_obj = match receiver {
                        Value::Object(o) => *o,
                        _ => cur_obj,
                    };
                    crate::core::call_accessor(mc, env, &receiver_obj, &Value::Getter(body, captured_env, home_opt))
                }
                _ => Ok(val),
            };
        }
        // Move up the prototype chain
        let proto = cur_obj.borrow().prototype;
        if let Some(p) = proto {
            // If prototype is a proxy wrapper, delegate to proxy [[Get]]
            if let Some(proxy_cell) = slot_get(&p, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                let res = crate::js_proxy::proxy_get_property_with_receiver(mc, proxy, key, Some(receiver.clone()), None)?;
                return Ok(res.unwrap_or(Value::Undefined));
            }
            if Gc::ptr_eq(p, cur_obj) {
                break; // Prevent infinite loop on circular prototype chains
            }
            cur = Some(p);
        } else {
            break;
        }
    }
    Ok(Value::Undefined)
}

/// ToPropertyDescriptor (spec 6.2.6.5) that invokes accessor getters.
/// This is needed for Reflect.defineProperty where the attributes object may
/// have accessor-defined properties like `enumerable` or `writable`.
fn to_property_descriptor_with_accessors<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
) -> Result<PropertyDescriptor<'gc>, EvalError<'gc>> {
    let read_bool = |name: &str| -> Result<Option<bool>, EvalError<'gc>> {
        if crate::core::get_own_property(obj, name).is_some()
            || obj
                .borrow()
                .prototype
                .and_then(|p| crate::core::get_own_property(&p, name))
                .is_some()
        {
            let v = crate::core::get_property_with_accessors(mc, env, obj, name)?;
            Ok(Some(v.to_truthy()))
        } else {
            Ok(None)
        }
    };

    let read_val = |name: &str| -> Result<Option<Value<'gc>>, EvalError<'gc>> {
        if crate::core::get_own_property(obj, name).is_some()
            || obj
                .borrow()
                .prototype
                .and_then(|p| crate::core::get_own_property(&p, name))
                .is_some()
        {
            let v = crate::core::get_property_with_accessors(mc, env, obj, name)?;
            Ok(Some(v))
        } else {
            Ok(None)
        }
    };

    let enumerable = read_bool("enumerable")?;
    let configurable = read_bool("configurable")?;

    let value = read_val("value")?;
    let writable = read_bool("writable")?;
    let get = read_val("get")?;
    let set = read_val("set")?;

    Ok(PropertyDescriptor {
        value,
        writable,
        get,
        set,
        enumerable,
        configurable,
    })
}

/// CreateListFromArrayLike (spec 7.3.17)
/// If argumentsList is not an Object, throw TypeError.
/// Get its "length" property (may throw), then iterate 0..len collecting elements.
fn create_list_from_array_like<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj_val: &Value<'gc>,
    context: &str,
) -> Result<Vec<Value<'gc>>, EvalError<'gc>> {
    let obj = match obj_val {
        Value::Object(o) => *o,
        _ => {
            return Err(raise_type_error!(format!("{context}: CreateListFromArrayLike called on non-object")).into());
        }
    };

    // Step 3-4: Let len be ? LengthOfArrayLike(obj) — Get(obj, "length") then ToLength
    let len_val = crate::core::get_property_with_accessors(mc, env, &obj, "length")?;
    let len = match &len_val {
        Value::Number(n) => {
            let n = *n;
            if n.is_nan() || n < 0.0 {
                0usize
            } else if n.is_infinite() {
                // spec ToLength caps at 2^53-1, but we use a practical limit
                return Err(raise_type_error!(format!("{context}: argumentsList too large")).into());
            } else {
                n.trunc().max(0.0) as usize
            }
        }
        Value::Undefined | Value::Null => 0,
        _ => {
            let n = crate::core::to_number(&len_val).unwrap_or(0.0);
            if n.is_nan() || n < 0.0 { 0 } else { n.trunc().max(0.0) as usize }
        }
    };

    let mut result = Vec::with_capacity(len);
    for i in 0..len {
        if let Some(val_rc) = object_get_key_value(&obj, i) {
            result.push(val_rc.borrow().clone());
        } else {
            // Try get via prototype chain / accessors
            let v = crate::core::get_property_with_accessors(mc, env, &obj, i)?;
            result.push(v);
        }
    }
    Ok(result)
}

/// Handle Reflect object method calls
pub fn handle_reflect_method<'gc>(
    mc: &MutationContext<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "apply" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.apply requires at least 2 arguments").into());
            }
            let target = args[0].clone();
            let this_arg = args[1].clone();
            let arguments_list = if args.len() > 2 { args[2].clone() } else { Value::Undefined };

            // Step 3: Let argList be ? CreateListFromArrayLike(argumentsList).
            // Per spec, if argumentsList is not provided (undefined) or not an
            // object, throw TypeError.
            let arg_values = match &arguments_list {
                Value::Object(_) => create_list_from_array_like(mc, env, &arguments_list, "Reflect.apply")?,
                // Missing 3rd argument -> argumentsList is undefined -> TypeError
                _ => {
                    return Err(raise_type_error!("CreateListFromArrayLike called on non-object").into());
                }
            };

            // If target is a native constructor object (e.g., String), call its native handler
            if let Value::Object(obj) = &target {
                // Check for proxy wrapper first
                if let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
                    && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                {
                    return crate::js_proxy::proxy_call(mc, proxy, &this_arg, &arg_values, env);
                }
            }

            if let Value::Object(obj) = &target
                && let Some(native_rc) = slot_get_chained(obj, &InternalSlot::NativeCtor)
                && let Value::String(name_utf16) = &*native_rc.borrow()
            {
                let name = utf16_to_utf8(name_utf16);
                return crate::js_function::handle_global_function(mc, &name, &arg_values, env);
            }

            // If target is a closure (sync or async) or an object wrapping a closure, invoke appropriately
            if let Some((_params, _body, _captured_env)) = crate::core::extract_closure_from_value(&target) {
                // Detect async closure (unused here; dispatcher handles it internally)
                let _is_async = matches!(target, Value::AsyncClosure(_))
                    || (if let Value::Object(obj) = &target {
                        if let Some(cl_ptr) = obj.borrow().get_closure() {
                            matches!(&*cl_ptr.borrow(), Value::AsyncClosure(_))
                        } else {
                            false
                        }
                    } else {
                        false
                    });

                // Delegate invocation to existing call dispatcher which handles sync/async/native functions
                return crate::core::evaluate_call_dispatch(mc, env, &target, Some(&this_arg), &arg_values);
            }

            match target {
                Value::Function(func_name) => Ok(crate::js_function::handle_global_function(mc, &func_name, &arg_values, env)?),
                Value::Object(object) => {
                    // If this object wraps an internal closure (function-object), invoke it
                    if let Some(cl_rc) = object.borrow().get_closure() {
                        let cl_val = cl_rc.borrow().clone();
                        if let Some((params, body, captured_env)) = crate::core::extract_closure_from_value(&cl_val) {
                            let func_env = prepare_function_call_env(
                                mc,
                                Some(&captured_env),
                                Some(&this_arg),
                                Some(&params),
                                &arg_values,
                                None,
                                Some(env),
                            )?;
                            return crate::core::evaluate_statements(mc, &func_env, &body);
                        }
                    }
                    Err(raise_type_error!("Reflect.apply target is not callable").into())
                }
                _ => Err(raise_type_error!("Reflect.apply target is not callable").into()),
            }
        }
        "construct" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.construct requires at least 1 argument").into());
            }
            let target = args[0].clone();
            let arguments_list = if args.len() > 1 { args[1].clone() } else { Value::Undefined };
            let new_target = if args.len() > 2 { args[2].clone() } else { target.clone() };

            fn is_constructor_value<'gc>(v: &Value<'gc>) -> bool {
                match v {
                    Value::Object(obj) => {
                        // Check IsConstructor slot DIRECTLY first (for proxy wrappers that have it set)
                        if crate::core::slot_get(obj, &InternalSlot::IsConstructor).is_some() {
                            return true;
                        }

                        if let Some(proxy_cell) = crate::core::slot_get_chained(obj, &InternalSlot::Proxy)
                            && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                        {
                            return is_constructor_value(&proxy.target);
                        }

                        if let Some(bound_target) = crate::core::slot_get_chained(obj, &InternalSlot::BoundTarget) {
                            return is_constructor_value(&bound_target.borrow());
                        }

                        if obj.borrow().class_def.is_some() || crate::core::slot_get_chained(obj, &InternalSlot::IsConstructor).is_some() {
                            return true;
                        }

                        // NativeCtor alone implies constructor, unless the object
                        // is explicitly marked as callable-only (Callable = true
                        // without IsConstructor).
                        if crate::core::slot_get_chained(obj, &InternalSlot::NativeCtor).is_some() {
                            let is_callable_only = crate::core::slot_get_chained(obj, &InternalSlot::Callable)
                                .map(|v| matches!(*v.borrow(), Value::Boolean(true)))
                                .unwrap_or(false);
                            if !is_callable_only {
                                return true;
                            }
                        }

                        if let Some(cl_ptr) = obj.borrow().get_closure() {
                            let closure_is_arrow = match &*cl_ptr.borrow() {
                                Value::Closure(cl) | Value::AsyncClosure(cl) => cl.is_arrow,
                                _ => false,
                            };
                            if closure_is_arrow {
                                return false;
                            }

                            // Constructor-ness is not determined by the *value* of `.prototype`,
                            // but ordinary constructor functions typically have an own `prototype`
                            // property. This allows cases where `.prototype` is reassigned to a
                            // primitive while preserving constructor behavior.
                            if crate::core::object_get_key_value(obj, "prototype").is_none() {
                                return false;
                            }

                            if obj.borrow().get_home_object().is_some() {
                                return true;
                            }
                            return true;
                        }

                        false
                    }
                    Value::Closure(cl) | Value::AsyncClosure(cl) => !cl.is_arrow,
                    Value::Function(name) => {
                        matches!(
                            name.as_str(),
                            "Date" | "Array" | "RegExp" | "Object" | "Number" | "Boolean" | "String" | "GeneratorFunction"
                        )
                    }
                    _ => false,
                }
            }

            if !is_constructor_value(&target) {
                return Err(raise_type_error!("Reflect.construct target is not a constructor").into());
            }
            if !is_constructor_value(&new_target) {
                return Err(raise_type_error!("Reflect.construct newTarget is not a constructor").into());
            }

            // Step 4: Let args be ? CreateListFromArrayLike(argumentsList).
            let arg_values = match &arguments_list {
                Value::Object(_) => create_list_from_array_like(mc, env, &arguments_list, "Reflect.construct")?,
                _ => {
                    return Err(raise_type_error!("CreateListFromArrayLike called on non-object").into());
                }
            };

            // If target is a proxy wrapper, dispatch through proxy [[Construct]]
            if let Value::Object(obj) = &target
                && let Some(proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                return crate::js_proxy::proxy_construct(mc, proxy, &arg_values, &new_target, env);
            }

            crate::js_class::evaluate_new(mc, env, &target, &arg_values, Some(&new_target))
        }
        "defineProperty" => {
            // Spec 26.1.3 Reflect.defineProperty(target, propertyKey, attributes)
            // Step 1: If Type(target) is not Object, throw TypeError.
            let target = args.first().cloned().unwrap_or(Value::Undefined);
            if !matches!(&target, Value::Object(_)) {
                return Err(raise_type_error!("Reflect.defineProperty target must be an object").into());
            }
            // Step 2: Let key be ? ToPropertyKey(propertyKey).
            let property_key = args.get(1).cloned().unwrap_or(Value::Undefined);
            let attributes = args.get(2).cloned().unwrap_or(Value::Undefined);

            match target {
                Value::Object(obj) => {
                    let prop_key = to_property_key(mc, env, property_key)?;
                    // Step 3: Let desc be ? ToPropertyDescriptor(attributes).
                    // Must invoke getters on the attributes object (abrupt propagation).
                    let attr_obj = match &attributes {
                        Value::Object(a) => *a,
                        _ => {
                            return Err(raise_type_error!("Property descriptor must be an object").into());
                        }
                    };
                    // ToPropertyDescriptor: read properties via accessors to detect abrupt completions
                    let requested = to_property_descriptor_with_accessors(mc, env, &attr_obj)?;
                    if let PropertyKey::String(s) = &prop_key {
                        crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                    }

                    let is_module_namespace = {
                        let b = obj.borrow();
                        b.deferred_module_path.is_some() || b.deferred_cache_env.is_some() || (b.prototype.is_none() && !b.is_extensible())
                    };
                    if is_module_namespace {
                        if crate::core::validate_descriptor_for_define(mc, &requested).is_err() {
                            return Ok(Value::Boolean(false));
                        }
                        if requested.get.is_some() || requested.set.is_some() {
                            return Ok(Value::Boolean(false));
                        }

                        match &prop_key {
                            PropertyKey::String(name) => {
                                if crate::core::build_property_descriptor(mc, &obj, &prop_key).is_none() {
                                    return Ok(Value::Boolean(false));
                                }
                                if requested.configurable == Some(true)
                                    || requested.enumerable == Some(false)
                                    || requested.writable == Some(false)
                                {
                                    return Ok(Value::Boolean(false));
                                }
                                if let Some(v) = requested.value {
                                    let cur = crate::core::get_property_with_accessors(mc, env, &obj, name.as_str())?;
                                    if !crate::core::values_equal(mc, &cur, &v) {
                                        return Ok(Value::Boolean(false));
                                    }
                                }
                                return Ok(Value::Boolean(true));
                            }
                            PropertyKey::Symbol(sym) if sym.description() == Some("Symbol.toStringTag") => {
                                if requested.configurable == Some(true)
                                    || requested.enumerable == Some(true)
                                    || requested.writable == Some(true)
                                {
                                    return Ok(Value::Boolean(false));
                                }
                                if let Some(v) = requested.value
                                    && !crate::core::values_equal(mc, &v, &Value::String(utf8_to_utf16("Module")))
                                {
                                    return Ok(Value::Boolean(false));
                                }
                                return Ok(Value::Boolean(true));
                            }
                            _ => {
                                return Ok(Value::Boolean(false));
                            }
                        }
                    }

                    if crate::core::validate_descriptor_for_define(mc, &requested).is_err() {
                        return Ok(Value::Boolean(false));
                    }

                    if crate::js_array::is_array(mc, &obj)
                        && let PropertyKey::String(s) = &prop_key
                        && s == "length"
                    {
                        if requested.get.is_some() || requested.set.is_some() {
                            return Ok(Value::Boolean(false));
                        }

                        let to_number_with_hint = |value: &Value<'gc>| -> Result<f64, EvalError<'gc>> {
                            let prim = crate::core::to_primitive(mc, value, "number", env)?;
                            crate::core::to_number(&prim)
                        };

                        let old_len = get_array_length(mc, &obj).unwrap_or(0);
                        let to_uint32 = |num: f64| -> u32 {
                            if !num.is_finite() || num == 0.0 || num.is_nan() {
                                return 0;
                            }
                            let int = num.signum() * num.abs().floor();
                            let two32 = 4294967296.0_f64;
                            let mut int_mod = int % two32;
                            if int_mod < 0.0 {
                                int_mod += two32;
                            }
                            int_mod as u32
                        };

                        let mut computed_new_len: Option<usize> = None;
                        if let Some(v) = requested.value.clone() {
                            let n1 = match to_number_with_hint(&v) {
                                Ok(n) => n,
                                Err(_) => return Ok(Value::Boolean(false)),
                            };
                            let uint32_len = to_uint32(n1);
                            let number_len = match to_number_with_hint(&v) {
                                Ok(n) => n,
                                Err(_) => return Ok(Value::Boolean(false)),
                            };

                            if (uint32_len as f64) != number_len {
                                return Ok(Value::Boolean(false));
                            }
                            computed_new_len = Some(uint32_len as usize);
                        }

                        if requested.configurable == Some(true) || requested.enumerable == Some(true) {
                            return Ok(Value::Boolean(false));
                        }

                        let length_writable = obj.borrow().is_writable("length");
                        if requested.writable == Some(true) && !length_writable {
                            return Ok(Value::Boolean(false));
                        }

                        if let Some(new_len) = computed_new_len {
                            if !length_writable && new_len != old_len {
                                return Ok(Value::Boolean(false));
                            }
                            if set_array_length(mc, &obj, new_len).is_err() {
                                return Ok(Value::Boolean(false));
                            }
                        }

                        if requested.writable == Some(false) {
                            obj.borrow_mut(mc).set_non_writable("length");
                        } else if requested.writable == Some(true) {
                            obj.borrow_mut(mc).set_writable("length");
                        }

                        return Ok(Value::Boolean(true));
                    }

                    // If the target is a proxy, invoke the defineProperty trap
                    if let Some(proxy_cell) = slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        let trap_result = crate::js_proxy::apply_proxy_trap(
                            mc,
                            proxy,
                            "defineProperty",
                            vec![
                                (*proxy.target).clone(),
                                crate::js_proxy::property_key_to_value_pub(&prop_key),
                                Value::Object(attr_obj),
                            ],
                            || {
                                // Default: forward to target
                                if let Value::Object(target_inner) = &*proxy.target {
                                    // If target is also a proxy, recurse through its [[DefineOwnProperty]]
                                    if let Some(inner_proxy_cell) = slot_get(target_inner, &InternalSlot::Proxy)
                                        && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                                    {
                                        return Ok(Value::Boolean(crate::js_proxy::proxy_define_own_property(
                                            mc,
                                            inner_proxy,
                                            &prop_key,
                                            &attr_obj,
                                        )?));
                                    }
                                    match crate::js_object::define_property_internal(mc, target_inner, &prop_key, &attr_obj) {
                                        Ok(()) => Ok(Value::Boolean(true)),
                                        Err(_) => Ok(Value::Boolean(false)),
                                    }
                                } else {
                                    Ok(Value::Boolean(false))
                                }
                            },
                        )?;
                        let success = match trap_result {
                            Value::Boolean(b) => b,
                            _ => trap_result.to_truthy(),
                        };

                        if success {
                            // Invariant checks per spec 10.5.6 steps 14-20
                            if let Value::Object(target_obj) = &*proxy.target {
                                let target_desc = crate::core::get_own_property(target_obj, &prop_key);
                                let extensible_target = target_obj.borrow().is_extensible();

                                let desc_configurable = crate::core::object_get_key_value(&attr_obj, "configurable")
                                    .map(|v| v.borrow().to_truthy())
                                    .unwrap_or(true);

                                if target_desc.is_none() {
                                    // Step 16: If targetDesc is undefined
                                    if !extensible_target {
                                        return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for defining property on non-extensible target which does not have the property").into());
                                    }
                                    // Step 16b: If settingConfigFalse is true, throw TypeError
                                    if !desc_configurable {
                                        return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for defining non-configurable property which does not exist on the target").into());
                                    }
                                } else {
                                    // Step 17-20: Validate compatibility
                                    let target_configurable = target_obj.borrow().is_configurable(&prop_key);

                                    // If desc is non-configurable but target property is configurable
                                    if !desc_configurable && target_configurable {
                                        return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for defining non-configurable property which is configurable in the target").into());
                                    }

                                    // Step 16c (spec 10.5.6 step 20): If targetDesc is data, non-configurable,
                                    // writable, and Desc has [[Writable]] = false → throw TypeError.
                                    // This check is independent of Desc.configurable.
                                    if !target_configurable {
                                        let target_writable = target_obj.borrow().is_writable(&prop_key);
                                        let desc_writable =
                                            crate::core::object_get_key_value(&attr_obj, "writable").map(|v| v.borrow().to_truthy());
                                        if target_writable && desc_writable == Some(false) {
                                            return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for defining non-configurable, non-writable property which is writable in the target").into());
                                        }
                                    }
                                }
                            }
                        }

                        return Ok(Value::Boolean(success));
                    }

                    // TypedArray [[DefineOwnProperty]] — ES2024 §10.4.5.3
                    if let PropertyKey::String(s) = &prop_key
                        && slot_get(&obj, &InternalSlot::TypedArray).is_some()
                        && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
                    {
                        // If it's a CanonicalNumericIndexString, we handle it entirely here
                        // Check IsValidIntegerIndex
                        let valid = if let Some(ta_cell) = slot_get(&obj, &InternalSlot::TypedArray)
                            && let Value::TypedArray(ta) = &*ta_cell.borrow()
                        {
                            crate::js_typedarray::is_valid_integer_index(ta, num_idx)
                        } else {
                            false
                        };
                        if !valid {
                            return Ok(Value::Boolean(false));
                        }
                        // If IsAccessorDescriptor(Desc), return false
                        if requested.get.is_some() || requested.set.is_some() {
                            return Ok(Value::Boolean(false));
                        }
                        // If Desc.[[Configurable]] is false, return false
                        if requested.configurable == Some(false) {
                            return Ok(Value::Boolean(false));
                        }
                        // If Desc.[[Enumerable]] is false, return false
                        if requested.enumerable == Some(false) {
                            return Ok(Value::Boolean(false));
                        }
                        // If Desc.[[Writable]] is false, return false
                        if requested.writable == Some(false) {
                            return Ok(Value::Boolean(false));
                        }
                        // If Desc has a [[Value]] field, perform IntegerIndexedElementSet
                        if let Some(val) = requested.value
                            && let Some(ta_cell) = slot_get(&obj, &InternalSlot::TypedArray)
                            && let Value::TypedArray(ta) = &*ta_cell.borrow()
                        {
                            let idx = num_idx as usize;
                            let is_bigint_ta = crate::js_typedarray::is_bigint_typed_array(&ta.kind);
                            if is_bigint_ta {
                                let n = crate::js_typedarray::to_bigint_i64(mc, env, &val)?;
                                ta.set_bigint(mc, idx, n)?;
                            } else {
                                let n = crate::core::to_number_with_env(mc, env, &val)?;
                                ta.set(mc, idx, n)?;
                            }
                        }
                        return Ok(Value::Boolean(true));
                    }

                    match crate::js_object::define_property_internal(mc, &obj, &prop_key, &attr_obj) {
                        Ok(()) => Ok(Value::Boolean(true)),
                        Err(_e) => Ok(Value::Boolean(false)),
                    }
                }
                _ => Err(raise_type_error!("Reflect.defineProperty target must be an object").into()),
            }
        }
        "deleteProperty" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.deleteProperty requires 2 arguments").into());
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = to_property_key(mc, env, property_key)?;
                    // Check for proxy wrapper
                    if let Some(proxy_cell) = slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        return Ok(Value::Boolean(crate::js_proxy::proxy_delete_property(mc, proxy, &prop_key)?));
                    }
                    if let PropertyKey::String(s) = &prop_key {
                        crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                    }
                    if obj.borrow().non_configurable.contains(&prop_key) {
                        return Ok(Value::Boolean(false));
                    }
                    let _ = obj.borrow_mut(mc).properties.shift_remove(&prop_key);
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.deleteProperty target must be an object").into()),
            }
        }
        "get" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.get requires at least 2 arguments").into());
            }
            let target = args[0].clone();
            let property_key = args[1].clone();
            let receiver = if args.len() > 2 { args[2].clone() } else { target.clone() };

            match target {
                Value::Object(obj) => {
                    let prop_key = to_property_key(mc, env, property_key)?;
                    if let PropertyKey::String(s) = &prop_key {
                        crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                    }

                    // If target is a proxy, delegate to proxy [[Get]] with the receiver
                    if let Some(proxy_cell) = slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        let res = crate::js_proxy::proxy_get_property_with_receiver(mc, proxy, &prop_key, Some(receiver.clone()), None)?;
                        return Ok(res.unwrap_or(Value::Undefined));
                    }

                    // OrdinaryGet with receiver: walk prototype chain, call accessor getter with receiver as `this`
                    reflect_get_with_receiver(mc, env, &obj, &prop_key, &receiver)
                }
                _ => Err(raise_type_error!("Reflect.get target must be an object").into()),
            }
        }
        "getOwnPropertyDescriptor" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.getOwnPropertyDescriptor requires 2 arguments").into());
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    let prop_key = to_property_key(mc, env, property_key)?;
                    if let PropertyKey::String(s) = &prop_key {
                        crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                    }

                    // If the target is a proxy wrapper, delegate to proxy GOPD trap
                    if let Some(proxy_cell) = crate::core::slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        match crate::js_proxy::proxy_get_own_property_descriptor(mc, proxy, &prop_key)? {
                            Some(desc_obj) => {
                                crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                                return Ok(Value::Object(desc_obj));
                            }
                            None => return Ok(Value::Undefined),
                        }
                    }

                    if let Some(_value_rc) = object_get_key_value(&obj, &prop_key) {
                        if let Some(mut pd) = crate::core::build_property_descriptor(mc, &obj, &prop_key) {
                            let is_deferred_namespace = obj.borrow().deferred_module_path.is_some();
                            let is_accessor_descriptor = pd.get.is_some() || pd.set.is_some();
                            let needs_hydration = (is_deferred_namespace || !is_accessor_descriptor)
                                && (pd.value.is_none() || matches!(pd.value, Some(Value::Undefined)));
                            if needs_hydration && let PropertyKey::String(s) = &prop_key {
                                let hydrated = crate::core::get_property_with_accessors(mc, env, &obj, s.as_str())?;
                                if !matches!(hydrated, Value::Undefined) {
                                    pd.value = Some(hydrated);
                                    pd.get = None;
                                    pd.set = None;
                                    if pd.writable.is_none() {
                                        pd.writable = Some(true);
                                    }
                                } else {
                                    let (module_path, cache_env) = {
                                        let b = obj.borrow();
                                        (b.deferred_module_path.clone(), b.deferred_cache_env)
                                    };
                                    if let (Some(module_path), Some(cache_env)) = (module_path, cache_env)
                                        && let Ok(Value::Object(exports_obj)) =
                                            crate::js_module::load_module(mc, module_path.as_str(), None, Some(cache_env))
                                        && let Some(v) = object_get_key_value(&exports_obj, s)
                                    {
                                        pd.value = Some(v.borrow().clone());
                                        pd.get = None;
                                        pd.set = None;
                                        if pd.writable.is_none() {
                                            pd.writable = Some(true);
                                        }
                                    }
                                }
                            }
                            let desc_obj = pd.to_object(mc)?;
                            crate::core::set_internal_prototype_from_constructor(mc, &desc_obj, env, "Object")?;
                            Ok(Value::Object(desc_obj))
                        } else {
                            Ok(Value::Undefined)
                        }
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Err(raise_type_error!("Reflect.getOwnPropertyDescriptor target must be an object").into()),
            }
        }
        "getPrototypeOf" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.getPrototypeOf requires 1 argument").into());
            }
            match &args[0] {
                Value::Object(obj) => {
                    if let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        return crate::js_proxy::proxy_get_prototype_of(mc, proxy);
                    }
                    if let Some(proto_rc) = obj.borrow().prototype {
                        Ok(Value::Object(proto_rc))
                    } else {
                        Ok(Value::Null)
                    }
                }
                _ => Err(raise_type_error!("Reflect.getPrototypeOf target must be an object").into()),
            }
        }
        "has" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.has requires 2 arguments").into());
            }
            let target = args[0].clone();
            let property_key = args[1].clone();

            match target {
                Value::Object(obj) => {
                    // Check for proxy wrapper
                    if let Some(proxy_cell) = slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        let prop_key = to_property_key(mc, env, property_key)?;
                        return Ok(Value::Boolean(crate::js_proxy::proxy_has_property(mc, proxy, prop_key)?));
                    }
                    let prop_key = to_property_key(mc, env, property_key)?;
                    if let PropertyKey::String(s) = &prop_key {
                        crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, Some(s.as_str()))?;
                    }

                    // TypedArray [[HasProperty]]: intercept CanonicalNumericIndexString
                    if let PropertyKey::String(s) = &prop_key
                        && let Some(ta_cell) = slot_get(&obj, &InternalSlot::TypedArray)
                        && let Value::TypedArray(ta) = &*ta_cell.borrow()
                        && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
                    {
                        return Ok(Value::Boolean(crate::js_typedarray::is_valid_integer_index(ta, num_idx)));
                    }

                    // OrdinaryHasProperty: check own, then walk prototype chain
                    let has_own = object_get_key_value(&obj, &prop_key).is_some();
                    if has_own {
                        return Ok(Value::Boolean(true));
                    }
                    // Walk prototype chain (may hit Proxy traps)
                    let mut cur_proto = obj.borrow().prototype;
                    while let Some(proto) = cur_proto {
                        // Check if proto is a Proxy
                        if let Some(proxy_cell) = slot_get(&proto, &InternalSlot::Proxy)
                            && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                        {
                            return Ok(Value::Boolean(crate::js_proxy::proxy_has_property(mc, proxy, prop_key)?));
                        }
                        if object_get_key_value(&proto, &prop_key).is_some() {
                            return Ok(Value::Boolean(true));
                        }
                        cur_proto = proto.borrow().prototype;
                    }
                    Ok(Value::Boolean(false))
                }
                _ => Err(raise_type_error!("Reflect.has target must be an object").into()),
            }
        }
        "isExtensible" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.isExtensible requires 1 argument").into());
            }
            let target = args[0].clone();

            match target {
                Value::Object(obj) => {
                    if let Some(proxy_cell) = slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        return Ok(Value::Boolean(crate::js_proxy::proxy_is_extensible(mc, proxy)?));
                    }
                    Ok(Value::Boolean(obj.borrow().is_extensible()))
                }
                _ => Err(raise_type_error!("Reflect.isExtensible target must be an object").into()),
            }
        }
        "ownKeys" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.ownKeys requires 1 argument").into());
            }
            match args[0] {
                Value::Object(obj) => {
                    crate::js_module::ensure_deferred_namespace_evaluated(mc, env, &obj, None)?;

                    // Check for proxy and call proxy_own_keys directly to preserve
                    // EvalError::Throw (avoids lossy EvalError→JSError roundtrip).
                    let keys_vec: Vec<crate::core::PropertyKey> = if let Some(proxy_cell) =
                        crate::core::slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        crate::js_proxy::proxy_own_keys(mc, proxy)?
                    } else {
                        crate::core::ordinary_own_property_keys(&obj)
                    };

                    let mut keys: Vec<Value> = Vec::new();
                    for key in keys_vec.iter() {
                        match key {
                            crate::core::PropertyKey::String(s) => keys.push(Value::String(utf8_to_utf16(s))),
                            crate::core::PropertyKey::Symbol(sd) => keys.push(Value::Symbol(*sd)),
                            _ => {}
                        }
                    }
                    let keys_len = keys.len();
                    let result_obj = crate::js_array::create_array(mc, env)?;
                    for (i, key) in keys.into_iter().enumerate() {
                        object_set_key_value(mc, &result_obj, i, &key)?;
                    }
                    set_array_length(mc, &result_obj, keys_len)?;
                    Ok(Value::Object(result_obj))
                }
                _ => Err(raise_type_error!("Reflect.ownKeys target must be an object").into()),
            }
        }
        "preventExtensions" => {
            if args.is_empty() {
                return Err(raise_type_error!("Reflect.preventExtensions requires 1 argument").into());
            }
            let target = args[0].clone();

            match target {
                Value::Object(obj) => {
                    if let Some(proxy_cell) = slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        return Ok(Value::Boolean(crate::js_proxy::proxy_prevent_extensions(mc, proxy)?));
                    }
                    obj.borrow_mut(mc).prevent_extensions();
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.preventExtensions target must be an object").into()),
            }
        }
        "set" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.set requires at least 2 arguments").into());
            }
            let target = args[0].clone();
            let property_key = args[1].clone();
            let value = if args.len() > 2 { args[2].clone() } else { Value::Undefined };
            let receiver = if args.len() > 3 { args[3].clone() } else { target.clone() };

            match target {
                Value::Object(obj) => {
                    let prop_key = to_property_key(mc, env, property_key)?;

                    // Per spec: Reflect.set(target, P, V, Receiver) → target.[[Set]](P, V, Receiver)
                    // If target is a proxy, call its [[Set]] trap
                    if let Some(proxy_cell) = crate::core::slot_get(&obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        let ok = crate::js_proxy::proxy_set_property_with_receiver(mc, proxy, &prop_key, &value, Some(&receiver))?;
                        return Ok(Value::Boolean(ok));
                    }

                    // TypedArray [[Set]] — ES2024 §10.4.5.5
                    if let PropertyKey::String(s) = &prop_key
                        && let Some(ta_cell) = slot_get(&obj, &InternalSlot::TypedArray)
                        && let Value::TypedArray(ta) = &*ta_cell.borrow()
                        && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
                    {
                        // Check if receiver is same object as target
                        let same = match &receiver {
                            Value::Object(recv_obj) => crate::core::Gc::ptr_eq(obj, *recv_obj),
                            _ => false,
                        };
                        if same {
                            // IntegerIndexedElementSet(O, numericIndex, V)
                            if crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                                let idx = num_idx as usize;
                                if crate::js_typedarray::is_bigint_typed_array(&ta.kind) {
                                    let n = crate::js_typedarray::to_bigint_i64(mc, env, &value)?;
                                    ta.set_bigint(mc, idx, n)?;
                                } else {
                                    let n = crate::core::to_number_with_env(mc, env, &value)?;
                                    ta.set(mc, idx, n)?;
                                }
                            }
                            return Ok(Value::Boolean(true));
                        } else {
                            // Receiver !== target
                            if !crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                                return Ok(Value::Boolean(true));
                            }
                            // Valid index, receiver !== target: fall through to OrdinarySet
                        }
                    }

                    // Non-proxy target: OrdinarySet(target, P, V, Receiver)
                    let ok = crate::js_proxy::ordinary_set(mc, &obj, &prop_key, &value, &receiver, env)?;
                    Ok(Value::Boolean(ok))
                }
                _ => Err(raise_type_error!("Reflect.set target must be an object").into()),
            }
        }
        "setPrototypeOf" => {
            if args.len() < 2 {
                return Err(raise_type_error!("Reflect.setPrototypeOf requires 2 arguments").into());
            }
            match &args[0] {
                Value::Object(obj) => {
                    // Step 2: If Type(proto) is not Object and proto is not null, throw TypeError
                    let proto_val = &args[1];
                    let new_proto: Option<JSObjectDataPtr<'gc>> = match proto_val {
                        Value::Object(proto_obj) => Some(*proto_obj),
                        Value::Null => None,
                        Value::Function(func_name) => {
                            // Functions are objects in JS; wrap in object shell
                            let fn_obj = new_js_object_data(mc);
                            if let Some(func_ctor_val) = object_get_key_value(env, "Function")
                                && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                                && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
                                && let Value::Object(func_proto) = &*proto_val.borrow()
                            {
                                fn_obj.borrow_mut(mc).prototype = Some(*func_proto);
                            }
                            fn_obj
                                .borrow_mut(mc)
                                .set_closure(Some(crate::core::new_gc_cell_ptr(mc, Value::Function(func_name.clone()))));
                            Some(fn_obj)
                        }
                        _ => return Err(raise_type_error!("Reflect.setPrototypeOf prototype must be an object or null").into()),
                    };

                    // Check for proxy
                    if let Some(proxy_cell) = slot_get(obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        return Ok(Value::Boolean(crate::js_proxy::proxy_set_prototype_of(mc, proxy, &args[1])?));
                    }

                    // OrdinarySetPrototypeOf (spec 10.1.2)
                    // Immutable prototype exotic objects (e.g. Object.prototype):
                    // [[SetPrototypeOf]](V) returns true only if SameValue(V, current), else false.
                    if crate::core::slot_has(obj, &InternalSlot::ImmutablePrototype) {
                        let current_proto = obj.borrow().prototype;
                        let same = match (&new_proto, &current_proto) {
                            (None, None) => true,
                            (Some(a), Some(b)) => crate::core::Gc::ptr_eq(*a, *b),
                            _ => false,
                        };
                        return Ok(Value::Boolean(same));
                    }

                    let current_proto = obj.borrow().prototype;
                    let is_extensible = obj.borrow().is_extensible();

                    // Step 5: If SameValue(V, current) return true
                    let same = match (&new_proto, &current_proto) {
                        (None, None) => true,
                        (Some(a), Some(b)) => crate::core::Gc::ptr_eq(*a, *b),
                        _ => false,
                    };
                    if same {
                        return Ok(Value::Boolean(true));
                    }

                    // Step 6: If not extensible, return false
                    if !is_extensible {
                        return Ok(Value::Boolean(false));
                    }

                    // Step 8: Cycle detection — walk the proto chain of V looking for O
                    if let Some(ref new_p) = new_proto {
                        let mut p = Some(*new_p);
                        while let Some(pp) = p {
                            if crate::core::Gc::ptr_eq(pp, *obj) {
                                // Cycle detected
                                return Ok(Value::Boolean(false));
                            }
                            // If pp's [[GetPrototypeOf]] is not the ordinary one (e.g., Proxy),
                            // stop the loop (spec says let done = true).
                            if crate::core::slot_has(&pp, &InternalSlot::Proxy) {
                                break;
                            }
                            p = pp.borrow().prototype;
                        }
                    }

                    // Step 9: Set the prototype
                    obj.borrow_mut(mc).prototype = new_proto;
                    Ok(Value::Boolean(true))
                }
                _ => Err(raise_type_error!("Reflect.setPrototypeOf target must be an object").into()),
            }
        }
        _ => Err(raise_eval_error!("Unknown Reflect method").into()),
    }
}
