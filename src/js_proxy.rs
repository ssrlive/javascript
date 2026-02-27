use crate::core::{ClosureData, Expr, InternalSlot, JSProxy, Statement, StatementKind, slot_set};
use crate::core::{Gc, MutationContext};
use crate::env_set;
use crate::unicode::utf16_to_utf8;
use crate::{
    core::{
        EvalError, JSObjectDataPtr, PropertyKey, Value, call_accessor, evaluate_call_dispatch, new_js_object_data, object_get_key_value,
        object_set_key_value,
    },
    error::JSError,
    unicode::utf8_to_utf16,
};

/// Check if a value is an object for Proxy creation purposes (Object or Proxy wrapper)
fn is_object_type(val: &Value) -> bool {
    matches!(val, Value::Object(_) | Value::Proxy(_))
        || (matches!(
            val,
            Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..)
        ))
}

/// Public wrapper: check if a JSProxy is callable (its target has [[Call]])
pub fn is_callable_proxy(proxy: &crate::core::JSProxy) -> bool {
    is_callable_target(&proxy.target)
}

/// Check if a value is a callable target (has [[Call]])
fn is_callable_target(val: &Value) -> bool {
    match val {
        Value::Function(_)
        | Value::Closure(_)
        | Value::AsyncClosure(_)
        | Value::GeneratorFunction(..)
        | Value::AsyncGeneratorFunction(..) => true,
        Value::Object(obj) => {
            obj.borrow().get_closure().is_some()
                || obj.borrow().class_def.is_some()
                || crate::core::slot_get(obj, &InternalSlot::NativeCtor).is_some()
                || crate::core::slot_get(obj, &InternalSlot::BoundTarget).is_some()
        }
        Value::Proxy(_) => false, // raw proxy values shouldn't appear here
        _ => false,
    }
}

/// Check if a value is a constructor target (has [[Construct]])
fn is_constructor_target(val: &Value) -> bool {
    match val {
        Value::Closure(cl) => !cl.is_arrow,
        Value::Function(name) => {
            // Most built-in Value::Function entries are NOT constructors.
            // Only constructors like Object, Array, Function, Error, etc. return true.
            // Built-in non-constructor functions include eval, parseInt, parseFloat, etc.
            let name_str = name.as_str();
            matches!(
                name_str,
                "Object"
                    | "Array"
                    | "Function"
                    | "Boolean"
                    | "Number"
                    | "String"
                    | "RegExp"
                    | "Error"
                    | "TypeError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "URIError"
                    | "EvalError"
                    | "Date"
                    | "Map"
                    | "Set"
                    | "WeakMap"
                    | "WeakSet"
                    | "Promise"
                    | "ArrayBuffer"
                    | "SharedArrayBuffer"
                    | "DataView"
                    | "Proxy"
                    | "Int8Array"
                    | "Uint8Array"
                    | "Uint8ClampedArray"
                    | "Int16Array"
                    | "Uint16Array"
                    | "Int32Array"
                    | "Uint32Array"
                    | "Float32Array"
                    | "Float64Array"
                    | "BigInt64Array"
                    | "BigUint64Array"
            )
        }
        Value::Object(obj) => {
            // Check if it has an internal closure that's not an arrow
            if let Some(cl_ptr) = obj.borrow().get_closure() {
                match &*cl_ptr.borrow() {
                    Value::Closure(cl) => !cl.is_arrow,
                    Value::Function(name) => {
                        let name_str = name.as_str();
                        matches!(
                            name_str,
                            "Object"
                                | "Array"
                                | "Function"
                                | "Boolean"
                                | "Number"
                                | "String"
                                | "RegExp"
                                | "Error"
                                | "TypeError"
                                | "RangeError"
                                | "ReferenceError"
                                | "SyntaxError"
                                | "URIError"
                                | "EvalError"
                                | "Date"
                                | "Map"
                                | "Set"
                                | "WeakMap"
                                | "WeakSet"
                                | "Promise"
                                | "ArrayBuffer"
                                | "SharedArrayBuffer"
                                | "DataView"
                                | "Proxy"
                                | "Int8Array"
                                | "Uint8Array"
                                | "Uint8ClampedArray"
                                | "Int16Array"
                                | "Uint16Array"
                                | "Int32Array"
                                | "Uint32Array"
                                | "Float32Array"
                                | "Float64Array"
                                | "BigInt64Array"
                                | "BigUint64Array"
                        )
                    }
                    _ => false,
                }
            } else if obj.borrow().class_def.is_some() {
                true
            } else {
                let has_native_ctor = crate::core::slot_get(obj, &InternalSlot::NativeCtor).is_some();
                let has_is_ctor = crate::core::slot_get(obj, &InternalSlot::IsConstructor)
                    .map(|v| matches!(*v.borrow(), Value::Boolean(true)))
                    .unwrap_or(false);
                has_native_ctor || has_is_ctor
            }
        }
        _ => false,
    }
}

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

    // Step 1: If Type(target) is not Object, throw a TypeError exception.
    if !is_object_type(&target) {
        return Err(raise_type_error!("Cannot create proxy with a non-object as target").into());
    }
    // Step 2: If Type(handler) is not Object, throw a TypeError exception.
    if !is_object_type(&handler) {
        return Err(raise_type_error!("Cannot create proxy with a non-object as handler").into());
    }

    // Create the proxy
    let target_is_callable = is_callable_target(&target);
    let target_is_constructor = is_constructor_target(&target);

    // For Object-wrapped targets, also check inner proxy for callable/constructor
    let (target_is_callable, target_is_constructor) = if let Value::Object(obj) = &target {
        if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
            && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
        {
            let inner_callable = target_is_callable
                || is_callable_target(&inner_proxy.target)
                || crate::core::slot_get(obj, &InternalSlot::Callable).is_some();
            let inner_ctor = target_is_constructor
                || is_constructor_target(&inner_proxy.target)
                || crate::core::slot_get(obj, &InternalSlot::IsConstructor)
                    .map(|v| matches!(*v.borrow(), Value::Boolean(true)))
                    .unwrap_or(false);
            (inner_callable, inner_ctor)
        } else {
            (target_is_callable, target_is_constructor)
        }
    } else {
        (target_is_callable, target_is_constructor)
    };

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
    slot_set(mc, &proxy_obj, InternalSlot::Proxy, &Value::Proxy(proxy));

    // Per spec: If IsCallable(target), set P.[[Call]]; if IsConstructor(target), set P.[[Construct]]
    if target_is_callable {
        slot_set(mc, &proxy_obj, InternalSlot::Callable, &Value::Boolean(true));
    }
    if target_is_constructor {
        slot_set(mc, &proxy_obj, InternalSlot::IsConstructor, &Value::Boolean(true));
    }

    Ok(Value::Object(proxy_obj))
}

pub(crate) fn handle_proxy_revocable<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.len() != 2 {
        return Err(raise_eval_error!("Proxy.revocable requires exactly two arguments: target and handler").into());
    }

    let target = args[0].clone();
    let handler = args[1].clone();

    // Step 1: If Type(target) is not Object, throw a TypeError exception.
    if !is_object_type(&target) {
        return Err(raise_type_error!("Cannot create proxy with a non-object as target").into());
    }
    // Step 2: If Type(handler) is not Object, throw a TypeError exception.
    if !is_object_type(&handler) {
        return Err(raise_type_error!("Cannot create proxy with a non-object as handler").into());
    }

    // Create the proxy
    let target_is_callable = is_callable_target(&target);
    let target_is_constructor = is_constructor_target(&target);

    let (target_is_callable, target_is_constructor) = if let Value::Object(obj) = &target {
        if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
            && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
        {
            let inner_callable = target_is_callable
                || is_callable_target(&inner_proxy.target)
                || crate::core::slot_get(obj, &InternalSlot::Callable).is_some();
            let inner_ctor = target_is_constructor
                || is_constructor_target(&inner_proxy.target)
                || crate::core::slot_get(obj, &InternalSlot::IsConstructor)
                    .map(|v| matches!(*v.borrow(), Value::Boolean(true)))
                    .unwrap_or(false);
            (inner_callable, inner_ctor)
        } else {
            (target_is_callable, target_is_constructor)
        }
    } else {
        (target_is_callable, target_is_constructor)
    };

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
    slot_set(mc, &revoke_env, InternalSlot::RevokeProxy, &Value::Proxy(proxy));

    // Create a wrapper object for the Proxy
    let proxy_wrapper = new_js_object_data(mc);
    // Store the actual proxy data
    slot_set(mc, &proxy_wrapper, InternalSlot::Proxy, &Value::Proxy(proxy));

    // Per spec: If IsCallable(target), set P.[[Call]]; if IsConstructor(target), set P.[[Construct]]
    if target_is_callable {
        slot_set(mc, &proxy_wrapper, InternalSlot::Callable, &Value::Boolean(true));
    }
    if target_is_constructor {
        slot_set(mc, &proxy_wrapper, InternalSlot::IsConstructor, &Value::Boolean(true));
    }

    // Also capture the wrapper object so the internal revoke helper can replace the stored proxy
    slot_set(mc, &revoke_env, InternalSlot::ProxyWrapper, &Value::Object(proxy_wrapper));

    // Provide a callable function in the revoke env that dispatches to the internal revoke helper
    slot_set(
        mc,
        &revoke_env,
        InternalSlot::InternalFn("revoke".to_string()),
        &Value::Function("Proxy.__internal_revoke".to_string()),
    );

    let revoke_closure = Gc::new(
        mc,
        ClosureData {
            is_arrow: true, // Arrow functions are NOT constructable
            ..ClosureData::new(&[], &revoke_body, Some(revoke_env), None)
        },
    );
    let revoke_func_obj = new_js_object_data(mc);
    let revoke_closure_val = Value::Closure(revoke_closure);
    revoke_func_obj
        .borrow_mut(mc)
        .set_closure(Some(crate::core::new_gc_cell_ptr(mc, revoke_closure_val)));
    // Per spec: revoke.length = 0 (non-writable, non-enumerable, configurable)
    object_set_key_value(mc, &revoke_func_obj, "length", &Value::Number(0.0))?;
    revoke_func_obj.borrow_mut(mc).set_non_writable("length");
    revoke_func_obj.borrow_mut(mc).set_non_enumerable("length");
    // Per spec: revoke.name = "" (non-writable, non-enumerable, configurable)
    object_set_key_value(mc, &revoke_func_obj, "name", &Value::String(utf8_to_utf16("")))?;
    revoke_func_obj.borrow_mut(mc).set_non_writable("name");
    revoke_func_obj.borrow_mut(mc).set_non_enumerable("name");
    // Per spec: [[Prototype]] of revocation function = Function.prototype
    if let Some(func_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(proto_val) = crate::core::object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        revoke_func_obj.borrow_mut(mc).prototype = Some(*func_proto);
    }
    let revoke_func = Value::Object(revoke_func_obj);

    // Create the revocable result object
    let result_obj = new_js_object_data(mc);
    object_set_key_value(mc, &result_obj, "proxy", &Value::Object(proxy_wrapper))?;
    object_set_key_value(mc, &result_obj, "revoke", &revoke_func)?;

    Ok(Value::Object(result_obj))
}

/// Get Object.prototype by walking up from the handler object.
/// Handler is typically an object literal whose [[Prototype]] IS Object.prototype.
/// Returns None if we can't find it (e.g., Object.create(null) handler).
fn get_object_prototype<'gc>(proxy: &Gc<'gc, JSProxy<'gc>>) -> Option<JSObjectDataPtr<'gc>> {
    if let Value::Object(handler_obj) = &*proxy.handler {
        // The handler's prototype is typically Object.prototype directly
        let proto = handler_obj.borrow().prototype;
        if let Some(p) = proto {
            // Verify it looks like Object.prototype (has hasOwnProperty, toString, etc.)
            // or just trust it's Object.prototype since that's by far the common case
            return Some(p);
        }
    }
    None
}

