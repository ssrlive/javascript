use crate::core::{
    ClosureData, EvalError, Expr, Gc, JSObjectDataPtr, MutationContext, Statement, StatementKind, Value, evaluate_expr,
    has_own_property_value, new_js_object_data, prepare_function_call_env,
};
use crate::core::{PropertyKey, object_get_key_value, object_set_key_value};
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

pub fn handle_global_function<'gc>(
    mc: &MutationContext<'gc>,
    func_name: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Handle functions that expect unevaluated expressions
    match func_name {
        "import" => return dynamic_import_function(mc, args),
        "Function" => return function_constructor(mc, args, env),
        "new" => return evaluate_new_expression(mc, args, env),
        "eval" => return evalute_eval_function(mc, args, env),
        "Date" => return handle_date_constructor(mc, args, env),
        "testWithIntlConstructors" => return test_with_intl_constructors(mc, args, env),

        "Object.prototype.valueOf" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                return crate::js_object::handle_value_of_method(mc, &this_val, args, env);
            }
            return Err(raise_eval_error!("Object.prototype.valueOf called without this").into());
        }
        "Object.prototype.toString" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                return Ok(crate::core::handle_object_prototype_to_string(mc, &this_val, env));
            }
            return Err(raise_eval_error!("Object.prototype.toString called without this").into());
        }
        "Object.prototype.hasOwnProperty" => return handle_object_has_own_property(args, env),
        "Object.prototype.propertyIsEnumerable" => return handle_object_property_is_enumerable(args, env),
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
        "parseInt" => return parse_int_function(args),
        "parseFloat" => return parse_float_function(args),
        "isNaN" => return is_nan_function(args),
        "isFinite" => return is_finite_function(args),
        "encodeURIComponent" => return encode_uri_component(args),
        "decodeURIComponent" => return decode_uri_component(args),
        "Object" => return crate::js_class::handle_object_constructor(mc, args, env),
        "BigInt" => return Ok(crate::js_bigint::bigint_constructor(mc, args, env)?),
        "Number" => return Ok(crate::js_number::number_constructor(mc, args, env)?),
        "Boolean" => return boolean_constructor(args),
        "Proxy.revocable" => return crate::js_proxy::handle_proxy_revocable(mc, args, env),
        "Proxy.__internal_revoke" => {
            // Revoke the proxy stored in the captured closure environment
            if let Some(revoke_rc) = crate::core::env_get(env, "__proxy_wrapper") {
                let revoke_val = revoke_rc.borrow().clone();
                if let Value::Object(wrapper_obj) = revoke_val {
                    // Get the stored __proxy__ property on the wrapper
                    if let Some(proxy_prop) = object_get_key_value(&wrapper_obj, "__proxy__") {
                        let proxy_val = proxy_prop.borrow().clone();
                        if let Value::Proxy(p) = proxy_val {
                            // Create a new proxy with revoked=true and same target/handler
                            let new_proxy = Gc::new(
                                mc,
                                crate::core::JSProxy {
                                    target: p.target.clone(),
                                    handler: p.handler.clone(),
                                    revoked: true,
                                },
                            );
                            *proxy_prop.borrow_mut(mc) = Value::Proxy(new_proxy);
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
        "encodeURI" => return encode_uri(args),
        "decodeURI" => return decode_uri(args),
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
        // "__internal_promise_all_resolve" => return internal_promise_all_resolve(mc, args, env),
        // "__internal_promise_all_reject" => return internal_promise_all_reject(mc, args, env),
        "Promise.resolve" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "resolve", args, env)?),
        "Promise.reject" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "reject", args, env)?),
        "Promise.all" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "all", args, env)?),
        "Promise.race" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "race", args, env)?),
        "Promise.any" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "any", args, env)?),
        "Promise.allSettled" => return Ok(crate::js_promise::handle_promise_static_method_val(mc, "allSettled", args, env)?),

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

        /*
        "Promise.prototype.then" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(_obj) = this_val {
                    // return crate::js_promise::handle_promise_then(mc, &obj, args, env);
                    todo!()
                }
            }
            return Err(raise_eval_error!("Promise.prototype.then called without a promise receiver").into());
        }
        "Promise.prototype.catch" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(_obj) = this_val {
                    // return crate::js_promise::handle_promise_catch(mc, &obj, args, env);
                    todo!()
                }
            }
            return Err(raise_eval_error!("Promise.prototype.catch called without a promise receiver").into());
        }
        "Promise.prototype.finally" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                if let Value::Object(_obj) = this_val {
                    // return crate::js_promise::handle_promise_finally(mc, &obj, args, env);
                    todo!()
                }
            }
            return Err(raise_eval_error!("Promise.prototype.finally called without a promise receiver").into());
        }
        // */
        "setTimeout" => return Ok(crate::js_promise::handle_set_timeout_val(mc, args, env)?),
        "clearTimeout" => return Ok(crate::js_promise::handle_clear_timeout_val(mc, args, env)?),
        "setInterval" => return Ok(crate::js_promise::handle_set_interval_val(mc, args, env)?),
        "clearInterval" => return Ok(crate::js_promise::handle_clear_interval_val(mc, args, env)?),
        "Function.prototype.call" => {
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                let callee_for_arguments = this_val.clone(); // The function object being called
                match this_val {
                    Value::Function(func_name) => {
                        if func_name.starts_with("Object.prototype.") || func_name.starts_with("Array.prototype.") {
                            if args.is_empty() {
                                return Err(raise_eval_error!("call requires a receiver").into());
                            }
                            let receiver_val = args[0].clone();
                            let forwarded_args = args[1..].to_vec();
                            println!("DEBUG Function.prototype.call forwarded args: {:?}", forwarded_args);
                            let call_env = prepare_call_env_with_this(mc, Some(env), Some(receiver_val), None, &[], None, Some(env), None)?;
                            return handle_global_function(mc, &func_name, &forwarded_args, &call_env);
                        }
                        return Err(raise_eval_error!(format!("Function.prototype.call target not supported: {}", func_name)).into());
                    }
                    Value::Closure(data) => {
                        if args.is_empty() {
                            return Err(raise_eval_error!("call requires a receiver").into());
                        }
                        let receiver_val = args[0].clone();
                        let forwarded = args[1..].to_vec();
                        let evaluated_args = forwarded.to_vec();
                        let params = &data.params;
                        let body = &data.body;
                        let captured_env = &data.env;
                        let func_env = prepare_function_call_env(
                            mc,
                            captured_env.as_ref(),
                            Some(receiver_val),
                            Some(params),
                            &evaluated_args,
                            None,
                            Some(env),
                        )?;

                        propagate_closure_strictness(mc, &func_env, &data)?;

                        // For raw closures (without wrapper object), we don't have a stable object identity for 'callee'.
                        // But we can check if there's a reference to the function object in the `this` binding? No.
                        // However, Function.prototype.call is usually called as `func.call(...)`.
                        // The `this_val` here IS the function object (or closure).
                        // So `callee_for_arguments` holds the correct Value::Closure or Value::Object.

                        crate::js_class::create_arguments_object(mc, &func_env, &evaluated_args, Some(callee_for_arguments))?;

                        return crate::core::evaluate_statements(mc, &func_env, body);
                    }
                    Value::Object(object) => {
                        log::trace!("Function.prototype.call on Value::Object");
                        if let Some(cl_rc) = object.borrow().get_closure()
                            && let Value::Closure(data) = &*cl_rc.borrow()
                        {
                            if args.is_empty() {
                                return Err(raise_eval_error!("call requires a receiver").into());
                            }
                            log::trace!("Function.prototype.call calling closure with callee={:?}", callee_for_arguments);
                            let receiver_val = args[0].clone();
                            let forwarded = args[1..].to_vec();
                            let evaluated_args = forwarded.to_vec();
                            let params = &data.params;
                            let body = &data.body;
                            let captured_env = &data.env;
                            let func_env = prepare_function_call_env(
                                mc,
                                captured_env.as_ref(),
                                Some(receiver_val),
                                Some(params),
                                &evaluated_args,
                                None,
                                Some(env),
                            )?;

                            propagate_closure_strictness(mc, &func_env, data)?;

                            crate::js_class::create_arguments_object(mc, &func_env, &evaluated_args, Some(callee_for_arguments))?;

                            return crate::core::evaluate_statements(mc, &func_env, body);
                        }
                        return Err(raise_eval_error!("Function.prototype.call called on non-callable").into());
                    }
                    _ => return Err(raise_eval_error!("Function.prototype.call called on non-callable").into()),
                }
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
                                            if let Some(val_rc) = crate::core::get_own_property(&arr_obj, &key.into()) {
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
                            let call_env = prepare_call_env_with_this(mc, Some(env), Some(receiver_val), None, &[], None, Some(env), None)?;
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
                                        if let Some(val_rc) = crate::core::get_own_property(&arr_obj, &key.into()) {
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
                            Some(receiver_val),
                            Some(params),
                            &evaluated_args,
                            None,
                            Some(env),
                        )?;

                        propagate_closure_strictness(mc, &func_env, &data)?;

                        crate::js_class::create_arguments_object(mc, &func_env, &evaluated_args, Some(callee_for_arguments))?;

                        return crate::core::evaluate_statements(mc, &func_env, body);
                    }
                    Value::Object(object) => {
                        if let Some(cl_rc) = object.borrow().get_closure()
                            && let Value::Closure(data) = &*cl_rc.borrow()
                        {
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
                                            if let Some(val_rc) = crate::core::get_own_property(&arr_obj, &key.into()) {
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
                                Some(receiver_val),
                                Some(params),
                                &evaluated_args,
                                None,
                                Some(env),
                            )?;

                            propagate_closure_strictness(mc, &func_env, data)?;

                            crate::js_class::create_arguments_object(mc, &func_env, &evaluated_args, Some(Value::Object(object)))?;

                            return crate::core::evaluate_statements(mc, &func_env, body);
                        }
                        return Err(raise_eval_error!("Function.prototype.apply called on non-callable").into());
                    }
                    _ => return Err(raise_eval_error!("Function.prototype.apply called on non-callable").into()),
                }
            } else {
                return Err(raise_eval_error!("Function.prototype.apply called without this").into());
            }
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
        "Array" => Ok(handle_array_constructor(mc, args, env)?),

        name if name.starts_with("Array.prototype.") => {
            let method = name.trim_start_matches("Array.prototype.");
            if let Some(this_rc) = crate::core::env_get(env, "this") {
                let this_val = this_rc.borrow().clone();
                match this_val {
                    Value::Object(obj) => {
                        return crate::js_array::handle_array_instance_method(mc, &obj, method, args, env);
                    }
                    Value::String(s) => {
                        let str_obj = crate::core::new_js_object_data(mc);
                        object_set_key_value(mc, &str_obj, "__value__", Value::String(s.clone()))?;
                        object_set_key_value(mc, &str_obj, "length", Value::Number(crate::unicode::utf16_len(&s) as f64))?;
                        let mut i = 0;
                        while let Some(c) = crate::unicode::utf16_char_at(&s, i) {
                            let char_str = crate::unicode::utf16_to_utf8(&[c]);
                            object_set_key_value(mc, &str_obj, i, Value::String(crate::unicode::utf8_to_utf16(&char_str)))?;
                            i += 1;
                        }
                        return crate::js_array::handle_array_instance_method(mc, &str_obj, method, args, env);
                    }
                    _ => {
                        return Err(raise_type_error!("Array.prototype method called on incompatible receiver").into());
                    }
                }
            }
            Err(raise_type_error!("Array.prototype method called without this").into())
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
            if func_name.starts_with("Object.") && !func_name.contains(".prototype.") {
                let method = &func_name["Object.".len()..];
                return Ok(crate::js_object::handle_object_method(mc, method, args, env)?);
            }
            Err(raise_eval_error!(format!("Global function {} not found", func_name)).into())
        }
    }
}

