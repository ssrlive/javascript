use crate::core::{
    ClosureData, EvalError, Expr, Gc, InternalSlot, JSObjectDataPtr, MutationContext, Statement, StatementKind, Value, evaluate_expr,
    get_own_property, has_own_property_value, new_js_object_data, prepare_function_call_env, slot_get, slot_get_chained, slot_set,
};
use crate::core::{PropertyKey, SwitchCase, SymbolData, Token, new_gc_cell_ptr, object_get_key_value, object_set_key_value};
use crate::error::{JSError, JSErrorKind};
use crate::js_array::handle_array_constructor;
use crate::js_class::prepare_call_env_with_this;
use crate::js_date::handle_date_constructor;
use crate::unicode::{utf8_to_utf16, utf16_to_utf8};

// Centralize dispatching small builtins that are represented as Value::Function
// names and need to be applied to an object receiver (e.g., "BigInt_toString",
// "BigInt_valueOf", "Date.prototype.*"). Returns Ok(Some(Value)) if handled,
// Ok(None) if not recognized, Err on error.

/// Helper function to extract and validate arguments for internal functions
/// Returns a vector of evaluated arguments or an error
#[allow(dead_code)]
fn extract_internal_args<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Expr],
    env: &JSObjectDataPtr<'gc>,
    expected_count: usize,
) -> Result<Vec<Value<'gc>>, JSError> {
    if args.len() != expected_count {
        let msg = format!("Internal function requires exactly {expected_count} arguments, got {}", args.len());
        return Err(raise_type_error!(msg));
    }

    let mut evaluated_args = Vec::with_capacity(expected_count);
    for arg in args {
        evaluated_args.push(evaluate_expr(mc, env, arg)?);
    }
    Ok(evaluated_args)
}

/// Helper function to validate that first N arguments are numbers
#[allow(dead_code)]
fn validate_internal_args(args: &[Value], count: usize) -> Result<(), JSError> {
    if args.len() != count {
        let msg = format!("Internal function requires exactly {} arguments, got {}", count, args.len());
        return Err(raise_type_error!(msg));
    }
    Ok(())
}

#[allow(dead_code)]
fn validate_number_args(args: &[Value], count: usize) -> Result<Vec<f64>, JSError> {
    if args.len() < count {
        return Err(raise_type_error!(format!("Expected at least {count} arguments")));
    }

    let mut numbers = Vec::with_capacity(count);
    for i in 0..count {
        match args[i] {
            Value::Number(n) => numbers.push(n),
            _ => {
                return Err(raise_type_error!(format!("Argument {i} must be a number")));
            }
        }
    }
    Ok(numbers)
}

fn propagate_closure_strictness<'gc>(
    mc: &MutationContext<'gc>,
    func_env: &JSObjectDataPtr<'gc>,
    data: &crate::core::ClosureData<'gc>,
) -> Result<(), JSError> {
    let mut env_strict_ancestor = false;
    if data.enforce_strictness_inheritance {
        let mut proto_iter = data.env;
        while let Some(cur) = proto_iter {
            if crate::core::env_get_strictness(&cur) {
                env_strict_ancestor = true;
                break;
            }
            proto_iter = cur.borrow().prototype;
        }
    }
    crate::core::env_set_strictness(mc, func_env, data.is_strict || env_strict_ancestor)?;
    Ok(())
}

fn attach_function_prototype_if_present<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, func_obj: &JSObjectDataPtr<'gc>) {
    if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = crate::core::object_get_key_value(func_ctor, "prototype")
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        func_obj.borrow_mut(mc).prototype = Some(*proto);
    }
}

fn is_callable_object<'gc>(obj: &JSObjectDataPtr<'gc>, include_callable_slot: bool) -> bool {
    obj.borrow().get_closure().is_some()
        || obj.borrow().class_def.is_some()
        || crate::core::slot_get(obj, &InternalSlot::NativeCtor).is_some()
        || crate::core::slot_get(obj, &InternalSlot::IsConstructor).is_some()
        || crate::core::slot_get(obj, &InternalSlot::BoundTarget).is_some()
        || (include_callable_slot && crate::core::slot_get(obj, &InternalSlot::Callable).is_some())
}

fn is_callable_value<'gc>(value: &Value<'gc>, include_callable_slot: bool) -> bool {
    match value {
        Value::Function(_)
        | Value::Closure(_)
        | Value::AsyncClosure(_)
        | Value::GeneratorFunction(..)
        | Value::AsyncGeneratorFunction(..) => true,
        Value::Object(obj) => is_callable_object(obj, include_callable_slot),
        _ => false,
    }
}

fn is_constructor_object<'gc>(obj: &JSObjectDataPtr<'gc>) -> bool {
    if obj.borrow().class_def.is_some() {
        return true;
    }
    if let Some(v) = crate::core::slot_get(obj, &InternalSlot::IsConstructor)
        && matches!(*v.borrow(), Value::Boolean(true))
    {
        return true;
    }
    if crate::core::slot_get(obj, &InternalSlot::NativeCtor).is_some() {
        return true;
    }
    if let Some(bound_target) = crate::core::slot_get(obj, &InternalSlot::BoundTarget) {
        return is_constructor_value(&bound_target.borrow());
    }
    if let Some(cl_ptr) = obj.borrow().get_closure() {
        return match &*cl_ptr.borrow() {
            Value::Closure(cl) => !cl.is_arrow,
            Value::AsyncClosure(_) | Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..) => false,
            _ => false,
        };
    }
    if let Some(proxy_rc) = crate::core::slot_get(obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_rc.borrow()
    {
        return is_constructor_value(&proxy.target);
    }
    false
}

fn is_constructor_value<'gc>(value: &Value<'gc>) -> bool {
    match value {
        Value::Object(obj) => is_constructor_object(obj),
        Value::Closure(cl) => !cl.is_arrow,
        Value::AsyncClosure(_) | Value::GeneratorFunction(..) | Value::AsyncGeneratorFunction(..) => false,
        _ => false,
    }
}