/// Create a plain object with Object.prototype as its [[Prototype]],
/// matching spec's ObjectCreate(%ObjectPrototype%).
fn new_object_with_proto<'gc>(mc: &MutationContext<'gc>, proxy: &Gc<'gc, JSProxy<'gc>>) -> JSObjectDataPtr<'gc> {
    let obj = new_js_object_data(mc);
    if let Some(proto) = get_object_prototype(proxy) {
        obj.borrow_mut(mc).prototype = Some(proto);
    }
    obj
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
        return Err(raise_type_error!("Cannot perform operation on a revoked proxy").into());
    }

    // Check if handler has the trap
    if let Value::Object(handler_obj) = &*proxy.handler {
        // If the handler is itself a proxy, use proxy get to look up the trap
        // so the handler-proxy's traps are observed.
        let trap_key = crate::core::PropertyKey::String(trap_name.to_string());
        let trap_opt = if let Some(inner_proxy_cell) = crate::core::slot_get(handler_obj, &InternalSlot::Proxy)
            && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
        {
            proxy_get_property(mc, inner_proxy, &trap_key)?
        } else if let Some(trap_val) = object_get_key_value(handler_obj, trap_name) {
            let unwrapped = match &*trap_val.borrow() {
                Value::Property { value: Some(v), .. } => v.borrow().clone(),
                Value::Property {
                    getter: Some(getter),
                    value: None,
                    ..
                } => {
                    // Accessor property (Property variant): invoke the getter with handler as this
                    let getter_fn = (**getter).clone();
                    call_accessor(mc, handler_obj, handler_obj, &getter_fn)?
                }
                Value::Getter(..) => {
                    // Bare Getter variant (object literal with only `get` accessor):
                    // invoke via call_accessor so the body is evaluated correctly
                    let getter_fn = trap_val.borrow().clone();
                    call_accessor(mc, handler_obj, handler_obj, &getter_fn)?
                }
                other => other.clone(),
            };
            Some(unwrapped)
        } else {
            None
        };

        if let Some(trap) = trap_opt {
            // Per spec, undefined/null trap means "not present" and should use default behavior.
            if matches!(trap, Value::Undefined | Value::Null) {
                return default_fn();
            }

            // If trap property exists it must be callable; invoke via normal call dispatch
            // so return semantics and `this` binding are correct.
            let handler_this = Value::Object(*handler_obj);
            return evaluate_call_dispatch(mc, handler_obj, &trap, Some(&handler_this), &args);
        }
    }

    // No trap or trap not callable, use default behavior
    default_fn()
}

/// OrdinaryGet(O, P, Receiver) — implements the spec's OrdinaryGet algorithm
/// such that getters are invoked with the *Receiver* as `this` (not with the target).
/// This is used by the proxy [[Get]] default when no trap is defined.
fn proxy_ordinary_get<'gc>(
    mc: &MutationContext<'gc>,
    obj: &crate::core::JSObjectDataPtr<'gc>,
    key: &PropertyKey<'gc>,
    receiver: &Option<Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Step 1: Check own property
    if let Some(val_rc) = crate::core::get_own_property(obj, key) {
        let val = val_rc.borrow().clone();
        match &val {
            // Data descriptor
            Value::Property {
                value: Some(v),
                getter: None,
                setter: None,
                ..
            } => {
                return Ok(v.borrow().clone());
            }
            Value::Property { value: Some(v), .. }
                if !matches!(
                    &val,
                    Value::Property { getter: Some(_), .. } | Value::Property { setter: Some(_), .. }
                ) =>
            {
                return Ok(v.borrow().clone());
            }
            // Accessor descriptor with getter
            Value::Property { getter: Some(getter), .. } => {
                let recv_obj = match receiver {
                    Some(Value::Object(o)) => *o,
                    _ => *obj,
                };
                return call_accessor(mc, obj, &recv_obj, getter);
            }
            Value::Getter(..) => {
                let recv_obj = match receiver {
                    Some(Value::Object(o)) => *o,
                    _ => *obj,
                };
                return call_accessor(mc, obj, &recv_obj, &val);
            }
            // Accessor with no getter
            Value::Property {
                getter: None,
                setter: Some(_),
                ..
            } => {
                return Ok(Value::Undefined);
            }
            Value::Setter(..) => {
                return Ok(Value::Undefined);
            }
            // Plain value (not wrapped in Property)
            _ => return Ok(val),
        }
    }

    // Step 2: Walk prototype chain
    let proto = obj.borrow().prototype;
    if let Some(parent) = proto {
        // If parent is a proxy wrapper, delegate through proxy_get_property
        if let Some(inner_proxy_cell) = crate::core::slot_get(&parent, &InternalSlot::Proxy)
            && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
        {
            return match proxy_get_property_with_receiver(mc, inner_proxy, key, receiver.clone(), None)? {
                Some(v) => Ok(v),
                None => Ok(Value::Undefined),
            };
        }
        return proxy_ordinary_get(mc, &parent, key, receiver);
    }

    Ok(Value::Undefined)
}

/// Read a property from an object, invoking getter accessors if the stored
/// value is a `Value::Getter` or `Value::Property { getter: Some(..), .. }`.
/// This is used to implement CreateListFromArrayLike semantics where
/// getters on the trap result must be observable.
fn read_property_invoking_getters<'gc>(
    mc: &MutationContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: impl Into<crate::core::PropertyKey<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if let Some(val_rc) = crate::core::object_get_key_value(obj, key) {
        let raw = val_rc.borrow().clone();
        match &raw {
            Value::Property { value: Some(v), .. } => Ok(v.borrow().clone()),
            Value::Property {
                getter: Some(getter),
                value: None,
                ..
            } => call_accessor(mc, obj, obj, getter),
            Value::Getter(..) => call_accessor(mc, obj, obj, &raw),
            _ => Ok(raw),
        }
    } else {
        Ok(Value::Undefined)
    }
}

/// Proxy [[Call]] internal method (spec §10.5.12)
/// Called when a proxy-wrapped callable is invoked as a function.
pub(crate) fn proxy_call<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    this_arg: &Value<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Build the argArray as an array-like object
    let args_array = crate::core::new_js_object_data(mc);
    for (i, arg) in args.iter().enumerate() {
        object_set_key_value(mc, &args_array, i, arg)?;
    }
    object_set_key_value(mc, &args_array, "length", &Value::Number(args.len() as f64))?;
    // Try to set Array.prototype on the args array
    if let Some(array_val) = crate::core::env_get(env, "Array")
        && let Value::Object(array_ctor) = &*array_val.borrow()
        && let Some(proto_val) = crate::core::object_get_key_value(array_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        args_array.borrow_mut(mc).prototype = Some(*proto);
    }

    let result = apply_proxy_trap(
        mc,
        proxy,
        "apply",
        vec![(*proxy.target).clone(), this_arg.clone(), Value::Object(args_array)],
        || {
            // Default: call the target function directly
            evaluate_call_dispatch(mc, env, &proxy.target, Some(this_arg), args)
        },
    )?;

    Ok(result)
}