fn dynamic_import_function<'gc>(mc: &MutationContext<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    // Dynamic import() function
    if args.len() != 1 {
        return Err(raise_type_error!("import() requires exactly one argument").into());
    }
    let module_specifier = args[0].clone();
    let module_name = match module_specifier {
        Value::String(s) => utf16_to_utf8(&s),
        _ => return Err(raise_type_error!("import() argument must be a string").into()),
    };

    // Load the module dynamically
    let _module_value = crate::js_module::load_module(mc, &module_name, None).map_err(EvalError::Js)?;

    // Return a Promise that resolves to the module
    // let promise = Gc::new(
    //     mc,
    //     GcCell::new(crate::js_promise::JSPromise {
    //         state: crate::js_promise::PromiseState::Fulfilled(module_value.clone()),
    //         value: Some(module_value),
    //         on_fulfilled: Vec::new(),
    //         on_rejected: Vec::new(),
    //     }),
    // );

    // let promise_obj = Value::Object(new_js_object_data(mc));
    // if let Value::Object(obj) = &promise_obj {
    //     object_set_key_value(mc, obj, &"__promise".into(), Value::Promise(promise))?;
    // }
    // Ok(promise_obj)
    todo!()
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

fn is_finite_function<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    // Evaluate all arguments for side effects

    let arg_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };

    match arg_val {
        Value::Number(n) => Ok(Value::Boolean(n.is_finite())),
        Value::String(s) => {
            let str_val = utf16_to_utf8(&s);
            match str_val.trim().parse::<f64>() {
                Ok(n) => Ok(Value::Boolean(n.is_finite())),
                Err(_) => Ok(Value::Boolean(false)), // Non-numeric strings are not finite
            }
        }
        Value::Boolean(_) => Ok(Value::Boolean(true)), // Booleans are finite
        Value::Undefined => Ok(Value::Boolean(false)), // undefined is not finite
        _ => Ok(Value::Boolean(false)),                // Objects, functions, etc. are not finite
    }
}

