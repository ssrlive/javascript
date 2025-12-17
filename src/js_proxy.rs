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

use crate::core::JSProxy;

/// Handle Proxy constructor calls
pub(crate) fn handle_proxy_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.len() != 2 {
        return Err(raise_eval_error!(
            "Proxy constructor requires exactly two arguments: target and handler"
        ));
    }

    let target = evaluate_expr(env, &args[0])?;
    let handler = evaluate_expr(env, &args[1])?;

    // Create the proxy
    let proxy = Rc::new(RefCell::new(JSProxy {
        target,
        handler,
        revoked: false,
    }));

    // Create a wrapper object for the Proxy
    let proxy_obj = new_js_object_data();
    // Store the actual proxy data
    proxy_obj.borrow_mut().insert(
        PropertyKey::String("__proxy__".to_string()),
        Rc::new(RefCell::new(Value::Proxy(proxy))),
    );

    Ok(Value::Object(proxy_obj))
}

/// Handle Proxy.revocable static method
pub(crate) fn handle_proxy_revocable(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    if args.len() != 2 {
        return Err(raise_eval_error!(
            "Proxy.revocable requires exactly two arguments: target and handler"
        ));
    }

    let target = evaluate_expr(env, &args[0])?;
    let handler = evaluate_expr(env, &args[1])?;

    // Create the proxy
    let proxy = Rc::new(RefCell::new(JSProxy {
        target: target.clone(),
        handler: handler.clone(),
        revoked: false,
    }));

    // Create the revoke function as a closure that captures the proxy
    let revoke_proxy_ref = proxy.clone();
    let revoke_body = vec![
        // This is a special statement that will be handled in evaluate_call
        crate::core::Statement::from(crate::core::StatementKind::Expr(crate::core::Expr::Call(
            Box::new(crate::core::Expr::Var("__revoke_proxy".to_string(), None, None)),
            Vec::new(),
        ))),
    ];

    let revoke_env = new_js_object_data();
    revoke_env
        .borrow_mut()
        .insert("__revoke_proxy".into(), Rc::new(RefCell::new(Value::Proxy(revoke_proxy_ref))));

    let revoke_func = Value::Closure(Vec::new(), revoke_body, revoke_env, None);

    // Create a wrapper object for the Proxy
    let proxy_wrapper = new_js_object_data();
    // Store the actual proxy data
    proxy_wrapper.borrow_mut().insert(
        PropertyKey::String("__proxy__".to_string()),
        Rc::new(RefCell::new(Value::Proxy(proxy))),
    );

    // Create the revocable result object
    let result_obj = new_js_object_data();
    obj_set_key_value(&result_obj, &"proxy".into(), Value::Object(proxy_wrapper))?;
    obj_set_key_value(&result_obj, &"revoke".into(), revoke_func)?;

    Ok(Value::Object(result_obj))
}

/// Apply a proxy trap if available, otherwise fall back to default behavior
pub(crate) fn apply_proxy_trap(
    proxy: &Rc<RefCell<JSProxy>>,
    trap_name: &str,
    args: Vec<Value>,
    default_fn: impl FnOnce() -> Result<Value, JSError>,
) -> Result<Value, JSError> {
    let proxy_borrow = proxy.borrow();
    if proxy_borrow.revoked {
        return Err(raise_eval_error!("Cannot perform operation on a revoked proxy"));
    }

    // Check if handler has the trap
    if let Value::Object(handler_obj) = &proxy_borrow.handler
        && let Some(trap_val) = obj_get_key_value(handler_obj, &trap_name.into())?
    {
        let trap = trap_val.borrow().clone();
        // Accept either a direct `Value::Closure` or a function-object that
        // stores the executable closure under the internal `__closure__` key.
        if let Some((params, body, captured_env)) = extract_closure_from_value(&trap) {
            // Create execution environment for the trap
            let trap_env = new_js_object_data();
            trap_env.borrow_mut().prototype = Some(captured_env);

            // Bind arguments to parameters
            for (i, arg) in args.iter().enumerate() {
                if i < params.len() {
                    let name = params[i].0.clone();
                    obj_set_key_value(&trap_env, &name.into(), arg.clone())?;
                }
            }

            // Evaluate the body
            return evaluate_statements(&trap_env, &body);
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
pub(crate) fn proxy_get_property(proxy: &Rc<RefCell<JSProxy>>, key: &PropertyKey) -> Result<Option<Rc<RefCell<Value>>>, JSError> {
    let result = apply_proxy_trap(
        proxy,
        "get",
        vec![proxy.borrow().target.clone(), property_key_to_value(key)],
        || {
            // Default behavior: get property from target
            match &proxy.borrow().target {
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
        val => Ok(Some(Rc::new(RefCell::new(val)))),
    }
}

/// Set property on proxy target, applying set trap if available
pub(crate) fn proxy_set_property(proxy: &Rc<RefCell<JSProxy>>, key: &PropertyKey, value: Value) -> Result<bool, JSError> {
    let result = apply_proxy_trap(
        proxy,
        "set",
        vec![proxy.borrow().target.clone(), property_key_to_value(key), value.clone()],
        || {
            // Default behavior: set property on target
            match &proxy.borrow().target {
                Value::Object(obj) => {
                    obj_set_key_value(obj, key, value)?;
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
pub(crate) fn _proxy_has_property(proxy: &Rc<RefCell<JSProxy>>, key: &PropertyKey) -> Result<bool, JSError> {
    let result = apply_proxy_trap(
        proxy,
        "has",
        vec![proxy.borrow().target.clone(), property_key_to_value(key)],
        || {
            // Default behavior: check if property exists on target
            match &proxy.borrow().target {
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
pub(crate) fn proxy_delete_property(proxy: &Rc<RefCell<JSProxy>>, key: &PropertyKey) -> Result<bool, JSError> {
    let result = apply_proxy_trap(
        proxy,
        "deleteProperty",
        vec![proxy.borrow().target.clone(), property_key_to_value(key)],
        || {
            // Default behavior: delete property from target
            match &proxy.borrow().target {
                Value::Object(obj) => {
                    let mut obj_borrow = obj.borrow_mut();
                    let existed = obj_borrow.properties.contains_key(key);
                    obj_borrow.properties.remove(key);
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
fn property_key_to_value(key: &PropertyKey) -> Value {
    match key {
        PropertyKey::String(s) => Value::String(utf8_to_utf16(s)),
        PropertyKey::Symbol(sym_rc) => sym_rc.borrow().clone(),
    }
}