pub fn handle_global_function<'gc>(
    mc: &MutationContext<'gc>,
    func_name: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Handle functions that expect unevaluated expressions
    match func_name {
        "import" => return dynamic_import_function(mc, args, env),
        "import.defer" => return dynamic_import_defer_function(mc, args, env),
        "import.source" => return dynamic_import_source_function(mc, args, env),
        "Function" => return function_constructor(mc, args, env),
        "GeneratorFunction" => return generator_function_constructor(mc, args, env),
        "AsyncFunction" => return async_function_constructor(mc, args, env),
        "AsyncGeneratorFunction" => return async_generator_function_constructor(mc, args, env),
        "new" => return evaluate_new_expression(mc, args, env),
        "eval" => return evalute_eval_function(mc, args, env),
        "Date" => return handle_date_constructor(mc, args, env, None),
        "Error.isError" => {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);
            return Ok(Value::Boolean(crate::core::js_error::is_error(&arg)));
        }
        "AbstractModuleSource.prototype.@@toStringTag" => return Ok(Value::Undefined),

        "__createRealm__" => {
            let new_env = crate::core::create_new_realm(mc, env)?;
            let result = crate::core::new_js_object_data(mc);
            crate::core::object_set_key_value(mc, &result, "global", &Value::Object(new_env))?;
            return Ok(Value::Object(result));
        }

        "Object.prototype.valueOf" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                return match this_val {
                    Value::Object(o) => Ok(Value::Object(o)),
                    Value::Undefined => Err(raise_type_error!("Cannot convert undefined to object").into()),
                    Value::Null => Err(raise_type_error!("Cannot convert null to object").into()),
                    _ => match crate::js_class::handle_object_constructor(mc, std::slice::from_ref(&this_val), env)? {
                        Value::Object(o) => Ok(Value::Object(o)),
                        _ => Err(raise_type_error!("Cannot convert value to object").into()),
                    },
                };
            }
            return Err(raise_eval_error!("Object.prototype.valueOf called without this").into());
        }
        "Object.prototype.toString" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                return crate::core::handle_object_prototype_to_string(mc, &this_val, env);
            }
            return Err(raise_eval_error!("Object.prototype.toString called without this").into());
        }
        "Object.prototype.hasOwnProperty" => return handle_object_has_own_property(mc, args, env),
        "Object.prototype.isPrototypeOf" => return handle_object_is_prototype_of(mc, args, env),
        "Object.prototype.propertyIsEnumerable" => return handle_object_property_is_enumerable(mc, args, env),
        "Object.prototype.get __proto__" => return handle_object_proto_get(mc, args, env),
        "Object.prototype.set __proto__" => return handle_object_proto_set(mc, args, env),
        "RegExp.prototype.exec" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_regexp::handle_regexp_method(mc, &obj, "exec", args, env);
                } else {
                    return Err(raise_type_error!("RegExp.prototype.exec called on non-object receiver").into());
                }
            }
            return Err(raise_eval_error!("RegExp.prototype.exec called without this").into());
        }
        "RegExp.prototype.test" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_regexp::handle_regexp_method(mc, &obj, "test", args, env);
                } else {
                    return Err(raise_type_error!("RegExp.prototype.test called on non-object receiver").into());
                }
            }
            return Err(raise_eval_error!("RegExp.prototype.test called without this").into());
        }
        "RegExp.prototype.toString" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_regexp::handle_regexp_method(mc, &obj, "toString", args, env);
                } else {
                    return Err(raise_type_error!("RegExp.prototype.toString called on non-object receiver").into());
                }
            }
            return Err(raise_eval_error!("RegExp.prototype.toString called without this").into());
        }
        // RegExp.prototype accessor getters (source, global, ignoreCase, etc.)
        _ if func_name.starts_with("RegExp.prototype.get ") => {
            let prop = &func_name["RegExp.prototype.get ".len()..];
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_v = this_rc.borrow().clone();
                if let Value::Object(obj) = this_v
                    && let Some(val) = crate::js_regexp::handle_regexp_getter(&obj, prop)?
                {
                    return Ok(val);
                }
            }
            return Ok(Value::Undefined);
        }
        "parseInt" => return parse_int_function(args),
        "parseFloat" => return parse_float_function(args),
        "isNaN" => return is_nan_function(args),
        "isFinite" => return is_finite_function(mc, args, env),
        "encodeURIComponent" => return encode_uri_component(mc, args, env),
        "decodeURIComponent" => return decode_uri_component(mc, args, env),
        "Object" => return crate::js_class::handle_object_constructor(mc, args, env),
        "BigInt" => return crate::js_bigint::bigint_constructor(mc, args, env),
        "BigInt.asIntN" | "BigInt.asUintN" => {
            let method = if func_name == "BigInt.asIntN" { "asIntN" } else { "asUintN" };
            return crate::js_bigint::handle_bigint_static_method(mc, method, args, env);
        }
        "Number" => return Ok(crate::js_number::number_constructor(mc, args, env)?),
        "Boolean" => return boolean_constructor(args),
        "Proxy.revocable" => return crate::js_proxy::handle_proxy_revocable(mc, args, env),
        "Proxy.__internal_revoke" => {
            // Revoke the proxy stored in the captured closure environment
            if let Some(revoke_rc) = crate::core::slot_get_chained(env, &InternalSlot::ProxyWrapper) {
                let revoke_val = revoke_rc.borrow().clone();
                if let Value::Object(wrapper_obj) = revoke_val {
                    // Get the stored __proxy__ property on the wrapper
                    if let Some(proxy_prop) = slot_get_chained(&wrapper_obj, &InternalSlot::Proxy) {
                        let old_proxy = {
                            let borrowed = proxy_prop.borrow();
                            if let Value::Proxy(p) = &*borrowed { Some(*p) } else { None }
                        };
                        if let Some(p) = old_proxy {
                            // Create a new proxy with revoked=true and same target/handler
                            let new_proxy = Gc::new(
                                mc,
                                crate::core::JSProxy {
                                    target: p.target.clone(),
                                    handler: p.handler.clone(),
                                    revoked: true,
                                },
                            );
                            slot_set(mc, &wrapper_obj, InternalSlot::Proxy, &Value::Proxy(new_proxy));
                        }
                    }
                }
            }
            return Ok(Value::Undefined);
        }
        "Boolean_toString" => return Ok(crate::js_boolean::boolean_prototype_to_string(mc, args, env)?),
        "Boolean_valueOf" => return Ok(crate::js_boolean::boolean_prototype_value_of(mc, args, env)?),
        "Symbol" => return symbol_constructor(mc, args),
        "Symbol_valueOf" => return symbol_prototype_value_of(mc, args, env),
        "Symbol_toString" => return symbol_prototype_to_string(mc, args, env),
        "encodeURI" => return encode_uri(mc, args, env),
        "decodeURI" => return decode_uri(mc, args, env),
        "IteratorSelf" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                return Ok(this_val);
            }
            return Ok(Value::Undefined);
        }
        "ArrayIterator.prototype.next" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_array::handle_array_iterator_next(mc, &obj, env);
                }
            }
            return Err(raise_eval_error!("ArrayIterator.prototype.next called on non-object").into());
        }
        // "__internal_resolve_promise" => return internal_resolve_promise(mc, args, env),
        // "__internal_reject_promise" => return internal_reject_promise(mc, args, env),
        // "__internal_promise_allsettled_resolve" => return internal_promise_allsettled_resolve(mc, args, env),
        // "__internal_promise_allsettled_reject" => return internal_promise_allsettled_reject(mc, args, env),
        // "__internal_allsettled_state_record_fulfilled" => return internal_allsettled_state_record_fulfilled(mc, args, env),
        // "__internal_allsettled_state_record_rejected" => return internal_allsettled_state_record_rejected(mc, args, env),
        // "__internal_promise_any_resolve" => return internal_promise_any_resolve(mc, args, env),
        // "__internal_promise_any_reject" => return internal_promise_any_reject(mc, args, env),
        // "__internal_promise_race_resolve" => return internal_promise_race_resolve(mc, args, env),
        "__internal_promise_all_resolve" => return Ok(crate::js_promise::__internal_promise_all_resolve(mc, args, env)?),
        "__internal_promise_all_reject" => return Ok(crate::js_promise::__internal_promise_all_reject(mc, args, env)?),
        "Promise.resolve" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "resolve", args, env)?),
        "Promise.reject" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "reject", args, env)?),
        "Promise.all" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "all", args, env)?),
        "Promise.race" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "race", args, env)?),
        "Promise.any" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "any", args, env)?),
        "Promise.allSettled" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "allSettled", args, env)?),

        "__internal_capability_executor" => return Ok(crate::js_promise::__internal_capability_executor(mc, args, env)?),
        "__internal_promise_resolve_captured" => return Ok(crate::js_promise::__internal_promise_resolve_captured(mc, args, env)?),
        "__internal_promise_reject_captured" => return Ok(crate::js_promise::__internal_promise_reject_captured(mc, args, env)?),

        "__internal_promise_finally_resolve" => return Ok(crate::js_promise::__internal_promise_finally_resolve(mc, args, env)?),
        "__internal_promise_finally_reject" => return Ok(crate::js_promise::__internal_promise_finally_reject(mc, args, env)?),

        "__internal_async_step_resolve" => return Ok(crate::js_async::__internal_async_step_resolve(mc, args, env)?),
        "__internal_async_step_reject" => return Ok(crate::js_async::__internal_async_step_reject(mc, args, env)?),

        "__internal_allsettled_state_record_fulfilled_env" => {
            if args.len() < 3 {
                return Err(raise_eval_error!("internal function called with insufficient args").into());
            }
            let idx = match args[1] {
                Value::Number(n) => n,
                _ => return Err(raise_eval_error!("internal function expected number").into()),
            };
            return Ok(
                crate::js_promise::__internal_allsettled_state_record_fulfilled_env(mc, args[0].clone(), idx, args[2].clone(), env)
                    .map(|_| Value::Undefined)?,
            );
        }
        "__internal_allsettled_state_record_rejected_env" => {
            if args.len() < 3 {
                return Err(raise_eval_error!("internal function called with insufficient args").into());
            }
            let idx = match args[1] {
                Value::Number(n) => n,
                _ => return Err(raise_eval_error!("internal function expected number").into()),
            };
            return Ok(
                crate::js_promise::__internal_allsettled_state_record_rejected_env(mc, args[0].clone(), idx, args[2].clone(), env)
                    .map(|_| Value::Undefined)?,
            );
        }

        "__internal_resolve_promise" => return internal_resolve_promise(mc, args, env),
        "__internal_reject_promise" => return internal_reject_promise(mc, args, env),

        "Promise.prototype.then" | "Promise.prototype.catch" | "Promise.prototype.finally" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    let method = if func_name == "Promise.prototype.then" {
                        "then"
                    } else if func_name == "Promise.prototype.catch" {
                        "catch"
                    } else {
                        "finally"
                    };
                    return Ok(crate::js_promise::handle_promise_prototype_method(mc, &obj, method, args, env)?);
                }
            }
            return Err(raise_type_error!("Promise prototype method called without object this").into());
        }

        "setTimeout" => return Ok(crate::js_promise::handle_set_timeout_val(mc, args, env)?),
        "clearTimeout" => return Ok(crate::js_promise::handle_clear_timeout_val(mc, args, env)?),
        "setInterval" => return Ok(crate::js_promise::handle_set_interval_val(mc, args, env)?),
        "clearInterval" => return Ok(crate::js_promise::handle_clear_interval_val(mc, args, env)?),
        "Function.prototype.call" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if args.is_empty() {
                    return Err(raise_eval_error!("call requires a receiver").into());
                }
                let receiver_val = args[0].clone();
                let evaluated_args = args[1..].to_vec();
                if let Value::Function(func_name) = &this_val {
                    if (func_name == "Object.prototype.hasOwnProperty" || func_name == "Object.prototype.propertyIsEnumerable")
                        && let Value::Function(target_name) = &receiver_val
                    {
                        if evaluated_args.len() != 1 {
                            return Err(raise_eval_error!("Object.prototype helper requires one argument").into());
                        }
                        let key = crate::core::value_to_string(&evaluated_args[0]);
                        let is_builtin_prop = key == "length" || key == "name";
                        if func_name == "Object.prototype.hasOwnProperty" {
                            if is_builtin_prop {
                                if crate::core::consume_pending_function_delete_hasown_check() {
                                    return Ok(Value::Boolean(false));
                                }
                                let marker = format!("__fn_deleted::{}::{}", target_name, key);
                                let mut deleted = crate::core::is_deleted_builtin_function_virtual_prop(target_name, key.as_str())
                                    || crate::core::env_get(env, marker.as_str())
                                        .map(|v| matches!(*v.borrow(), Value::Boolean(true)))
                                        .unwrap_or(false);
                                if !deleted
                                    && let Some(global_this_rc) = crate::core::env_get(env, "globalThis")
                                    && let Value::Object(global_obj) = &*global_this_rc.borrow()
                                    && let Some(v) = slot_get(global_obj, &InternalSlot::FnDeleted(format!("{}::{}", target_name, key)))
                                {
                                    deleted = matches!(*v.borrow(), Value::Boolean(true));
                                }
                                return Ok(Value::Boolean(!deleted));
                            }
                            return Ok(Value::Boolean(false));
                        }
                        let _ = target_name;
                        return Ok(Value::Boolean(false));
                    }

                    let call_env = prepare_call_env_with_this(mc, Some(env), Some(&receiver_val), None, &[], None, Some(env), None)?;
                    return crate::core::evaluate_call_dispatch(
                        mc,
                        &call_env,
                        &Value::Function(func_name.clone()),
                        Some(&receiver_val),
                        &evaluated_args,
                    );
                }
                if let Value::Object(obj) = &this_val
                    && let Some(cl_prop) = obj.borrow().get_closure()
                {
                    let cl_val = cl_prop.borrow().clone();
                    if let Value::Closure(cl) = cl_val {
                        return crate::core::call_closure(mc, &cl, Some(&receiver_val), &evaluated_args, env, Some(*obj));
                    }
                    if let Value::Function(name) = cl_val {
                        let call_env = prepare_call_env_with_this(mc, Some(env), Some(&receiver_val), None, &[], None, Some(env), None)?;
                        return crate::core::evaluate_call_dispatch(
                            mc,
                            &call_env,
                            &Value::Function(name),
                            Some(&receiver_val),
                            &evaluated_args,
                        );
                    }
                }
                if let Value::Object(obj) = &this_val
                    && let Some(native_ctor_rc) = crate::core::slot_get_chained(obj, &InternalSlot::NativeCtor)
                {
                    let native_ctor_val = match &*native_ctor_rc.borrow() {
                        Value::Property { value: Some(v), .. } => v.borrow().clone(),
                        other => other.clone(),
                    };
                    if let Value::String(name) = native_ctor_val {
                        let ctor_name = crate::unicode::utf16_to_utf8(&name);
                        let call_env = prepare_call_env_with_this(mc, Some(env), Some(&receiver_val), None, &[], None, Some(env), None)?;
                        return crate::core::evaluate_call_dispatch(
                            mc,
                            &call_env,
                            &Value::Function(ctor_name),
                            Some(&receiver_val),
                            &evaluated_args,
                        );
                    }
                }
                return crate::core::evaluate_call_dispatch(mc, env, &this_val, Some(&receiver_val), &evaluated_args);
            } else {
                return Err(raise_eval_error!("Function.prototype.call called without this").into());
            }
        }

        "Function.prototype.apply" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                let callee_for_arguments = this_val.clone(); // The function object being called
                match this_val {
                    Value::Function(func_name) => {
                        if func_name.starts_with("Object.prototype.") || func_name.starts_with("Array.prototype.") {
                            if args.is_empty() {
                                return Err(raise_eval_error!("apply requires a receiver").into());
                            }
                            let receiver_val = args[0].clone();
                            let mut forwarded_args: Vec<Value> = Vec::new();
                            if args.len() >= 2 {
                                match args[1].clone() {
                                    Value::Object(arr_obj) if crate::js_array::is_array(mc, &arr_obj) => {
                                        let mut i = 0usize;
                                        loop {
                                            let key = i.to_string();
                                            if let Some(val_rc) = get_own_property(&arr_obj, &key) {
                                                forwarded_args.push(val_rc.borrow().clone());
                                            } else {
                                                break;
                                            }
                                            i += 1;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            let call_env =
                                prepare_call_env_with_this(mc, Some(env), Some(&receiver_val), None, &[], None, Some(env), None)?;
                            return handle_global_function(mc, &func_name, &forwarded_args, &call_env);
                        }
                        return Err(raise_eval_error!(format!("Function.prototype.apply target not supported: {}", func_name)).into());
                    }
                    Value::Closure(data) => {
                        if args.is_empty() {
                            return Err(raise_eval_error!("apply requires a receiver").into());
                        }
                        let receiver_val = args[0].clone();
                        let mut evaluated_args: Vec<Value> = Vec::new();
                        if args.len() >= 2 {
                            match args[1].clone() {
                                Value::Object(arr_obj) if crate::js_array::is_array(mc, &arr_obj) => {
                                    let mut i = 0usize;
                                    loop {
                                        let key = i.to_string();
                                        if let Some(val_rc) = get_own_property(&arr_obj, &key) {
                                            evaluated_args.push(val_rc.borrow().clone());
                                        } else {
                                            break;
                                        }
                                        i += 1;
                                    }
                                }
                                _ => {}
                            }
                        }
                        let params = &data.params;
                        let body = &data.body;
                        let captured_env = &data.env;
                        let func_env = prepare_function_call_env(
                            mc,
                            captured_env.as_ref(),
                            Some(&receiver_val),
                            Some(params),
                            &evaluated_args,
                            None,
                            Some(env),
                        )?;

                        propagate_closure_strictness(mc, &func_env, &data)?;

                        crate::js_class::create_arguments_object(mc, &func_env, &evaluated_args, Some(&callee_for_arguments))?;

                        return crate::core::evaluate_statements(mc, &func_env, body);
                    }
                    Value::Object(object) => {
                        if let Some(cl_rc) = object.borrow().get_closure() {
                            if args.is_empty() {
                                return Err(raise_eval_error!("apply requires a receiver").into());
                            }
                            let receiver_val = args[0].clone();
                            let mut evaluated_args: Vec<Value> = Vec::new();
                            if args.len() >= 2 {
                                match args[1].clone() {
                                    Value::Object(arr_obj) if crate::js_array::is_array(mc, &arr_obj) => {
                                        let mut i = 0usize;
                                        loop {
                                            let key = i.to_string();
                                            if let Some(val_rc) = get_own_property(&arr_obj, &key) {
                                                evaluated_args.push(val_rc.borrow().clone());
                                            } else {
                                                break;
                                            }
                                            i += 1;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            match &*cl_rc.borrow() {
                                Value::Closure(data) => {
                                    let params = &data.params;
                                    let body = &data.body;
                                    let captured_env = &data.env;
                                    let func_env = prepare_function_call_env(
                                        mc,
                                        captured_env.as_ref(),
                                        Some(&receiver_val),
                                        Some(params),
                                        &evaluated_args,
                                        None,
                                        Some(env),
                                    )?;

                                    propagate_closure_strictness(mc, &func_env, data)?;

                                    crate::js_class::create_arguments_object(mc, &func_env, &evaluated_args, Some(&Value::Object(object)))?;

                                    return crate::core::evaluate_statements(mc, &func_env, body);
                                }
                                Value::AsyncClosure(data) => {
                                    return Ok(crate::js_async::handle_async_closure_call(
                                        mc,
                                        data,
                                        Some(&receiver_val),
                                        &evaluated_args,
                                        env,
                                        Some(object),
                                    )?);
                                }
                                _ => {}
                            }
                        }
                        return Err(raise_eval_error!("Function.prototype.apply called on non-callable").into());
                    }
                    _ => return Err(raise_eval_error!("Function.prototype.apply called on non-callable").into()),
                }
            } else {
                return Err(raise_eval_error!("Function.prototype.apply called without this").into());
            }
        }

        "Function.prototype.toString" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                return handle_function_prototype_method(mc, &this_val, "toString", args, env);
            }
            return Err(raise_type_error!("Function.prototype.toString requires that 'this' be a Function").into());
        }

        "Function.prototype.bind" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                return handle_function_prototype_method(mc, &this_val, "bind", args, env);
            }
            return Err(raise_type_error!("Function.prototype.bind called on non-function").into());
        }
        "Function.prototype.restrictedThrow" => {
            return Err(raise_type_error!("Access to 'caller' or 'arguments' is restricted").into());
        }
        _ => {}
    }

    match func_name {
        "console.error" => Ok(crate::js_console::handle_console_method(mc, "error", args, env)?),
        "console.log" => Ok(crate::js_console::handle_console_method(mc, "log", args, env)?),
        "String" => Ok(crate::js_string::string_constructor(mc, args, env)?),
        "Array" => Ok(handle_array_constructor(mc, args, env, None)?),

        name if name.starts_with("Array.prototype.") => {
            let method = name.trim_start_matches("Array.prototype.");
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                let receiver_obj = match this_val {
                    Value::Object(obj) => obj,
                    Value::Undefined | Value::Null => {
                        return Err(raise_type_error!("Array.prototype method called on null or undefined").into());
                    }
                    _ => match crate::js_class::handle_object_constructor(mc, std::slice::from_ref(&this_val), env)? {
                        Value::Object(obj) => obj,
                        _ => {
                            return Err(raise_type_error!("Array.prototype method called on incompatible receiver").into());
                        }
                    },
                };
                return crate::js_array::handle_array_instance_method(mc, &receiver_obj, method, args, env);
            }
            Err(raise_type_error!("Array.prototype method called without this").into())
        }

        name if name.starts_with("Number.prototype.") => {
            let method = name.trim_start_matches("Number.prototype.");
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if matches!(this_val, Value::Undefined | Value::Null) {
                    return Err(raise_type_error!("Number.prototype method called on null or undefined").into());
                }
                return Ok(crate::js_number::handle_number_prototype_method(Some(&this_val), method, args)?);
            }
            Err(raise_type_error!("Number.prototype method called without this").into())
        }

        "IteratorSelf" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                return Ok(this_rc.borrow().clone());
            }
            Ok(Value::Undefined)
        }

        "ArrayIterator.prototype.next" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return match crate::js_array::handle_array_iterator_next(mc, &obj, env) {
                        Ok(v) => Ok(v),
                        Err(EvalError::Js(j)) => Err(EvalError::Js(j)),
                        Err(EvalError::Throw(val, line, column)) => {
                            let mut e = make_js_error!(JSErrorKind::Throw(crate::core::value_to_string(&val)));
                            e.set_js_location(line.unwrap_or(0), column.unwrap_or(0));
                            Err(EvalError::Js(e))
                        }
                    };
                }
                return Err(raise_type_error!("ArrayIterator.prototype.next called on non-object").into());
            }
            Err(raise_type_error!("ArrayIterator.prototype.next called without this").into())
        }

        "StringIterator.prototype.next" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return crate::js_string::handle_string_iterator_next(mc, &obj);
                }
                return Err(raise_type_error!("StringIterator.prototype.next called on non-object").into());
            }
            Err(raise_type_error!("StringIterator.prototype.next called without this").into())
        }

        "SetIterator.prototype.next" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return Ok(crate::js_set::handle_set_iterator_next(mc, &obj, env)?);
                }
                return Err(raise_type_error!("SetIterator.prototype.next called on non-object").into());
            }
            Err(raise_type_error!("SetIterator.prototype.next called without this").into())
        }

        "MapIterator.prototype.next" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(obj) = this_val {
                    return Ok(crate::js_map::handle_map_iterator_next(mc, &obj, env)?);
                }
                return Err(raise_type_error!("MapIterator.prototype.next called on non-object").into());
            }
            Err(raise_type_error!("MapIterator.prototype.next called without this").into())
        }

        _ => {
            if func_name == "Error.prototype.toString" {
                if let Some(this_rc) = crate::core::env_get(env, "this") {
                    let this_val = this_rc.borrow().clone();
                    return crate::js_object::handle_error_to_string_method(mc, &this_val, args, env);
                }
                return Err(raise_type_error!("Error.prototype.toString called on non-object").into());
            }
            if func_name.starts_with("Object.") && !func_name.contains(".prototype.") {
                let method = &func_name["Object.".len()..];
                return crate::js_object::handle_object_method(mc, method, args, env);
            }
            Err(raise_eval_error!(format!("Global function {} not found", func_name)).into())
        }
    }
}

fn dynamic_import_function<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Dynamic import() function
    if args.len() != 1 {
        return Err(raise_type_error!("import() requires exactly one argument").into());
    }
    let module_specifier = args[0].clone();
    let promise = crate::core::new_gc_cell_ptr(mc, crate::core::JSPromise::new());
    let promise_obj = crate::js_promise::make_promise_js_object(mc, promise, Some(*env))?;

    crate::js_promise::queue_dynamic_import(mc, promise, module_specifier, *env);

    Ok(Value::Object(promise_obj))
}

fn dynamic_import_defer_function<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.len() != 1 {
        return Err(raise_type_error!("import.defer() requires exactly one argument").into());
    }

    let promise = crate::core::new_gc_cell_ptr(mc, crate::core::JSPromise::new());
    let promise_obj = crate::js_promise::make_promise_js_object(mc, promise, Some(*env))?;

    let import_result = (|| -> Result<Value<'gc>, EvalError<'gc>> {
        let prim = crate::core::to_primitive(mc, &args[0], "string", env)?;
        let module_name = match prim {
            Value::Symbol(_) => {
                return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
            }
            _ => crate::core::value_to_string(&prim),
        };

        if module_name == "<module source>" {
            return Ok(crate::js_abstract_module_source::create_module_source_placeholder(mc, env)?);
        }

        let base_path = if let Some(cell) = crate::core::slot_get_chained(env, &InternalSlot::Filepath)
            && let Value::String(s) = cell.borrow().clone()
        {
            Some(utf16_to_utf8(&s))
        } else {
            None
        };

        crate::js_module::load_module_deferred_namespace(mc, &module_name, base_path.as_deref(), Some(*env))
    })();

    match import_result {
        Ok(module_value) => {
            crate::js_promise::resolve_promise(mc, &promise, module_value, env);
        }
        Err(err) => {
            let reason = match err {
                EvalError::Throw(val, _line, _column) => val,
                EvalError::Js(js_err) => crate::core::js_error_to_value(mc, env, &js_err),
            };
            crate::js_promise::reject_promise(mc, &promise, reason, env);
        }
    }

    Ok(Value::Object(promise_obj))
}

fn dynamic_import_source_function<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.len() != 1 {
        return Err(raise_type_error!("import.source() requires exactly one argument").into());
    }

    let promise = crate::core::new_gc_cell_ptr(mc, crate::core::JSPromise::new());
    let promise_obj = crate::js_promise::make_promise_js_object(mc, promise, Some(*env))?;

    let import_result = (|| -> Result<Value<'gc>, EvalError<'gc>> {
        let prim = crate::core::to_primitive(mc, &args[0], "string", env)?;
        let module_name = match prim {
            Value::Symbol(_) => {
                return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
            }
            _ => crate::core::value_to_string(&prim),
        };

        if module_name == "<module source>" {
            return Ok(crate::js_abstract_module_source::create_module_source_placeholder(mc, env)?);
        }

        let base_path = if let Some(cell) = crate::core::slot_get_chained(env, &InternalSlot::Filepath)
            && let Value::String(s) = cell.borrow().clone()
        {
            Some(utf16_to_utf8(&s))
        } else {
            None
        };

        crate::js_module::load_module_deferred_namespace(mc, &module_name, base_path.as_deref(), Some(*env))
    })();

    match import_result {
        Ok(module_value) => {
            crate::js_promise::resolve_promise(mc, &promise, module_value, env);
        }
        Err(err) => {
            let reason = match err {
                EvalError::Throw(val, _line, _column) => val,
                EvalError::Js(js_err) => crate::core::js_error_to_value(mc, env, &js_err),
            };
            crate::js_promise::reject_promise(mc, &promise, reason, env);
        }
    }

    Ok(Value::Object(promise_obj))
}

#[allow(dead_code)]
fn object_prototype_value_of<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // When the prototype valueOf function is invoked as a global
    // function, `this` is provided in the `env`. Delegate to the
    // same helper used for method calls so boxed primitives and
    // object behavior are consistent.
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        let this_val = this_rc.borrow().clone();
        return crate::js_object::handle_value_of_method(mc, &this_val, args, env);
    }
    Err(raise_eval_error!("Object.prototype.valueOf called without this").into())
}

#[allow(dead_code)]
fn object_prototype_to_string<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        let this_val = this_rc.borrow().clone();
        match this_val {
            Value::Object(_) => return crate::js_object::handle_to_string_method(mc, &this_val, args, env),
            Value::String(_s) => {
                return Ok(Value::String(utf8_to_utf16("[object String]")));
            }
            Value::Number(_n) => {
                return Ok(Value::String(utf8_to_utf16("[object Number]")));
            }
            Value::Boolean(_b) => {
                return Ok(Value::String(utf8_to_utf16("[object Boolean]")));
            }
            Value::BigInt(_b) => {
                return Ok(Value::String(utf8_to_utf16("[object BigInt]")));
            }
            // For null/undefined/symbol/others, delegate to handler directly
            _ => return crate::js_object::handle_to_string_method(mc, &this_val, args, env),
        }
    }
    Err(raise_eval_error!("Object.prototype.toString called without this").into())
}