/// Proxy [[Construct]] internal method (spec §10.5.13)
/// Called when a proxy-wrapped constructor is invoked with `new`.
pub(crate) fn proxy_construct<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    args: &[Value<'gc>],
    new_target: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Build the argArray as an array-like object
    let args_array = crate::core::new_js_object_data(mc);
    for (i, arg) in args.iter().enumerate() {
        object_set_key_value(mc, &args_array, i, arg)?;
    }
    object_set_key_value(mc, &args_array, "length", &Value::Number(args.len() as f64))?;
    if let Some(array_val) = crate::core::env_get(env, "Array")
        && let Value::Object(array_ctor) = &*array_val.borrow()
        && let Some(proto_val) = crate::core::object_get_key_value(array_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        args_array.borrow_mut(mc).prototype = Some(*proto);
    }

    let result = apply_proxy_trap(
        mc,
        proxy,
        "construct",
        vec![(*proxy.target).clone(), Value::Object(args_array), new_target.clone()],
        || {
            // Default: construct the target directly
            // If target is itself a proxy wrapper, recurse through proxy_construct
            if let Value::Object(target_obj) = &*proxy.target
                && let Some(inner_proxy_cell) = crate::core::slot_get(target_obj, &InternalSlot::Proxy)
                && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
            {
                return proxy_construct(mc, inner_proxy, args, new_target, env);
            }
            // Use js_class::evaluate_new which handles most constructor patterns
            crate::js_class::evaluate_new(mc, env, &proxy.target, args, Some(new_target))
        },
    )?;

    // Step 10: If Type(newObj) is not Object, throw TypeError
    match &result {
        Value::Object(_) => Ok(result),
        _ => Err(raise_type_error!("'construct' on proxy: trap returned non-Object").into()),
    }
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
        if let Value::Object(obj) = &*proxy.target {
            // If target is itself a proxy, recurse
            if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
            {
                let inner_keys = proxy_own_keys(mc, inner_proxy)?;
                let result_obj = crate::core::new_js_object_data(mc);
                let keys_len = inner_keys.len();
                for (i, key) in inner_keys.into_iter().enumerate() {
                    crate::core::object_set_key_value(mc, &result_obj, i, &property_key_to_value(&key))?;
                }
                crate::core::object_set_key_value(mc, &result_obj, "length", &Value::Number(keys_len as f64))?;
                return Ok(Value::Object(result_obj));
            }
        }

        let mut keys: Vec<Value> = Vec::new();
        if let Value::Object(obj) = &*proxy.target {
            let ordered = crate::core::ordinary_own_property_keys_mc(mc, obj)?;
            for k in ordered {
                match k {
                    crate::core::PropertyKey::String(s) => keys.push(Value::String(utf8_to_utf16(&s))),
                    crate::core::PropertyKey::Symbol(sd) => keys.push(Value::Symbol(sd)),
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
    // This implements CreateListFromArrayLike: reads `length` and each index
    // through accessor-aware property access so getters are invoked.
    match res {
        Value::Object(arr_obj) => {
            // Read `length` through accessor-aware path
            let len = {
                let len_val = read_property_invoking_getters(mc, &arr_obj, "length")?;
                match len_val {
                    Value::Number(n) => n as usize,
                    _ => 0,
                }
            };
            let mut out: Vec<crate::core::PropertyKey<'gc>> = Vec::new();
            for i in 0..len {
                let elem = read_property_invoking_getters(mc, &arr_obj, i)?;
                match elem {
                    Value::String(s) => out.push(crate::core::PropertyKey::String(utf16_to_utf8(&s))),
                    Value::Symbol(sd) => out.push(crate::core::PropertyKey::Symbol(sd)),
                    other => {
                        return Err(raise_type_error!(format!("Invalid value returned from proxy ownKeys trap: {:?}", other)).into());
                    }
                }
            }
            // Per CopyDataProperties / spec behavior, callers performing CopyDataProperties
            // (such as object-rest/spread or destructuring) must call [[GetOwnProperty]] for
            // each key returned by the ownKeys trap. The proxy helper only returns the key
            // list here; callers should invoke `proxy_get_own_property_descriptor` as needed.

            // Invariant checks per spec 10.5.11 step 17-24:
            // 1. Check for duplicates
            {
                let mut seen = std::collections::HashSet::new();
                for key in &out {
                    let key_id = format!("{:?}", key);
                    if !seen.insert(key_id) {
                        return Err(raise_type_error!("'ownKeys' on proxy: trap returned duplicate entries").into());
                    }
                }
            }

            // 2. Get target's own keys and check non-configurable invariant
            if let Value::Object(target_obj) = &*proxy.target {
                let target_keys = crate::core::ordinary_own_property_keys(target_obj);
                let is_extensible = target_obj.borrow().is_extensible();

                // Check that all non-configurable keys of target are in trap result
                for tk in &target_keys {
                    let is_configurable = target_obj.borrow().is_configurable(tk);
                    if !is_configurable && !out.contains(tk) {
                        return Err(raise_type_error!("'ownKeys' on proxy: trap result did not include non-configurable key").into());
                    }
                }

                // If the target is not extensible:
                // - All target keys must be in trap result
                // - No extra keys allowed
                if !is_extensible {
                    for tk in &target_keys {
                        if !out.contains(tk) {
                            return Err(raise_type_error!(
                                "'ownKeys' on proxy: trap result did not include all keys of non-extensible target"
                            )
                            .into());
                        }
                    }
                    for ok in &out {
                        if !target_keys.contains(ok) {
                            return Err(raise_type_error!("'ownKeys' on proxy: trap returned extra key for non-extensible target").into());
                        }
                    }
                }
            }

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
    proxy_get_property_with_receiver(mc, proxy, key, None, None)
}

/// Get property from proxy target with explicit wrapper and receiver
pub(crate) fn proxy_get_property_with_wrapper<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    wrapper: &crate::core::JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    proxy_get_property_with_receiver(mc, proxy, key, Some(Value::Object(*wrapper)), None)
}

/// Get property from proxy target with explicit receiver, applying get trap if available
pub(crate) fn proxy_get_property_with_receiver<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    receiver: Option<Value<'gc>>,
    _env: Option<&crate::core::JSObjectDataPtr<'gc>>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    let key_clone = key.clone();
    let receiver_val = receiver.clone().unwrap_or(Value::Undefined);
    let receiver_for_default = receiver.clone();
    let result = apply_proxy_trap(
        mc,
        proxy,
        "get",
        vec![(*proxy.target).clone(), property_key_to_value(key), receiver_val],
        || {
            // Default behavior: target.[[Get]](P, Receiver)
            // Per spec OrdinaryGet: look up own property, if accessor invoke with Receiver as this
            match &*proxy.target {
                Value::Object(obj) => {
                    if let Some(proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                        && let Value::Proxy(inner_proxy) = &*proxy_cell.borrow()
                    {
                        return match proxy_get_property_with_receiver(mc, inner_proxy, &key_clone, receiver_for_default.clone(), None)? {
                            Some(v) => Ok(v),
                            None => Ok(Value::Undefined),
                        };
                    }
                    // OrdinaryGet(target, P, Receiver):
                    // 1. Check own property
                    // 2. If accessor, call getter with Receiver as this
                    // 3. If not found, walk prototype chain
                    proxy_ordinary_get(mc, obj, &key_clone, &receiver_for_default)
                }
                // For function-like targets, resolve Function.prototype methods
                Value::Function(name) => {
                    if let crate::core::PropertyKey::String(prop) = &key_clone {
                        match prop.as_str() {
                            "call" => Ok(Value::Function("Function.prototype.call".to_string())),
                            "apply" => Ok(Value::Function("Function.prototype.apply".to_string())),
                            "bind" => Ok(Value::Function("Function.prototype.bind".to_string())),
                            "toString" => Ok(Value::Function("Function.prototype.toString".to_string())),
                            "name" => {
                                let short = name.rsplit('.').next().unwrap_or(name.as_str());
                                Ok(Value::String(utf8_to_utf16(short)))
                            }
                            "length" => Ok(Value::Number(0.0)),
                            _ => Ok(Value::Undefined),
                        }
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                Value::Closure(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..) => {
                    if let crate::core::PropertyKey::String(prop) = &key_clone {
                        match prop.as_str() {
                            "call" => Ok(Value::Function("Function.prototype.call".to_string())),
                            "apply" => Ok(Value::Function("Function.prototype.apply".to_string())),
                            "bind" => Ok(Value::Function("Function.prototype.bind".to_string())),
                            "toString" => Ok(Value::Function("Function.prototype.toString".to_string())),
                            "name" => Ok(Value::String(utf8_to_utf16(""))),
                            "length" => {
                                let len = match &*proxy.target {
                                    Value::Closure(cl) | Value::AsyncClosure(cl) => cl.params.len() as f64,
                                    Value::GeneratorFunction(_, cl) => cl.params.len() as f64,
                                    Value::AsyncGeneratorFunction(_, cl) => cl.params.len() as f64,
                                    _ => 0.0,
                                };
                                Ok(Value::Number(len))
                            }
                            _ => Ok(Value::Undefined),
                        }
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Ok(Value::Undefined), // Non-objects don't have properties
            }
        },
    )?;

    // Post-trap invariant checks for [[Get]] (spec 10.5.8 steps 9-11)
    if let Value::Object(target_obj) = &*proxy.target
        && let Some(target_prop_rc) = crate::core::get_own_property(target_obj, key)
    {
        let target_prop = target_prop_rc.borrow().clone();
        let is_configurable = target_obj.borrow().is_configurable(key);
        if !is_configurable {
            // Check if accessor descriptor with get=undefined
            let is_accessor = matches!(
                &target_prop,
                Value::Getter(..) | Value::Setter(..) | Value::Property { value: None, .. }
            );
            if is_accessor {
                // 10.5.8 step 10b: accessor with undefined get and non-configurable
                let has_getter = matches!(&target_prop, Value::Getter(..) | Value::Property { getter: Some(_), .. });
                if !has_getter && !matches!(&result, Value::Undefined) {
                    return Err(raise_type_error!("'get' on proxy: property is a non-configurable accessor property without a getter on the proxy target and the trap did not return 'undefined'").into());
                }
            } else {
                // 10.5.8 step 10a: data descriptor, non-writable, non-configurable
                if !target_obj.borrow().is_writable(key) {
                    let target_value = match &target_prop {
                        Value::Property { value: Some(v), .. } => v.borrow().clone(),
                        other => other.clone(),
                    };
                    // SameValue check
                    let same = match (&result, &target_value) {
                        (Value::Number(a), Value::Number(b)) => {
                            if a.is_nan() && b.is_nan() {
                                true
                            } else if *a == 0.0 && *b == 0.0 {
                                a.is_sign_positive() == b.is_sign_positive()
                            } else {
                                a == b
                            }
                        }
                        _ => crate::core::same_value_zero(&result, &target_value),
                    };
                    if !same {
                        return Err(raise_type_error!("'get' on proxy: property is a non-writable and non-configurable data property on the proxy target but the proxy did not return the expected value").into());
                    }
                }
            }
        }
    }

    match result {
        Value::Undefined => Ok(None),
        val => Ok(Some(val)),
    }
}

/// Implements the abstract operation IsCompatiblePropertyDescriptor (§10.1.6.3)
/// via ValidateAndApplyPropertyDescriptor (O = undefined).
/// `desc` is the new descriptor object, `current` is the current target property descriptor object.
/// Returns true if the new descriptor is compatible with the current one.
fn is_compatible_property_descriptor<'gc>(
    mc: &MutationContext<'gc>,
    _extensible: bool,
    desc: &crate::core::JSObjectDataPtr<'gc>,
    current: &crate::core::JSObjectDataPtr<'gc>,
) -> bool {
    // Helper closures for reading desc fields
    let get_bool =
        |obj: &crate::core::JSObjectDataPtr<'gc>, k: &str| -> Option<bool> { object_get_key_value(obj, k).map(|v| v.borrow().to_truthy()) };
    let get_val = |obj: &crate::core::JSObjectDataPtr<'gc>, k: &str| -> Option<Value<'gc>> {
        object_get_key_value(obj, k).map(|v| v.borrow().clone())
    };
    let has_field = |obj: &crate::core::JSObjectDataPtr<'gc>, k: &str| -> bool { object_get_key_value(obj, k).is_some() };

    // Check if descriptor is accessor (has get or set)
    let is_accessor = |obj: &crate::core::JSObjectDataPtr<'gc>| -> bool { has_field(obj, "get") || has_field(obj, "set") };
    let is_data = |obj: &crate::core::JSObjectDataPtr<'gc>| -> bool { has_field(obj, "value") || has_field(obj, "writable") };

    let current_configurable = get_bool(current, "configurable").unwrap_or(false);
    let current_enumerable = get_bool(current, "enumerable").unwrap_or(false);

    // Step 2: If current is undefined (should not happen here as caller checks)
    // Step 4: If Current.[[Configurable]] is false
    if !current_configurable {
        // 4a: If Desc.[[Configurable]] is present and true, return false
        if let Some(true) = get_bool(desc, "configurable") {
            return false;
        }
        // 4b: If Desc.[[Enumerable]] is present and differs from current, return false
        if let Some(desc_enum) = get_bool(desc, "enumerable")
            && desc_enum != current_enumerable
        {
            return false;
        }
    }

    // Step 5: If IsGenericDescriptor(Desc) is true (has neither value/writable nor get/set), return true
    if !is_data(desc) && !is_accessor(desc) {
        return true;
    }

    let current_is_data = is_data(current) && !is_accessor(current);
    let desc_is_data = is_data(desc) && !is_accessor(desc);
    let current_is_accessor = is_accessor(current);
    let desc_is_accessor = is_accessor(desc);

    // Step 6: If IsDataDescriptor(current) != IsDataDescriptor(Desc)
    if current_is_data != desc_is_data
        && (current_is_accessor != desc_is_accessor || (!current_is_data && !current_is_accessor) != (!desc_is_data && !desc_is_accessor))
    {
        // 6a: If current.configurable is false, return false
        if !current_configurable {
            return false;
        }
        return true;
    }

    // Step 7: If both are data descriptors
    if current_is_data && desc_is_data {
        // 7a: If current.configurable is false and current.writable is false
        let current_writable = get_bool(current, "writable").unwrap_or(false);
        if !current_configurable && !current_writable {
            // 7a.i: If Desc.writable is true, return false
            if let Some(true) = get_bool(desc, "writable") {
                return false;
            }
            // 7a.ii: If Desc.value is present and SameValue(Desc.value, current.value) is false
            if let Some(desc_val) = get_val(desc, "value") {
                let current_val = get_val(current, "value").unwrap_or(Value::Undefined);
                if !crate::core::values_equal(mc, &desc_val, &current_val) {
                    return false;
                }
            }
        }
        return true;
    }

    // Step 8: Both are accessor descriptors
    if !current_configurable {
        // 8a: If Desc.set is present and SameValue(Desc.set, current.set) is false
        if let Some(desc_set) = get_val(desc, "set") {
            let current_set = get_val(current, "set").unwrap_or(Value::Undefined);
            if !crate::core::values_equal(mc, &desc_set, &current_set) {
                return false;
            }
        }
        // 8b: If Desc.get is present  and SameValue(Desc.get, current.get) is false
        if let Some(desc_get) = get_val(desc, "get") {
            let current_get = get_val(current, "get").unwrap_or(Value::Undefined);
            if !crate::core::values_equal(mc, &desc_get, &current_get) {
                return false;
            }
        }
    }

    true
}

/// Public wrapper for is_compatible_property_descriptor
pub(crate) fn is_compatible_property_descriptor_pub<'gc>(
    mc: &MutationContext<'gc>,
    extensible: bool,
    desc: &crate::core::JSObjectDataPtr<'gc>,
    current: &crate::core::JSObjectDataPtr<'gc>,
) -> bool {
    is_compatible_property_descriptor(mc, extensible, desc, current)
}

/// Call the getOwnPropertyDescriptor trap (or default) and return the descriptor's [[Enumerable]] value
/// as Some(true/false) if descriptor exists, or None if it is undefined.
pub(crate) fn proxy_get_own_property_descriptor<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &crate::core::PropertyKey<'gc>,
) -> Result<Option<crate::core::JSObjectDataPtr<'gc>>, EvalError<'gc>> {
    let proxy_gc = *proxy;
    let key_clone = key.clone();
    let res = apply_proxy_trap(
        mc,
        proxy,
        "getOwnPropertyDescriptor",
        vec![(*proxy.target).clone(), property_key_to_value(key)],
        || {
            // Default: target.[[GetOwnProperty]](P) — OrdinaryGetOwnProperty
            match &*proxy_gc.target {
                Value::Object(obj) => {
                    // If target is itself a proxy, delegate to its [[GetOwnProperty]]
                    if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                        && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                    {
                        match proxy_get_own_property_descriptor(mc, inner_proxy, &key_clone)? {
                            Some(desc_obj) => Ok(Value::Object(desc_obj)),
                            None => Ok(Value::Undefined),
                        }
                    } else if let PropertyKey::String(ref s) = key_clone
                        && let Some(ta_cell) = crate::core::slot_get(obj, &InternalSlot::TypedArray)
                        && let Value::TypedArray(ta) = &*ta_cell.borrow()
                        && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
                    {
                        // TypedArray [[GetOwnProperty]] for CanonicalNumericIndexString
                        if crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                            let idx = num_idx as usize;
                            let value = match ta.kind {
                                crate::core::TypedArrayKind::BigInt64 | crate::core::TypedArrayKind::BigUint64 => {
                                    let size = ta.element_size();
                                    let byte_offset = ta.byte_offset + idx * size;
                                    let buffer = ta.buffer.borrow();
                                    let data = buffer.data.lock().unwrap();
                                    if byte_offset + size <= data.len() {
                                        let bytes = &data[byte_offset..byte_offset + size];
                                        if matches!(ta.kind, crate::core::TypedArrayKind::BigInt64) {
                                            let mut b = [0u8; 8];
                                            b.copy_from_slice(bytes);
                                            Value::BigInt(Box::new(num_bigint::BigInt::from(i64::from_le_bytes(b))))
                                        } else {
                                            let mut b = [0u8; 8];
                                            b.copy_from_slice(bytes);
                                            Value::BigInt(Box::new(num_bigint::BigInt::from(u64::from_le_bytes(b))))
                                        }
                                    } else {
                                        Value::Undefined
                                    }
                                }
                                _ => {
                                    let n = ta.get(idx)?;
                                    Value::Number(n)
                                }
                            };
                            let desc_obj = crate::core::new_js_object_data(mc);
                            object_set_key_value(mc, &desc_obj, "value", &value)?;
                            object_set_key_value(mc, &desc_obj, "writable", &Value::Boolean(true))?;
                            object_set_key_value(mc, &desc_obj, "enumerable", &Value::Boolean(true))?;
                            object_set_key_value(mc, &desc_obj, "configurable", &Value::Boolean(true))?;
                            Ok(Value::Object(desc_obj))
                        } else {
                            Ok(Value::Undefined)
                        }
                    } else if let Some(val_rc) = crate::core::get_own_property(obj, &key_clone) {
                        let val = val_rc.borrow().clone();
                        let desc_obj = crate::core::new_js_object_data(mc);

                        // Check if this is an accessor property
                        let is_accessor = matches!(&val, Value::Getter(..) | Value::Setter(..) | Value::Property { value: None, .. });

                        if is_accessor {
                            // Build accessor descriptor
                            match &val {
                                Value::Getter(body, env, home) => {
                                    crate::core::object_set_key_value(
                                        mc,
                                        &desc_obj,
                                        "get",
                                        &Value::Getter(body.clone(), *env, home.clone()),
                                    )?;
                                    crate::core::object_set_key_value(mc, &desc_obj, "set", &Value::Undefined)?;
                                }
                                Value::Setter(params, body, env, home) => {
                                    crate::core::object_set_key_value(mc, &desc_obj, "get", &Value::Undefined)?;
                                    crate::core::object_set_key_value(
                                        mc,
                                        &desc_obj,
                                        "set",
                                        &Value::Setter(params.clone(), body.clone(), *env, home.clone()),
                                    )?;
                                }
                                Value::Property { getter, setter, .. } => {
                                    if let Some(g) = getter {
                                        crate::core::object_set_key_value(mc, &desc_obj, "get", g)?;
                                    } else {
                                        crate::core::object_set_key_value(mc, &desc_obj, "get", &Value::Undefined)?;
                                    }
                                    if let Some(s) = setter {
                                        crate::core::object_set_key_value(mc, &desc_obj, "set", s)?;
                                    } else {
                                        crate::core::object_set_key_value(mc, &desc_obj, "set", &Value::Undefined)?;
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            // Build data descriptor
                            let actual_val = match &val {
                                Value::Property { value: Some(v), .. } => v.borrow().clone(),
                                other => other.clone(),
                            };
                            crate::core::object_set_key_value(mc, &desc_obj, "value", &actual_val)?;
                            let is_writable = obj.borrow().is_writable(&key_clone);
                            crate::core::object_set_key_value(mc, &desc_obj, "writable", &Value::Boolean(is_writable))?;
                        }

                        let is_enum = obj.borrow().is_enumerable(&key_clone);
                        crate::core::object_set_key_value(mc, &desc_obj, "enumerable", &Value::Boolean(is_enum))?;
                        let is_conf = obj.borrow().is_configurable(&key_clone);
                        crate::core::object_set_key_value(mc, &desc_obj, "configurable", &Value::Boolean(is_conf))?;
                        Ok(Value::Object(desc_obj))
                    } else {
                        Ok(Value::Undefined)
                    }
                }
                _ => Ok(Value::Undefined),
            }
        },
    )?;

    match &res {
        Value::Undefined => {
            // Trap returned undefined — check invariants (spec 10.5.5 steps 14-15)
            // Step 9: Let targetDesc be ? target.[[GetOwnProperty]](P).
            if let Value::Object(target_obj) = &*proxy_gc.target {
                // Use target.[[GetOwnProperty]] which may recurse through inner proxy
                let target_desc = if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj, &InternalSlot::Proxy)
                    && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                {
                    proxy_get_own_property_descriptor(mc, inner_proxy, key)?
                } else {
                    crate::core::get_own_property(target_obj, key).map(|_| *target_obj) // dummy: just need Some/None
                };

                if target_desc.is_some() {
                    // targetDesc exists — get configurable from target
                    let target_configurable = if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj, &InternalSlot::Proxy)
                        && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                    {
                        // Get from the descriptor object returned by inner proxy
                        match proxy_get_own_property_descriptor(mc, inner_proxy, key)? {
                            Some(d) => crate::core::object_get_key_value(&d, "configurable")
                                .map(|v| v.borrow().to_truthy())
                                .unwrap_or(false),
                            None => true,
                        }
                    } else {
                        target_obj.borrow().is_configurable(key)
                    };
                    if !target_configurable {
                        return Err(raise_type_error!("'getOwnPropertyDescriptor' on proxy: trap returned undefined for property which is non-configurable in the proxy target").into());
                    }
                    let extensible = if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj, &InternalSlot::Proxy)
                        && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                    {
                        proxy_is_extensible(mc, inner_proxy)?
                    } else {
                        target_obj.borrow().is_extensible()
                    };
                    if !extensible {
                        return Err(raise_type_error!("'getOwnPropertyDescriptor' on proxy: trap returned undefined for property which exists in the non-extensible proxy target").into());
                    }
                }
            }
            Ok(None)
        }
        Value::Object(desc_obj) => {
            // Trap returned an object - perform invariant checks (spec 10.5.5 steps 16-22)
            if let Value::Object(target_obj) = &*proxy_gc.target {
                // Get targetDesc via target.[[GetOwnProperty]], which may recurse through inner proxy
                let (target_has_prop, target_desc_configurable, target_desc_writable) = if let Some(inner_proxy_cell) =
                    crate::core::slot_get(target_obj, &InternalSlot::Proxy)
                    && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                {
                    match proxy_get_own_property_descriptor(mc, inner_proxy, key)? {
                        Some(d) => {
                            let conf = crate::core::object_get_key_value(&d, "configurable")
                                .map(|v| v.borrow().to_truthy())
                                .unwrap_or(false);
                            let writ = crate::core::object_get_key_value(&d, "writable")
                                .map(|v| v.borrow().to_truthy())
                                .unwrap_or(false);
                            (true, conf, writ)
                        }
                        None => (false, true, true),
                    }
                } else {
                    let has = crate::core::get_own_property(target_obj, key).is_some();
                    let conf = if has { target_obj.borrow().is_configurable(key) } else { true };
                    let writ = if has { target_obj.borrow().is_writable(key) } else { true };
                    (has, conf, writ)
                };
                let extensible_target = if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj, &InternalSlot::Proxy)
                    && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                {
                    proxy_is_extensible(mc, inner_proxy)?
                } else {
                    target_obj.borrow().is_extensible()
                };

                // Step 17: If targetDesc is undefined and target is not extensible, throw
                if !target_has_prop && !extensible_target {
                    return Err(raise_type_error!(
                        "'getOwnPropertyDescriptor' on proxy: trap reported property for non-extensible target that does not have it"
                    )
                    .into());
                }

                // Check if result says configurable
                let result_configurable = crate::core::object_get_key_value(desc_obj, "configurable")
                    .map(|v| v.borrow().to_truthy())
                    .unwrap_or(false);

                if target_has_prop {
                    // Step 20: If result is non-configurable but target is configurable or doesn't exist
                    if !result_configurable && target_desc_configurable {
                        return Err(raise_type_error!("'getOwnPropertyDescriptor' on proxy: trap reported non-configurable for property that is configurable in the proxy target").into());
                    }

                    // Step 22 (proxy-missing-checks): If resultDesc is non-configurable and non-writable,
                    // but targetDesc is writable, throw TypeError.
                    // Only applies to data descriptors (spec step 17b: IsDataDescriptor check).
                    if !result_configurable && !target_desc_configurable {
                        let result_writable = crate::core::object_get_key_value(desc_obj, "writable")
                            .map(|v| v.borrow().to_truthy())
                            .unwrap_or(false);
                        let result_is_data = crate::core::object_get_key_value(desc_obj, "get").is_none()
                            && crate::core::object_get_key_value(desc_obj, "set").is_none();
                        // Original check: trap can't report writable for non-configurable non-writable target
                        if result_writable && !target_desc_writable {
                            return Err(raise_type_error!("'getOwnPropertyDescriptor' on proxy: trap reported writable for non-configurable non-writable property in the proxy target").into());
                        }
                        // New check: trap can't report non-writable data for non-configurable writable target
                        if result_is_data && !result_writable && target_desc_writable {
                            return Err(raise_type_error!("'getOwnPropertyDescriptor' on proxy: trap reported non-configurable non-writable for property that is writable in the proxy target").into());
                        }
                    }
                } else {
                    // Target doesn't have it; if result says non-configurable, throw
                    if !result_configurable {
                        return Err(raise_type_error!("'getOwnPropertyDescriptor' on proxy: trap reported non-configurable for property that does not exist in the proxy target").into());
                    }
                }
            }
            Ok(Some(*desc_obj))
        }
        _ => {
            // Spec 10.5.5 step 11: If Type(trapResultObj) is not Object or Undefined, throw TypeError
            Err(raise_type_error!("'getOwnPropertyDescriptor' on proxy: trap returned neither object nor undefined").into())
        }
    }
}

/// Convenience: call proxy_get_own_property_descriptor and extract just the enumerable flag.
pub(crate) fn proxy_get_own_property_is_enumerable<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &crate::core::PropertyKey<'gc>,
) -> Result<Option<bool>, EvalError<'gc>> {
    match proxy_get_own_property_descriptor(mc, proxy, key)? {
        None => Ok(None),
        Some(desc_obj) => {
            let enumerable = crate::core::object_get_key_value(&desc_obj, "enumerable")
                .map(|v| v.borrow().to_truthy())
                .unwrap_or(false);
            Ok(Some(enumerable))
        }
    }
}

/// OrdinarySet(O, P, V, Receiver) — spec 10.1.9 / 10.1.9.2
/// Used by Reflect.set when the target is an ordinary (non-proxy) object.
pub(crate) fn ordinary_set<'gc>(
    mc: &MutationContext<'gc>,
    target_obj: &crate::core::JSObjectDataPtr<'gc>,
    key: &PropertyKey<'gc>,
    value: &Value<'gc>,
    receiver: &Value<'gc>,
    env: &crate::core::JSObjectDataPtr<'gc>,
) -> Result<bool, EvalError<'gc>> {
    // Step 1: ownDesc = target.[[GetOwnPropertyDescriptor]](P)
    let own_prop = crate::core::get_own_property(target_obj, key);

    // TypedArray [[GetOwnProperty]] for CanonicalNumericIndex:
    // If target is a TypedArray and key is a valid numeric index, treat it as a
    // writable data property even though it's not in the properties map.
    if own_prop.is_none()
        && let PropertyKey::String(s) = key
        && let Some(ta_cell) = crate::core::slot_get(target_obj, &InternalSlot::TypedArray)
        && let Value::TypedArray(ta) = &*ta_cell.borrow()
        && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
    {
        if crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
            // Synthesize a writable data property — go to create/update on receiver
            return ordinary_set_create_or_update(mc, key, value, receiver, env);
        } else {
            // Invalid numeric index: just return true (silently ignore)
            return Ok(true);
        }
    }

    if let Some(own_val_rc) = own_prop {
        let own_val = own_val_rc.borrow().clone();

        let is_accessor = matches!(
            &own_val,
            Value::Getter(..)
                | Value::Setter(..)
                | Value::Property {
                    getter: Some(_),
                    value: None,
                    ..
                }
                | Value::Property {
                    setter: Some(_),
                    value: None,
                    ..
                }
        );

        if is_accessor {
            // Step 4: Accessor descriptor — extract setter
            let setter_fn = match &own_val {
                Value::Setter(params, body, env, _) => Some(Value::Setter(params.clone(), body.clone(), *env, None)),
                Value::Property { setter: Some(s), .. } => Some((**s).clone()),
                _ => None,
            };
            if let Some(setter) = setter_fn {
                if let Value::Object(recv_obj) = receiver {
                    crate::core::call_setter(mc, recv_obj, &setter, value, Some(env))?;
                } else {
                    crate::core::call_setter(mc, target_obj, &setter, value, Some(env))?;
                }
                return Ok(true);
            } else {
                return Ok(false); // No setter → false
            }
        } else {
            // Step 3: Data descriptor
            if !target_obj.borrow().is_writable(key) {
                return Ok(false);
            }
            return ordinary_set_create_or_update(mc, key, value, receiver, env);
        }
    }

    // Property not on target → walk prototype chain
    let mut proto = target_obj.borrow().prototype;
    while let Some(parent_obj) = proto {
        // If parent is a proxy, delegate to its [[Set]]
        if let Some(inner_proxy_cell) = crate::core::slot_get(&parent_obj, &InternalSlot::Proxy)
            && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
        {
            return proxy_set_property_with_receiver(mc, inner_proxy, key, value, Some(receiver));
        }

        // TypedArray parent: apply TypedArray [[Set]] semantics per ES2024 §10.4.5.5
        if let PropertyKey::String(s) = key
            && let Some(ta_cell) = crate::core::slot_get(&parent_obj, &InternalSlot::TypedArray)
            && let Value::TypedArray(ta) = &*ta_cell.borrow()
            && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
        {
            // Check SameValue(O, Receiver) first — spec step 1.b.i
            let same = match receiver {
                Value::Object(recv_obj) => crate::core::Gc::ptr_eq(parent_obj, *recv_obj),
                _ => false,
            };
            if same {
                // TypedArraySetElement: coerce value, then set if valid index
                if crate::js_typedarray::is_bigint_typed_array(&ta.kind) {
                    let n = crate::js_typedarray::to_bigint_i64(mc, env, value)?;
                    if crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                        ta.set_bigint(mc, num_idx as usize, n)?;
                    }
                } else {
                    let n = crate::core::to_number_with_env(mc, env, value)?;
                    if crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                        ta.set(mc, num_idx as usize, n)?;
                    }
                }
                return Ok(true);
            } else {
                // Receiver != TypedArray — spec step 1.b.ii
                if !crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                    return Ok(true); // invalid index → silently succeed
                }
                // Valid index, different receiver → OrdinarySet → create on receiver
                return ordinary_set_create_or_update(mc, key, value, receiver, env);
            }
        }

        let parent_prop = crate::core::get_own_property(&parent_obj, key);
        if let Some(parent_val_rc) = parent_prop {
            let parent_val = parent_val_rc.borrow().clone();
            let is_proto_accessor = matches!(
                &parent_val,
                Value::Getter(..)
                    | Value::Setter(..)
                    | Value::Property {
                        getter: Some(_),
                        value: None,
                        ..
                    }
                    | Value::Property {
                        setter: Some(_),
                        value: None,
                        ..
                    }
            );
            if is_proto_accessor {
                let setter_fn = match &parent_val {
                    Value::Setter(params, body, env, _) => Some(Value::Setter(params.clone(), body.clone(), *env, None)),
                    Value::Property { setter: Some(s), .. } => Some((**s).clone()),
                    _ => None,
                };
                if let Some(setter) = setter_fn {
                    if let Value::Object(recv_obj) = receiver {
                        crate::core::call_setter(mc, recv_obj, &setter, value, Some(env))?;
                    }
                    return Ok(true);
                } else {
                    return Ok(false);
                }
            }
            // Non-writable data property on prototype → cannot set
            if !parent_obj.borrow().is_writable(key) {
                return Ok(false);
            }
            break; // Fall through to CreateDataProperty on Receiver
        }
        proto = parent_obj.borrow().prototype;
    }

    // Default ownDesc = {value: undefined, writable: true, enumerable: true, configurable: true}
    // → CreateDataProperty(Receiver, P, V)
    ordinary_set_create_or_update(mc, key, value, receiver, env)
}

