use crate::core::{
    ClosureData, DestructuringElement, EvalError, Expr, JSGenerator, JSObjectDataPtr, Value, object_get_key_value, object_set_key_value,
};
use crate::core::{Gc, GcPtr, MutationContext};
use crate::error::JSError;
use crate::js_promise::{call_function_with_this, make_promise_js_object};
use crate::unicode::utf8_to_utf16;

pub fn handle_async_closure_call<'gc>(
    mc: &MutationContext<'gc>,
    closure: &ClosureData<'gc>,
    _this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
    _fn_obj: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, JSError> {
    // Evaluate async function body asynchronously (via microtask queue)
    // and wrap the result in a resolved/rejected promise.
    // This allows the caller to receive the promise immediately (pending)
    // and facilitates interleaving of async tasks.
    let (promise, resolve, reject) = crate::js_promise::create_promise_capability(mc, env)?;

    // Create a new Closure wrapped in Value to pass to the task
    // We clone the closure data because the task needs to own its reference/copy
    // Instead of deferring the entire execution, synchronously start the generator
    // so that the body runs up to the first `await` (per spec) before returning the Promise.
    match crate::js_generator::handle_generator_function_call(mc, closure, args, _this_val) {
        Ok(gen_val) => {
            if let Value::Object(gen_obj) = gen_val
                && let Some(gen_inner) = object_get_key_value(&gen_obj, "__generator__")
                && let Value::Generator(gen_ptr) = &*gen_inner.borrow()
            {
                // Synchronously perform the initial step (like generator.next(undefined))
                if let Err(e) = step(mc, *gen_ptr, resolve.clone(), reject.clone(), env, Ok(Value::Undefined)) {
                    // If stepping threw synchronously, log full error for diagnosis
                    log::debug!("sync step error: {:?}", e);
                    // Create a JS Error object with message and (if available) line/column info
                    let msg = e.message();
                    // Use core::create_error to create an Error object preserving prototype/etc.
                    let prototype = None; // use default Error prototype
                    let err_val = crate::core::create_error(mc, prototype, (msg.clone()).into()).unwrap_or(Value::Undefined);
                    if let Value::Object(err_obj) = &err_val {
                        if let Some(line) = e.js_line()
                            && let Err(e) = object_set_key_value(mc, err_obj, "__line__", Value::Number(line as f64))
                        {
                            log::debug!("error setting __line__ on error object: {:?}", e);
                        }
                        if let Some(col) = e.js_column()
                            && let Err(e) = object_set_key_value(mc, err_obj, "__column__", Value::Number(col as f64))
                        {
                            log::debug!("error setting __column__ on error object: {:?}", e);
                        }
                    }
                    // Reject with an Error object so unhandled reporting can include line info
                    if let Err(e) = crate::js_promise::call_function(mc, &reject, &[err_val], env) {
                        log::debug!("error calling reject on promise: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            let rej_val = match e {
                EvalError::Throw(v, _, _) => v,
                EvalError::Js(je) => {
                    let msg = je.message();
                    let err_val = crate::core::create_error(mc, None, Value::String(utf8_to_utf16(&msg))).unwrap_or(Value::Undefined);
                    if let Value::Object(obj) = &err_val {
                        if let Some(line) = je.js_line()
                            && let Err(e) = object_set_key_value(mc, obj, "__line__", Value::Number(line as f64))
                        {
                            log::debug!("error setting __line__ on error object: {:?}", e);
                        }
                        if let Some(col) = je.js_column()
                            && let Err(e) = object_set_key_value(mc, obj, "__column__", Value::Number(col as f64))
                        {
                            log::debug!("error setting __column__ on error object: {:?}", e);
                        }
                    }
                    err_val
                }
            };
            if let Err(e) = crate::js_promise::call_function(mc, &reject, &[rej_val], env) {
                log::debug!("error calling reject on promise: {:?}", e);
            }
        }
    }

    // The rest of execution (after yields) will be handled via microtasks in step()
    let promise_obj = make_promise_js_object(mc, promise, Some(*env))?;
    Ok(Value::Object(promise_obj))
}

fn step<'gc>(
    mc: &MutationContext<'gc>,
    generator: GcPtr<'gc, JSGenerator<'gc>>,
    resolve: Value<'gc>,
    reject: Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
    next_val: Result<Value<'gc>, Value<'gc>>, // Ok(val) for next(val), Err(err) for throw(err)
) -> Result<(), JSError> {
    log::trace!("DEBUG: step called");
    // Invoke generator.next(val) or generator.throw(err)
    // println!("STEP: next_val={:?}", next_val);
    let result = match next_val {
        Ok(val) => crate::js_generator::generator_next(mc, &generator, val),
        Err(err) => crate::js_generator::generator_throw(mc, &generator, err),
    };

    match result {
        Ok(res_obj) => {
            // Check if done
            let done = if let Value::Object(obj) = &res_obj {
                if let Some(d) = object_get_key_value(obj, "done") {
                    crate::js_boolean::to_boolean(&d.borrow())
                } else {
                    false
                }
            } else {
                false
            };

            let value = if let Value::Object(obj) = &res_obj {
                if let Some(v) = object_get_key_value(obj, "value") {
                    v.borrow().clone()
                } else {
                    Value::Undefined
                }
            } else {
                Value::Undefined
            };

            if done {
                // Resolve the outer promise with the return value
                crate::js_promise::call_function(mc, &resolve, &[value], env)?;
            } else {
                // Not done, "value" is the yielded promise (or value to be awaited)
                // Promise.resolve(value).then(res => step(next(res)), err => step(throw(err)))

                let promise_resolve = if let Some(ctor) = crate::core::env_get(env, "Promise") {
                    if let Some(resolve_method) = object_get_key_value(
                        &match ctor.borrow().clone() {
                            Value::Object(o) => o,
                            _ => return Err(crate::raise_eval_error!("Promise not object")),
                        },
                        "resolve",
                    ) {
                        resolve_method.borrow().clone()
                    } else {
                        return Err(crate::raise_eval_error!("Promise.resolve missing"));
                    }
                } else {
                    return Err(crate::raise_eval_error!("Promise not found"));
                };

                let p_val = crate::js_promise::call_function(mc, &promise_resolve, &[value], env)?;

                let on_fulfilled = create_async_step_callback(mc, generator, resolve.clone(), reject.clone(), *env, false);
                let on_rejected = create_async_step_callback(mc, generator, resolve.clone(), reject.clone(), *env, true);

                log::trace!("DEBUG: p_val type: {:?}", p_val);

                if let Value::Object(p_obj) = p_val
                    && let Some(then_method) = object_get_key_value(&p_obj, "then")
                {
                    log::trace!("DEBUG: Calling then method with this");
                    call_function_with_this(mc, &then_method.borrow(), Some(p_val), &[on_fulfilled, on_rejected], env)?;
                }
            }
        }
        Err(e) => {
            // Generator threw an error synchronously (or during processing), reject the promise
            let err_val = match e {
                EvalError::Throw(v, _, _) => v,
                EvalError::Js(j) => {
                    let msg = j.message();
                    let val = crate::core::create_error(mc, None, Value::String(utf8_to_utf16(&msg))).unwrap_or(Value::Undefined);
                    if let Value::Object(obj) = &val {
                        if let Some(line) = j.js_line()
                            && let Err(e) = object_set_key_value(mc, obj, "__line__", Value::Number(line as f64))
                        {
                            log::warn!("error setting __line__ on error object: {:?}", e);
                        }
                        if let Some(col) = j.js_column()
                            && let Err(e) = object_set_key_value(mc, obj, "__column__", Value::Number(col as f64))
                        {
                            log::warn!("error setting __column__ on error object: {:?}", e);
                        }
                    }
                    val
                }
            };
            crate::js_promise::call_function(mc, &reject, &[err_val], env)?;
        }
    }
    Ok(())
}

fn create_async_step_callback<'gc>(
    mc: &MutationContext<'gc>,
    generator: GcPtr<'gc, JSGenerator<'gc>>,
    resolve: Value<'gc>,
    reject: Value<'gc>,
    global_env: JSObjectDataPtr<'gc>,
    is_reject: bool,
) -> Value<'gc> {
    let env = crate::new_js_object_data(mc);
    env.borrow_mut(mc).prototype = Some(global_env);

    object_set_key_value(mc, &env, "__async_generator", Value::Generator(generator)).unwrap();
    object_set_key_value(mc, &env, "__async_resolve", resolve).unwrap();
    object_set_key_value(mc, &env, "__async_reject", reject).unwrap();

    let func_name = if is_reject {
        "__internal_async_step_reject"
    } else {
        "__internal_async_step_resolve"
    };

    let body = vec![crate::js_promise::stmt_expr(Expr::Call(
        Box::new(Expr::Var(func_name.to_string(), None, None)),
        vec![Expr::Var("value".to_string(), None, None)],
    ))];

    Value::Closure(Gc::new(
        mc,
        ClosureData::new(&[DestructuringElement::Variable("value".to_string(), None)], &body, Some(env), None),
    ))
}