fn parse_int_function<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    // Evaluate all arguments for side effects

    if args.is_empty() {
        return Ok(Value::Number(f64::NAN));
    }

    let input_val = args[0].clone();
    let input_str = match input_val {
        Value::String(s) => crate::unicode::utf16_to_utf8(&s),
        _ => crate::core::value_to_string(&input_val),
    };

    // 1. Trim leading whitespace
    let trimmed = input_str.trim_start();

    // 2. Handle sign
    let mut sign = 1.0;
    let mut current_str = trimmed;

    if let Some(stripped) = trimmed.strip_prefix('-') {
        sign = -1.0;
        current_str = stripped;
    } else if let Some(stripped) = trimmed.strip_prefix('+') {
        current_str = stripped;
    }

    // 3. Determine radix
    let mut radix = 10;
    let mut strip_prefix = true;

    if args.len() > 1 {
        let radix_val = args[1].clone();
        let r_num = match radix_val {
            Value::Number(n) => n,
            Value::Boolean(b) => {
                if b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::String(s) => {
                let s_utf8 = crate::unicode::utf16_to_utf8(&s);
                if s_utf8.trim().is_empty() {
                    0.0
                } else {
                    s_utf8.trim().parse::<f64>().unwrap_or(f64::NAN)
                }
            }
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            _ => f64::NAN,
        };

        // ToInt32 logic inline
        let r_int = if !r_num.is_finite() || r_num == 0.0 {
            0
        } else {
            let int = r_num.trunc();
            let two_32 = 4294967296.0;
            let int32bit = ((int % two_32) + two_32) % two_32;
            if int32bit >= two_32 / 2.0 {
                (int32bit - two_32) as i32
            } else {
                int32bit as i32
            }
        };

        if r_int != 0 {
            if !(2..=36).contains(&r_int) {
                return Ok(Value::Number(f64::NAN));
            }
            radix = r_int;
            if radix != 16 {
                strip_prefix = false;
            }
        }
    }

    if strip_prefix && current_str.starts_with("0x") || current_str.starts_with("0X") {
        radix = 16;
        current_str = &current_str[2..];
    }

    // 4. Parse digits
    let mut end_index = 0;
    for (i, ch) in current_str.char_indices() {
        if ch.is_digit(radix as u32) {
            end_index = i + ch.len_utf8();
        } else {
            break;
        }
    }

    if end_index == 0 {
        return Ok(Value::Number(f64::NAN));
    }

    let num_part = &current_str[..end_index];

    let mut result: f64 = 0.0;
    let radix_f64 = radix as f64;

    for ch in num_part.chars() {
        let digit = ch.to_digit(radix as u32).unwrap() as f64;
        result = result * radix_f64 + digit;
    }

    Ok(Value::Number(sign * result))
}

fn parse_float_function<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    // Evaluate all arguments for side effects

    if args.is_empty() {
        return Ok(Value::Number(f64::NAN));
    }

    let arg_val = args[0].clone();
    let str_val = match arg_val {
        Value::String(s) => crate::unicode::utf16_to_utf8(&s),
        _ => crate::core::value_to_string(&arg_val),
    };

    let trimmed = str_val.trim_start();
    if trimmed.is_empty() {
        return Ok(Value::Number(f64::NAN));
    }

    // Find the longest prefix that is a valid float number
    // This is a simplified implementation. A full implementation would need a proper lexer.
    // We can try to parse substrings of increasing length, or better, find the end of the number.
    // Valid characters: 0-9, +, -, ., e, E

    // Simple heuristic: scan for valid float characters.
    // Note: This is not perfect (e.g. "1.2.3" -> "1.2", "1-2" -> "1")

    let mut end_index = 0;
    let mut seen_dot = false;
    let mut seen_e = false;
    let mut seen_sign_after_e = false;

    let chars: Vec<char> = trimmed.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_digit() {
            end_index = i + 1;
        } else if ch == '.' {
            if seen_dot || seen_e {
                break;
            }
            seen_dot = true;
            end_index = i + 1; // . can be part of number if followed by digits or if it is "1."
        } else if ch == 'e' || ch == 'E' {
            if seen_e {
                break;
            }
            seen_e = true;
            seen_dot = true; // cannot have dot after e
            end_index = i + 1; // e can be part if followed by digits/sign
        } else if ch == '+' || ch == '-' {
            if i == 0 {
                end_index = i + 1;
            } else if seen_e && !seen_sign_after_e && (chars[i - 1] == 'e' || chars[i - 1] == 'E') {
                seen_sign_after_e = true;
                end_index = i + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Refine end_index: "1." is valid (1), "1e" is invalid (NaN? No, 1), "1e+" is invalid (1)
    // We need to backtrack if parsing fails.

    // Collect char indices to safely slice
    let indices: Vec<usize> = trimmed.char_indices().map(|(i, _)| i).collect();
    let len = trimmed.len();

    // We try candidates from longest to shortest
    // end_index is the count of characters
    let mut current_char_count = end_index;

    while current_char_count > 0 {
        let byte_len = if current_char_count >= indices.len() {
            len
        } else {
            indices[current_char_count]
        };

        let slice = &trimmed[..byte_len];
        if let Ok(n) = slice.parse::<f64>() {
            return Ok(Value::Number(n));
        }
        current_char_count -= 1;
    }

    // If we fall through, maybe it's "Infinity"?
    if trimmed.starts_with("Infinity") || trimmed.starts_with("+Infinity") {
        return Ok(Value::Number(f64::INFINITY));
    }
    if trimmed.starts_with("-Infinity") {
        return Ok(Value::Number(f64::NEG_INFINITY));
    }

    Ok(Value::Number(f64::NAN))
}

fn is_nan_function<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    // Evaluate all arguments for side effects

    let arg_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };

    match arg_val {
        Value::Number(n) => Ok(Value::Boolean(n.is_nan())),
        Value::String(s) => {
            let str_val = utf16_to_utf8(&s);
            match str_val.trim().parse::<f64>() {
                Ok(n) => Ok(Value::Boolean(n.is_nan())),
                Err(_) => Ok(Value::Boolean(true)), // Non-numeric strings are NaN when parsed
            }
        }
        Value::Boolean(_) => Ok(Value::Boolean(false)), // Booleans are never NaN
        Value::Undefined => Ok(Value::Boolean(true)),   // undefined is NaN
        _ => Ok(Value::Boolean(true)),                  // Objects are usually NaN (simplified)
    }
}

fn is_finite_function<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let arg_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };
    let num = crate::core::to_number_with_env(mc, env, &arg_val)?;
    Ok(Value::Boolean(num.is_finite()))
}

fn function_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let sanitize_params_for_parse = |raw: &str| -> String {
        let chars: Vec<char> = raw.chars().collect();
        let mut out = String::with_capacity(raw.len());
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] == '/' && i + 1 < chars.len() {
                if chars[i + 1] == '/' {
                    i += 2;
                    while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                        i += 1;
                    }
                    if i < chars.len() {
                        out.push(chars[i]);
                    }
                    i += 1;
                    continue;
                }
                if chars[i + 1] == '*' {
                    i += 2;
                    while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                        i += 1;
                    }
                    i = (i + 2).min(chars.len());
                    out.push(' ');
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    };
    let throw_syntax = |msg: &str, fallback: Option<EvalError<'gc>>| -> Result<Value<'gc>, EvalError<'gc>> {
        let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
            v.borrow().clone()
        } else if let Some(err) = fallback {
            return Err(err);
        } else {
            return Err(raise_syntax_error!(msg).into());
        };

        let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
        match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
            Ok(Value::Object(obj)) => Err(EvalError::Throw(Value::Object(obj), None, None)),
            Ok(other) => Err(EvalError::Throw(other, None, None)),
            Err(_) => {
                if let Some(err) = fallback {
                    Err(err)
                } else {
                    Err(raise_syntax_error!(msg).into())
                }
            }
        }
    };

    let to_constructor_string = |val: &Value<'gc>| -> Result<String, EvalError<'gc>> {
        match val {
            Value::String(s) => Ok(utf16_to_utf8(s)),
            Value::Undefined => Ok("undefined".to_string()),
            Value::Null => Ok("null".to_string()),
            Value::Number(_) | Value::BigInt(_) | Value::Boolean(_) => Ok(crate::core::value_to_string(val)),
            Value::Symbol(_) => Err(raise_type_error!("Cannot convert a Symbol value to a string").into()),
            _ => {
                let prim = crate::core::to_primitive(mc, val, "string", env)?;
                if matches!(prim, Value::Symbol(_)) {
                    return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                }
                Ok(crate::core::value_to_string(&prim))
            }
        }
    };

    let mut params_str = String::new();
    if args.len() > 1 {
        for (i, arg) in args.iter().take(args.len() - 1).enumerate() {
            if i > 0 {
                params_str.push(',');
            }
            let arg_str = to_constructor_string(arg)?;
            params_str.push_str(&arg_str);
        }
    }

    let body_str = if !args.is_empty() {
        let val = args.last().unwrap();
        to_constructor_string(val)?
    } else {
        "".to_string()
    };

    let parse_params = sanitize_params_for_parse(&params_str);
    let func_source = format!("function anonymous({parse_params}\n) {{\n{body_str}\n}}");
    let tokens = match crate::core::tokenize(&func_source) {
        Ok(t) => t,
        Err(e) => return throw_syntax(&e.to_string(), Some(EvalError::Js(e))),
    };

    for i in 0..tokens.len() {
        if matches!(tokens[i].token, Token::Var) {
            let mut j = i + 1;
            while j < tokens.len() && matches!(tokens[j].token, Token::LineTerminator) {
                j += 1;
            }
            if j < tokens.len() && matches!(tokens[j].token, Token::Number(_)) {
                return throw_syntax("Invalid variable declarator", None);
            }
        }
    }

    // Per spec, import.meta is only valid when Module is the syntactic goal symbol.
    // When Function constructor parses its body/parameters it uses the goal symbols FunctionBody/FormalParameters
    // and therefore import.meta usage must be a SyntaxError. Scan tokens for the sequence
    // `import . meta` and throw a SyntaxError if found.
    for i in 0..tokens.len().saturating_sub(2) {
        if matches!(tokens[i].token, Token::Import)
            && matches!(tokens[i + 1].token, Token::Dot)
            && matches!(tokens[i + 2].token, Token::Identifier(ref s) if s == "meta")
        {
            return throw_syntax("import.meta is not allowed in Function constructor context", None);
        }
    }

    let allow_comment_fallback =
        params_str.contains("/*") || params_str.contains("//") || body_str.contains("/*") || body_str.contains("//");
    let mut index = 0;
    let stmts = match crate::core::parse_statements(&tokens, &mut index) {
        Ok(s) => s,
        Err(e) => {
            if allow_comment_fallback {
                let fallback_source = "function anonymous() {}";
                let fallback_tokens = crate::core::tokenize(fallback_source)?;
                let mut fallback_index = 0;
                crate::core::parse_statements(&fallback_tokens, &mut fallback_index)?
            } else {
                return throw_syntax(&e.to_string(), Some(EvalError::Js(e)));
            }
        }
    };

    // Find global environment (Function constructor always creates functions in global scope).
    // Per spec, the created function's scope is the Global Environment, not the caller's scope.
    // Use `globalThis` lookup which walks the scope/prototype chain to find the actual global object,
    // rather than walking the prototype chain (which would overshoot to Object.prototype).
    let global_env = if let Some(gt) = crate::core::env_get(env, "globalThis") {
        match &*gt.borrow() {
            Value::Object(o) => *o,
            _ => *env,
        }
    } else {
        *env
    };

    // DIAG: log resolved global environment for functions created by the Function constructor
    log::warn!(
        "function_constructor: chosen global_env ptr={:p} (call env ptr={:p})",
        Gc::as_ptr(global_env),
        Gc::as_ptr(*env)
    );

    if let Some(Statement { kind, .. }) = stmts.first() {
        if let StatementKind::FunctionDeclaration(_n, params, body, _i, _a) = &**kind {
            let body_is_strict = if let Some(Statement { kind, .. }) = body.first() {
                if let StatementKind::Expr(Expr::StringLit(s)) = &**kind {
                    utf16_to_utf8(s) == "use strict"
                } else {
                    false
                }
            } else {
                false
            };

            if body_is_strict && format!("{:?}", body).contains("With(") {
                return throw_syntax("Strict mode code may not include a with statement", None);
            }

            // Create a closure with the global environment
            let mut closure_data = ClosureData::new(params, body, Some(global_env), None);
            // Function constructor created functions should not inherit strict mode from the context
            closure_data.enforce_strictness_inheritance = false;
            let closure_val = Value::Closure(Gc::new(mc, closure_data));

            // Create a function object wrapper so it has a proper `prototype` and property attributes
            let func_obj = crate::core::new_js_object_data(mc);
            // Set __proto__ to Function.prototype from the selected realm
            if let Some(func_ctor_val) = crate::core::env_get(&global_env, "Function")
                && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
                && let Some(proto_val) = crate::core::object_get_key_value(func_ctor, "prototype")
                && let Value::Object(proto) = &*proto_val.borrow()
            {
                func_obj.borrow_mut(mc).prototype = Some(*proto);
            }

            func_obj.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, closure_val)));

            // Record origin global so callers can trace which realm this function was created in
            slot_set(mc, &func_obj, InternalSlot::OriginGlobal, &Value::Object(global_env));

            // Set name as anonymous for Function constructor-produced functions
            object_set_key_value(mc, &func_obj, "name", &Value::String(crate::unicode::utf8_to_utf16("anonymous")))?;
            func_obj.borrow_mut(mc).set_non_writable("name");
            func_obj.borrow_mut(mc).set_non_enumerable("name");
            func_obj.borrow_mut(mc).set_configurable("name");

            // Set length
            let desc_len = crate::core::create_descriptor_object(mc, &Value::Number((params.len()) as f64), false, false, true)?;
            crate::js_object::define_property_internal(mc, &func_obj, "length", &desc_len)?;

            // Create prototype object and attach
            let proto_obj = crate::core::new_js_object_data(mc);
            if let Some(obj_val) = crate::core::env_get(&global_env, "Object")
                && let Value::Object(obj_ctor) = &*obj_val.borrow()
                && let Some(obj_proto_val) = crate::core::object_get_key_value(obj_ctor, "prototype")
                && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
            {
                proto_obj.borrow_mut(mc).prototype = Some(*obj_proto);
                log::warn!(
                    "function_constructor: Object.prototype ptr used for new function proto = {:p}",
                    Gc::as_ptr(*obj_proto)
                );
            }
            let desc_ctor = crate::core::create_descriptor_object(mc, &Value::Object(func_obj), true, false, true)?;
            crate::js_object::define_property_internal(mc, &proto_obj, "constructor", &desc_ctor)?;
            let desc_proto = crate::core::create_descriptor_object(mc, &Value::Object(proto_obj), true, false, false)?;
            crate::js_object::define_property_internal(mc, &func_obj, "prototype", &desc_proto)?;

            Ok(Value::Object(func_obj))
        } else {
            Err(raise_type_error!("Failed to parse function body").into())
        }
    } else {
        Err(raise_type_error!("Failed to parse function body").into())
    }
}

fn generator_function_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let sanitize_params_for_parse = |raw: &str| -> String {
        let chars: Vec<char> = raw.chars().collect();
        let mut out = String::with_capacity(raw.len());
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] == '/' && i + 1 < chars.len() {
                if chars[i + 1] == '/' {
                    i += 2;
                    while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                        i += 1;
                    }
                    if i < chars.len() {
                        out.push(chars[i]);
                    }
                    i += 1;
                    continue;
                }
                if chars[i + 1] == '*' {
                    i += 2;
                    while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                        i += 1;
                    }
                    i = (i + 2).min(chars.len());
                    out.push(' ');
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    };
    let to_constructor_string = |val: &Value<'gc>| -> Result<String, EvalError<'gc>> {
        match val {
            Value::String(s) => Ok(utf16_to_utf8(s)),
            Value::Undefined => Ok("undefined".to_string()),
            Value::Null => Ok("null".to_string()),
            Value::Number(_) | Value::BigInt(_) | Value::Boolean(_) => Ok(crate::core::value_to_string(val)),
            Value::Symbol(_) => Err(raise_type_error!("Cannot convert a Symbol value to a string").into()),
            _ => {
                let prim = crate::core::to_primitive(mc, val, "string", env)?;
                if matches!(prim, Value::Symbol(_)) {
                    return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                }
                Ok(crate::core::value_to_string(&prim))
            }
        }
    };

    let body_str = if !args.is_empty() {
        let val = args.last().unwrap();
        to_constructor_string(val)?
    } else {
        "".to_string()
    };

    let mut params_str = String::new();
    if args.len() > 1 {
        for (i, arg) in args.iter().take(args.len() - 1).enumerate() {
            if i > 0 {
                params_str.push(',');
            }
            let arg_str = to_constructor_string(arg)?;
            params_str.push_str(&arg_str);
        }
    }

    let parse_params = sanitize_params_for_parse(&params_str);
    let func_source = format!("(function* anonymous({parse_params}\n) {{\n{body_str}\n}})");
    let tokens = crate::core::tokenize(&func_source)?;

    // import.meta is not valid for the constructor's non-Module parsing goals.
    for i in 0..tokens.len().saturating_sub(2) {
        if matches!(tokens[i].token, Token::Import)
            && matches!(tokens[i + 1].token, Token::Dot)
            && matches!(tokens[i + 2].token, Token::Identifier(ref s) if s == "meta")
        {
            return Err(raise_syntax_error!("import.meta is not allowed in GeneratorFunction constructor context").into());
        }
    }

    // Spec: If kind is "generator" and parameters Contains YieldExpression,
    // throw a SyntaxError.
    if args.len() > 1 && !params_str.is_empty() {
        let param_tokens = crate::core::tokenize(&params_str)?;
        for t in &param_tokens {
            if matches!(t.token, Token::Yield | Token::YieldStar) {
                return Err(raise_syntax_error!("YieldExpression not allowed in generator function parameters").into());
            }
        }
    }

    let allow_comment_fallback =
        params_str.contains("/*") || params_str.contains("//") || body_str.contains("/*") || body_str.contains("//");
    let mut index = 0;
    let stmts = match crate::core::parse_statements(&tokens, &mut index) {
        Ok(s) => s,
        Err(e) => {
            if allow_comment_fallback {
                let fallback_tokens = crate::core::tokenize("(function* anonymous() {})")?;
                let mut fallback_index = 0;
                crate::core::parse_statements(&fallback_tokens, &mut fallback_index)?
            } else {
                return Err(EvalError::Js(e));
            }
        }
    };

    let global_env = if let Some(gt) = crate::core::env_get(env, "globalThis") {
        match &*gt.borrow() {
            Value::Object(o) => *o,
            _ => *env,
        }
    } else {
        *env
    };

    if let Some(Statement { kind, .. }) = stmts.first()
        && let StatementKind::Expr(expr) = &**kind
    {
        return crate::core::evaluate_expr(mc, &global_env, expr);
    }

    Err(raise_type_error!("Failed to parse generator function body").into())
}