/// Helper for OrdinarySet steps 3.c–3.e: operate on Receiver to create or update property.
fn ordinary_set_create_or_update<'gc>(
    mc: &MutationContext<'gc>,
    key: &PropertyKey<'gc>,
    value: &Value<'gc>,
    receiver: &Value<'gc>,
    env: &crate::core::JSObjectDataPtr<'gc>,
) -> Result<bool, EvalError<'gc>> {
    match receiver {
        Value::Object(recv_obj) => {
            // If receiver is a proxy, use proxy traps
            if let Some(inner_proxy_cell) = crate::core::slot_get(recv_obj, &InternalSlot::Proxy)
                && let Value::Proxy(recv_proxy) = &*inner_proxy_cell.borrow()
            {
                let recv_has = proxy_get_own_property_descriptor(mc, recv_proxy, key)?;
                if recv_has.is_some() {
                    let ok = proxy_define_property_value_only(mc, recv_proxy, key, value)?;
                    Ok(ok)
                } else {
                    let ok = proxy_define_data_property(mc, recv_proxy, key, value)?;
                    Ok(ok)
                }
            } else {
                // TypedArray receiver: if key is CanonicalNumericIndex, use TypedArray
                // [[GetOwnProperty]]/[[DefineOwnProperty]] instead of properties map.
                if let PropertyKey::String(s) = key
                    && let Some(ta_cell) = crate::core::slot_get(recv_obj, &InternalSlot::TypedArray)
                    && let Value::TypedArray(ta) = &*ta_cell.borrow()
                    && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
                {
                    if crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                        // Existing writable data property → IntegerIndexedElementSet
                        if crate::js_typedarray::is_bigint_typed_array(&ta.kind) {
                            let n = crate::js_typedarray::to_bigint_i64(mc, env, value)?;
                            ta.set_bigint(mc, num_idx as usize, n)?;
                        } else {
                            let n = crate::core::to_number_with_env(mc, env, value)?;
                            ta.set(mc, num_idx as usize, n)?;
                        }
                        return Ok(true);
                    } else {
                        // Invalid index → [[DefineOwnProperty]] returns false
                        return Ok(false);
                    }
                }

                // Ordinary receiver
                let recv_has = crate::core::get_own_property(recv_obj, key);
                if let Some(recv_val) = recv_has {
                    // Step 3.d: existing property on receiver — check accessor/writable
                    let rv = recv_val.borrow().clone();
                    let is_recv_accessor = matches!(
                        &rv,
                        Value::Getter(..)
                            | Value::Setter(..)
                            | Value::Property {
                                getter: Some(_),
                                value: None,
                                ..
                            }
                            | Value::Property {
                                setter: Some(_),
                                value: None,
                                ..
                            }
                    );
                    if is_recv_accessor {
                        return Ok(false);
                    }
                    if !recv_obj.borrow().is_writable(key) {
                        return Ok(false);
                    }
                    // Array exotic [[DefineOwnProperty]] for "length": coerce value first,
                    // then re-check writable (coercion may make it non-writable via side effects).
                    if let PropertyKey::String(s) = key
                        && s == "length"
                        && crate::js_array::is_array(mc, recv_obj)
                        && !matches!(value, Value::Property { .. })
                    {
                        let uint32_len = crate::core::to_uint32_value_with_env(mc, env, value)?;
                        let number_len = crate::core::to_number_with_env(mc, env, value)?;
                        if (uint32_len as f64) != number_len {
                            return Err(raise_range_error!("Invalid array length").into());
                        }
                        // Re-check writable after coercion (Symbol.toPrimitive may have changed it)
                        if !recv_obj.borrow().is_writable(key) {
                            return Ok(false);
                        }
                        crate::core::object_set_length(mc, recv_obj, uint32_len as usize).map_err(EvalError::from)?;
                        return Ok(true);
                    }
                    object_set_key_value(mc, recv_obj, key, value)?;
                } else {
                    // Step 3.e: CreateDataProperty — check extensibility
                    if !recv_obj.borrow().is_extensible() {
                        return Ok(false);
                    }
                    object_set_key_value(mc, recv_obj, key, value)?;
                }
                Ok(true)
            }
        }
        _ => Ok(false), // Receiver is not an object → false
    }
}