fn function_constructor<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Evaluate arguments

    let body_str = if !args.is_empty() {
        let val = args.last().unwrap();
        match val {
            Value::String(s) => utf16_to_utf8(s),
            _ => crate::core::value_to_string(val),
        }
    } else {
        "".to_string()
    };

    let mut params_str = String::new();
    if args.len() > 1 {
        for (i, arg) in args.iter().take(args.len() - 1).enumerate() {
            if i > 0 {
                params_str.push(',');
            }
            let arg_str = match arg {
                Value::String(s) => utf16_to_utf8(s),
                _ => crate::core::value_to_string(arg),
            };
            params_str.push_str(&arg_str);
        }
    }

    let func_source = format!("function anonymous({params_str}) {{ {body_str} }}");
    let tokens = crate::core::tokenize(&func_source)?;

    let mut index = 0;
    let stmts = crate::core::parse_statements(&tokens, &mut index)?;

    // Find global environment (Function constructor always creates functions in global scope)
    let mut global_env = *env;
    while let Some(proto) = global_env.borrow().prototype {
        global_env = proto;
    }

    if let Some(Statement { kind, .. }) = stmts.first() {
        if let StatementKind::FunctionDeclaration(_n, params, body, _i, _a) = &**kind {
            // Create a closure with the global environment
            let mut closure_data = ClosureData::new(params, body, Some(global_env), None);
            // Function constructor created functions should not inherit strict mode from the context
            closure_data.enforce_strictness_inheritance = false;
            Ok(Value::Closure(Gc::new(mc, closure_data)))
        } else {
            Err(raise_type_error!("Failed to parse function body").into())
        }
    } else {
        Err(raise_type_error!("Failed to parse function body").into())
    }
}