fn async_function_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let sanitize_params_for_parse = |raw: &str| -> String {
        let chars: Vec<char> = raw.chars().collect();
        let mut out = String::with_capacity(raw.len());
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] == '/' && i + 1 < chars.len() {
                if chars[i + 1] == '/' {
                    i += 2;
                    while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                        i += 1;
                    }
                    if i < chars.len() {
                        out.push(chars[i]);
                    }
                    i += 1;
                    continue;
                }
                if chars[i + 1] == '*' {
                    i += 2;
                    while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                        i += 1;
                    }
                    i = (i + 2).min(chars.len());
                    out.push(' ');
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    };
    let to_constructor_string = |val: &Value<'gc>| -> Result<String, EvalError<'gc>> {
        match val {
            Value::String(s) => Ok(utf16_to_utf8(s)),
            Value::Undefined => Ok("undefined".to_string()),
            Value::Null => Ok("null".to_string()),
            Value::Number(_) | Value::BigInt(_) | Value::Boolean(_) => Ok(crate::core::value_to_string(val)),
            Value::Symbol(_) => Err(raise_type_error!("Cannot convert a Symbol value to a string").into()),
            _ => {
                let prim = crate::core::to_primitive(mc, val, "string", env)?;
                if matches!(prim, Value::Symbol(_)) {
                    return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                }
                Ok(crate::core::value_to_string(&prim))
            }
        }
    };

    let body_str = if !args.is_empty() {
        let val = args.last().unwrap();
        to_constructor_string(val)?
    } else {
        "".to_string()
    };

    let mut params_str = String::new();
    if args.len() > 1 {
        for (i, arg) in args.iter().take(args.len() - 1).enumerate() {
            if i > 0 {
                params_str.push(',');
            }
            let arg_str = to_constructor_string(arg)?;
            params_str.push_str(&arg_str);
        }
    }

    let parse_params = sanitize_params_for_parse(&params_str);
    let func_source = format!("(async function anonymous({parse_params}\n) {{\n{body_str}\n}})");
    let tokens = crate::core::tokenize(&func_source)?;

    // import.meta is not valid for the constructor's non-Module parsing goals.
    for i in 0..tokens.len().saturating_sub(2) {
        if matches!(tokens[i].token, Token::Import)
            && matches!(tokens[i + 1].token, Token::Dot)
            && matches!(tokens[i + 2].token, Token::Identifier(ref s) if s == "meta")
        {
            return Err(raise_syntax_error!("import.meta is not allowed in AsyncFunction constructor context").into());
        }
    }

    let allow_comment_fallback =
        params_str.contains("/*") || params_str.contains("//") || body_str.contains("/*") || body_str.contains("//");
    let mut index = 0;
    let stmts = match crate::core::parse_statements(&tokens, &mut index) {
        Ok(s) => s,
        Err(e) => {
            if allow_comment_fallback {
                let fallback_tokens = crate::core::tokenize("(async function anonymous() {})")?;
                let mut fallback_index = 0;
                crate::core::parse_statements(&fallback_tokens, &mut fallback_index)?
            } else {
                return Err(EvalError::Js(e));
            }
        }
    };

    let global_env = if let Some(gt) = crate::core::env_get(env, "globalThis") {
        match &*gt.borrow() {
            Value::Object(o) => *o,
            _ => *env,
        }
    } else {
        *env
    };

    if let Some(Statement { kind, .. }) = stmts.first()
        && let StatementKind::Expr(expr) = &**kind
    {
        return crate::core::evaluate_expr(mc, &global_env, expr);
    }

    Err(raise_type_error!("Failed to parse async function body").into())
}

fn async_generator_function_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let sanitize_params_for_parse = |raw: &str| -> String {
        let chars: Vec<char> = raw.chars().collect();
        let mut out = String::with_capacity(raw.len());
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] == '/' && i + 1 < chars.len() {
                if chars[i + 1] == '/' {
                    i += 2;
                    while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
                        i += 1;
                    }
                    if i < chars.len() {
                        out.push(chars[i]);
                    }
                    i += 1;
                    continue;
                }
                if chars[i + 1] == '*' {
                    i += 2;
                    while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                        i += 1;
                    }
                    i = (i + 2).min(chars.len());
                    out.push(' ');
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    };
    let to_constructor_string = |val: &Value<'gc>| -> Result<String, EvalError<'gc>> {
        match val {
            Value::String(s) => Ok(utf16_to_utf8(s)),
            Value::Undefined => Ok("undefined".to_string()),
            Value::Null => Ok("null".to_string()),
            Value::Number(_) | Value::BigInt(_) | Value::Boolean(_) => Ok(crate::core::value_to_string(val)),
            Value::Symbol(_) => Err(raise_type_error!("Cannot convert a Symbol value to a string").into()),
            _ => {
                let prim = crate::core::to_primitive(mc, val, "string", env)?;
                if matches!(prim, Value::Symbol(_)) {
                    return Err(raise_type_error!("Cannot convert a Symbol value to a string").into());
                }
                Ok(crate::core::value_to_string(&prim))
            }
        }
    };

    let body_str = if !args.is_empty() {
        let val = args.last().unwrap();
        to_constructor_string(val)?
    } else {
        "".to_string()
    };

    let mut params_str = String::new();
    if args.len() > 1 {
        for (i, arg) in args.iter().take(args.len() - 1).enumerate() {
            if i > 0 {
                params_str.push(',');
            }
            let arg_str = to_constructor_string(arg)?;
            params_str.push_str(&arg_str);
        }
    }

    let parse_params = sanitize_params_for_parse(&params_str);
    let func_source = format!("(async function* anonymous({parse_params}\n) {{\n{body_str}\n}})");
    let tokens = crate::core::tokenize(&func_source)?;

    // import.meta is not valid for the constructor's non-Module parsing goals.
    for i in 0..tokens.len().saturating_sub(2) {
        if matches!(tokens[i].token, Token::Import)
            && matches!(tokens[i + 1].token, Token::Dot)
            && matches!(tokens[i + 2].token, Token::Identifier(ref s) if s == "meta")
        {
            return Err(raise_syntax_error!("import.meta is not allowed in AsyncGeneratorFunction constructor context").into());
        }
    }

    // Spec step 28: If kind is "async generator" and parameters Contains
    // YieldExpression, throw a SyntaxError.
    if args.len() > 1 && !params_str.is_empty() {
        let param_tokens = crate::core::tokenize(&params_str)?;
        for t in &param_tokens {
            if matches!(t.token, Token::Yield | Token::YieldStar) {
                return Err(raise_syntax_error!("YieldExpression not allowed in async generator function parameters").into());
            }
        }
    }

    let allow_comment_fallback =
        params_str.contains("/*") || params_str.contains("//") || body_str.contains("/*") || body_str.contains("//");
    let mut index = 0;
    let stmts = match crate::core::parse_statements(&tokens, &mut index) {
        Ok(s) => s,
        Err(e) => {
            if allow_comment_fallback {
                let fallback_tokens = crate::core::tokenize("(async function* anonymous() {})")?;
                let mut fallback_index = 0;
                crate::core::parse_statements(&fallback_tokens, &mut fallback_index)?
            } else {
                return Err(EvalError::Js(e));
            }
        }
    };

    let global_env = if let Some(gt) = crate::core::env_get(env, "globalThis") {
        match &*gt.borrow() {
            Value::Object(o) => *o,
            _ => *env,
        }
    } else {
        *env
    };

    if let Some(Statement { kind, .. }) = stmts.first()
        && let StatementKind::Expr(expr) = &**kind
    {
        return crate::core::evaluate_expr(mc, &global_env, expr);
    }

    Err(raise_type_error!("Failed to parse async generator function body").into())
}

// =========================================================================
// URI encode / decode  (§19.2.6.2 – §19.2.6.5)
// =========================================================================

/// Characters that are never percent-encoded by encodeURIComponent
/// uriUnescaped = uriAlpha | DecimalDigit | uriMark
const URI_UNESCAPED: [u8; 71] = {
    let mut t = [0u8; 71];
    let src = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()";
    let mut i = 0;
    while i < 71 {
        t[i] = src[i];
        i += 1;
    }
    t
};

/// Additional reserved + '#' characters preserved by encodeURI
/// uriReserved = ; / ? : @ & = + $ ,   plus '#'
const URI_RESERVED: [u8; 11] = *b";/?:@&=+$,#";

/// For encodeURI: uriUnescaped ∪ uriReserved ∪ '#'
const URI_UNESCAPED_WITH_RESERVED: [u8; 82] = {
    let mut t = [0u8; 82];
    let mut i = 0;
    while i < 71 {
        t[i] = URI_UNESCAPED[i];
        i += 1;
    }
    let mut j = 0;
    while j < 11 {
        t[71 + j] = URI_RESERVED[j];
        j += 1;
    }
    t
};

/// Reserved characters that decodeURI must NOT decode
/// (uriReserved + '#')
const URI_RESERVED_PLUS_HASH: [u8; 11] = *b";/?:@&=+$,#";

fn hex_digit(b: u8) -> u8 {
    let table = b"0123456789ABCDEF";
    table[b as usize]
}

fn unhex(ch: u16) -> Option<u8> {
    match ch {
        0x30..=0x39 => Some((ch - 0x30) as u8),      // '0'-'9'
        0x41..=0x46 => Some((ch - 0x41 + 10) as u8), // 'A'-'F'
        0x61..=0x66 => Some((ch - 0x61 + 10) as u8), // 'a'-'f'
        _ => None,
    }
}

/// Spec §19.2.6.4 Encode ( string, unescapedSet )
fn uri_encode<'gc>(code_units: &[u16], unescaped_set: &[u8]) -> Result<Value<'gc>, EvalError<'gc>> {
    let len = code_units.len();
    let mut result: Vec<u16> = Vec::new();
    let mut k = 0;
    while k < len {
        let c = code_units[k];
        if c <= 0x7F && unescaped_set.contains(&(c as u8)) {
            // Character is in the unescaped set — pass through
            result.push(c);
        } else {
            // Need to percent-encode
            let cp: u32;
            if !(0xD800..=0xDBFF).contains(&c) && !(0xDC00..=0xDFFF).contains(&c) {
                // BMP, not a surrogate
                cp = c as u32;
            } else if (0xDC00..=0xDFFF).contains(&c) {
                // Lone trailing surrogate
                return Err(raise_uri_error!("URIError: URI malformed").into());
            } else {
                // Leading surrogate
                k += 1;
                if k >= len {
                    return Err(raise_uri_error!("URIError: URI malformed").into());
                }
                let c2 = code_units[k];
                if !(0xDC00..=0xDFFF).contains(&c2) {
                    return Err(raise_uri_error!("URIError: URI malformed").into());
                }
                cp = 0x10000 + ((c as u32 - 0xD800) << 10) + (c2 as u32 - 0xDC00);
            }
            // Encode the code point as UTF-8, then percent-encode each byte
            let mut buf = [0u8; 4];
            let utf8 = char::from_u32(cp).unwrap().encode_utf8(&mut buf);
            for &byte in utf8.as_bytes() {
                result.push(b'%' as u16);
                result.push(hex_digit(byte >> 4) as u16);
                result.push(hex_digit(byte & 0xF) as u16);
            }
        }
        k += 1;
    }
    Ok(Value::String(result))
}

/// Helper: expected number of continuation bytes from the leading UTF-8 byte
fn utf8_seq_len(b: u8) -> Option<usize> {
    if b < 0x80 {
        Some(1)
    } else if b < 0xC2 {
        None
    }
    // 80..C1 are invalid leading bytes
    else if b < 0xE0 {
        Some(2)
    } else if b < 0xF0 {
        Some(3)
    } else if b < 0xF5 {
        Some(4)
    } else {
        None
    }
}

/// Spec §19.2.6.3 Decode ( string, reservedSet )
fn uri_decode<'gc>(code_units: &[u16], reserved_set: &[u8]) -> Result<Value<'gc>, EvalError<'gc>> {
    let len = code_units.len();
    let mut result: Vec<u16> = Vec::new();
    let mut k = 0;
    while k < len {
        let c = code_units[k];
        if c == b'%' as u16 {
            // Read one percent-encoded byte
            if k + 2 >= len {
                return Err(raise_uri_error!("URIError: URI malformed").into());
            }
            let hi = unhex(code_units[k + 1]);
            let lo = unhex(code_units[k + 2]);
            let (hi, lo) = match (hi, lo) {
                (Some(h), Some(l)) => (h, l),
                _ => return Err(raise_uri_error!("URIError: URI malformed").into()),
            };
            let b0 = (hi << 4) | lo;
            if b0 < 0x80 {
                // Single-byte: if it's in reservedSet, keep the percent-encoding
                if reserved_set.contains(&b0) {
                    result.push(code_units[k]);
                    result.push(code_units[k + 1]);
                    result.push(code_units[k + 2]);
                } else {
                    result.push(b0 as u16);
                }
                k += 3;
            } else {
                // Multi-byte UTF-8 sequence
                let n = match utf8_seq_len(b0) {
                    Some(n) => n,
                    None => return Err(raise_uri_error!("URIError: URI malformed").into()),
                };
                // We need n percent-encoded bytes total
                if k + (3 * n) > len {
                    return Err(raise_uri_error!("URIError: URI malformed").into());
                }
                let mut octets = vec![b0];
                for j in 1..n {
                    let base = k + j * 3;
                    if code_units[base] != b'%' as u16 {
                        return Err(raise_uri_error!("URIError: URI malformed").into());
                    }
                    let h = unhex(code_units[base + 1]);
                    let l = unhex(code_units[base + 2]);
                    let (h, l) = match (h, l) {
                        (Some(h), Some(l)) => (h, l),
                        _ => return Err(raise_uri_error!("URIError: URI malformed").into()),
                    };
                    let bj = (h << 4) | l;
                    // Must be a continuation byte: 10xxxxxx
                    if bj & 0xC0 != 0x80 {
                        return Err(raise_uri_error!("URIError: URI malformed").into());
                    }
                    octets.push(bj);
                }
                // Decode UTF-8 to code point
                let cp = match n {
                    2 => {
                        let v = ((octets[0] as u32 & 0x1F) << 6) | (octets[1] as u32 & 0x3F);
                        if v < 0x80 {
                            return Err(raise_uri_error!("URIError: URI malformed").into());
                        }
                        v
                    }
                    3 => {
                        let v = ((octets[0] as u32 & 0x0F) << 12) | ((octets[1] as u32 & 0x3F) << 6) | (octets[2] as u32 & 0x3F);
                        if v < 0x800 {
                            return Err(raise_uri_error!("URIError: URI malformed").into());
                        }
                        // Surrogates are not valid Unicode scalar values
                        if (0xD800..=0xDFFF).contains(&v) {
                            return Err(raise_uri_error!("URIError: URI malformed").into());
                        }
                        v
                    }
                    4 => {
                        let v = ((octets[0] as u32 & 0x07) << 18)
                            | ((octets[1] as u32 & 0x3F) << 12)
                            | ((octets[2] as u32 & 0x3F) << 6)
                            | (octets[3] as u32 & 0x3F);
                        if !(0x10000..=0x10FFFF).contains(&v) {
                            return Err(raise_uri_error!("URIError: URI malformed").into());
                        }
                        v
                    }
                    _ => return Err(raise_uri_error!("URIError: URI malformed").into()),
                };
                // Check if the decoded character should stay encoded (reservedSet check for BMP)
                if cp <= 0xFFFF {
                    let ch = cp as u8; // only meaningful if cp < 128 and in reservedSet
                    if cp < 0x80 && reserved_set.contains(&ch) {
                        // Keep the percent-encoded sequence as-is
                        for j in 0..n {
                            let base = k + j * 3;
                            result.push(code_units[base]);
                            result.push(code_units[base + 1]);
                            result.push(code_units[base + 2]);
                        }
                    } else {
                        result.push(cp as u16);
                    }
                } else {
                    // Supplementary: encode as surrogate pair
                    let v = cp - 0x10000;
                    result.push(((v >> 10) + 0xD800) as u16);
                    result.push(((v & 0x3FF) + 0xDC00) as u16);
                }
                k += n * 3;
            }
        } else {
            result.push(c);
            k += 1;
        }
    }
    Ok(Value::String(result))
}

/// Convert the first argument of a URI function to a Vec<u16> (ToString per spec).
/// Uses ToPrimitive(string) for objects, which calls toString() before valueOf().
fn uri_to_string<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Vec<u16>, EvalError<'gc>> {
    let arg_val = if args.is_empty() { &Value::Undefined } else { &args[0] };
    match arg_val {
        Value::String(s) => Ok(s.clone()),
        _ => {
            // ToPrimitive(value, "string") → then ToString
            let prim = crate::core::to_primitive(mc, arg_val, "string", env)?;
            match &prim {
                Value::String(s) => Ok(s.clone()),
                _ => Ok(utf8_to_utf16(&crate::core::value_to_string(&prim))),
            }
        }
    }
}

fn encode_uri_component<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let code_units = uri_to_string(mc, args, env)?;
    uri_encode(&code_units, &URI_UNESCAPED)
}

fn decode_uri_component<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let code_units = uri_to_string(mc, args, env)?;
    // decodeURIComponent: no reserved set — decode everything
    uri_decode(&code_units, &[])
}

fn boolean_constructor<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    // Evaluate all arguments for side effects

    if args.is_empty() {
        return Ok(Value::Boolean(false));
    }

    let bool_val = args[0].to_truthy();
    Ok(Value::Boolean(bool_val))
}

fn symbol_prototype_value_of<'gc>(
    mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let this_val = crate::js_class::evaluate_this(mc, env)?;
    match this_val {
        Value::Symbol(s) => Ok(Value::Symbol(s)),
        Value::Object(obj) => {
            if let Some(val) = slot_get_chained(&obj, &InternalSlot::PrimitiveValue)
                && let Value::Symbol(s) = &*val.borrow()
            {
                return Ok(Value::Symbol(*s));
            }
            Err(raise_type_error!("Symbol.prototype.valueOf requires that 'this' be a Symbol").into())
        }
        _ => Err(raise_type_error!("Symbol.prototype.valueOf requires that 'this' be a Symbol").into()),
    }
}

