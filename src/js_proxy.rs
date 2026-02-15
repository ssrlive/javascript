use crate::core::{ClosureData, Expr, JSProxy, Statement, StatementKind, prepare_closure_call_env};
use crate::core::{Gc, MutationContext, new_gc_cell_ptr};
use crate::env_set;
use crate::unicode::utf16_to_utf8;
use crate::{
    core::{
        EvalError, JSObjectDataPtr, PropertyKey, Value, evaluate_statements, extract_closure_from_value, new_js_object_data,
        object_get_key_value, object_set_key_value,
    },
    error::JSError,
    unicode::utf8_to_utf16,
};

/// Handle Proxy constructor calls (arguments already evaluated)
pub(crate) fn handle_proxy_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.len() != 2 {
        return Err(raise_eval_error!("Proxy constructor requires exactly two arguments: target and handler").into());
    }

    let target = args[0].clone();
    let handler = args[1].clone();

    // Create the proxy
    let proxy = Gc::new(
        mc,
        JSProxy {
            target: Box::new(target),
            handler: Box::new(handler),
            revoked: false,
        },
    );

    // Create a wrapper object for the Proxy
    let proxy_obj = new_js_object_data(mc);
    // Store the actual proxy data
    proxy_obj.borrow_mut(mc).insert(
        PropertyKey::String("__proxy__".to_string()),
        new_gc_cell_ptr(mc, Value::Proxy(proxy)),
    );

    Ok(Value::Object(proxy_obj))
}

/// Handle Proxy.revocable static method (arguments already evaluated)
pub(crate) fn handle_proxy_revocable<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.len() != 2 {
        return Err(raise_eval_error!("Proxy.revocable requires exactly two arguments: target and handler").into());
    }

    let target = args[0].clone();
    let handler = args[1].clone();

    // Create the proxy
    let proxy = Gc::new(
        mc,
        JSProxy {
            target: Box::new(target.clone()),
            handler: Box::new(handler.clone()),
            revoked: false,
        },
    );

    // Create the revoke function as a closure that captures the proxy
    let revoke_body = vec![
        // Call internal builtin to perform the actual revoke (mutates underlying proxy wrapper)
        Statement::from(StatementKind::Expr(Expr::Call(
            Box::new(Expr::Var("__internal_revoke".to_string(), None, None)),
            Vec::new(),
        ))),
    ];

    let revoke_env = new_js_object_data(mc);
    revoke_env
        .borrow_mut(mc)
        .insert("__revoke_proxy", new_gc_cell_ptr(mc, Value::Proxy(proxy)));

    // Create a wrapper object for the Proxy
    let proxy_wrapper = new_js_object_data(mc);
    // Store the actual proxy data
    proxy_wrapper.borrow_mut(mc).insert(
        PropertyKey::String("__proxy__".to_string()),
        new_gc_cell_ptr(mc, Value::Proxy(proxy)),
    );

    // Also capture the wrapper object so the internal revoke helper can replace the stored proxy
    revoke_env
        .borrow_mut(mc)
        .insert("__proxy_wrapper", new_gc_cell_ptr(mc, Value::Object(proxy_wrapper)));

    // Provide a callable function in the revoke env that dispatches to the internal revoke helper
    revoke_env.borrow_mut(mc).insert(
        "__internal_revoke",
        new_gc_cell_ptr(mc, Value::Function("Proxy.__internal_revoke".to_string())),
    );

    let revoke_func = Value::Closure(Gc::new(mc, ClosureData::new(&[], &revoke_body, Some(revoke_env), None)));

    // Create the revocable result object
    let result_obj = new_js_object_data(mc);
    object_set_key_value(mc, &result_obj, "proxy", &Value::Object(proxy_wrapper))?;
    object_set_key_value(mc, &result_obj, "revoke", &revoke_func)?;

    Ok(Value::Object(result_obj))
}

