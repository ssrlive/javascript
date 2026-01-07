use crate::core::JSProxy;
use crate::core::{Collect, Gc, GcCell, GcPtr, MutationContext, Trace};
use crate::{
    core::{
        Expr, JSObjectDataPtr, PropertyKey, Value, evaluate_expr, evaluate_statements, extract_closure_from_value, new_js_object_data,
        obj_get_key_value, obj_set_key_value,
    },
    error::JSError,
    unicode::utf8_to_utf16,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Handle Proxy constructor calls
pub(crate) fn handle_proxy_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Expr],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if args.len() != 2 {
        return Err(raise_eval_error!(
            "Proxy constructor requires exactly two arguments: target and handler"
        ));
    }

    let target = evaluate_expr(mc, env, &args[0])?;
    let handler = evaluate_expr(mc, env, &args[1])?;

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
        Gc::new(mc, GcCell::new(Value::Proxy(proxy))),
    );

    Ok(Value::Object(proxy_obj))
}

/// Handle Proxy.revocable static method
pub(crate) fn handle_proxy_revocable<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Expr],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    if args.len() != 2 {
        return Err(raise_eval_error!(
            "Proxy.revocable requires exactly two arguments: target and handler"
        ));
    }

    let target = evaluate_expr(mc, env, &args[0])?;
    let handler = evaluate_expr(mc, env, &args[1])?;

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
    let revoke_proxy_ref = proxy.clone();
    let revoke_body = vec![
        // This is a special statement that will be handled in evaluate_call
        crate::core::Statement::from(crate::core::StatementKind::Expr(crate::core::Expr::Call(
            Box::new(crate::core::Expr::Var("__revoke_proxy".to_string(), None, None)),
            Vec::new(),
        ))),
    ];

    let revoke_env = new_js_object_data(mc);
    revoke_env
        .borrow_mut(mc)
        .insert("__revoke_proxy".into(), Gc::new(mc, GcCell::new(Value::Proxy(revoke_proxy_ref))));

    let revoke_func = Value::Closure(Gc::new(mc, crate::core::ClosureData::new(&[], &revoke_body, &revoke_env, None)));

    // Create a wrapper object for the Proxy
    let proxy_wrapper = new_js_object_data(mc);
    // Store the actual proxy data
    proxy_wrapper.borrow_mut(mc).insert(
        PropertyKey::String("__proxy__".to_string()),
        Gc::new(mc, GcCell::new(Value::Proxy(proxy))),
    );

    // Create the revocable result object
    let result_obj = new_js_object_data(mc);
    obj_set_key_value(mc, &result_obj, &"proxy".into(), Value::Object(proxy_wrapper))?;
    obj_set_key_value(mc, &result_obj, &"revoke".into(), revoke_func)?;

    Ok(Value::Object(result_obj))
}

/// Apply a proxy trap if available, otherwise fall back to default behavior
pub(crate) fn apply_proxy_trap<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, GcCell<JSProxy<'gc>>>,
    trap_name: &str,
    args: Vec<Value<'gc>>,
    default_fn: impl FnOnce() -> Result<Value<'gc>, JSError>,
) -> Result<Value<'gc>, JSError> {
    let proxy_borrow = proxy.borrow();
    if proxy_borrow.revoked {
        return Err(raise_eval_error!("Cannot perform operation on a revoked proxy"));
    }

    // Check if handler has the trap
    if let Value::Object(handler_obj) = &*proxy_borrow.handler
        && let Some(trap_val) = obj_get_key_value(&handler_obj, &trap_name.into())?
    {
        let trap = trap_val.borrow().clone();
        // Accept either a direct `Value::Closure` or a function-object that
        // stores the executable closure under the internal `__closure__` key.
        if let Some((params, body, captured_env)) = extract_closure_from_value(&trap) {
            // Create execution environment for the trap and bind parameters
            let trap_env = crate::core::prepare_closure_call_env(&captured_env, Some(&params), &args, None)?;

            // Evaluate the body
            return Ok(evaluate_statements(mc, &trap_env, &body)?);
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
    proxy: &Gc<'gc, GcCell<JSProxy<'gc>>>,
    key: &PropertyKey<'gc>,
) -> Result<Option<Value<'gc>>, JSError> {
    let result = apply_proxy_trap(
        mc,
        proxy,
        "get",
        vec![(*proxy.borrow().target).clone(), property_key_to_value(key)],
        || {
            // Default behavior: get property from target
            match &*proxy.borrow().target {
                Value::Object(obj) => {
                    let val_opt = obj_get_key_value(obj, key)?;
                    match val_opt {
                        Some(val_rc) => Ok(val_rc.borrow().clone()),
                        None => Ok(Value::Undefined),
                    }
                }
                _ => Ok(Value::Undefined), // Non-objects don't have properties
            }
        },
    )?;

    match result {
        Value::Undefined => Ok(None),
        val => Ok(Some(val)),
    }
}

/// Set property on proxy target, applying set trap if available
pub(crate) fn proxy_set_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, GcCell<JSProxy<'gc>>>,
    key: &PropertyKey<'gc>,
    value: Value<'gc>,
) -> Result<bool, JSError> {
    let result = apply_proxy_trap(
        mc,
        proxy,
        "set",
        vec![(*proxy.borrow().target).clone(), property_key_to_value(key), value.clone()],
        || {
            // Default behavior: set property on target
            match &*proxy.borrow().target {
                Value::Object(obj) => {
                    obj_set_key_value(mc, &obj, key, value)?;
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
    proxy: &Gc<'gc, GcCell<JSProxy<'gc>>>,
    key: &PropertyKey<'gc>,
) -> Result<bool, JSError> {
    let result = apply_proxy_trap(
        mc,
        proxy,
        "has",
        vec![(*proxy.borrow().target).clone(), property_key_to_value(key)],
        || {
            // Default behavior: check if property exists on target
            match &*proxy.borrow().target {
                Value::Object(obj) => Ok(Value::Boolean(obj_get_key_value(obj, key)?.is_some())),
                _ => Ok(Value::Boolean(false)), // Non-objects don't have properties
            }
        },
    )?;

    match result {
        Value::Boolean(b) => Ok(b),
        _ => Ok(false), // Non-boolean return from trap is treated as false
    }
}

/// Delete property from proxy target, applying deleteProperty trap if available
pub(crate) fn proxy_delete_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, GcCell<JSProxy<'gc>>>,
    key: &PropertyKey<'gc>,
) -> Result<bool, JSError> {
    let result = apply_proxy_trap(
        mc,
        proxy,
        "deleteProperty",
        vec![(*proxy.borrow().target).clone(), property_key_to_value(key)],
        || {
            // Default behavior: delete property from target
            match &*proxy.borrow().target {
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
        PropertyKey::Symbol(sd) => Value::Symbol(sd.clone()),
    }
}