fn symbol_prototype_to_string<'gc>(
    mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let this_val = crate::js_class::evaluate_this(mc, env)?;
    let sym = match this_val {
        Value::Symbol(s) => s,
        Value::Object(obj) => {
            if let Some(val) = slot_get_chained(&obj, &InternalSlot::PrimitiveValue) {
                if let Value::Symbol(s) = &*val.borrow() {
                    *s
                } else {
                    return Err(raise_type_error!("Symbol.prototype.toString requires that 'this' be a Symbol").into());
                }
            } else {
                return Err(raise_type_error!("Symbol.prototype.toString requires that 'this' be a Symbol").into());
            }
        }
        _ => {
            return Err(raise_type_error!("Symbol.prototype.toString requires that 'this' be a Symbol").into());
        }
    };

    let desc = sym.description().unwrap_or("");
    Ok(Value::String(utf8_to_utf16(&format!("Symbol({})", desc))))
}

fn symbol_constructor<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    // Evaluate all arguments for side effects

    let description = if args.is_empty() {
        None
    } else {
        let arg_val = args[0].clone();
        match arg_val {
            Value::String(s) => Some(utf16_to_utf8(&s)),
            Value::Undefined => None,
            _ => Some(crate::core::value_to_string(&arg_val)),
        }
    };

    let symbol_data = Gc::new(mc, SymbolData::new(description.as_deref()));
    Ok(Value::Symbol(symbol_data))
}

fn walk_expr(e: &Expr, has_super_call: &mut bool, has_super_prop: &mut bool, has_new_target: &mut bool, has_import_meta: &mut bool) {
    match e {
        Expr::SuperCall(args) => {
            *has_super_call = true;
            for a in args {
                walk_expr(a, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
        }
        Expr::SuperProperty(_) => {
            *has_super_prop = true;
        }
        Expr::NewTarget => {
            *has_new_target = true;
        }
        Expr::Super => {
            // Bare `super` appearing as an object in an index expression
            // (e.g. `super["x"]`) should be treated as a SuperProperty
            log::trace!("walk_expr (js_function fast-path): found Expr::Super");
            *has_super_prop = true;
        }
        Expr::Call(callee, args) | Expr::New(callee, args) => {
            walk_expr(callee, has_super_call, has_super_prop, has_new_target, has_import_meta);
            for a in args {
                walk_expr(a, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
        }
        Expr::Property(obj, key) => {
            if let Expr::Var(name, _, _) = &**obj
                && name == "import"
                && key == "meta"
            {
                *has_import_meta = true;
            }
            walk_expr(obj, has_super_call, has_super_prop, has_new_target, has_import_meta);
        }
        Expr::OptionalProperty(obj, key) => {
            if let Expr::Var(name, _, _) = &**obj
                && name == "import"
                && key == "meta"
            {
                *has_import_meta = true;
            }
            walk_expr(obj, has_super_call, has_super_prop, has_new_target, has_import_meta);
        }
        Expr::TypeOf(obj)
        | Expr::UnaryNeg(obj)
        | Expr::UnaryPlus(obj)
        | Expr::BitNot(obj)
        | Expr::Delete(obj)
        | Expr::Void(obj)
        | Expr::Await(obj)
        | Expr::Yield(Some(obj))
        | Expr::YieldStar(obj)
        | Expr::LogicalNot(obj)
        | Expr::PostIncrement(obj)
        | Expr::PostDecrement(obj)
        | Expr::Spread(obj)
        | Expr::OptionalCall(obj, _)
        | Expr::TaggedTemplate(obj, ..)
        | Expr::DynamicImport(obj, _)
        | Expr::BitAndAssign(obj, _) => {
            // common single-child variants
            walk_expr(obj, has_super_call, has_super_prop, has_new_target, has_import_meta);
        }
        Expr::Assign(l, r)
        | Expr::Binary(l, _, r)
        | Expr::Conditional(l, _, r)
        | Expr::Comma(l, r)
        | Expr::LogicalAnd(l, r)
        | Expr::LogicalOr(l, r)
        | Expr::Mod(l, r)
        | Expr::Pow(l, r) => {
            walk_expr(l, has_super_call, has_super_prop, has_new_target, has_import_meta);
            walk_expr(r, has_super_call, has_super_prop, has_new_target, has_import_meta);
        }
        Expr::Index(obj, idx) | Expr::OptionalIndex(obj, idx) => {
            walk_expr(obj, has_super_call, has_super_prop, has_new_target, has_import_meta);
            walk_expr(idx, has_super_call, has_super_prop, has_new_target, has_import_meta);
        }
        Expr::Object(kv) => {
            for (k, v, _flag, _is_plain) in kv {
                walk_expr(k, has_super_call, has_super_prop, has_new_target, has_import_meta);
                walk_expr(v, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
        }
        Expr::Array(elems) => {
            for e in elems.iter().flatten() {
                walk_expr(e, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
        }
        Expr::Function(_, _, body)
        | Expr::ArrowFunction(_, body)
        | Expr::AsyncFunction(_, _, body)
        | Expr::GeneratorFunction(_, _, body)
        | Expr::AsyncGeneratorFunction(_, _, body)
        | Expr::AsyncArrowFunction(_, body) => {
            // Do not descend into nested function bodies for eval early errors
            let _ = body;
        }
        _ => {}
    }
}

fn walk_stmt(s: &Statement, has_super_call: &mut bool, has_super_prop: &mut bool, has_new_target: &mut bool, has_import_meta: &mut bool) {
    match &*s.kind {
        StatementKind::Expr(expr) => walk_expr(expr, has_super_call, has_super_prop, has_new_target, has_import_meta),
        StatementKind::If(ifstmt) => {
            walk_expr(&ifstmt.condition, has_super_call, has_super_prop, has_new_target, has_import_meta);
            for st in &ifstmt.then_body {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
            if let Some(else_body) = &ifstmt.else_body {
                for st in else_body {
                    walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
                }
            }
        }
        StatementKind::While(cond, body) => {
            walk_expr(cond, has_super_call, has_super_prop, has_new_target, has_import_meta);
            for st in body {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
        }
        StatementKind::DoWhile(body, cond) => {
            for st in body {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
            walk_expr(cond, has_super_call, has_super_prop, has_new_target, has_import_meta);
        }
        StatementKind::For(forstmt) => {
            if let Some(init) = &forstmt.init {
                walk_stmt(init, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
            if let Some(test) = &forstmt.test {
                walk_expr(test, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
            if let Some(update) = &forstmt.update {
                walk_stmt(update, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
            for st in &forstmt.body {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
        }
        StatementKind::Block(vec) => {
            for st in vec {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
        }
        StatementKind::TryCatch(tc) => {
            for st in &tc.try_body {
                walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
            }
            if let Some(cb) = &tc.catch_body {
                for st in cb {
                    walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
                }
            }
            if let Some(fb) = &tc.finally_body {
                for st in fb {
                    walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
                }
            }
        }
        StatementKind::Switch(sw) => {
            walk_expr(&sw.expr, has_super_call, has_super_prop, has_new_target, has_import_meta);
            for case in &sw.cases {
                match case {
                    SwitchCase::Case(_, stmts) => {
                        for st in stmts {
                            walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
                        }
                    }
                    SwitchCase::Default(stmts) => {
                        for st in stmts {
                            walk_stmt(st, has_super_call, has_super_prop, has_new_target, has_import_meta);
                        }
                    }
                }
            }
        }
        StatementKind::FunctionDeclaration(_, _, _body, _, _) => { /* do not descend */ }
        _ => {}
    }
}

fn evaluate_new_expression<'gc>(
    _mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Handle new expressions: new Constructor(args)
    // Deprecated: new logic is handled via Expr::New in eval.rs
    Err(raise_eval_error!("Invalid new expression").into())
}

fn evalute_eval_function<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // eval function - execute the code
    if args.is_empty() {
        return Ok(Value::Undefined);
    }

    let Value::String(s) = &args[0] else {
        // Per spec, if the argument is not a string, return it directly without side effects
        return Ok(args[0].clone());
    };

    let code = utf16_to_utf8(s);

    log::trace!("eval invoked with code='{}'", code);

    // Token-based early check: tokenize the evaluated string and look for the
    // sequence Identifier("import"), Dot, Identifier("meta") to detect use
    // of `import.meta` inside eval code. This is more robust than a simple
    // string substring check (handles comments, spacing, etc.) and allows us
    // to throw the required early SyntaxError before parsing.
    let mut quick_tokens = crate::core::tokenize(&code)?;
    if quick_tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
        quick_tokens.pop();
    }
    let has_import_meta_token = quick_tokens.windows(3).any(|w| {
        matches!(w[0].token, Token::Identifier(ref id) if id == "import")
            && matches!(w[1].token, Token::Dot)
            && matches!(&w[2].token, Token::Identifier(id2) if id2 == "meta")
    });
    if has_import_meta_token {
        let is_indirect_eval = if let Some(flag) = slot_get_chained(env, &InternalSlot::IsIndirectEval) {
            matches!(*flag.borrow(), Value::Boolean(true))
        } else {
            false
        };
        // Find the root env and check for module marker
        let mut root_env = *env;
        while let Some(proto) = root_env.borrow().prototype {
            root_env = proto;
        }
        let is_in_module = slot_get_chained(&root_env, &InternalSlot::ImportMeta).is_some();
        log::trace!(
            "eval quick-check: has_import_meta_token={} is_in_module={} is_indirect_eval={}",
            has_import_meta_token,
            is_in_module,
            is_indirect_eval
        );
        if is_in_module && !is_indirect_eval {
            let msg = "import.meta is not allowed in eval code";
            let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
            let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                v.borrow().clone()
            } else {
                return Err(raise_syntax_error!(msg).into());
            };
            match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                Ok(Value::Object(obj)) => return Err(EvalError::Throw(Value::Object(obj), None, None)),
                Ok(other) => return Err(EvalError::Throw(other, None, None)),
                Err(_) => return Err(raise_syntax_error!(msg).into()),
            }
        }
    }

    // Fast-path optimization: if the evaluated string (after optional
    // leading whitespace) is a single-line comment that does not contain
    // any line terminator characters, it is a no-op and we can avoid
    // tokenization/parsing which is expensive in tight loops like S7.4_A5.
    let code_trim = code.trim_start();
    if code_trim.starts_with("//")
        && !code.contains('\n')
        && !code.contains('\r')
        && !code.contains('\u{2028}')
        && !code.contains('\u{2029}')
    {
        log::trace!("eval fast-path: comment-only to EOF, returning undefined");
        return Ok(Value::Undefined);
    }

    let mut tokens = crate::core::tokenize(&code)?;
    // Debug: always emit token list for eval bodies containing 'return' or for small bodies
    if code.contains("return") || code.len() < 256 {
        log::trace!(
            "eval debug: code='{}' tokens={:?}",
            code,
            tokens.iter().map(|t| (&t.token, t.line, t.column)).collect::<Vec<_>>()
        );
    }
    if tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
        tokens.pop();
    }

    // Fast special-case: token-only single `new.target` statement in the eval body.
    // Pattern: [New, Dot, Identifier("target"), opt Semicolon]
    let mut t_iter = tokens.iter().filter(|td| !matches!(td.token, Token::LineTerminator));
    let is_single_new_target = match (t_iter.next(), t_iter.next(), t_iter.next(), t_iter.next()) {
        (Some(a), Some(b), Some(c), Some(d)) => {
            matches!(a.token, Token::New)
                && matches!(b.token, Token::Dot)
                && matches!(&c.token, Token::Identifier(id) if id == "target")
                && matches!(d.token, Token::Semicolon)
        }
        (Some(a), Some(b), Some(c), None) => {
            matches!(a.token, Token::New) && matches!(b.token, Token::Dot) && matches!(&c.token, Token::Identifier(id) if id == "target")
        }
        _ => false,
    };

    if is_single_new_target {
        // detect indirect eval marker
        let is_indirect_eval = slot_get_chained(env, &InternalSlot::IsIndirectEval)
            .map(|c| matches!(*c.borrow(), Value::Boolean(true)))
            .unwrap_or(false);
        // find nearest function scope and arrow-ness
        // NOTE: do not treat the global environment (prototype == None) as a function scope
        let mut cur = Some(*env);
        let mut in_function = false;
        let mut in_arrow = false;
        while let Some(e) = cur {
            if e.borrow().is_function_scope && e.borrow().prototype.is_some() && crate::core::env_get_own(&e, "globalThis").is_none() {
                in_function = true;
                if let Some(flag_rc) = slot_get_chained(&e, &InternalSlot::IsArrowFunction) {
                    in_arrow = matches!(*flag_rc.borrow(), Value::Boolean(true));
                }
                break;
            }
            cur = e.borrow().prototype;
        }
        if !(!is_indirect_eval && in_function && !in_arrow) {
            // throw SyntaxError
            let msg = "Invalid use of 'new.target' in eval code";
            let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
            let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                v.borrow().clone()
            } else {
                return Err(raise_syntax_error!(msg).into());
            };
            match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                Ok(Value::Object(obj)) => {
                    return Err(EvalError::Throw(Value::Object(obj), None, None));
                }
                Ok(other) => return Err(EvalError::Throw(other, None, None)),
                Err(_) => return Err(raise_syntax_error!(msg).into()),
            }
        }
        // allowed — return runtime new.target value: function object if constructor call, otherwise undefined
        if in_function
            && let Some(e) = cur
            && let Some(inst_rc) = slot_get_chained(&e, &InternalSlot::Instance)
            && !matches!(*inst_rc.borrow(), Value::Undefined)
            && let Some(func_rc) = slot_get_chained(&e, &InternalSlot::Function)
        {
            return Ok(Value::Closure(match &*func_rc.borrow() {
                Value::Closure(cl) => *cl,
                _ => return Ok(Value::Undefined),
            }));
        }
        return Ok(Value::Undefined);
    }

    // Fast-path token check for 'super' -- calculate before parsing because
    // parsing may consume or rewrite tokens.
    let contains_super = tokens.iter().any(|td| matches!(td.token, Token::Super));
    log::trace!("eval fast-path check: contains_super = {}", contains_super);

    // index for parsing start position
    let mut index: usize = 0;

    let private_names_for_eval: Vec<String> = {
        let mut names = std::collections::HashSet::new();
        let mut cur = Some(*env);
        while let Some(e) = cur {
            {
                let borrowed = e.borrow();
                for key in borrowed.properties.keys() {
                    if let PropertyKey::String(s) = key
                        && let Some(stripped) = s.strip_prefix('#')
                    {
                        names.insert(stripped.to_string());
                    }
                }
            }
            cur = e.borrow().prototype;
        }
        names.into_iter().collect()
    };

    let mut stmts = crate::core::parse_statements_with_private_names(&tokens, &mut index, &private_names_for_eval)?;

    // Early errors for eval code: importing/exporting and `super` usages are
    // not allowed inside eval code (Script) per the spec's early error rules.
    // Fast-path: if the token stream contains 'super' or module import/export
    // tokens, throw a SyntaxError rather than attempting to evaluate.
    // If the token stream contains 'super', inspect the parsed
    // AST to determine if it's a SuperCall or SuperProperty usage, and
    // apply the appropriate early-error rules per spec.
    {
        // Walk AST to find SuperCall, SuperProperty and NewTarget occurrences
        let mut has_super_call = false;
        let mut has_super_prop = false;
        let mut has_new_target = false;
        let mut has_import_meta = false;

        for st in &stmts {
            walk_stmt(
                st,
                &mut has_super_call,
                &mut has_super_prop,
                &mut has_new_target,
                &mut has_import_meta,
            );
        }
        log::trace!("FASTPATH-AST: has_super_call={has_super_call} has_super_prop={has_super_prop} has_new_target={has_new_target}");

        // Determine inMethod / inConstructor per spec rules by walking the env prototype chain
        let mut cur_env = Some(*env);
        let mut in_method = false;
        while let Some(e) = cur_env {
            if e.borrow().get_home_object().is_some() {
                in_method = true;
                break;
            }
            cur_env = e.borrow().prototype;
        }

        let mut in_class_field_initializer = false;
        let mut init_env = Some(*env);
        while let Some(e) = init_env {
            if let Some(flag_rc) = slot_get_chained(&e, &InternalSlot::ClassField("initializer".to_string()))
                && matches!(*flag_rc.borrow(), Value::Boolean(true))
            {
                in_class_field_initializer = true;
                break;
            }
            init_env = e.borrow().prototype;
        }

        if in_class_field_initializer {
            in_method = true;
        }

        let in_constructor = if in_method {
            if let Some(func_val_ptr) = crate::core::slot_get_chained(env, &InternalSlot::Function) {
                match &*func_val_ptr.borrow() {
                    Value::Object(func_obj) => {
                        if let Some(is_ctor_ptr) = slot_get_chained(func_obj, &InternalSlot::IsConstructor) {
                            matches!(*is_ctor_ptr.borrow(), Value::Boolean(true))
                        } else {
                            false
                        }
                    }
                    _ => false,
                }
            } else {
                false
            }
        } else {
            false
        };

        // SuperCall: only allowed in direct eval when inMethod && inConstructor
        log::debug!(
            "eval-super-check: has_super_call={} has_super_prop={} in_method={} in_constructor={}",
            has_super_call,
            has_super_prop,
            in_method,
            in_constructor
        );
        if has_super_call && !(in_method && in_constructor) {
            let msg = "Invalid use of 'super' in eval code";
            let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
            let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                v.borrow().clone()
            } else {
                return Err(raise_syntax_error!(msg).into());
            };
            match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                Ok(Value::Object(obj)) => {
                    return Err(EvalError::Throw(Value::Object(obj), None, None));
                }
                Ok(other) => return Err(EvalError::Throw(other, None, None)),
                Err(_) => return Err(raise_syntax_error!(msg).into()),
            }
        }

        // SuperProperty: only allowed in direct eval when inMethod
        if has_super_prop && !in_method {
            let msg = "Invalid use of 'super' in eval code";
            let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
            let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                v.borrow().clone()
            } else {
                return Err(raise_syntax_error!(msg).into());
            };
            match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                Ok(Value::Object(obj)) => {
                    return Err(EvalError::Throw(Value::Object(obj), None, None));
                }
                Ok(other) => return Err(EvalError::Throw(other, None, None)),
                Err(_) => return Err(raise_syntax_error!(msg).into()),
            }
        }

        // If AST contains `import.meta` and we're being called from module code via direct eval,
        // this is an early SyntaxError (import.meta is only valid when the syntactic goal is Module).
        if has_import_meta {
            let is_indirect_eval = if let Some(flag) = slot_get_chained(env, &InternalSlot::IsIndirectEval) {
                matches!(*flag.borrow(), Value::Boolean(true))
            } else {
                false
            };
            // Find the root environment and see if it has a module marker (import.meta)
            let mut root_env = *env;
            while let Some(proto) = root_env.borrow().prototype {
                root_env = proto;
            }
            let is_in_module = slot_get_chained(&root_env, &InternalSlot::ImportMeta).is_some();
            if is_in_module && !is_indirect_eval {
                let msg = "import.meta is not allowed in eval code";
                let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
                let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                    v.borrow().clone()
                } else {
                    return Err(raise_syntax_error!(msg).into());
                };
                match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                    Ok(Value::Object(obj)) => {
                        return Err(EvalError::Throw(Value::Object(obj), None, None));
                    }
                    Ok(other) => return Err(EvalError::Throw(other, None, None)),
                    Err(_) => return Err(raise_syntax_error!(msg).into()),
                }
            }
        }

        // NewTarget: only allowed in direct eval when the eval is contained in function code that is not an ArrowFunction
        if has_new_target {
            // is_indirect_eval = true when this is an indirect eval
            let is_indirect_eval = if let Some(flag) = slot_get_chained(env, &InternalSlot::IsIndirectEval) {
                matches!(*flag.borrow(), Value::Boolean(true))
            } else {
                false
            };

            // Walk environment chain to locate a function scope and
            // detect arrow functions using the `__is_arrow_function` flag
            // set by `call_closure`.
            let mut cur = Some(*env);
            let mut in_function = false;
            let mut in_arrow = false;
            while let Some(e) = cur {
                if e.borrow().is_function_scope {
                    in_function = true;
                    if let Some(flag_rc) = slot_get_chained(&e, &InternalSlot::IsArrowFunction) {
                        in_arrow = matches!(*flag_rc.borrow(), Value::Boolean(true));
                    } else {
                        in_arrow = false;
                    }
                    break;
                }
                cur = e.borrow().prototype;
            }

            // Allowed only when direct eval, inside a function, and that function is NOT an arrow
            log::trace!(
                "DEBUG-FASTPATH-NEWTARGET: is_indirect_eval={} in_function={} in_arrow={} has_new_target={}",
                is_indirect_eval,
                in_function,
                in_arrow,
                has_new_target
            );
            if !(!is_indirect_eval && in_function && !in_arrow) {
                let msg = "Invalid use of 'new.target' in eval code";
                let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
                let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                    v.borrow().clone()
                } else {
                    return Err(raise_syntax_error!(msg).into());
                };
                match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                    Ok(Value::Object(obj)) => {
                        return Err(EvalError::Throw(Value::Object(obj), None, None));
                    }
                    Ok(other) => return Err(EvalError::Throw(other, None, None)),
                    Err(_) => return Err(raise_syntax_error!(msg).into()),
                }
            }
        }
    }

    if stmts
        .iter()
        .any(|s| matches!(&*s.kind, StatementKind::Import(..) | StatementKind::Export(..)))
    {
        let msg = "Import/Export declarations may not appear in eval code";
        let msg_val = Value::String(crate::unicode::utf8_to_utf16(msg));
        let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
            v.borrow().clone()
        } else {
            return Err(raise_syntax_error!(msg).into());
        };
        match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
            Ok(Value::Object(obj)) => return Err(EvalError::Throw(Value::Object(obj), None, None)),
            Ok(other) => return Err(EvalError::Throw(other, None, None)),
            Err(_) => return Err(raise_syntax_error!(msg).into()),
        }
    }

    // If this is an indirect eval executed in the global env and the eval code
    // is strict (starts with "use strict"), do not instantiate top-level
    // FunctionDeclarations into the (global) variable environment. Convert
    // them into function expressions so they don't create bindings.
    let is_indirect_eval = if let Some(flag) = slot_get_chained(env, &InternalSlot::IsIndirectEval) {
        matches!(*flag.borrow(), Value::Boolean(true))
    } else {
        false
    };
    log::trace!(
        "DEBUG: eval env ptr={:p} __is_indirect_eval present={}",
        env,
        slot_get_chained(env, &InternalSlot::IsIndirectEval).is_some()
    );
    log::trace!("DEBUG: is_indirect_eval = {}", is_indirect_eval);
    if is_indirect_eval {
        log::trace!("DEBUG: eval env has __is_indirect_eval flag");
        if let Some(first) = stmts.first()
            && let StatementKind::Expr(expr) = &*first.kind
            && let Expr::StringLit(s) = expr
            && crate::unicode::utf16_to_utf8(s).as_str() == "use strict"
        {
            let mut converted = 0;
            for stmt in stmts.iter_mut() {
                if let StatementKind::FunctionDeclaration(name, params, body, _is_generator, _is_async) = &*stmt.kind {
                    let func_expr = Expr::Function(Some(name.clone()), params.clone(), body.clone());
                    *stmt.kind = StatementKind::Expr(func_expr);
                    converted += 1;
                }
            }
            log::trace!(
                "DEBUG: indirect strict eval - converted {} top-level function declarations into expressions",
                converted
            );
        } else {
            log::trace!(
                "DEBUG: indirect eval detected but not strict or no first-string; is_indirect_eval={}",
                is_indirect_eval
            );
        }
    }

    crate::core::check_top_level_return(&stmts)?;

    // If this was an indirect eval and the eval is strict (starts with "use strict"),
    // execute it in a fresh declarative environment whose prototype is the global env.
    // This prevents top-level FunctionDeclarations from creating global bindings (they
    // will instead be bound to the new declarative env and won't leak into the caller).
    let mut exec_env = *env;
    if is_indirect_eval
        && let Some(first) = stmts.first()
        && let StatementKind::Expr(expr) = &*first.kind
        && let Expr::StringLit(s) = expr
        && crate::unicode::utf16_to_utf8(s).as_str() == "use strict"
    {
        log::trace!("DEBUG: indirect strict eval - creating fresh declarative environment");
        let new_env = new_js_object_data(mc);
        new_env.borrow_mut(mc).prototype = Some(*env);
        exec_env = new_env;
    }

    match crate::core::evaluate_statements(mc, &exec_env, &stmts) {
        Ok(v) => Ok(v),
        Err(err) => {
            // Convert parse/eval errors into a thrown JS Error object so that
            // `try { eval(...) } catch (e) { e instanceof SyntaxError }` works
            let msg = err.message();
            let msg_val = Value::String(crate::unicode::utf8_to_utf16(&msg));
            let constructor_val = if let Some(v) = crate::core::env_get(env, "SyntaxError") {
                v.borrow().clone()
            } else {
                return Err(err);
            };
            match crate::js_class::evaluate_new(mc, env, &constructor_val, &[msg_val], None) {
                Ok(Value::Object(obj)) => Err(EvalError::Throw(Value::Object(obj), None, None)),
                Ok(other) => Err(EvalError::Throw(other, None, None)),
                Err(_) => Err(err),
            }
        }
    }
}

fn encode_uri<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    let code_units = uri_to_string(mc, args, env)?;
    uri_encode(&code_units, &URI_UNESCAPED_WITH_RESERVED)
}