fn encode_uri_component<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    // Evaluate all arguments for side effects

    let arg_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };

    let str_val = match arg_val {
        Value::String(s) => utf16_to_utf8(&s),
        _ => crate::core::value_to_string(&arg_val),
    };

    // Simple URI encoding - replace spaces with %20 and some special chars
    let encoded = str_val
        .replace("%", "%25")
        .replace(" ", "%20")
        .replace("\"", "%22")
        .replace("'", "%27")
        .replace("<", "%3C")
        .replace(">", "%3E")
        .replace("&", "%26");
    Ok(Value::String(utf8_to_utf16(&encoded)))
}

fn decode_uri_component<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    // Evaluate all arguments for side effects

    let arg_val = if args.is_empty() { Value::Undefined } else { args[0].clone() };

    let str_val = match arg_val {
        Value::String(s) => utf16_to_utf8(&s),
        _ => crate::core::value_to_string(&arg_val),
    };

    // Simple URI decoding - replace %20 with spaces and some special chars
    let decoded = str_val
        .replace("%20", " ")
        .replace("%22", "\"")
        .replace("%27", "'")
        .replace("%3C", "<")
        .replace("%3E", ">")
        .replace("%26", "&")
        .replace("%25", "%");
    Ok(Value::String(utf8_to_utf16(&decoded)))
}

fn boolean_constructor<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    // Evaluate all arguments for side effects

    if args.is_empty() {
        return Ok(Value::Boolean(false));
    }

    let arg_val = args[0].clone();
    let bool_val = match arg_val {
        Value::Boolean(b) => b,
        Value::Number(n) => n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Object(_) => true,
        Value::Undefined => false,
        Value::Null => false,
        _ => false,
    };
    Ok(Value::Boolean(bool_val))
}