/// Apply a proxy trap if available, otherwise fall back to default behavior
pub(crate) fn apply_proxy_trap<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    trap_name: &str,
    args: Vec<Value<'gc>>,
    default_fn: impl FnOnce() -> Result<Value<'gc>, EvalError<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if proxy.revoked {
        return Err(raise_eval_error!("Cannot perform operation on a revoked proxy").into());
    }

    // Check if handler has the trap
    if let Value::Object(handler_obj) = &*proxy.handler
        && let Some(trap_val) = object_get_key_value(handler_obj, trap_name)
    {
        let trap = match &*trap_val.borrow() {
            Value::Property { value: Some(v), .. } => v.borrow().clone(),
            other => other.clone(),
        };

        // Accept either a direct `Value::Closure` or a function-object that
        // stores the executable closure in the internal closure slot.
        if let Some((params, body, captured_env)) = extract_closure_from_value(&trap) {
            // Create execution environment for the trap and bind parameters
            let trap_env = prepare_closure_call_env(mc, Some(&captured_env), Some(&params), &args, None)?;

            // Evaluate the body
            return evaluate_statements(mc, &trap_env, &body);
        } else if matches!(trap, Value::Function(_)) {
            // For now, we don't handle built-in functions as traps
            // Fall through to default
        } else {
            // Not a callable trap, fall through to default
        }
    }

    // No trap or trap not callable, use default behavior
    default_fn()
}

/// Obtain the "ownKeys" result for a proxy by invoking the trap (if present)
/// and converting the returned array-like into a vector of PropertyKey.
pub(crate) fn proxy_own_keys<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
) -> Result<Vec<crate::core::PropertyKey<'gc>>, EvalError<'gc>> {
    log::trace!("proxy_own_keys: proxy_ptr={:p}", Gc::as_ptr(*proxy));
    // If trap exists it will be invoked; default behavior returns the target's own keys
    let res = apply_proxy_trap(mc, proxy, "ownKeys", vec![(*proxy.target).clone()], || {
        // Default: collect own property keys from target (string and symbol keys)
        let mut keys: Vec<Value> = Vec::new();
        if let Value::Object(obj) = &*proxy.target {
            for k in obj.borrow().properties.keys() {
                match k {
                    crate::core::PropertyKey::String(s) => keys.push(Value::String(utf8_to_utf16(s))),
                    crate::core::PropertyKey::Symbol(sd) => keys.push(Value::Symbol(*sd)),
                    _ => {}
                }
            }
        }

        // Build an array-like result object without needing an `env`
        let result_obj = crate::core::new_js_object_data(mc);
        let keys_len = keys.len();
        for (i, key) in keys.into_iter().enumerate() {
            crate::core::object_set_key_value(mc, &result_obj, i, &key)?;
        }
        crate::core::object_set_key_value(mc, &result_obj, "length", &Value::Number(keys_len as f64))?;
        Ok(Value::Object(result_obj))
    })?;

    log::trace!("proxy_own_keys: trap returned {:?}", res);

    // Convert the returned array-like into PropertyKey vector
    match res {
        Value::Object(arr_obj) => {
            let len = crate::js_array::get_array_length(mc, &arr_obj).unwrap_or(0);
            let mut out: Vec<crate::core::PropertyKey<'gc>> = Vec::new();
            for i in 0..len {
                if let Some(val_rc) = crate::core::object_get_key_value(&arr_obj, i) {
                    match &*val_rc.borrow() {
                        Value::String(s) => out.push(crate::core::PropertyKey::String(utf16_to_utf8(s))),
                        Value::Symbol(sd) => out.push(crate::core::PropertyKey::Symbol(*sd)),
                        other => {
                            return Err(raise_type_error!(format!("Invalid value returned from proxy ownKeys trap: {:?}", other)).into());
                        }
                    }
                } else {
                    return Err(raise_type_error!("Proxy ownKeys trap returned a non-dense array").into());
                }
            }
            // Per CopyDataProperties / spec behavior, callers performing CopyDataProperties
            // (such as object-rest/spread or destructuring) must call [[GetOwnProperty]] for
            // each key returned by the ownKeys trap. The proxy helper only returns the key
            // list here; callers should invoke `proxy_get_own_property_descriptor` as needed.
            Ok(out)
        }
        _ => Err(raise_type_error!("Proxy ownKeys trap did not return an object").into()),
    }
}

