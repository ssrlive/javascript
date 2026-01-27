use crate::core::{ClosureData, Expr, JSProxy, Statement, StatementKind, prepare_closure_call_env};
use crate::core::{Gc, MutationContext, new_gc_cell_ptr};
use crate::env_set;
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
        return Err(EvalError::Js(raise_eval_error!(
            "Proxy constructor requires exactly two arguments: target and handler"
        )));
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
        return Err(EvalError::Js(raise_eval_error!(
            "Proxy.revocable requires exactly two arguments: target and handler"
        )));
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
        .insert("__revoke_proxy".into(), new_gc_cell_ptr(mc, Value::Proxy(proxy)));

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
        .insert("__proxy_wrapper".into(), new_gc_cell_ptr(mc, Value::Object(proxy_wrapper)));

    // Provide a callable function in the revoke env that dispatches to the internal revoke helper
    revoke_env.borrow_mut(mc).insert(
        "__internal_revoke".into(),
        new_gc_cell_ptr(mc, Value::Function("Proxy.__internal_revoke".to_string())),
    );

    let revoke_func = Value::Closure(Gc::new(mc, ClosureData::new(&[], &revoke_body, Some(revoke_env), None)));

    // Create the revocable result object
    let result_obj = new_js_object_data(mc);
    object_set_key_value(mc, &result_obj, "proxy", Value::Object(proxy_wrapper)).map_err(EvalError::Js)?;
    object_set_key_value(mc, &result_obj, "revoke", revoke_func).map_err(EvalError::Js)?;

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
        return Err(EvalError::Js(raise_eval_error!("Cannot perform operation on a revoked proxy")));
    }

    // Check if handler has the trap
    if let Value::Object(handler_obj) = &*proxy.handler
        && let Some(trap_val) = object_get_key_value(handler_obj, trap_name)
    {
        let trap = trap_val.borrow().clone();
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
                    Some(val_rc) => Ok(val_rc.borrow().clone()),
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

/// Set property on proxy target, applying set trap if available
pub(crate) fn proxy_set_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    value: Value<'gc>,
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
                    object_set_key_value(mc, obj, key, value).map_err(EvalError::Js)?;
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

/// Check if property exists on proxy target, applying has trap if available
pub(crate) fn _proxy_has_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
) -> Result<bool, EvalError<'gc>> {
    let result = apply_proxy_trap(mc, proxy, "has", vec![(*proxy.target).clone(), property_key_to_value(key)], || {
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
    }
}

/// Initialize Proxy constructor and prototype
pub fn initialize_proxy<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let proxy_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &proxy_ctor, "__is_constructor", Value::Boolean(true))?;
    object_set_key_value(mc, &proxy_ctor, "__native_ctor", Value::String(utf8_to_utf16("Proxy")))?;

    // Set up prototype linked to Object.prototype if available
    let object_proto = if let Some(obj_val) = object_get_key_value(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let proxy_proto = new_js_object_data(mc);
    if let Some(proto) = object_proto {
        proxy_proto.borrow_mut(mc).prototype = Some(proto);
    }

    object_set_key_value(mc, &proxy_ctor, "prototype", Value::Object(proxy_proto))?;
    object_set_key_value(mc, &proxy_proto, "constructor", Value::Object(proxy_ctor))?;

    // Register revocable static method
    object_set_key_value(mc, &proxy_ctor, "revocable", Value::Function("Proxy.revocable".to_string()))?;

    env_set(mc, env, "Proxy", Value::Object(proxy_ctor))?;
    Ok(())
}