pub fn __internal_async_step_resolve<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    log::trace!("DEBUG: __internal_async_step_resolve called with arg count {}", args.len());
    if !args.is_empty() {
        log::trace!("DEBUG: __internal_async_step_resolve arg[0]={:?}", args[0]);
    }
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    continue_async_step(mc, env, Ok(value))
}

pub fn __internal_async_step_reject<'gc>(
    mc: &MutationContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    log::trace!("DEBUG: __internal_async_step_reject called with arg count {}", args.len());
    let reason = args.first().cloned().unwrap_or(Value::Undefined);
    continue_async_step(mc, env, Err(reason))
}

fn continue_async_step<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    result: Result<Value<'gc>, Value<'gc>>,
) -> Result<Value<'gc>, JSError> {
    let generator_val = object_get_key_value(env, "__async_generator").unwrap().borrow().clone();
    let resolve_val = object_get_key_value(env, "__async_resolve").unwrap().borrow().clone();
    let reject_val = object_get_key_value(env, "__async_reject").unwrap().borrow().clone();

    if let Value::Generator(gen_ref) = generator_val {
        // Prefer the generator's stored pre-execution environment (pre_env)
        // when available so bindings created before a yield/await are preserved
        // across async resumption. Fall back to the generator's captured env
        // if no pre_env is present.
        let call_env = {
            let gen_b = gen_ref.borrow();
            match &gen_b.state {
                crate::core::GeneratorState::Suspended { pre_env: Some(pre), .. } => {
                    log::debug!("continue_async_step: using generator pre_env");
                    *pre
                }
                _ => {
                    log::debug!("continue_async_step: no pre_env, using gen.env");
                    gen_b.env
                }
            }
        };
        step(mc, gen_ref, resolve_val, reject_val, &call_env, result)?;
    }

    Ok(Value::Undefined)
}