/// Get property from proxy target, applying get trap if available
pub(crate) fn proxy_get_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    let result = apply_proxy_trap(mc, proxy, "get", vec![(*proxy.target).clone(), property_key_to_value(key)], || {
        // Default behavior: get property from target
        match &*proxy.target {
            Value::Object(obj) => {
                let val_opt = object_get_key_value(obj, key);
                match val_opt {
                    Some(val_rc) => {
                        let unwrapped = match &*val_rc.borrow() {
                            Value::Property { value: Some(v), .. } => v.borrow().clone(),
                            Value::Property { value: None, .. } => Value::Undefined,
                            other => other.clone(),
                        };
                        Ok(unwrapped)
                    }
                    None => Ok(Value::Undefined),
                }
            }
            _ => Ok(Value::Undefined), // Non-objects don't have properties
        }
    })?;

    match result {
        Value::Undefined => Ok(None),
        val => Ok(Some(val)),
    }
}

/// Call the getOwnPropertyDescriptor trap (or default) and return the descriptor's [[Enumerable]] value
/// as Some(true/false) if descriptor exists, or None if it is undefined.
pub(crate) fn proxy_get_own_property_descriptor<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &crate::core::PropertyKey<'gc>,
) -> Result<Option<bool>, EvalError<'gc>> {
    let res = apply_proxy_trap(
        mc,
        proxy,
        "getOwnPropertyDescriptor",
        vec![(*proxy.target).clone(), property_key_to_value(key)],
        || {
            // Default: return an object descriptor for target's own property, or undefined
            match &*proxy.target {
                Value::Object(obj) => {
                    if let Some(val_rc) = object_get_key_value(obj, key) {
                        let desc_obj = crate::core::new_js_object_data(mc);
                        crate::core::object_set_key_value(mc, &desc_obj, "value", &val_rc.borrow().clone())?;
                        // Use object's enumerable flag for default
                        let is_enum = obj.borrow().is_enumerable(key);
                        crate::core::object_set_key_value(mc, &desc_obj, "enumerable", &Value::Boolean(is_enum))?;
                        crate::core::object_set_key_value(mc, &desc_obj, "writable", &Value::Boolean(true))?;
                        crate::core::object_set_key_value(mc, &desc_obj, "configurable", &Value::Boolean(true))?;
                        Ok(Value::Object(desc_obj))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Ok(Value::Undefined),
            }
        },
    )?;

    match res {
        Value::Undefined => Ok(None),
        Value::Object(desc_obj) => {
            if let Some(enumerable_rc) = object_get_key_value(&desc_obj, "enumerable") {
                match &*enumerable_rc.borrow() {
                    Value::Boolean(b) => Ok(Some(*b)),
                    _ => Ok(Some(false)),
                }
            } else {
                Ok(Some(false))
            }
        }
        _ => Err(raise_type_error!("Proxy getOwnPropertyDescriptor trap returned non-object").into()),
    }
}

/// Set property on proxy target, applying set trap if available
pub(crate) fn proxy_set_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    value: &Value<'gc>,
) -> Result<bool, EvalError<'gc>> {
    let result = apply_proxy_trap(
        mc,
        proxy,
        "set",
        vec![(*proxy.target).clone(), property_key_to_value(key), value.clone()],
        || {
            // Default behavior: set property on target
            match &*proxy.target {
                Value::Object(obj) => {
                    object_set_key_value(mc, obj, key, value)?;
                    Ok(Value::Boolean(true))
                }
                _ => Ok(Value::Boolean(false)), // Non-objects can't have properties set
            }
        },
    )?;

    match result {
        Value::Boolean(b) => Ok(b),
        _ => Ok(true), // Non-boolean return from trap is treated as true
    }
}