fn symbol_prototype_value_of<'gc>(
    mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let this_val = crate::js_class::evaluate_this(mc, env).map_err(EvalError::Js)?;
    match this_val {
        Value::Symbol(s) => Ok(Value::Symbol(s)),
        Value::Object(obj) => {
            if let Some(val) = object_get_key_value(&obj, "__value__")
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
    let this_val = crate::js_class::evaluate_this(mc, env).map_err(EvalError::Js)?;
    let sym = match this_val {
        Value::Symbol(s) => s,
        Value::Object(obj) => {
            if let Some(val) = object_get_key_value(&obj, "__value__") {
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

    let desc = sym.description.as_deref().unwrap_or("");
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

    let symbol_data = Gc::new(mc, crate::core::SymbolData { description });
    Ok(Value::Symbol(symbol_data))
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
    if !args.is_empty() {
        let arg_val = args[0].clone();
        match arg_val {
            Value::String(s) => {
                let code = utf16_to_utf8(&s);

                log::trace!("eval invoked with code='{}'", code);

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

                let mut tokens = crate::core::tokenize(&code).map_err(EvalError::Js)?;
                // Debug: always emit token list for eval bodies containing 'return' or for small bodies
                if code.contains("return") || code.len() < 256 {
                    log::trace!(
                        "eval debug: code='{}' tokens={:?}",
                        code,
                        tokens.iter().map(|t| (&t.token, t.line, t.column)).collect::<Vec<_>>()
                    );
                }
                if tokens.last().map(|td| td.token == crate::core::Token::EOF).unwrap_or(false) {
                    tokens.pop();
                }

                // index for parsing start position
                let mut index: usize = 0;

                let mut stmts = crate::core::parse_statements(&tokens, &mut index).map_err(EvalError::Js)?;

                // If this is an indirect eval executed in the global env and the eval code
                // is strict (starts with "use strict"), do not instantiate top-level
                // FunctionDeclarations into the (global) variable environment. Convert
                // them into function expressions so they don't create bindings.
                let is_indirect_eval = if let Some(flag) = crate::core::object_get_key_value(env, "__is_indirect_eval") {
                    matches!(*flag.borrow(), crate::core::Value::Boolean(true))
                } else {
                    false
                };
                log::trace!(
                    "DEBUG: eval env ptr={:p} __is_indirect_eval present={}",
                    env,
                    crate::core::object_get_key_value(env, "__is_indirect_eval").is_some()
                );
                log::trace!("DEBUG: is_indirect_eval = {}", is_indirect_eval);
                if is_indirect_eval {
                    log::trace!("DEBUG: eval env has __is_indirect_eval flag");
                    if let Some(first) = stmts.first()
                        && let crate::core::StatementKind::Expr(expr) = &*first.kind
                        && let crate::core::Expr::StringLit(s) = expr
                        && crate::unicode::utf16_to_utf8(s).as_str() == "use strict"
                    {
                        let mut converted = 0;
                        for stmt in stmts.iter_mut() {
                            if let crate::core::StatementKind::FunctionDeclaration(name, params, body, _is_generator, _is_async) =
                                &*stmt.kind
                            {
                                let func_expr = crate::core::Expr::Function(Some(name.clone()), params.clone(), body.clone());
                                *stmt.kind = crate::core::StatementKind::Expr(func_expr);
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
                    && let crate::core::StatementKind::Expr(expr) = &*first.kind
                    && let crate::core::Expr::StringLit(s) = expr
                    && crate::unicode::utf16_to_utf8(s).as_str() == "use strict"
                {
                    log::trace!("DEBUG: indirect strict eval - creating fresh declarative environment");
                    let new_env = crate::core::new_js_object_data(mc);
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
                        match crate::js_class::evaluate_new(mc, env, constructor_val, &[msg_val]) {
                            Ok(Value::Object(obj)) => Err(EvalError::Throw(Value::Object(obj), None, None)),
                            Ok(other) => Err(EvalError::Throw(other, None, None)),
                            Err(_) => Err(err),
                        }
                    }
                }
            }
            _ => Ok(arg_val),
        }
    } else {
        Ok(Value::Undefined)
    }
}

fn encode_uri<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    if !args.is_empty() {
        let arg_val = args[0].clone();
        match arg_val {
            Value::String(s) => {
                let str_val = utf16_to_utf8(&s);
                // Simple URI encoding - replace spaces with %20
                let encoded = str_val.replace(" ", "%20");
                Ok(Value::String(utf8_to_utf16(&encoded)))
            }
            _ => {
                let str_val = match arg_val {
                    Value::Number(n) => n.to_string(),
                    Value::Boolean(b) => b.to_string(),
                    _ => "[object Object]".to_string(),
                };
                Ok(Value::String(utf8_to_utf16(&str_val)))
            }
        }
    } else {
        Ok(Value::String(Vec::new()))
    }
}

fn decode_uri<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError<'gc>> {
    if !args.is_empty() {
        let arg_val = args[0].clone();
        match arg_val {
            Value::String(s) => {
                let str_val = utf16_to_utf8(&s);
                // Simple URI decoding - replace %20 with spaces
                let decoded = str_val.replace("%20", " ");
                Ok(Value::String(utf8_to_utf16(&decoded)))
            }
            _ => {
                let str_val = match arg_val {
                    Value::Number(n) => n.to_string(),
                    Value::Boolean(b) => b.to_string(),
                    _ => "[object Object]".to_string(),
                };
                Ok(Value::String(utf8_to_utf16(&str_val)))
            }
        }
    } else {
        Ok(Value::String(Vec::new()))
    }
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
    validate_internal_args(args, 3).map_err(EvalError::Js)?;
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
    validate_internal_args(args, 3).map_err(EvalError::Js)?;
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
    validate_internal_args(args, 3).map_err(EvalError::Js)?;
    let numbers = validate_number_args(args, 2).map_err(EvalError::Js)?;
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
    validate_internal_args(args, 3).map_err(EvalError::Js)?;
    let numbers = validate_number_args(args, 2).map_err(EvalError::Js)?;
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

#[allow(dead_code)]
fn internal_promise_all_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function for Promise.all resolve - requires 3 args: (idx, value, state)
    validate_internal_args(args, 3)?;
    let numbers = validate_number_args(args, 1)?;
    let idx = numbers[0] as usize;
    let value = args[1].clone();
    if let Value::Object(state_obj) = args[2].clone() {
        // Store value in results[idx]
        if let Some(results_val_rc) = object_get_key_value(&state_obj, "results")
            && let Value::Object(results_obj) = &*results_val_rc.borrow()
        {
            object_set_key_value(mc, results_obj, idx, value).map_err(EvalError::Js)?;
        }
        // Increment completed
        if let Some(completed_val_rc) = object_get_key_value(&state_obj, "completed")
            && let Value::Number(completed) = &*completed_val_rc.borrow()
        {
            let new_completed = completed + 1.0;
            object_set_key_value(mc, &state_obj, "completed", Value::Number(new_completed)).map_err(EvalError::Js)?;
            // Check if all completed
            if let Some(total_val_rc) = object_get_key_value(&state_obj, "total")
                && let Value::Number(total) = &*total_val_rc.borrow()
                && new_completed == *total
            {
                // Resolve result_promise with results array
                if let Some(promise_val_rc) = object_get_key_value(&state_obj, "result_promise")
                    && let Value::Promise(_result_promise) = &*promise_val_rc.borrow()
                    && let Some(results_val_rc) = object_get_key_value(&state_obj, "results")
                    && let Value::Object(_results_obj) = &*results_val_rc.borrow()
                {
                    // crate::js_promise::resolve_promise(mc, result_promise, Value::Object(results_obj.clone()));
                    todo!("Implement resolve_promise call");
                }
            }
        }
    }
    Ok(Value::Undefined)
}

#[allow(dead_code)]
fn internal_promise_all_reject<'gc>(
    _mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Internal function for Promise.all reject - requires 2 args: (reason, state)
    validate_internal_args(args, 2).map_err(EvalError::Js)?;
    let _reason = args[0].clone();
    if let Value::Object(state_obj) = args[1].clone() {
        // Reject result_promise
        if let Some(promise_val_rc) = object_get_key_value(&state_obj, "result_promise")
            && let Value::Promise(_result_promise) = &*promise_val_rc.borrow()
        {
            // crate::js_promise::reject_promise(mc, result_promise, reason);
            todo!("Implement reject_promise call");
        }
    }
    Ok(Value::Undefined)
}

#[allow(dead_code)]
fn test_with_intl_constructors<'gc>(
    _mc: &MutationContext<'gc>,
    _args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    /*
    // testWithIntlConstructors function - used for testing Intl constructors
    if args.len() != 1 {
        return Err(EvalError::Js(raise_type_error!("testWithIntlConstructors requires exactly 1 argument")));
    }
    let callback = args[0].clone();
    let callback_func = match callback {
        Value::Closure(data) | Value::AsyncClosure(data) => (data.params.clone(), data.body.clone(), data.env.clone()),
        _ => {
            return Err(EvalError::Js(raise_type_error!("testWithIntlConstructors requires a function as argument")));
        }
    };

    // Create a mock constructor
    let mock_constructor = crate::js_testintl::create_mock_intl_constructor(mc)?;

    // Call the callback function with the mock constructor as argument
    // Create a fresh function environment and bind parameters
    let args_vals = vec![mock_constructor];
    let func_env =
        crate::core::prepare_function_call_env(mc, Some(&callback_func.2), None, Some(&callback_func.0), &args_vals, None, None)?;
    // Execute function body
    crate::core::evaluate_statements(mc, &func_env, &callback_func.1)?;

    Ok(Value::Undefined)
    // */
    todo!("testWithIntlConstructors is not yet implemented");
}

fn handle_object_has_own_property<'gc>(args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
    // hasOwnProperty should inspect the bound `this` and take one argument
    if args.len() != 1 {
        return Err(raise_eval_error!("hasOwnProperty requires one argument").into());
    }
    let key_val = args[0].clone();
    if let Some(this_rc) = crate::core::env_get(env, "this") {
        let this_val = this_rc.borrow().clone();
        match this_val {
            Value::Object(obj) => {
                let exists = has_own_property_value(&obj, &key_val);
                Ok(Value::Boolean(exists))
            }
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

fn handle_object_property_is_enumerable<'gc>(args: &[Value<'gc>], env: &JSObjectDataPtr<'gc>) -> Result<Value<'gc>, EvalError<'gc>> {
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
            // Convert key value to a PropertyKey
            let key: PropertyKey<'gc> = key_val.into();

            // Check own property and enumerability
            if crate::core::get_own_property(&obj, &key).is_some() {
                return Ok(Value::Boolean(obj.borrow().is_enumerable(&key)));
            }
            Ok(Value::Boolean(false))
        }
        _ => Ok(Value::Boolean(false)),
    }
}

pub fn initialize_function<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    let func_ctor = new_js_object_data(mc);
    object_set_key_value(mc, &func_ctor, "name", Value::String(utf8_to_utf16("Function")))?;
    object_set_key_value(mc, &func_ctor, "__is_constructor", Value::Boolean(true))?;
    object_set_key_value(mc, &func_ctor, "__native_ctor", Value::String(utf8_to_utf16("Function")))?;

    let func_proto = new_js_object_data(mc);

    // Link Function.prototype to Object.prototype so function objects inherit object methods
    if let Some(obj_val) = crate::core::env_get(env, "Object")
        && let Value::Object(obj_ctor) = &*obj_val.borrow()
        && let Some(obj_proto_val) = crate::core::object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(obj_proto) = &*obj_proto_val.borrow()
    {
        func_proto.borrow_mut(mc).prototype = Some(*obj_proto);
    }

    object_set_key_value(mc, &func_ctor, "prototype", Value::Object(func_proto))?;
    object_set_key_value(mc, &func_proto, "constructor", Value::Object(func_ctor))?;

    // Function.prototype.bind
    object_set_key_value(mc, &func_proto, "bind", Value::Function("Function.prototype.bind".to_string()))?;
    func_proto.borrow_mut(mc).set_non_enumerable(crate::core::PropertyKey::from("bind"));

    // Function.prototype.call
    object_set_key_value(mc, &func_proto, "call", Value::Function("Function.prototype.call".to_string()))?;
    func_proto.borrow_mut(mc).set_non_enumerable(crate::core::PropertyKey::from("call"));

    // Function.prototype.apply
    object_set_key_value(mc, &func_proto, "apply", Value::Function("Function.prototype.apply".to_string()))?;
    func_proto
        .borrow_mut(mc)
        .set_non_enumerable(crate::core::PropertyKey::from("apply"));

    func_proto
        .borrow_mut(mc)
        .set_non_enumerable(crate::core::PropertyKey::from("constructor"));

    // Define restricted 'caller' and 'arguments' accessors that throw a TypeError when accessed or assigned
    let restricted_desc = crate::core::new_js_object_data(mc);
    let val = Value::Function("Function.prototype.restrictedThrow".to_string());
    object_set_key_value(mc, &restricted_desc, "get", val)?;

    let val = Value::Function("Function.prototype.restrictedThrow".to_string());
    object_set_key_value(mc, &restricted_desc, "set", val)?;

    object_set_key_value(mc, &restricted_desc, "configurable", Value::Boolean(true))?;
    crate::js_object::define_property_internal(
        mc,
        &func_proto,
        &crate::core::PropertyKey::String("caller".to_string()),
        &restricted_desc,
    )?;
    crate::js_object::define_property_internal(
        mc,
        &func_proto,
        &crate::core::PropertyKey::String("arguments".to_string()),
        &restricted_desc,
    )?;

    crate::core::env_set(mc, env, "Function", Value::Object(func_ctor))?;

    // Ensure any native constructors created earlier (e.g., Error, TypeError)
    // get Function.prototype as their internal prototype so `instanceof Function`
    // behaves correctly.
    let native_constructors = ["Error", "ReferenceError", "TypeError", "RangeError", "SyntaxError"];
    for name in native_constructors {
        if let Some(ctor_rc) = crate::core::object_get_key_value(env, name)
            && let Value::Object(ctor_obj) = &*ctor_rc.borrow()
        {
            ctor_obj.borrow_mut(mc).prototype = Some(func_proto);
        }
    }

    Ok(())
}