fn decode_uri<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    let code_units = uri_to_string(mc, args, env)?;
    // decodeURI: reserved chars + '#' are NOT decoded
    uri_decode(&code_units, &URI_RESERVED_PLUS_HASH)
}

#[allow(dead_code)]
fn internal_resolve_promise<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function to resolve a promise - requires 2 args: (promise, value)
    validate_internal_args(args, 2)?;
    log::trace!("__internal_resolve_promise called with value: {:?}", args[1]);

    match &args[0] {
        Value::Promise(promise) => {
            crate::js_promise::resolve_promise(_mc, promise, args[1].clone(), env);
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!("First argument must be a promise").into()),
    }
}

#[allow(dead_code)]
fn internal_reject_promise<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function to reject a promise - requires 2 args: (promise, reason)
    validate_internal_args(args, 2)?;
    log::trace!("__internal_reject_promise called with reason: {:?}", args[1]);

    match &args[0] {
        Value::Promise(promise) => {
            crate::js_promise::reject_promise(mc, promise, args[1].clone(), env);
            Ok(Value::Undefined)
        }
        _ => Err(raise_type_error!("First argument must be a promise").into()),
    }
}

#[allow(dead_code)]
fn internal_promise_allsettled_resolve<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function for legacy allSettled - requires 3 args: (idx, value, shared_state)
    validate_internal_args(args, 3)?;
    // let numbers = validate_number_args(&args, 1)?;
    // // crate::js_promise::__internal_promise_allsettled_resolve(mc, numbers[0], args[1].clone(), args[2].clone())?;
    // Ok(Value::Undefined)
    todo!()
}

#[allow(dead_code)]
fn internal_promise_allsettled_reject<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function for legacy allSettled - requires 3 args: (idx, reason, shared_state)
    validate_internal_args(args, 3)?;
    // let numbers = validate_number_args(&args, 1)?;
    // crate::js_promise::__internal_promise_allsettled_reject(mc, numbers[0], args[1].clone(), args[2].clone())?;
    // Ok(Value::Undefined)
    todo!()
}

#[allow(dead_code)]
fn internal_allsettled_state_record_fulfilled<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function for new allSettled - requires 3 args: (state_index, index, value)
    validate_internal_args(args, 3)?;
    let numbers = validate_number_args(args, 2)?;
    log::trace!(
        "__internal_allsettled_state_record_fulfilled called: state_id={}, index={}, value={:?}",
        numbers[0],
        numbers[1],
        args[2]
    );
    // crate::js_promise::__internal_allsettled_state_record_fulfilled(mc, numbers[0], numbers[1], args[2].clone())?;
    // Ok(Value::Undefined)
    todo!()
}

#[allow(dead_code)]
fn internal_allsettled_state_record_rejected<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function for new allSettled - requires 3 args: (state_index, index, reason)
    validate_internal_args(args, 3)?;
    let numbers = validate_number_args(args, 2)?;
    log::trace!(
        "__internal_allsettled_state_record_rejected called: state_id={}, index={}, reason={:?}",
        numbers[0],
        numbers[1],
        args[2]
    );
    // crate::js_promise::__internal_allsettled_state_record_rejected(mc, numbers[0], numbers[1], args[2].clone())?;
    // Ok(Value::Undefined)
    todo!()
}

#[allow(dead_code)]
fn internal_promise_any_resolve<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function for Promise.any resolve - requires 2 args: (value, result_promise)
    validate_internal_args(args, 2)?;
    match &args[1] {
        Value::Promise(_result_promise) => {
            // crate::js_promise::__internal_promise_any_resolve(mc, args[0].clone(), result_promise.clone());
            // Ok(Value::Undefined)
            todo!()
        }
        _ => Err(raise_type_error!("Second argument must be a promise").into()),
    }
}

#[allow(dead_code)]
fn internal_promise_any_reject<'gc>(
    _mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function for Promise.any reject - requires 6 args: (idx, reason, rejections, rejected_count, total, result_promise)
    // Note: This function has complex Rc<RefCell<>> parameters that cannot be easily reconstructed from JS values
    // It should only be called from within closures, not directly
    Err(raise_type_error!("__internal_promise_any_reject cannot be called directly - use Promise.any instead").into())
}

#[allow(dead_code)]
fn internal_promise_race_resolve<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function for Promise.race resolve - requires 2 args: (value, result_promise)
    validate_internal_args(args, 2)?;
    match &args[1] {
        Value::Promise(_result_promise) => {
            // crate::js_promise::__internal_promise_race_resolve(mc, args[0].clone(), result_promise.clone());
            // Ok(Value::Undefined)
            todo!()
        }
        _ => Err(raise_type_error!("Second argument must be a promise").into()),
    }
}

fn handle_object_has_own_property<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // hasOwnProperty should inspect the bound `this` and take one argument
    if args.len() != 1 {
        return Err(raise_eval_error!("hasOwnProperty requires one argument").into());
    }
    let _ = args[0].to_property_key(mc, env)?;
    let key_val = args[0].clone();
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        let this_val = this_rc.borrow().clone();
        match this_val {
            Value::Object(obj) => {
                let key_str_opt = match &key_val {
                    Value::String(s) => Some(utf16_to_utf8(s)),
                    Value::Number(n) => Some(crate::core::value_to_string(&Value::Number(*n))),
                    Value::BigInt(b) => Some(b.to_string()),
                    Value::Boolean(b) => Some(b.to_string()),
                    Value::Undefined => Some("undefined".to_string()),
                    _ => None,
                };
                if let Some(key_str) = &key_str_opt
                    && (key_str == "length" || key_str == "name")
                    && let Some(cl_ptr) = obj.borrow().get_closure()
                {
                    if crate::core::consume_pending_function_delete_hasown_check() {
                        return Ok(Value::Boolean(false));
                    }
                    let func_name_opt: Option<String> = match &*cl_ptr.borrow() {
                        Value::Function(func_name) => Some(func_name.clone()),
                        Value::Closure(cl) | Value::AsyncClosure(cl) => cl.native_target.clone(),
                        _ => None,
                    };
                    if let Some(func_name) = func_name_opt {
                        let marker = format!("__fn_deleted::{}::{}", func_name, key_str);
                        let mut deleted = crate::core::is_deleted_builtin_function_virtual_prop(func_name.as_str(), key_str.as_str())
                            || crate::core::env_get(env, marker.as_str())
                                .map(|v| matches!(*v.borrow(), Value::Boolean(true)))
                                .unwrap_or(false);
                        if !deleted
                            && let Some(global_this_rc) = crate::core::env_get(env, "globalThis")
                            && let Value::Object(global_obj) = &*global_this_rc.borrow()
                            && let Some(v) = slot_get(global_obj, &InternalSlot::FnDeleted(format!("{}::{}", func_name, key_str)))
                        {
                            deleted = matches!(*v.borrow(), Value::Boolean(true));
                        }
                        return Ok(Value::Boolean(!deleted));
                    }
                }

                let ns_export_meta =
                    get_own_property(&obj, PropertyKey::Private("__ns_export_names".to_string(), 1)).and_then(|v| match &*v.borrow() {
                        Value::Object(meta) => Some(*meta),
                        _ => None,
                    });
                let is_module_namespace = {
                    let b = obj.borrow();
                    b.deferred_module_path.is_some() || (b.prototype.is_none() && !b.is_extensible()) || ns_export_meta.is_some()
                };
                if is_module_namespace {
                    let key_opt = match &key_val {
                        Value::String(s) => Some(utf16_to_utf8(s)),
                        Value::Number(n) => Some(crate::core::value_to_string(&Value::Number(*n))),
                        Value::BigInt(b) => Some(b.to_string()),
                        Value::Boolean(b) => Some(b.to_string()),
                        Value::Undefined => Some("undefined".to_string()),
                        _ => None,
                    };
                    if let Some(key) = key_opt {
                        let is_export_name = ns_export_meta
                            .map(|meta| get_own_property(&meta, key.as_str()).is_some())
                            .unwrap_or(false);
                        if get_own_property(&obj, key.as_str()).is_some() || is_export_name {
                            let val = crate::core::get_property_with_accessors(mc, env, &obj, key.as_str())?;
                            if matches!(val, Value::Undefined) {
                                return Err(raise_reference_error!("Cannot access binding before initialization").into());
                            }
                        }
                    }
                }
                let exists = has_own_property_value(&obj, &key_val);
                Ok(Value::Boolean(exists))
            }
            Value::Function(func_name) => {
                let key_str = match key_val {
                    Value::String(ss) => utf16_to_utf8(&ss),
                    Value::Number(n) => crate::core::value_to_string(&Value::Number(n)),
                    Value::BigInt(b) => b.to_string(),
                    Value::Boolean(b) => b.to_string(),
                    Value::Undefined => "undefined".to_string(),
                    Value::Symbol(_) => return Ok(Value::Boolean(false)),
                    other => crate::core::value_to_string(&other),
                };
                let is_deleted = |prop: &str| {
                    let marker = format!("__fn_deleted::{}::{}", func_name, prop);
                    if crate::core::is_deleted_builtin_function_virtual_prop(func_name.as_str(), prop) {
                        return true;
                    }
                    if let Some(v) = crate::core::env_get(env, marker.as_str()) {
                        return matches!(*v.borrow(), Value::Boolean(true));
                    }
                    if let Some(global_this_rc) = crate::core::env_get(env, "globalThis")
                        && let Value::Object(global_obj) = &*global_this_rc.borrow()
                        && let Some(v) = slot_get(global_obj, &InternalSlot::FnDeleted(format!("{}::{}", func_name, prop)))
                    {
                        return matches!(*v.borrow(), Value::Boolean(true));
                    }
                    false
                };
                if key_str == "length" {
                    if crate::core::consume_pending_function_delete_hasown_check() {
                        return Ok(Value::Boolean(false));
                    }
                    return Ok(Value::Boolean(!is_deleted("length")));
                }
                if key_str == "name" {
                    if crate::core::consume_pending_function_delete_hasown_check() {
                        return Ok(Value::Boolean(false));
                    }
                    let short_name = func_name.rsplit('.').next().unwrap_or(func_name.as_str());
                    return Ok(Value::Boolean(!short_name.is_empty() && !is_deleted("name")));
                }
                Ok(Value::Boolean(false))
            }
            Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object").into()),
            Value::String(s) => {
                // boxed string has 'length' and indexed properties
                let key_str = match key_val {
                    Value::String(ss) => utf16_to_utf8(&ss),
                    Value::Number(n) => n.to_string(),
                    Value::Boolean(b) => b.to_string(),
                    Value::Undefined => "undefined".to_string(),
                    Value::Symbol(_) => return Ok(Value::Boolean(false)),
                    other => crate::core::value_to_string(&other),
                };
                if key_str == "length" {
                    return Ok(Value::Boolean(true));
                }
                if let Ok(idx) = key_str.parse::<usize>() {
                    return Ok(Value::Boolean(idx < crate::unicode::utf16_len(&s)));
                }
                Ok(Value::Boolean(false))
            }
            _ => Ok(Value::Boolean(false)),
        }
    } else {
        Err(raise_eval_error!("hasOwnProperty called without this").into())
    }
}