/// Define a data property on proxy target, applying defineProperty trap if available
pub(crate) fn proxy_define_data_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    value: &Value<'gc>,
) -> Result<bool, EvalError<'gc>> {
    let desc_obj = new_js_object_data(mc);
    object_set_key_value(mc, &desc_obj, "value", value)?;
    object_set_key_value(mc, &desc_obj, "writable", &Value::Boolean(true))?;
    object_set_key_value(mc, &desc_obj, "enumerable", &Value::Boolean(true))?;
    object_set_key_value(mc, &desc_obj, "configurable", &Value::Boolean(true))?;

    let result = apply_proxy_trap(
        mc,
        proxy,
        "defineProperty",
        vec![(*proxy.target).clone(), property_key_to_value(key), Value::Object(desc_obj)],
        || match &*proxy.target {
            Value::Object(obj) => {
                let target_desc = crate::core::create_descriptor_object(mc, value, true, true, true)?;
                crate::js_object::define_property_internal(mc, obj, key.clone(), &target_desc)?;
                Ok(Value::Boolean(true))
            }
            _ => Ok(Value::Boolean(false)),
        },
    )?;

    match result {
        Value::Boolean(b) => Ok(b),
        _ => Ok(true),
    }
}

/// Check if property exists on proxy target, applying has trap if available
pub(crate) fn proxy_has_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: impl Into<PropertyKey<'gc>>,
) -> Result<bool, EvalError<'gc>> {
    let key = key.into();
    let result = apply_proxy_trap(mc, proxy, "has", vec![(*proxy.target).clone(), property_key_to_value(&key)], || {
        // Default behavior: check if property exists on target
        match &*proxy.target {
            Value::Object(obj) => Ok(Value::Boolean(object_get_key_value(obj, key).is_some())),
            _ => Ok(Value::Boolean(false)), // Non-objects don't have properties
        }
    })?;

    match result {
        Value::Boolean(b) => Ok(b),
        _ => Ok(false), // Non-boolean return from trap is treated as false
    }
}

/// Delete property from proxy target, applying deleteProperty trap if available
pub(crate) fn proxy_delete_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
) -> Result<bool, EvalError<'gc>> {
    let result = apply_proxy_trap(
        mc,
        proxy,
        "deleteProperty",
        vec![(*proxy.target).clone(), property_key_to_value(key)],
        || {
            // Default behavior: delete property from target
            match &*proxy.target {
                Value::Object(obj) => {
                    let mut obj_borrow = obj.borrow_mut(mc);
                    let existed = obj_borrow.properties.contains_key(key);
                    obj_borrow.properties.shift_remove(key);
                    Ok(Value::Boolean(existed))
                }
                _ => Ok(Value::Boolean(false)), // Non-objects don't have properties
            }
        },
    )?;

    match result {
        Value::Boolean(b) => Ok(b),
        _ => Ok(false), // Non-boolean return from trap is treated as false
    }
}

/// Helper function to convert PropertyKey to Value for trap arguments
fn property_key_to_value<'gc>(key: &PropertyKey<'gc>) -> Value<'gc> {
    match key {
        PropertyKey::String(s) => Value::String(utf8_to_utf16(s)),
        PropertyKey::Symbol(sd) => Value::Symbol(*sd),
        PropertyKey::Private(..) => unreachable!("Private keys should not be passed to proxy traps"),
    }
}

/// Initialize Proxy constructor and prototype
pub fn initialize_proxy<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let proxy_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &proxy_ctor, "__is_constructor", &Value::Boolean(true))?;
    object_set_key_value(mc, &proxy_ctor, "__native_ctor", &Value::String(utf8_to_utf16("Proxy")))?;

    // Register revocable static method
    object_set_key_value(mc, &proxy_ctor, "revocable", &Value::Function("Proxy.revocable".to_string()))?;

    env_set(mc, env, "Proxy", &Value::Object(proxy_ctor))?;
    Ok(())
}