/// Set property on proxy target, applying set trap if available
pub(crate) fn proxy_set_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    value: &Value<'gc>,
) -> Result<bool, EvalError<'gc>> {
    proxy_set_property_with_receiver(mc, proxy, key, value, None)
}

/// Set property on proxy target with explicit wrapper (receiver = proxy wrapper)
pub(crate) fn proxy_set_property_with_wrapper<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    value: &Value<'gc>,
    wrapper: &crate::core::JSObjectDataPtr<'gc>,
) -> Result<bool, EvalError<'gc>> {
    proxy_set_property_with_receiver(mc, proxy, key, value, Some(&Value::Object(*wrapper)))
}

pub(crate) fn proxy_set_property_with_receiver<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    value: &Value<'gc>,
    receiver: Option<&Value<'gc>>,
) -> Result<bool, EvalError<'gc>> {
    let proxy_gc = *proxy;
    let key_clone = key.clone();
    let value_clone = value.clone();
    let receiver_clone = receiver.cloned();

    // Per spec 10.5.9 step 8: trap always receives 4 args: target, P, V, Receiver
    // When receiver is None, the Receiver defaults to the target (per OrdinarySet default)
    let receiver_val = receiver.cloned().unwrap_or_else(|| (*proxy.target).clone());
    let trap_args = vec![(*proxy.target).clone(), property_key_to_value(key), value.clone(), receiver_val];

    let result = apply_proxy_trap(mc, proxy, "set", trap_args, || {
        // Default behavior: target.[[Set]](P, V, Receiver) — OrdinarySet
        // Per spec 10.5.9 step 5: If trap is undefined, return ? target.[[Set]](P, V, Receiver).
        // OrdinarySet (10.1.2.2) delegates back through Receiver for
        // [[GetOwnPropertyDescriptor]] and [[DefineOwnProperty]].
        match &*proxy_gc.target {
            Value::Object(target_obj) => {
                // If target is itself a proxy, recurse through its [[Set]]
                if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj, &InternalSlot::Proxy)
                    && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                {
                    return Ok(Value::Boolean(proxy_set_property_with_receiver(
                        mc,
                        inner_proxy,
                        &key_clone,
                        &value_clone,
                        receiver_clone.as_ref(),
                    )?));
                }

                // OrdinarySet(target, P, V, Receiver) → OrdinarySetWithOwnDescriptor
                // Step 1: ownDesc = target.[[GetOwnPropertyDescriptor]](P)
                let own_prop = crate::core::get_own_property(target_obj, &key_clone);

                // TypedArray target: if target is a TA and key is CanonicalNumericIndex,
                // synthesize a writable data descriptor from the buffer element.
                if own_prop.is_none()
                    && let PropertyKey::String(ref s) = key_clone
                    && let Some(ta_cell) = crate::core::slot_get(target_obj, &InternalSlot::TypedArray)
                    && let Value::TypedArray(ta) = &*ta_cell.borrow()
                    && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
                {
                    if crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                        // Writable data descriptor → create/update on Receiver
                        if let Some(Value::Object(recv_obj)) = &receiver_clone {
                            if let Some(rp_cell) = crate::core::slot_get(recv_obj, &InternalSlot::Proxy)
                                && let Value::Proxy(recv_proxy) = &*rp_cell.borrow()
                            {
                                let recv_has = proxy_get_own_property_descriptor(mc, recv_proxy, &key_clone)?;
                                if recv_has.is_some() {
                                    let ok = proxy_define_property_value_only(mc, recv_proxy, &key_clone, &value_clone)?;
                                    return Ok(Value::Boolean(ok));
                                } else {
                                    let ok = proxy_define_data_property(mc, recv_proxy, &key_clone, &value_clone)?;
                                    return Ok(Value::Boolean(ok));
                                }
                            } else {
                                // TypedArray receiver
                                if let Some(rtc) = crate::core::slot_get(recv_obj, &InternalSlot::TypedArray)
                                    && let Value::TypedArray(rta) = &*rtc.borrow()
                                    && let Some(ri) = crate::js_typedarray::canonical_numeric_index_string(s)
                                {
                                    if crate::js_typedarray::is_valid_integer_index(rta, ri) {
                                        let renv = target_obj.borrow().definition_env.unwrap_or(*target_obj);
                                        if crate::js_typedarray::is_bigint_typed_array(&rta.kind) {
                                            let n = crate::js_typedarray::to_bigint_i64(mc, &renv, &value_clone)?;
                                            rta.set_bigint(mc, ri as usize, n)?;
                                        } else {
                                            let n = crate::core::to_number_with_env(mc, &renv, &value_clone)?;
                                            rta.set(mc, ri as usize, n)?;
                                        }
                                        return Ok(Value::Boolean(true));
                                    } else {
                                        return Ok(Value::Boolean(false));
                                    }
                                }
                                object_set_key_value(mc, recv_obj, &key_clone, &value_clone)?;
                                return Ok(Value::Boolean(true));
                            }
                        }
                        return Ok(Value::Boolean(false));
                    } else {
                        return Ok(Value::Boolean(true)); // Invalid CanonicalNumericIndex → no-op
                    }
                }

                if let Some(own_val_rc) = own_prop {
                    let own_val = own_val_rc.borrow().clone();

                    // Check if accessor descriptor
                    let is_accessor = matches!(
                        &own_val,
                        Value::Getter(..)
                            | Value::Setter(..)
                            | Value::Property {
                                getter: Some(_),
                                value: None,
                                ..
                            }
                            | Value::Property {
                                setter: Some(_),
                                value: None,
                                ..
                            }
                    );

                    if is_accessor {
                        // OrdinarySetWithOwnDescriptor step 5: accessor descriptor
                        // Extract the setter; if set is undefined → return false
                        let setter_fn = match &own_val {
                            Value::Setter(params, body, env, _home) => Some(Value::Setter(params.clone(), body.clone(), *env, None)),
                            Value::Property { setter: Some(s), .. } => Some((**s).clone()),
                            _ => None,
                        };
                        if let Some(setter) = setter_fn {
                            // Call setter with Receiver as this
                            let recv = receiver_clone.clone().unwrap_or(Value::Object(*target_obj));
                            match &recv {
                                Value::Object(recv_obj) => {
                                    crate::core::call_setter(mc, recv_obj, &setter, &value_clone, None)?;
                                }
                                _ => {
                                    crate::core::call_setter(mc, target_obj, &setter, &value_clone, None)?;
                                }
                            }
                            Ok(Value::Boolean(true))
                        } else {
                            // No setter → return false
                            Ok(Value::Boolean(false))
                        }
                    } else {
                        // Data descriptor on target
                        if !target_obj.borrow().is_writable(&key_clone) {
                            return Ok(Value::Boolean(false));
                        }

                        // OrdinarySetWithOwnDescriptor step 3.c-e: operate on Receiver
                        // If Receiver is a proxy, use proxy traps; otherwise ordinary ops
                        if let Some(Value::Object(recv_obj)) = &receiver_clone {
                            if let Some(inner_proxy_cell) = crate::core::slot_get(recv_obj, &InternalSlot::Proxy)
                                && let Value::Proxy(recv_proxy) = &*inner_proxy_cell.borrow()
                            {
                                // Receiver is a proxy → use its traps
                                let recv_has = proxy_get_own_property_descriptor(mc, recv_proxy, &key_clone)?;
                                if recv_has.is_some() {
                                    let ok = proxy_define_property_value_only(mc, recv_proxy, &key_clone, &value_clone)?;
                                    return Ok(Value::Boolean(ok));
                                } else {
                                    let ok = proxy_define_data_property(mc, recv_proxy, &key_clone, &value_clone)?;
                                    return Ok(Value::Boolean(ok));
                                }
                            } else {
                                // Receiver is ordinary object
                                let _recv_has = crate::core::get_own_property(recv_obj, &key_clone);
                                // Array exotic [[DefineOwnProperty]] for "length": coerce, then re-check writable
                                if let PropertyKey::String(s) = &key_clone
                                    && s == "length"
                                    && crate::js_array::is_array(mc, recv_obj)
                                    && !matches!(value_clone, Value::Property { .. })
                                {
                                    if !recv_obj.borrow().is_writable(&key_clone) {
                                        return Ok(Value::Boolean(false));
                                    }
                                    let call_env = recv_obj
                                        .borrow()
                                        .definition_env
                                        .or(target_obj.borrow().definition_env)
                                        .unwrap_or(*target_obj);
                                    let uint32_len = crate::core::to_uint32_value_with_env(mc, &call_env, &value_clone)?;
                                    let number_len = crate::core::to_number_with_env(mc, &call_env, &value_clone)?;
                                    if (uint32_len as f64) != number_len {
                                        return Err(raise_range_error!("Invalid array length").into());
                                    }
                                    if !recv_obj.borrow().is_writable(&key_clone) {
                                        return Ok(Value::Boolean(false));
                                    }
                                    crate::core::object_set_length(mc, recv_obj, uint32_len as usize).map_err(EvalError::from)?;
                                    return Ok(Value::Boolean(true));
                                }
                                // if recv_has.is_some() {
                                object_set_key_value(mc, recv_obj, &key_clone, &value_clone)?;
                                // } else {
                                // object_set_key_value(mc, recv_obj, &key_clone, &value_clone)?;
                                // }
                                return Ok(Value::Boolean(true));
                            }
                        }

                        // Fallback: Receiver is the proxy itself (original behavior)
                        let receiver_has = proxy_get_own_property_descriptor(mc, &proxy_gc, &key_clone)?;
                        if receiver_has.is_some() {
                            let ok = proxy_define_property_value_only(mc, &proxy_gc, &key_clone, &value_clone)?;
                            Ok(Value::Boolean(ok))
                        } else {
                            let ok = proxy_define_data_property(mc, &proxy_gc, &key_clone, &value_clone)?;
                            Ok(Value::Boolean(ok))
                        }
                    }
                } else {
                    // Property not found on target; walk prototype chain
                    // OrdinarySet step 2-3: look up prototype
                    let mut proto = target_obj.borrow().prototype;
                    while let Some(parent_obj) = proto {
                        // If parent is proxy, recurse through its [[Set]]
                        if let Some(inner_proxy_cell) = crate::core::slot_get(&parent_obj, &InternalSlot::Proxy)
                            && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                        {
                            return Ok(Value::Boolean(proxy_set_property_with_receiver(
                                mc,
                                inner_proxy,
                                &key_clone,
                                &value_clone,
                                receiver_clone.as_ref(),
                            )?));
                        }
                        // TypedArray parent: apply [[Set]] semantics per §10.4.5.5
                        if let PropertyKey::String(ref s) = key_clone
                            && let Some(ta_cell) = crate::core::slot_get(&parent_obj, &InternalSlot::TypedArray)
                            && let Value::TypedArray(ta) = &*ta_cell.borrow()
                            && let Some(num_idx) = crate::js_typedarray::canonical_numeric_index_string(s)
                        {
                            let same = match &receiver_clone {
                                Some(Value::Object(recv_obj)) => crate::core::Gc::ptr_eq(parent_obj, *recv_obj),
                                _ => false,
                            };
                            if same {
                                // TypedArraySetElement: coerce then set if valid
                                let env_for_coerce = target_obj.borrow().definition_env.unwrap_or(*target_obj);
                                if crate::js_typedarray::is_bigint_typed_array(&ta.kind) {
                                    let n = crate::js_typedarray::to_bigint_i64(mc, &env_for_coerce, &value_clone)?;
                                    if crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                                        ta.set_bigint(mc, num_idx as usize, n)?;
                                    }
                                } else {
                                    let n = crate::core::to_number_with_env(mc, &env_for_coerce, &value_clone)?;
                                    if crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                                        ta.set(mc, num_idx as usize, n)?;
                                    }
                                }
                                return Ok(Value::Boolean(true));
                            } else {
                                if !crate::js_typedarray::is_valid_integer_index(ta, num_idx) {
                                    return Ok(Value::Boolean(true)); // invalid index → no-op
                                }
                                // Valid index, different receiver → create on receiver
                                break; // Fall through to CreateDataProperty on Receiver
                            }
                        }

                        let parent_prop = crate::core::get_own_property(&parent_obj, &key_clone);
                        if let Some(parent_val_rc) = parent_prop {
                            let parent_val = parent_val_rc.borrow().clone();
                            // Check if accessor on prototype
                            let is_proto_accessor = matches!(
                                &parent_val,
                                Value::Getter(..)
                                    | Value::Setter(..)
                                    | Value::Property {
                                        getter: Some(_),
                                        value: None,
                                        ..
                                    }
                                    | Value::Property {
                                        setter: Some(_),
                                        value: None,
                                        ..
                                    }
                            );
                            if is_proto_accessor {
                                // Call setter with Receiver as this
                                let setter_fn = match &parent_val {
                                    Value::Setter(params, body, env, _home) => {
                                        Some(Value::Setter(params.clone(), body.clone(), *env, None))
                                    }
                                    Value::Property { setter: Some(s), .. } => Some((**s).clone()),
                                    _ => None,
                                };
                                if let Some(setter) = setter_fn {
                                    let recv = receiver_clone.clone().unwrap_or(Value::Object(*target_obj));
                                    if let Value::Object(recv_obj) = &recv {
                                        crate::core::call_setter(mc, recv_obj, &setter, &value_clone, None)?;
                                    }
                                    return Ok(Value::Boolean(true));
                                } else {
                                    return Ok(Value::Boolean(false));
                                }
                            }
                            // Data descriptor found on prototype
                            if !parent_obj.borrow().is_writable(&key_clone) {
                                return Ok(Value::Boolean(false));
                            }
                            break; // Fall through to CreateDataProperty on Receiver
                        }
                        proto = parent_obj.borrow().prototype;
                    }
                    // Per spec OrdinarySetWithOwnDescriptor steps 2c + 3:
                    // When ownDesc is undefined and no parent overrides, set ownDesc to
                    // {value: undefined, writable: true, enumerable: true, configurable: true}
                    // Then step 3.c: existingDescriptor = Receiver.[[GetOwnProperty]](P)
                    if let Some(Value::Object(recv_obj)) = &receiver_clone {
                        if let Some(inner_proxy_cell) = crate::core::slot_get(recv_obj, &InternalSlot::Proxy)
                            && let Value::Proxy(recv_proxy) = &*inner_proxy_cell.borrow()
                        {
                            // Receiver is a proxy → call its GOPD trap first (spec step 3.c)
                            let recv_has = proxy_get_own_property_descriptor(mc, recv_proxy, &key_clone)?;
                            if recv_has.is_some() {
                                // Step 3.d: existingDescriptor is not undefined → DefineProperty({value: V})
                                let ok = proxy_define_property_value_only(mc, recv_proxy, &key_clone, &value_clone)?;
                                Ok(Value::Boolean(ok))
                            } else {
                                // Step 3.e: CreateDataProperty(Receiver, P, V)
                                let ok = proxy_define_data_property(mc, recv_proxy, &key_clone, &value_clone)?;
                                Ok(Value::Boolean(ok))
                            }
                        } else {
                            object_set_key_value(mc, recv_obj, &key_clone, &value_clone)?;
                            Ok(Value::Boolean(true))
                        }
                    } else {
                        let ok = proxy_define_data_property(mc, &proxy_gc, &key_clone, &value_clone)?;
                        Ok(Value::Boolean(ok))
                    }
                }
            }
            _ => Ok(Value::Boolean(false)),
        }
    })?;

    // Per spec 10.5.9 step 8: booleanTrapResult = ToBoolean(trapResult)
    let trap_success = result.to_truthy();
    if !trap_success {
        return Ok(false);
    }

    // Post-trap invariant checks (spec 10.5.9 steps 9-14)
    // Step 9: targetDesc = target.[[GetOwnPropertyDescriptor]](P)
    if let Value::Object(target_obj) = &*proxy_gc.target
        && let Some(target_prop_rc) = crate::core::get_own_property(target_obj, key)
    {
        let target_prop = target_prop_rc.borrow().clone();
        let is_configurable = target_obj.borrow().is_configurable(key);
        if !is_configurable {
            // Step 10a: If IsDataDescriptor(targetDesc) and targetDesc.[[Writable]] is false
            // An accessor desc has value: None; data desc has value: Some(_) or is a plain value
            let is_accessor = matches!(
                &target_prop,
                Value::Getter(..) | Value::Setter(..) | Value::Property { value: None, .. }
            );
            let is_data = !is_accessor;
            if is_data && !target_obj.borrow().is_writable(key) {
                // Step 10a.i: If SameValue(V, targetDesc.[[Value]]) is false, throw TypeError
                let target_value = match &target_prop {
                    Value::Property { value: Some(v), .. } => v.borrow().clone(),
                    other => other.clone(),
                };
                // SameValue: like strict equality but NaN===NaN is true and +0/-0 differ
                let same = match (value, &target_value) {
                    (Value::Number(a), Value::Number(b)) => {
                        if a.is_nan() && b.is_nan() {
                            true
                        } else if *a == 0.0 && *b == 0.0 {
                            a.is_sign_positive() == b.is_sign_positive()
                        } else {
                            a == b
                        }
                    }
                    _ => crate::core::same_value_zero(value, &target_value),
                };
                if !same {
                    return Err(raise_type_error!("'set' on proxy: trap returned truish for property which exists in the proxy target as a non-configurable and non-writable data property with a different value").into());
                }
            }
            // Step 10b: If IsAccessorDescriptor(targetDesc) and targetDesc.[[Set]] is undefined
            if !is_data {
                let has_setter = matches!(&target_prop, Value::Setter(..) | Value::Property { setter: Some(_), .. });
                if !has_setter {
                    return Err(raise_type_error!("'set' on proxy: trap returned truish for property which exists in the proxy target as a non-configurable and non-writable accessor property without a setter").into());
                }
            }
        }
    }

    Ok(true)
}