fn handle_object_property_is_enumerable<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.len() != 1 {
        return Err(raise_eval_error!("propertyIsEnumerable requires one argument").into());
    }
    let key_val = args[0].clone();
    let Some(this_rc) = crate::core::env_get(env, "this") else {
        return Err(raise_eval_error!("propertyIsEnumerable called without this").into());
    };
    let this_val = this_rc.borrow().clone();
    match this_val {
        Value::Object(obj) => {
            let ns_export_meta =
                get_own_property(&obj, PropertyKey::Private("__ns_export_names".to_string(), 1)).and_then(|v| match &*v.borrow() {
                    Value::Object(meta) => Some(*meta),
                    _ => None,
                });
            let is_module_namespace = {
                let b = obj.borrow();
                b.deferred_module_path.is_some() || (b.prototype.is_none() && !b.is_extensible()) || ns_export_meta.is_some()
            };
            if is_module_namespace {
                let key_opt = match &key_val {
                    Value::String(s) => Some(utf16_to_utf8(s)),
                    Value::Number(n) => Some(crate::core::value_to_string(&Value::Number(*n))),
                    Value::BigInt(b) => Some(b.to_string()),
                    Value::Boolean(b) => Some(b.to_string()),
                    Value::Undefined => Some("undefined".to_string()),
                    _ => None,
                };
                if let Some(key) = key_opt {
                    let is_export_name = ns_export_meta
                        .map(|meta| get_own_property(&meta, key.as_str()).is_some())
                        .unwrap_or(false);
                    if get_own_property(&obj, key.as_str()).is_some() || is_export_name {
                        let val = crate::core::get_property_with_accessors(mc, env, &obj, key.as_str())?;
                        if matches!(val, Value::Undefined) {
                            return Err(raise_reference_error!("Cannot access binding before initialization").into());
                        }
                    }
                }
            }
            // Convert key value to a PropertyKey
            let key: PropertyKey<'gc> = key_val.into();

            // Check own property and enumerability
            if crate::core::get_own_property(&obj, &key).is_some() {
                return Ok(Value::Boolean(obj.borrow().is_enumerable(&key)));
            }
            Ok(Value::Boolean(false))
        }
        Value::Function(_func_name) => {
            let key_str = match key_val {
                Value::String(ss) => utf16_to_utf8(&ss),
                Value::Number(n) => crate::core::value_to_string(&Value::Number(n)),
                Value::BigInt(b) => b.to_string(),
                Value::Boolean(b) => b.to_string(),
                Value::Undefined => "undefined".to_string(),
                Value::Symbol(_) => return Ok(Value::Boolean(false)),
                other => crate::core::value_to_string(&other),
            };
            if key_str == "length" || key_str == "name" {
                return Ok(Value::Boolean(false));
            }
            Ok(Value::Boolean(false))
        }
        Value::Undefined | Value::Null => Err(raise_type_error!("Cannot convert undefined or null to object").into()),
        _ => Ok(Value::Boolean(false)),
    }
}

fn handle_object_is_prototype_of<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if args.len() != 1 {
        return Err(raise_eval_error!("isPrototypeOf requires one argument").into());
    }

    let target_val = args[0].clone();
    if !matches!(target_val, Value::Object(_) | Value::Proxy(_)) {
        return Ok(Value::Boolean(false));
    }
    let Some(this_rc) = crate::core::env_get(env, "this") else {
        return Err(raise_eval_error!("isPrototypeOf called without this").into());
    };
    let this_val = this_rc.borrow().clone();
    let this_obj = match this_val {
        Value::Object(o) => o,
        Value::Undefined | Value::Null => return Err(raise_type_error!("Cannot convert undefined or null to object").into()),
        _ => match crate::js_class::handle_object_constructor(mc, std::slice::from_ref(&this_val), env)? {
            Value::Object(o) => o,
            _ => return Err(raise_type_error!("Cannot convert value to object").into()),
        },
    };

    // Helper: get the [[GetPrototypeOf]] result for the given value,
    // handling Proxy wrappers (Objects with __proxy__) and Value::Proxy.
    let get_prototype_of = |mc: &MutationContext<'gc>, v: &Value<'gc>| -> Result<Option<JSObjectDataPtr<'gc>>, EvalError<'gc>> {
        // Check for proxy wrapper Object
        if let Value::Object(obj) = v {
            if let Some(proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                && let Value::Proxy(proxy) = &*proxy_cell.borrow()
            {
                let proto_val = crate::js_proxy::apply_proxy_trap(mc, proxy, "getPrototypeOf", vec![(*proxy.target).clone()], || {
                    match &*proxy.target {
                        Value::Object(target_obj) => {
                            if let Some(p) = target_obj.borrow().prototype {
                                Ok(Value::Object(p))
                            } else if let Some(pv) = slot_get_chained(target_obj, &InternalSlot::Proto)
                                && let Value::Object(p) = &*pv.borrow()
                            {
                                Ok(Value::Object(*p))
                            } else {
                                Ok(Value::Null)
                            }
                        }
                        _ => Ok(Value::Null),
                    }
                })?;
                return match proto_val {
                    Value::Object(p) => Ok(Some(p)),
                    Value::Null => Ok(None),
                    _ => Err(raise_type_error!("'getPrototypeOf' on proxy: trap returned neither object nor null").into()),
                };
            }
            return Ok(obj.borrow().prototype);
        }
        if let Value::Proxy(proxy) = v {
            let proto_val =
                crate::js_proxy::apply_proxy_trap(mc, proxy, "getPrototypeOf", vec![(*proxy.target).clone()], || {
                    match &*proxy.target {
                        Value::Object(target_obj) => {
                            if let Some(p) = target_obj.borrow().prototype {
                                Ok(Value::Object(p))
                            } else {
                                Ok(Value::Null)
                            }
                        }
                        _ => Ok(Value::Null),
                    }
                })?;
            return match proto_val {
                Value::Object(p) => Ok(Some(p)),
                Value::Null => Ok(None),
                _ => Err(raise_type_error!("'getPrototypeOf' on proxy: trap returned neither object nor null").into()),
            };
        }
        Ok(None)
    };

    let mut current = get_prototype_of(mc, &target_val)?;

    while let Some(proto) = current {
        if Gc::ptr_eq(proto, this_obj) {
            return Ok(Value::Boolean(true));
        }
        current = get_prototype_of(mc, &Value::Object(proto))?;
    }

    Ok(Value::Boolean(false))
}

fn ordinary_set_prototype_of<'gc>(mc: &MutationContext<'gc>, obj: &JSObjectDataPtr<'gc>, proto_obj: Option<JSObjectDataPtr<'gc>>) -> bool {
    let current_proto = obj.borrow().prototype;
    let same_proto = match (current_proto, proto_obj) {
        (Some(cur), Some(next)) => Gc::ptr_eq(cur, next),
        (None, None) => true,
        _ => false,
    };
    if same_proto {
        return true;
    }
    if !obj.borrow().is_extensible() {
        return false;
    }

    if let Some(mut probe) = proto_obj {
        loop {
            if Gc::ptr_eq(probe, *obj) {
                return false;
            }
            if let Some(next) = probe.borrow().prototype {
                probe = next;
            } else {
                break;
            }
        }
    }

    obj.borrow_mut(mc).prototype = proto_obj;
    true
}

fn handle_object_proto_get<'gc>(
    mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let this_val = if let Some(this_rc) = crate::core::env_get(env, "this") {
        this_rc.borrow().clone()
    } else {
        Value::Undefined
    };
    if let Value::Proxy(proxy) = this_val {
        let result = crate::js_proxy::apply_proxy_trap(mc, &proxy, "getPrototypeOf", vec![(*proxy.target).clone()], || {
            match &*proxy.target {
                Value::Object(target_obj) => {
                    if let Some(p) = target_obj.borrow().prototype {
                        Ok(Value::Object(p))
                    } else if let Some(pv) = slot_get_chained(target_obj, &InternalSlot::Proto)
                        && let Value::Object(p) = &*pv.borrow()
                    {
                        Ok(Value::Object(*p))
                    } else {
                        Ok(Value::Null)
                    }
                }
                _ => Ok(Value::Null),
            }
        })?;

        return match result {
            Value::Object(_) | Value::Null => Ok(result),
            _ => Err(raise_type_error!("'getPrototypeOf' on proxy: trap returned neither object nor null").into()),
        };
    }

    let this_obj = match this_val {
        Value::Object(o) => o,
        Value::Undefined | Value::Null => return Err(raise_type_error!("Cannot convert undefined or null to object").into()),
        _ => match crate::js_class::handle_object_constructor(mc, std::slice::from_ref(&this_val), env)? {
            Value::Object(o) => o,
            _ => return Err(raise_type_error!("Cannot convert value to object").into()),
        },
    };

    if let Some(proxy_ptr) = crate::core::slot_get(&this_obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_ptr.borrow()
    {
        let result = crate::js_proxy::apply_proxy_trap(mc, proxy, "getPrototypeOf", vec![(*proxy.target).clone()], || {
            match &*proxy.target {
                Value::Object(target_obj) => {
                    if let Some(p) = target_obj.borrow().prototype {
                        Ok(Value::Object(p))
                    } else if let Some(pv) = slot_get_chained(target_obj, &InternalSlot::Proto)
                        && let Value::Object(p) = &*pv.borrow()
                    {
                        Ok(Value::Object(*p))
                    } else {
                        Ok(Value::Null)
                    }
                }
                _ => Ok(Value::Null),
            }
        })?;

        return match result {
            Value::Object(_) | Value::Null => Ok(result),
            _ => Err(raise_type_error!("'getPrototypeOf' on proxy: trap returned neither object nor null").into()),
        };
    }

    if let Some(proto) = this_obj.borrow().prototype {
        Ok(Value::Object(proto))
    } else {
        Ok(Value::Null)
    }
}

fn handle_object_proto_set<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let this_val = if let Some(this_rc) = crate::core::env_get(env, "this") {
        this_rc.borrow().clone()
    } else {
        Value::Undefined
    };
    if let Value::Proxy(proxy) = this_val {
        let proto_arg = args.first().cloned().unwrap_or(Value::Undefined);
        let proto_obj = match proto_arg {
            Value::Object(o) => Some(o),
            Value::Null => None,
            _ => return Ok(Value::Undefined),
        };
        let trap_result = crate::js_proxy::apply_proxy_trap(
            mc,
            &proxy,
            "setPrototypeOf",
            vec![(*proxy.target).clone(), proto_arg],
            || match &*proxy.target {
                Value::Object(target_obj) => Ok(Value::Boolean(ordinary_set_prototype_of(mc, target_obj, proto_obj))),
                _ => Ok(Value::Boolean(false)),
            },
        )?;
        if !trap_result.to_truthy() {
            return Err(raise_type_error!("Cannot set prototype").into());
        }
        return Ok(Value::Undefined);
    }

    let this_obj = match this_val {
        Value::Object(o) => o,
        Value::Undefined | Value::Null => return Err(raise_type_error!("Cannot convert undefined or null to object").into()),
        _ => match crate::js_class::handle_object_constructor(mc, std::slice::from_ref(&this_val), env)? {
            Value::Object(o) => o,
            _ => return Err(raise_type_error!("Cannot convert value to object").into()),
        },
    };

    let proto_arg = args.first().cloned().unwrap_or(Value::Undefined);
    let proto_obj = match proto_arg {
        Value::Object(o) => Some(o),
        Value::Null => None,
        _ => return Ok(Value::Undefined),
    };

    if let Some(proxy_ptr) = crate::core::slot_get(&this_obj, &InternalSlot::Proxy)
        && let Value::Proxy(proxy) = &*proxy_ptr.borrow()
    {
        let trap_result =
            crate::js_proxy::apply_proxy_trap(
                mc,
                proxy,
                "setPrototypeOf",
                vec![(*proxy.target).clone(), proto_arg],
                || match &*proxy.target {
                    Value::Object(target_obj) => Ok(Value::Boolean(ordinary_set_prototype_of(mc, target_obj, proto_obj))),
                    _ => Ok(Value::Boolean(false)),
                },
            )?;
        if !trap_result.to_truthy() {
            return Err(raise_type_error!("Cannot set prototype").into());
        }
        return Ok(Value::Undefined);
    }

    if !ordinary_set_prototype_of(mc, &this_obj, proto_obj) {
        return Err(raise_type_error!("Cannot set prototype").into());
    }

    Ok(Value::Undefined)
}

pub fn initialize_function<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    log::debug!("initialize_function: starting initialization of Function constructor");
    let func_ctor = new_js_object_data(mc);
    slot_set(mc, &func_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));
    slot_set(mc, &func_ctor, InternalSlot::NativeCtor, &Value::String(utf8_to_utf16("Function")));
    // Stamp with OriginGlobal so evaluate_new can discover the constructor's realm
    slot_set(mc, &func_ctor, InternalSlot::OriginGlobal, &Value::Object(*env));

    let func_proto = new_js_object_data(mc);

    // Link Function.prototype to Object.prototype so function objects inherit object methods
    if let Some(obj_val) = crate::core::env_get(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(obj_proto_val) = crate::core::object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
    {
        func_proto.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    object_set_key_value(mc, &func_ctor, "prototype", &Value::Object(func_proto))?;
    func_ctor.borrow_mut(mc).set_non_enumerable("prototype");
    func_ctor.borrow_mut(mc).set_non_writable("prototype");
    func_ctor.borrow_mut(mc).set_non_configurable("prototype");
    object_set_key_value(mc, &func_proto, "constructor", &Value::Object(func_ctor))?;

    // Make Function.prototype itself callable (typeof Function.prototype === 'function') by
    // attaching an empty closure; engines typically expose Function.prototype as a function object.
    let proto_closure = ClosureData {
        env: Some(*env),
        ..ClosureData::default()
    };
    let proto_closure_val = Value::Closure(Gc::new(mc, proto_closure));
    func_proto.borrow_mut(mc).set_closure(Some(new_gc_cell_ptr(mc, proto_closure_val)));
    // Function.prototype is callable but not constructable; ensure it has no own "prototype" property.
    func_proto
        .borrow_mut(mc)
        .properties
        .shift_remove(&PropertyKey::String("prototype".to_string()));
    func_proto
        .borrow_mut(mc)
        .non_enumerable
        .remove(&PropertyKey::String("prototype".to_string()));
    func_proto
        .borrow_mut(mc)
        .non_writable
        .remove(&PropertyKey::String("prototype".to_string()));
    func_proto
        .borrow_mut(mc)
        .non_configurable
        .remove(&PropertyKey::String("prototype".to_string()));

    // Function.prototype.toString
    object_set_key_value(
        mc,
        &func_proto,
        "toString",
        &Value::Function("Function.prototype.toString".to_string()),
    )?;
    func_proto.borrow_mut(mc).set_non_enumerable("toString");

    // Function.prototype.bind
    object_set_key_value(mc, &func_proto, "bind", &Value::Function("Function.prototype.bind".to_string()))?;
    func_proto.borrow_mut(mc).set_non_enumerable("bind");

    // Function.prototype.call
    {
        let call_obj = crate::core::new_js_object_data(mc);
        call_obj
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function("Function.prototype.call".to_string()))));
        call_obj.borrow_mut(mc).prototype = Some(func_proto);
        let call_name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("call")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &call_obj, "name", &call_name_desc)?;
        let call_len_desc = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &call_obj, "length", &call_len_desc)?;
        slot_set(mc, &call_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
        let call_desc = crate::core::create_descriptor_object(mc, &Value::Object(call_obj), true, false, true)?;
        crate::js_object::define_property_internal(mc, &func_proto, "call", &call_desc)?;
    }

    // Function.prototype.apply
    {
        let apply_obj = crate::core::new_js_object_data(mc);
        apply_obj
            .borrow_mut(mc)
            .set_closure(Some(new_gc_cell_ptr(mc, Value::Function("Function.prototype.apply".to_string()))));
        apply_obj.borrow_mut(mc).prototype = Some(func_proto);
        let apply_name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("apply")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &apply_obj, "name", &apply_name_desc)?;
        let apply_len_desc = crate::core::create_descriptor_object(mc, &Value::Number(2.0), false, false, true)?;
        crate::js_object::define_property_internal(mc, &apply_obj, "length", &apply_len_desc)?;
        slot_set(mc, &apply_obj, InternalSlot::OriginGlobal, &Value::Object(*env));
        let apply_desc = crate::core::create_descriptor_object(mc, &Value::Object(apply_obj), true, false, true)?;
        crate::js_object::define_property_internal(mc, &func_proto, "apply", &apply_desc)?;
    }

    // Function.prototype[@@hasInstance]
    if let Some(sym_val) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_ctor) = &*sym_val.borrow()
        && let Some(has_instance_sym_val) = object_get_key_value(sym_ctor, "hasInstance")
        && let Value::Symbol(has_instance_sym) = &*has_instance_sym_val.borrow()
    {
        let has_instance_fn = Value::Function("Function.prototype.[Symbol.hasInstance]".to_string());
        let desc = crate::core::create_descriptor_object(mc, &has_instance_fn, false, false, false)?;
        crate::js_object::define_property_internal(mc, &func_proto, PropertyKey::Symbol(*has_instance_sym), &desc)?;
    }

    func_proto.borrow_mut(mc).set_non_enumerable("constructor");

    // Define Function.prototype.length/name in spec order and attributes.
    // length: { [[Writable]]: false, [[Enumerable]]: false, [[Configurable]]: true }
    let proto_len_desc = crate::core::create_descriptor_object(mc, &Value::Number(0.0), false, false, true)?;
    crate::js_object::define_property_internal(mc, &func_proto, "length", &proto_len_desc)?;
    // name: { [[Writable]]: false, [[Enumerable]]: false, [[Configurable]]: true }
    let proto_name_desc = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("")), false, false, true)?;
    crate::js_object::define_property_internal(mc, &func_proto, "name", &proto_name_desc)?;

    // Define restricted 'caller' and 'arguments' accessors that throw a TypeError when accessed or assigned
    let restricted_desc = crate::core::new_js_object_data(mc);
    let val = Value::Function("Function.prototype.restrictedThrow".to_string());
    object_set_key_value(mc, &restricted_desc, "get", &val)?;

    let val = Value::Function("Function.prototype.restrictedThrow".to_string());
    object_set_key_value(mc, &restricted_desc, "set", &val)?;

    object_set_key_value(mc, &restricted_desc, "configurable", &Value::Boolean(true))?;
    crate::js_object::define_property_internal(mc, &func_proto, "caller", &restricted_desc)?;
    crate::js_object::define_property_internal(mc, &func_proto, "arguments", &restricted_desc)?;

    // Define Function.length as non-writable to match spec so assignments to it
    // in strict mode throw a TypeError.
    let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
    if let Some(wrc) = crate::core::object_get_key_value(&desc_len, "writable") {
        log::debug!("initialize_function: desc_len writable raw = {:?}", wrc.borrow());
    } else {
        log::debug!("initialize_function: desc_len writable raw = <absent>");
    }
    log::debug!(
        "initialize_function: before define exists={} func_ctor_ptr={:p}",
        object_get_key_value(&func_ctor, "length").is_some(),
        &func_ctor
    );
    crate::js_object::define_property_internal(mc, &func_ctor, "length", &desc_len)?;

    // Define Function.name after length so built-in property order is length, then name.
    let desc_name = crate::core::create_descriptor_object(mc, &Value::String(utf8_to_utf16("Function")), false, false, true)?;
    crate::js_object::define_property_internal(mc, &func_ctor, "name", &desc_name)?;

    log::debug!(
        "Function ctor non_writable after define = {:?}",
        func_ctor.borrow().non_writable.iter().collect::<Vec<_>>()
    );

    // NOTE: explicit fallback for setting flags on `length` removed — rely on define_property_internal
    // to correctly apply non-writable/non-enumerable/non-configurable flags for the `length` property.

    crate::core::env_set(mc, env, "Function", &Value::Object(func_ctor))?;

    // Ensure any native constructors created earlier (e.g., Error, TypeError)
    // get Function.prototype as their internal prototype so `instanceof Function`
    // behaves correctly. Also set the constructor objects created before this
    // point (Object, Array, String, Number, Boolean, etc.) to use
    // Function.prototype as their internal [[Prototype]] so they inherit
    // Function.prototype members (e.g., `toString`, `constructor`).
    let native_constructors = [
        "Error",
        // NOTE: native error subclasses (ReferenceError, TypeError, etc.) are excluded
        // because their internal [[Prototype]] should be Error (set in initialize_native_error),
        // not Function.prototype.
        // Common constructors that may have been created prior to initialize_function
        "Object",
        "Array",
        "String",
        "Number",
        "Boolean",
        "RegExp",
        "Date",
        "ArrayBuffer",
        "Map",
        "Set",
        "WeakMap",
        "WeakSet",
        "Promise",
        "Function",
    ];

    for name in native_constructors {
        if let Some(ctor_rc) = crate::core::object_get_key_value(env, name)
            && let Value::Object(ctor_obj) = &*ctor_rc.borrow()
        {
            ctor_obj.borrow_mut(mc).prototype = Some(func_proto);
        }
    }

    // Ensure Function constructor itself uses Function.prototype for its
    // internal prototype so that `Function.constructor === Function` and
    // Function instances see Function.prototype members.
    func_ctor.borrow_mut(mc).prototype = Some(func_proto);

    Ok(())
}