pub fn handle_function_prototype_method<'gc>(
    mc: &MutationContext<'gc>,
    this_value: &Value<'gc>,
    method: &str,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "bind" => {
            let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
            // function.bind(thisArg, ...args)
            if let Value::Closure(closure_gc) = this_value {
                let original = closure_gc;
                let effective_bound_this = if original.bound_this.is_some() {
                    original.bound_this.clone()
                } else {
                    Some(this_arg)
                };
                let new_closure_data = ClosureData {
                    params: original.params.clone(),
                    body: original.body.clone(),
                    env: original.env,
                    home_object: original.home_object.clone(),
                    captured_envs: original.captured_envs.clone(),
                    bound_this: effective_bound_this,
                    is_arrow: original.is_arrow,
                    is_strict: original.is_strict,
                    native_target: None,
                    enforce_strictness_inheritance: true,
                };
                Ok(Value::Closure(Gc::new(mc, new_closure_data)))
            } else if let Value::Object(obj) = this_value {
                // Support calling bind on a function object wrapper (object with internal closure)
                if let Some(cl_prop) = obj.borrow().get_closure()
                    && let Value::Closure(original) = &*cl_prop.borrow()
                {
                    let effective_bound_this = if original.bound_this.is_some() {
                        original.bound_this.clone()
                    } else {
                        Some(this_arg)
                    };
                    let new_closure_data = ClosureData {
                        params: original.params.clone(),
                        body: original.body.clone(),
                        env: original.env,
                        home_object: original.home_object.clone(),
                        captured_envs: original.captured_envs.clone(),
                        bound_this: effective_bound_this,
                        is_arrow: original.is_arrow,
                        is_strict: original.is_strict,
                        native_target: None,
                        enforce_strictness_inheritance: true,
                    };
                    return Ok(Value::Closure(Gc::new(mc, new_closure_data)));
                }
                Err(crate::raise_type_error!("Function.prototype.bind called on non-function").into())
            } else if let Value::Function(name) = this_value {
                let effective_bound_this = Some(this_arg);
                let new_closure_data = ClosureData {
                    env: Some(*env),
                    bound_this: effective_bound_this,
                    native_target: Some(name.clone()),
                    enforce_strictness_inheritance: true,
                    ..ClosureData::default()
                };
                Ok(Value::Closure(Gc::new(mc, new_closure_data)))
            } else {
                Err(crate::raise_type_error!("Function.prototype.bind called on non-function").into())
            }
        }
        _ => Err(crate::raise_type_error!(format!("Unknown Function.prototype method: {method}")).into()),
    }
}