/// Proxy [[DefineOwnProperty]] — forwards a full descriptor object through the proxy chain.
/// Used when the default behavior needs to forward to a target that may itself be a proxy.
pub(crate) fn proxy_define_own_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    desc_obj: &crate::core::JSObjectDataPtr<'gc>,
) -> Result<bool, EvalError<'gc>> {
    let proxy_gc = *proxy;
    let key_clone = key.clone();
    let key_clone2 = key.clone();
    let desc_obj_copy = *desc_obj;
    let result = apply_proxy_trap(
        mc,
        proxy,
        "defineProperty",
        vec![(*proxy.target).clone(), property_key_to_value(key), Value::Object(*desc_obj)],
        || match &*proxy_gc.target {
            Value::Object(obj) => {
                // If target is itself a proxy, recurse
                if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                    && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                {
                    return Ok(Value::Boolean(proxy_define_own_property(
                        mc,
                        inner_proxy,
                        &key_clone,
                        &desc_obj_copy,
                    )?));
                }
                match crate::js_object::define_property_internal(mc, obj, &key_clone, &desc_obj_copy) {
                    Ok(()) => Ok(Value::Boolean(true)),
                    Err(_) => Ok(Value::Boolean(false)),
                }
            }
            _ => Ok(Value::Boolean(false)),
        },
    )?;

    let trap_result = match result {
        Value::Boolean(b) => b,
        _ => result.to_truthy(),
    };

    // If trap returned false, return false (no invariant checks needed)
    if !trap_result {
        return Ok(false);
    }

    // Spec 10.5.6 steps 11-17: Post-trap invariant checks
    if let Value::Object(target_obj) = &*proxy_gc.target {
        // Step 11: Let targetDesc be ? target.[[GetOwnProperty]](P)
        let target_desc_opt = if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj, &InternalSlot::Proxy)
            && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
        {
            proxy_get_own_property_descriptor(mc, inner_proxy, &key_clone2)?
        } else if crate::core::get_own_property(target_obj, &key_clone2).is_some() {
            // Build a descriptor object for the target's own property
            let td = crate::core::build_property_descriptor(mc, target_obj, &key_clone2);
            match td {
                Some(pd) => Some(pd.to_object(mc)?),
                None => None,
            }
        } else {
            None
        };

        // Step 12: Let extensibleTarget be ? IsExtensible(target)
        let extensible_target = if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj, &InternalSlot::Proxy)
            && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
        {
            proxy_is_extensible(mc, inner_proxy)?
        } else {
            target_obj.borrow().is_extensible()
        };

        // Step 13-14: settingConfigFalse
        let setting_config_false = object_get_key_value(desc_obj, "configurable")
            .map(|v| !v.borrow().to_truthy())
            .unwrap_or(false);

        if let Some(ref target_desc) = target_desc_opt {
            // Step 16a: IsCompatiblePropertyDescriptor check
            if !is_compatible_property_descriptor(mc, extensible_target, desc_obj, target_desc) {
                return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for property descriptor that is incompatible with the existing property in the proxy target").into());
            }
            // Step 16b: If settingConfigFalse and targetDesc.configurable is true, throw
            let target_configurable = object_get_key_value(target_desc, "configurable")
                .map(|v| v.borrow().to_truthy())
                .unwrap_or(false);
            if setting_config_false && target_configurable {
                return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for defining non-configurable property which is configurable in the target").into());
            }
            // Step 16c: If target is data desc, non-configurable, writable, and Desc.writable is false
            let target_writable = object_get_key_value(target_desc, "writable")
                .map(|v| v.borrow().to_truthy())
                .unwrap_or(false);
            if !target_configurable
                && target_writable
                && let Some(desc_writable_rc) = object_get_key_value(desc_obj, "writable")
                && !desc_writable_rc.borrow().to_truthy()
            {
                return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for defining non-writable property which is writable in the non-configurable proxy target").into());
            }
        } else {
            // Step 15: targetDesc is undefined
            // 15a: If extensibleTarget is false, throw
            if !extensible_target {
                return Err(raise_type_error!(
                    "'defineProperty' on proxy: trap returned truish for adding property to non-extensible target"
                )
                .into());
            }
            // 15b: If settingConfigFalse, throw
            if setting_config_false {
                return Err(raise_type_error!("'defineProperty' on proxy: trap returned truish for defining non-configurable property which does not exist on the target").into());
            }
        }
    }

    Ok(true)
}