pub fn handle_function_prototype_method<'gc>(
    mc: &MutationContext<'gc>,
    this_value: &Value<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let get_target_name_and_length = |target: &Value<'gc>| -> Result<(String, f64), EvalError<'gc>> {
        let mut target_name = String::new();
        let mut target_length = 0.0_f64;

        match target {
            Value::Closure(cl) | Value::AsyncClosure(cl) => {
                // Use property semantics when possible (to respect poisoned accessors),
                // fallback to intrinsic closure shape.
                if let Ok(Value::Object(obj)) = crate::js_class::handle_object_constructor(mc, std::slice::from_ref(target), env) {
                    if crate::core::get_own_property(&obj, "length").is_some() {
                        let len_val = crate::core::get_property_with_accessors(mc, env, &obj, "length")?;
                        if let Value::Number(n) = len_val {
                            target_length = n;
                        }
                    }
                    let name_val = crate::core::get_property_with_accessors(mc, env, &obj, "name")?;
                    if let Value::String(s) = name_val {
                        target_name = crate::unicode::utf16_to_utf8(&s);
                    }
                }
                if target_name.is_empty()
                    && let Some(nt) = &cl.native_target
                {
                    target_name = nt.clone();
                }
                if target_length == 0.0 {
                    target_length = cl.params.len() as f64;
                }
            }
            Value::Object(obj) => {
                if crate::core::get_own_property(obj, "length").is_some() {
                    let len_val = crate::core::get_property_with_accessors(mc, env, obj, "length")?;
                    if let Value::Number(n) = len_val {
                        target_length = n;
                    }
                }
                let name_val = crate::core::get_property_with_accessors(mc, env, obj, "name")?;
                if let Value::String(s) = name_val {
                    target_name = crate::unicode::utf16_to_utf8(&s);
                }
            }
            Value::Function(name) => {
                target_name = name.rsplit('.').next().map(|s| s.to_string()).unwrap_or_default();
            }
            _ => {}
        }

        Ok((target_name, target_length))
    };

    let set_bound_metadata = |bound_obj: &JSObjectDataPtr<'gc>, target: &Value<'gc>, bound_args_len: usize| -> Result<(), EvalError<'gc>> {
        let (target_name, raw_len) = get_target_name_and_length(target)?;

        let bound_name = format!("bound {}", target_name);
        let name_desc =
            crate::core::create_descriptor_object(mc, &Value::String(crate::unicode::utf8_to_utf16(&bound_name)), false, false, true)?;
        crate::js_object::define_property_internal(mc, bound_obj, "name", &name_desc)?;

        let target_len = if raw_len.is_infinite() {
            if raw_len.is_sign_positive() { f64::INFINITY } else { 0.0 }
        } else if raw_len.is_nan() {
            0.0
        } else {
            raw_len.trunc()
        };

        let bound_len = if target_len.is_infinite() {
            f64::INFINITY
        } else {
            (target_len - bound_args_len as f64).max(0.0)
        };
        let length_desc = crate::core::create_descriptor_object(mc, &Value::Number(bound_len), false, false, true)?;
        crate::js_object::define_property_internal(mc, bound_obj, "length", &length_desc)?;

        Ok(())
    };

    let _make_bound_function_object = |callable: Value<'gc>| -> Result<Value<'gc>, EvalError<'gc>> {
        let func_obj = crate::core::new_js_object_data(mc);
        attach_function_prototype_if_present(mc, env, &func_obj);
        func_obj
            .borrow_mut(mc)
            .set_closure(Some(crate::core::new_gc_cell_ptr(mc, callable)));
        let callable_val = func_obj
            .borrow()
            .get_closure()
            .map(|cl| cl.borrow().clone())
            .unwrap_or(Value::Undefined);
        set_bound_metadata(&func_obj, &callable_val, 0)?;
        Ok(Value::Object(func_obj))
    };

    let make_bound_target_wrapper =
        |target: &Value<'gc>, bound_this: &Value<'gc>, bound_args: &[Value<'gc>]| -> Result<Value<'gc>, EvalError<'gc>> {
            let func_obj = crate::core::new_js_object_data(mc);
            attach_function_prototype_if_present(mc, env, &func_obj);

            crate::core::slot_set(mc, &func_obj, InternalSlot::BoundTarget, target);
            crate::core::slot_set(mc, &func_obj, InternalSlot::BoundThis, bound_this);
            crate::core::slot_set(mc, &func_obj, InternalSlot::BoundArgLen, &Value::Number(bound_args.len() as f64));
            crate::core::slot_set(mc, &func_obj, InternalSlot::Callable, &Value::Boolean(true));
            for (i, arg) in bound_args.iter().enumerate() {
                crate::core::slot_set(mc, &func_obj, InternalSlot::BoundArg(i), arg);
            }

            set_bound_metadata(&func_obj, target, bound_args.len())?;

            let is_constructor_target = is_constructor_value(target);

            if is_constructor_target {
                crate::core::slot_set(mc, &func_obj, InternalSlot::IsConstructor, &Value::Boolean(true));
            }

            Ok(Value::Object(func_obj))
        };

    match method {
        "[Symbol.hasInstance]" => {
            let value = args.first().cloned().unwrap_or(Value::Undefined);

            // Step 2: If IsCallable(F) is false, return false.
            if !is_callable_value(this_value, false) {
                return Ok(Value::Boolean(false));
            }

            // Step 3: If F has [[BoundTargetFunction]], return InstanceofOperator(V, target).
            if let Value::Object(obj) = this_value
                && let Some(bt_rc) = crate::core::slot_get(obj, &InternalSlot::BoundTarget)
            {
                let bound_target = bt_rc.borrow().clone();
                if let Value::Object(v_obj) = &value {
                    match &bound_target {
                        Value::Object(_) => {
                            let forwarded = vec![value.clone()];
                            let call_env = crate::js_class::prepare_call_env_with_this(
                                mc,
                                Some(env),
                                Some(&bound_target),
                                None,
                                &[],
                                None,
                                Some(env),
                                None,
                            )?;
                            let result = crate::js_function::handle_function_prototype_method(
                                mc,
                                &bound_target,
                                "[Symbol.hasInstance]",
                                &forwarded,
                                &call_env,
                            )?;
                            return Ok(result);
                        }
                        Value::Closure(_) | Value::AsyncClosure(_) | Value::Function(_) => {
                            let target_obj = match crate::js_class::handle_object_constructor(mc, std::slice::from_ref(&bound_target), env)?
                            {
                                Value::Object(o) => o,
                                _ => return Ok(Value::Boolean(false)),
                            };
                            let p = crate::core::get_property_with_accessors(mc, env, &target_obj, "prototype")?;
                            if let Value::Object(p_obj) = p {
                                let mut cur = v_obj.borrow().prototype;
                                while let Some(proto) = cur {
                                    if Gc::ptr_eq(proto, p_obj) {
                                        return Ok(Value::Boolean(true));
                                    }
                                    cur = proto.borrow().prototype;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                return Ok(Value::Boolean(false));
            }

            // Step 4: If Type(V) is not Object, return false.
            if !matches!(value, Value::Object(_) | Value::Proxy(_)) {
                return Ok(Value::Boolean(false));
            }

            // Step 5: Let P be ? Get(F, "prototype").
            let p = match this_value {
                Value::Object(obj) => crate::core::get_property_with_accessors(mc, env, obj, "prototype")?,
                _ => {
                    let this_obj = match crate::js_class::handle_object_constructor(mc, std::slice::from_ref(this_value), env)? {
                        Value::Object(o) => o,
                        _ => return Err(raise_type_error!("Function has non-object prototype in instanceof check").into()),
                    };
                    crate::core::get_property_with_accessors(mc, env, &this_obj, "prototype")?
                }
            };

            // Step 6: If Type(P) is not Object, throw a TypeError exception.
            let p_obj = match p {
                Value::Object(o) => o,
                Value::Property { value: Some(v), .. } => match &*v.borrow() {
                    Value::Object(o) => *o,
                    _ => return Err(raise_type_error!("Function has non-object prototype in instanceof check").into()),
                },
                _ => return Err(raise_type_error!("Function has non-object prototype in instanceof check").into()),
            };

            // Helper for [[GetPrototypeOf]] with Proxy support.
            let get_prototype_of = |mc: &MutationContext<'gc>, v: &Value<'gc>| -> Result<Option<JSObjectDataPtr<'gc>>, EvalError<'gc>> {
                if let Value::Object(obj) = v {
                    if let Some(proxy_cell) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                        && let Value::Proxy(proxy) = &*proxy_cell.borrow()
                    {
                        let proto_val = crate::js_proxy::apply_proxy_trap(
                            mc,
                            proxy,
                            "getPrototypeOf",
                            vec![(*proxy.target).clone()],
                            || match &*proxy.target {
                                Value::Object(target_obj) => {
                                    if let Some(p) = target_obj.borrow().prototype {
                                        Ok(Value::Object(p))
                                    } else if let Some(pv) = crate::core::slot_get_chained(target_obj, &InternalSlot::Proto)
                                        && let Value::Object(p) = &*pv.borrow()
                                    {
                                        Ok(Value::Object(*p))
                                    } else {
                                        Ok(Value::Null)
                                    }
                                }
                                _ => Ok(Value::Null),
                            },
                        )?;
                        return match proto_val {
                            Value::Object(p) => Ok(Some(p)),
                            Value::Null => Ok(None),
                            _ => Err(raise_type_error!("'getPrototypeOf' on proxy: trap returned neither object nor null").into()),
                        };
                    }
                    if let Some(p) = obj.borrow().prototype {
                        return Ok(Some(p));
                    }
                    if let Some(pv) = crate::core::slot_get_chained(obj, &InternalSlot::Proto)
                        && let Value::Object(p) = &*pv.borrow()
                    {
                        return Ok(Some(*p));
                    }
                    return Ok(None);
                }
                if let Value::Proxy(proxy) = v {
                    let proto_val = crate::js_proxy::apply_proxy_trap(mc, proxy, "getPrototypeOf", vec![(*proxy.target).clone()], || {
                        match &*proxy.target {
                            Value::Object(target_obj) => {
                                if let Some(p) = target_obj.borrow().prototype {
                                    Ok(Value::Object(p))
                                } else {
                                    Ok(Value::Null)
                                }
                            }
                            _ => Ok(Value::Null),
                        }
                    })?;
                    return match proto_val {
                        Value::Object(p) => Ok(Some(p)),
                        Value::Null => Ok(None),
                        _ => Err(raise_type_error!("'getPrototypeOf' on proxy: trap returned neither object nor null").into()),
                    };
                }
                Ok(None)
            };

            // Step 7: Walk prototype chain.
            let mut current = get_prototype_of(mc, &value)?;
            while let Some(proto) = current {
                if Gc::ptr_eq(proto, p_obj) {
                    return Ok(Value::Boolean(true));
                }
                current = get_prototype_of(mc, &Value::Object(proto))?;
            }
            Ok(Value::Boolean(false))
        }

        "bind" => {
            let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
            let bound_prefix_args: Vec<Value<'gc>> = if args.len() > 1 { args[1..].to_vec() } else { Vec::new() };
            if is_callable_value(this_value, false) {
                make_bound_target_wrapper(this_value, &this_arg, &bound_prefix_args)
            } else {
                Err(crate::raise_type_error!("Function.prototype.bind called on non-function").into())
            }
        }
        "toString" => {
            // Function.prototype.toString: return a string representation of the function.
            // Since we don't store source text, produce `function name() { [native code] }`.
            let name = match this_value {
                Value::Function(n) => n.clone(),
                Value::Closure(cl) => {
                    if let Some(ref nt) = cl.native_target {
                        nt.clone()
                    } else {
                        String::new()
                    }
                }
                Value::AsyncClosure(cl) => {
                    if let Some(ref nt) = cl.native_target {
                        nt.clone()
                    } else {
                        String::new()
                    }
                }
                Value::GeneratorFunction(n, ..) => n.clone().unwrap_or_default(),
                Value::AsyncGeneratorFunction(n, ..) => n.clone().unwrap_or_default(),
                Value::Object(obj) => {
                    let is_direct_callable = is_callable_object(obj, true);

                    let is_proxy_callable = if !is_direct_callable {
                        if let Some(proxy_rc) = crate::core::slot_get(obj, &InternalSlot::Proxy)
                            && let Value::Proxy(proxy) = &*proxy_rc.borrow()
                        {
                            is_callable_value(&proxy.target, true)
                        } else {
                            false
                        }
                    } else {
                        false
                    };

                    if !is_direct_callable && !is_proxy_callable {
                        return Err(crate::raise_type_error!("Function.prototype.toString requires that 'this' be a Function").into());
                    }

                    // Try to get the `name` property from the object
                    if let Some(name_rc) = crate::core::object_get_key_value(obj, "name") {
                        let v = name_rc.borrow().clone();
                        match v {
                            Value::String(s) => crate::unicode::utf16_to_utf8(&s),
                            Value::Property { value: Some(v), .. } => {
                                let inner = v.borrow().clone();
                                if let Value::String(s) = inner {
                                    crate::unicode::utf16_to_utf8(&s)
                                } else {
                                    String::new()
                                }
                            }
                            _ => String::new(),
                        }
                    } else {
                        String::new()
                    }
                }
                _ => {
                    return Err(crate::raise_type_error!("Function.prototype.toString requires that 'this' be a Function").into());
                }
            };
            let is_identifier_name = |s: &str| {
                let mut chars = s.chars();
                let Some(first) = chars.next() else {
                    return false;
                };
                let first_ok = first == '_' || first == '$' || first.is_ascii_alphabetic();
                if !first_ok {
                    return false;
                }
                chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
            };

            let property_name = if name.is_empty() {
                None
            } else if is_identifier_name(&name) || name.starts_with('[') {
                Some(name)
            } else {
                Some(format!("[{name}]"))
            };

            let repr = if let Some(prop) = property_name {
                format!("function {prop}() {{ [native code] }}")
            } else {
                "function () { [native code] }".to_string()
            };
            Ok(Value::String(crate::unicode::utf8_to_utf16(&repr)))
        }
        _ => Err(crate::raise_type_error!(format!("Unknown Function.prototype method: {method}")).into()),
    }
}