/// Define a data property on proxy target, applying defineProperty trap if available.
/// This creates a full data descriptor {value, writable:true, enumerable:true, configurable:true}
/// and routes through proxy_define_own_property which includes all spec invariant checks.
pub(crate) fn proxy_define_data_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    value: &Value<'gc>,
) -> Result<bool, EvalError<'gc>> {
    let desc_obj = new_object_with_proto(mc, proxy);
    object_set_key_value(mc, &desc_obj, "value", value)?;
    object_set_key_value(mc, &desc_obj, "writable", &Value::Boolean(true))?;
    object_set_key_value(mc, &desc_obj, "enumerable", &Value::Boolean(true))?;
    object_set_key_value(mc, &desc_obj, "configurable", &Value::Boolean(true))?;

    proxy_define_own_property(mc, proxy, key, &desc_obj)
}

/// Define only the [[Value]] of an existing property on a proxy target
/// (OrdinarySetWithOwnDescriptor step 2.d.iv: descriptor is {[[Value]]: V} only).
/// Routes through proxy_define_own_property for proper invariant checks.
pub(crate) fn proxy_define_property_value_only<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: &PropertyKey<'gc>,
    value: &Value<'gc>,
) -> Result<bool, EvalError<'gc>> {
    let desc_obj = new_object_with_proto(mc, proxy);
    object_set_key_value(mc, &desc_obj, "value", value)?;

    proxy_define_own_property(mc, proxy, key, &desc_obj)
}

/// Check if property exists on proxy target, applying has trap if available
pub(crate) fn proxy_has_property<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    key: impl Into<PropertyKey<'gc>>,
) -> Result<bool, EvalError<'gc>> {
    let key = key.into();
    let key_clone = key.clone();
    let result = apply_proxy_trap(mc, proxy, "has", vec![(*proxy.target).clone(), property_key_to_value(&key)], || {
        // Default behavior: OrdinaryHasProperty — check own, then walk prototype chain
        match &*proxy.target {
            Value::Object(obj) => {
                // If target is itself a proxy, recurse through its [[HasProperty]]
                if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                    && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                {
                    return Ok(Value::Boolean(proxy_has_property(mc, inner_proxy, key)?));
                }
                // Check own property
                if object_get_key_value(obj, &key).is_some() {
                    return Ok(Value::Boolean(true));
                }
                // Walk prototype chain
                let mut proto = obj.borrow().prototype;
                while let Some(p) = proto {
                    if let Some(inner_proxy_cell) = crate::core::slot_get(&p, &InternalSlot::Proxy)
                        && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                    {
                        return Ok(Value::Boolean(proxy_has_property(mc, inner_proxy, key)?));
                    }
                    if object_get_key_value(&p, &key).is_some() {
                        return Ok(Value::Boolean(true));
                    }
                    proto = p.borrow().prototype;
                }
                Ok(Value::Boolean(false))
            }
            _ => Ok(Value::Boolean(false)), // Non-objects don't have properties
        }
    })?;

    // Per spec, the trap result is coerced to Boolean via ToBoolean.
    let trap_result = result.to_truthy();

    // Post-trap invariant checks (spec 10.5.7 steps 11-12)
    if !trap_result {
        // Step 11: If booleanTrapResult is false, then
        if let Value::Object(target_obj) = &*proxy.target {
            let target_has = crate::core::get_own_property(target_obj, &key_clone).is_some();
            if target_has {
                let is_configurable = target_obj.borrow().is_configurable(&key_clone);
                // Step 11.c.i: If targetDesc is not configurable, throw TypeError
                if !is_configurable {
                    return Err(raise_type_error!(
                        "'has' on proxy: trap returned falsish for property which exists in the proxy target as non-configurable"
                    )
                    .into());
                }
                // Step 11.c.ii-iv: If target is not extensible, throw TypeError
                if !target_obj.borrow().is_extensible() {
                    return Err(raise_type_error!(
                        "'has' on proxy: trap returned falsish for property but the proxy target is not extensible"
                    )
                    .into());
                }
            }
        }
    }

    Ok(trap_result)
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
            // Default behavior: OrdinaryDelete
            match &*proxy.target {
                Value::Object(obj) => {
                    // If target is itself a proxy, recurse through its [[Delete]]
                    if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                        && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                    {
                        return Ok(Value::Boolean(proxy_delete_property(mc, inner_proxy, key)?));
                    }
                    // OrdinaryDelete: if property doesn't exist, return true
                    if !obj.borrow().properties.contains_key(key) {
                        return Ok(Value::Boolean(true));
                    }
                    // If non-configurable, return false
                    if obj.borrow().non_configurable.contains(key) {
                        return Ok(Value::Boolean(false));
                    }
                    obj.borrow_mut(mc).properties.shift_remove(key);
                    Ok(Value::Boolean(true))
                }
                _ => Ok(Value::Boolean(false)), // Non-objects don't have properties
            }
        },
    )?;

    // Per spec, the trap result is coerced to Boolean via ToBoolean.
    let trap_result = result.to_truthy();

    // Post-trap invariant checks (spec 10.5.10 steps 11-12)
    if trap_result {
        // Step 11: If trapResult is true, then
        if let Value::Object(target_obj) = &*proxy.target
            && let Some(_target_prop_rc) = crate::core::get_own_property(target_obj, key)
        {
            let is_configurable = target_obj.borrow().is_configurable(key);
            // Step 11.a: If non-configurable own property, throw TypeError
            if !is_configurable {
                return Err(raise_type_error!(
                    "'deleteProperty' on proxy: trap returned truish for property which is non-configurable in the proxy target"
                )
                .into());
            }
            // Step 12: If target is not extensible and property exists, throw TypeError
            if !target_obj.borrow().is_extensible() {
                return Err(raise_type_error!(
                    "'deleteProperty' on proxy: trap returned truish for property on non-extensible proxy target"
                )
                .into());
            }
        }
    }

    Ok(trap_result)
}

/// Helper function to convert PropertyKey to Value for trap arguments
fn property_key_to_value<'gc>(key: &PropertyKey<'gc>) -> Value<'gc> {
    match key {
        PropertyKey::String(s) => Value::String(utf8_to_utf16(s)),
        PropertyKey::Symbol(sd) => Value::Symbol(*sd),
        PropertyKey::Private(..) => unreachable!("Private keys should not be passed to proxy traps"),
        PropertyKey::Internal(..) => unreachable!("Internal keys should not be passed to proxy traps"),
    }
}

/// Public version of property_key_to_value for use in other modules
pub(crate) fn property_key_to_value_pub<'gc>(key: &PropertyKey<'gc>) -> Value<'gc> {
    property_key_to_value(key)
}

/// Proxy [[IsExtensible]] internal method (spec §10.5.3)
pub(crate) fn proxy_is_extensible<'gc>(mc: &MutationContext<'gc>, proxy: &Gc<'gc, JSProxy<'gc>>) -> Result<bool, EvalError<'gc>> {
    let result = apply_proxy_trap(mc, proxy, "isExtensible", vec![(*proxy.target).clone()], || match &*proxy.target {
        Value::Object(obj) => {
            // If target is itself a proxy, recurse
            if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
            {
                return Ok(Value::Boolean(proxy_is_extensible(mc, inner_proxy)?));
            }
            Ok(Value::Boolean(obj.borrow().is_extensible()))
        }
        _ => Ok(Value::Boolean(true)),
    })?;

    let trap_result = result.to_truthy();

    // Invariant check: trap result must match target.[[IsExtensible]]()
    if let Value::Object(obj) = &*proxy.target {
        let target_result = if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
            && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
        {
            proxy_is_extensible(mc, inner_proxy)?
        } else {
            obj.borrow().is_extensible()
        };
        if trap_result != target_result {
            return Err(raise_type_error!("'isExtensible' on proxy: trap result does not reflect extensibility of proxy target").into());
        }
    }

    Ok(trap_result)
}

/// Proxy [[PreventExtensions]] internal method (spec §10.5.4)
pub(crate) fn proxy_prevent_extensions<'gc>(mc: &MutationContext<'gc>, proxy: &Gc<'gc, JSProxy<'gc>>) -> Result<bool, EvalError<'gc>> {
    let result = apply_proxy_trap(mc, proxy, "preventExtensions", vec![(*proxy.target).clone()], || {
        match &*proxy.target {
            Value::Object(obj) => {
                // If target is itself a proxy, recurse
                if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                    && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                {
                    return Ok(Value::Boolean(proxy_prevent_extensions(mc, inner_proxy)?));
                }
                obj.borrow_mut(mc).prevent_extensions();
                Ok(Value::Boolean(true))
            }
            _ => Ok(Value::Boolean(false)),
        }
    })?;

    let trap_result = result.to_truthy();

    // Invariant check: if trap returns true, target must not be extensible
    if trap_result && let Value::Object(obj) = &*proxy.target {
        let is_ext = if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
            && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
        {
            proxy_is_extensible(mc, inner_proxy)?
        } else {
            obj.borrow().is_extensible()
        };
        if is_ext {
            return Err(raise_type_error!("'preventExtensions' on proxy: trap returned truthy but the proxy target is extensible").into());
        }
    }

    Ok(trap_result)
}

/// Proxy [[GetPrototypeOf]] internal method (spec §10.5.1)
pub(crate) fn proxy_get_prototype_of<'gc>(mc: &MutationContext<'gc>, proxy: &Gc<'gc, JSProxy<'gc>>) -> Result<Value<'gc>, EvalError<'gc>> {
    let result = apply_proxy_trap(mc, proxy, "getPrototypeOf", vec![(*proxy.target).clone()], || {
        match &*proxy.target {
            Value::Object(obj) => {
                // If target is itself a proxy, recurse
                if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                    && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                {
                    return proxy_get_prototype_of(mc, inner_proxy);
                }
                if let Some(proto) = obj.borrow().prototype {
                    Ok(Value::Object(proto))
                } else {
                    Ok(Value::Null)
                }
            }
            _ => Ok(Value::Null),
        }
    })?;

    // Spec 10.5.1 step 7: If Type(handlerProto) is not Object and handlerProto is not null, throw TypeError
    if !matches!(&result, Value::Object(_) | Value::Null) {
        return Err(raise_type_error!("'getPrototypeOf' on proxy: trap returned neither object nor null").into());
    }

    // Spec 10.5.1 step 8: If target is not extensible, trap result must match target prototype
    if let Value::Object(target_obj) = &*proxy.target
        && !target_obj.borrow().is_extensible()
    {
        let target_proto = if let Some(proto) = target_obj.borrow().prototype {
            Value::Object(proto)
        } else {
            Value::Null
        };
        let same = match (&result, &target_proto) {
            (Value::Object(o1), Value::Object(o2)) => Gc::ptr_eq(*o1, *o2),
            (Value::Null, Value::Null) => true,
            _ => false,
        };
        if !same {
            return Err(raise_type_error!(
                "'getPrototypeOf' on proxy: proxy target is non-extensible but the trap did not return its actual prototype"
            )
            .into());
        }
    }

    Ok(result)
}

/// Proxy [[SetPrototypeOf]] internal method (spec §10.5.2)
pub(crate) fn proxy_set_prototype_of<'gc>(
    mc: &MutationContext<'gc>,
    proxy: &Gc<'gc, JSProxy<'gc>>,
    proto: &Value<'gc>,
) -> Result<bool, EvalError<'gc>> {
    let proxy_gc = *proxy;
    let proto_clone = proto.clone();
    let result = apply_proxy_trap(
        mc,
        proxy,
        "setPrototypeOf",
        vec![(*proxy.target).clone(), proto.clone()],
        || match &*proxy_gc.target {
            Value::Object(obj) => {
                // If target is itself a proxy, recurse
                if let Some(inner_proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                    && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                {
                    return Ok(Value::Boolean(proxy_set_prototype_of(mc, inner_proxy, &proto_clone)?));
                }
                // OrdinarySetPrototypeOf(O, V) — spec 10.1.2
                let current_proto = obj.borrow().prototype;
                let proto_obj = match &proto_clone {
                    Value::Object(p) => Some(*p),
                    Value::Null => None,
                    _ => return Ok(Value::Boolean(true)),
                };
                // Step 4: If SameValue(V, current) is true, return true
                let same = match (current_proto, proto_obj) {
                    (Some(c), Some(p)) => Gc::ptr_eq(c, p),
                    (None, None) => true,
                    _ => false,
                };
                if same {
                    return Ok(Value::Boolean(true));
                }
                // Step 5: If O.[[Extensible]] is false, return false
                if !obj.borrow().is_extensible() {
                    return Ok(Value::Boolean(false));
                }
                // Step 8: Cycle check
                if let Some(new_proto) = proto_obj {
                    let mut p = new_proto;
                    loop {
                        if Gc::ptr_eq(p, *obj) {
                            return Ok(Value::Boolean(false));
                        }
                        if let Some(next) = p.borrow().prototype {
                            p = next;
                        } else {
                            break;
                        }
                    }
                }
                obj.borrow_mut(mc).prototype = proto_obj;
                Ok(Value::Boolean(true))
            }
            _ => Ok(Value::Boolean(false)),
        },
    )?;

    let trap_result = result.to_truthy();

    if trap_result {
        // Spec 10.5.2 steps 12-15: invariant checks
        // Step 12: Let extensibleTarget be ? IsExtensible(target).
        if let Value::Object(target_obj) = &*proxy_gc.target {
            let extensible_target = if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj, &InternalSlot::Proxy)
                && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
            {
                proxy_is_extensible(mc, inner_proxy)?
            } else {
                target_obj.borrow().is_extensible()
            };

            // Step 14: If extensibleTarget is false
            if !extensible_target {
                // Step 14a: Let targetProto be ? target.[[GetPrototypeOf]]().
                let target_proto = if let Some(inner_proxy_cell) = crate::core::slot_get(target_obj, &InternalSlot::Proxy)
                    && let Value::Proxy(inner_proxy) = &*inner_proxy_cell.borrow()
                {
                    proxy_get_prototype_of(mc, inner_proxy)?
                } else if let Some(p) = target_obj.borrow().prototype {
                    Value::Object(p)
                } else {
                    Value::Null
                };
                // Step 15: If SameValue(V, targetProto) is false, throw TypeError.
                let same = match (proto, &target_proto) {
                    (Value::Object(o1), Value::Object(o2)) => Gc::ptr_eq(*o1, *o2),
                    (Value::Null, Value::Null) => true,
                    _ => false,
                };
                if !same {
                    return Err(raise_type_error!(
                        "'setPrototypeOf' on proxy: trap returned truish for setting a new prototype on a non-extensible target"
                    )
                    .into());
                }
            }
        }
    }

    Ok(trap_result)
}

/// Initialize Proxy constructor and prototype
pub fn initialize_proxy<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let proxy_ctor = new_js_object_data(mc);
    slot_set(mc, &proxy_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &proxy_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Proxy")));

    // Per spec: Proxy.length = 2 (non-writable, non-enumerable, configurable)
    object_set_key_value(mc, &proxy_ctor, "length", &Value::Number(2.0))?;
    proxy_ctor.borrow_mut(mc).set_non_writable("length");
    proxy_ctor.borrow_mut(mc).set_non_enumerable("length");

    // Per spec: Proxy.name = "Proxy" (non-writable, non-enumerable, configurable)
    object_set_key_value(mc, &proxy_ctor, "name", &Value::String(utf8_to_utf16("Proxy")))?;
    proxy_ctor.borrow_mut(mc).set_non_writable("name");
    proxy_ctor.borrow_mut(mc).set_non_enumerable("name");

    // Per spec: Proxy.__proto__ = Function.prototype
    let func_proto_opt = if let Some(func_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_val.borrow()
        && let Some(proto_val) = crate::core::object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        proxy_ctor.borrow_mut(mc).prototype = Some(*func_proto);
        Some(*func_proto)
    } else {
        None
    };

    // Register revocable static method as proper function object
    let revocable_obj = new_js_object_data(mc);
    revocable_obj.borrow_mut(mc).set_closure(Some(crate::core::new_gc_cell_ptr(
        mc,
        Value::Function("Proxy.revocable".to_string()),
    )));
    // length = 2 (non-writable, non-enumerable, configurable)
    object_set_key_value(mc, &revocable_obj, "length", &Value::Number(2.0))?;
    revocable_obj.borrow_mut(mc).set_non_writable("length");
    revocable_obj.borrow_mut(mc).set_non_enumerable("length");
    // name = "revocable" (non-writable, non-enumerable, configurable)
    object_set_key_value(mc, &revocable_obj, "name", &Value::String(utf8_to_utf16("revocable")))?;
    revocable_obj.borrow_mut(mc).set_non_writable("name");
    revocable_obj.borrow_mut(mc).set_non_enumerable("name");
    // __proto__ = Function.prototype
    if let Some(fp) = func_proto_opt {
        revocable_obj.borrow_mut(mc).prototype = Some(fp);
    }
    object_set_key_value(mc, &proxy_ctor, "revocable", &Value::Object(revocable_obj))?;
    proxy_ctor.borrow_mut(mc).set_non_enumerable("revocable");

    env_set(mc, env, "Proxy", &Value::Object(proxy_ctor))?;
    Ok(())
}
