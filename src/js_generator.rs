use crate::core::{Gc, GcCell, GcPtr, GeneratorPendingCompletion, GeneratorState, InternalSlot, MutationContext, slot_set};
use crate::{
    core::{
        CatchParamPattern, ClassDefinition, ClassMember, DestructuringElement, EvalError, Expr, JSGenerator, JSObjectDataPtr,
        ObjectDestructuringElement, PropertyKey, Statement, StatementKind, Value, VarDeclKind, env_get, env_get_own, env_get_strictness,
        env_set, env_set_recursive, env_set_strictness, evaluate_call_dispatch, evaluate_expr, new_js_object_data, object_get_key_value,
        object_set_key_value, prepare_function_call_env, prepare_function_call_env_with_home,
    },
    error::JSError,
};

fn close_pending_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iter_obj: &JSObjectDataPtr<'gc>,
) -> Result<(), EvalError<'gc>> {
    let return_val = crate::core::get_property_with_accessors(mc, env, iter_obj, "return")?;
    if matches!(return_val, Value::Undefined | Value::Null) {
        return Ok(());
    }

    let is_callable = matches!(return_val, Value::Function(_) | Value::Closure(_) | Value::Object(_));
    if !is_callable {
        return Err(raise_type_error!("Iterator return property is not callable").into());
    }

    let call_res = crate::core::evaluate_call_dispatch(mc, env, &return_val, Some(&Value::Object(*iter_obj)), &[])?;
    if !matches!(call_res, Value::Object(_)) {
        return Err(raise_type_error!("Iterator return did not return an object").into());
    }

    Ok(())
}

fn eval_error_to_value<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, err: EvalError<'gc>) -> Value<'gc> {
    match err {
        EvalError::Throw(v, ..) => v,
        EvalError::Js(j) => crate::core::js_error_to_value(mc, env, &j),
    }
}

/// Public wrapper so other modules (e.g. js_iterator_helpers) can call GetIterator.
pub fn public_get_iterator<'gc>(
    mc: &MutationContext<'gc>,
    val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    get_iterator(mc, val, env)
}

fn get_iterator<'gc>(
    mc: &MutationContext<'gc>,
    val: &Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    if let Some(sym_ctor_val) = crate::core::env_get(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor_val.borrow()
        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
    {
        let method = match val {
            Value::Object(o) => {
                log::debug!(
                    "get_iterator: looking up Symbol.iterator on obj ptr={:p} sym={:?}",
                    Gc::as_ptr(*o),
                    iter_sym
                );
                crate::core::get_property_with_accessors(mc, env, o, PropertyKey::Symbol(*iter_sym))?
            }
            _ => {
                let proto_name = match val {
                    Value::BigInt(_) => "BigInt",
                    Value::Number(_) => "Number",
                    Value::String(_) => "String",
                    Value::Boolean(_) => "Boolean",
                    Value::Symbol(_) => "Symbol",
                    _ => "",
                };
                if proto_name.is_empty() {
                    Value::Undefined
                } else if let Some(ctor) = crate::core::env_get(env, proto_name)
                    && let Value::Object(ctor_obj) = &*ctor.borrow()
                    && let Some(proto_ref) = object_get_key_value(ctor_obj, "prototype")
                    && let Value::Object(proto) = &*proto_ref.borrow()
                {
                    // Walk the prototype chain starting from proto to find Symbol.iterator,
                    // handling accessor descriptors with the primitive as `this`
                    let sym_key = PropertyKey::Symbol(*iter_sym);
                    let mut found = Value::Undefined;
                    let mut cur_proto = Some(*proto);
                    while let Some(cp) = cur_proto {
                        if let Some(raw_ptr) = crate::core::get_own_property(&cp, &sym_key) {
                            let raw = raw_ptr.borrow().clone();
                            found = match raw {
                                Value::Property { getter, value, .. } => {
                                    if let Some(g) = getter {
                                        // The getter may be a Closure, Function, or Getter value.
                                        // For Getter (AST-based), evaluate inline with primitive `this`.
                                        // For Closure/Function, use evaluate_call_dispatch.
                                        match *g {
                                            Value::Getter(ref body, ref captured_env, ref home_opt) => {
                                                let call_env = crate::core::new_js_object_data(mc);
                                                call_env.borrow_mut(mc).prototype = Some(*captured_env);
                                                call_env.borrow_mut(mc).is_function_scope = true;
                                                object_set_key_value(mc, &call_env, "this", val)?;
                                                call_env.borrow_mut(mc).set_home_object(home_opt.clone());
                                                let body_clone = body.clone();
                                                match crate::core::evaluate_statements_with_labels(mc, &call_env, &body_clone, &[], &[])? {
                                                    crate::core::ControlFlow::Return(v) => v,
                                                    crate::core::ControlFlow::Normal(_) => Value::Undefined,
                                                    crate::core::ControlFlow::Throw(v, line, col) => {
                                                        return Err(EvalError::Throw(v, line, col));
                                                    }
                                                    _ => Value::Undefined,
                                                }
                                            }
                                            _ => crate::core::evaluate_call_dispatch(mc, env, &g, Some(val), &[])?,
                                        }
                                    } else if let Some(v) = value {
                                        v.borrow().clone()
                                    } else {
                                        Value::Undefined
                                    }
                                }
                                Value::Getter(ref body, ref captured_env, ref home_opt) => {
                                    let call_env = crate::core::new_js_object_data(mc);
                                    call_env.borrow_mut(mc).prototype = Some(*captured_env);
                                    call_env.borrow_mut(mc).is_function_scope = true;
                                    object_set_key_value(mc, &call_env, "this", val)?;
                                    call_env.borrow_mut(mc).set_home_object(home_opt.clone());
                                    let body_clone = body.clone();
                                    match crate::core::evaluate_statements_with_labels(mc, &call_env, &body_clone, &[], &[])? {
                                        crate::core::ControlFlow::Return(v) => v,
                                        crate::core::ControlFlow::Normal(_) => Value::Undefined,
                                        crate::core::ControlFlow::Throw(v, line, col) => return Err(EvalError::Throw(v, line, col)),
                                        _ => Value::Undefined,
                                    }
                                }
                                other => other,
                            };
                            break;
                        }
                        cur_proto = cp.borrow().prototype;
                    }
                    found
                } else {
                    Value::Undefined
                }
            }
        };
        log::debug!("get_iterator: method lookup result = {:?}", method);
        if !matches!(method, Value::Undefined | Value::Null) {
            let iter = crate::core::evaluate_call_dispatch(mc, env, &method, Some(val), &[])?;
            if let Value::Object(o) = iter {
                return Ok(o);
            }
            return Err(raise_type_error!("Iterator is not an object").into());
        }
    }
    Err(raise_type_error!("Value is not iterable").into())
}

fn get_for_await_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iter_val: &Value<'gc>,
) -> Result<(JSObjectDataPtr<'gc>, bool), EvalError<'gc>> {
    let mut iterator: Option<JSObjectDataPtr<'gc>> = None;
    let mut is_async_iter = false;

    if let Some(sym_ctor) = crate::core::env_get(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(async_iter_sym_val) = object_get_key_value(sym_obj, "asyncIterator")
        && let Value::Symbol(async_iter_sym) = &*async_iter_sym_val.borrow()
        && let Value::Object(obj) = iter_val
    {
        let method = crate::core::get_property_with_accessors(mc, env, obj, async_iter_sym)?;
        if !matches!(method, Value::Undefined | Value::Null) {
            let res = evaluate_call_dispatch(mc, env, &method, Some(iter_val), &[])?;
            if let Value::Object(iter_obj) = res {
                iterator = Some(iter_obj);
                is_async_iter = true;
            }
        }
    }

    if iterator.is_none()
        && let Some(sym_ctor) = crate::core::env_get(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
        && let Value::Object(obj) = iter_val
    {
        let method = crate::core::get_property_with_accessors(mc, env, obj, iter_sym)?;
        if !matches!(method, Value::Undefined | Value::Null) {
            let res = evaluate_call_dispatch(mc, env, &method, Some(iter_val), &[])?;
            if let Value::Object(iter_obj) = res {
                iterator = Some(iter_obj);
                is_async_iter = false;
            }
        }
    }

    if let Some(iter_obj) = iterator {
        return Ok((iter_obj, is_async_iter));
    }

    Err(raise_type_error!("Value is not iterable").into())
}

fn bind_for_of_iteration_env<'gc>(
    mc: &MutationContext<'gc>,
    func_env: &JSObjectDataPtr<'gc>,
    decl_kind: Option<VarDeclKind>,
    var_name: &str,
    value: &Value<'gc>,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    match decl_kind {
        Some(VarDeclKind::Let) | Some(VarDeclKind::Const) => {
            let iter_env = crate::core::new_js_object_data(mc);
            iter_env.borrow_mut(mc).prototype = Some(*func_env);
            object_set_key_value(mc, &iter_env, var_name, value)?;
            if matches!(decl_kind, Some(VarDeclKind::Const)) {
                iter_env.borrow_mut(mc).set_const(var_name.to_string());
            }
            Ok(iter_env)
        }
        _ => {
            if object_get_key_value(func_env, var_name).is_none() {
                object_set_key_value(mc, func_env, var_name, &Value::Undefined)?;
            }
            env_set_recursive(mc, func_env, var_name, value)?;
            Ok(*func_env)
        }
    }
}

fn evaluate_for_of_body_first_yield<'gc>(
    mc: &MutationContext<'gc>,
    iter_env: &JSObjectDataPtr<'gc>,
    body: &[Statement],
) -> Result<(YieldKind, Option<Box<Expr>>), EvalError<'gc>> {
    eval_statements_prefix_until_first_yield(mc, iter_env, body)?;
    if let Some((_idx, _inner_idx_opt, yield_kind, yield_inner)) = find_first_yield_in_statements(body) {
        Ok((yield_kind, yield_inner))
    } else {
        Err(raise_eval_error!("Expected yield in for-of body").into())
    }
}

#[allow(clippy::type_complexity)]
fn find_first_for_await_in_statements(stmts: &[Statement]) -> Option<(usize, Option<VarDeclKind>, String, Expr, Vec<Statement>)> {
    for (i, s) in stmts.iter().enumerate() {
        if let StatementKind::ForAwaitOf(decl_kind_opt, var_name, iterable, body) = &*s.kind {
            return Some((i, *decl_kind_opt, var_name.clone(), iterable.clone(), body.clone()));
        }
    }
    None
}

/// Handle generator function calls (creating generator objects)
pub fn handle_generator_function_call<'gc>(
    mc: &MutationContext<'gc>,
    closure: &crate::core::ClosureData<'gc>,
    args: &[Value<'gc>],
    this_val: Option<&Value<'gc>>,
    ctor_prototype: Option<JSObjectDataPtr<'gc>>,
    fn_obj: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let has_param_expressions = closure.params.iter().any(|p| match p {
        crate::core::DestructuringElement::Variable(_, default_opt) => default_opt.is_some(),
        crate::core::DestructuringElement::NestedArray(_, default_opt) => default_opt.is_some(),
        crate::core::DestructuringElement::NestedObject(_, default_opt) => default_opt.is_some(),
        _ => false,
    });

    // Eagerly initialize the function environment to enforce argument destructuring/defaults
    // errors at call time (per spec), rather than delaying to the first .next() call.
    log::debug!("handle_generator_function_call: has_param_expressions={}", has_param_expressions);
    let func_env = if has_param_expressions {
        // Build a separate parameter environment so default initializers do not
        // capture body-level declarations.
        let home_opt = if let Some(home_obj) = &closure.home_object {
            Some(home_obj.clone())
        } else if let Some(fn_obj_ptr) = fn_obj {
            fn_obj_ptr.borrow().get_home_object()
        } else {
            None
        };
        let param_env = prepare_function_call_env_with_home(
            mc,
            closure.env.as_ref(),
            this_val,
            Some(&closure.params[..]),
            args,
            None,
            None,
            home_opt,
        )?;
        // Early: propagate [[HomeObject]] into the parameter environment so
        // any frames created during parameter initialization or early evaluation
        // can see it when resolving `super`.
        if let Some(home_obj) = &closure.home_object {
            param_env.borrow_mut(mc).set_home_object(Some(home_obj.clone()));
        } else if let Some(fn_obj_ptr) = fn_obj
            && let Some(home) = fn_obj_ptr.borrow().get_home_object()
        {
            param_env.borrow_mut(mc).set_home_object(Some(home.clone()));
        }
        let var_env = new_js_object_data(mc);
        var_env.borrow_mut(mc).prototype = Some(param_env);
        var_env.borrow_mut(mc).is_function_scope = true;
        slot_set(mc, &var_env, InternalSlot::IsArrowFunction, &Value::Boolean(false));

        let mut env_strict_ancestor = false;
        if closure.enforce_strictness_inheritance {
            let mut proto_iter = closure.env;
            while let Some(cur) = proto_iter {
                if env_get_strictness(&cur) {
                    env_strict_ancestor = true;
                    break;
                }
                proto_iter = cur.borrow().prototype;
            }
        }
        let fn_is_strict = closure.is_strict || env_strict_ancestor;
        env_set_strictness(mc, &param_env, fn_is_strict)?;
        env_set_strictness(mc, &var_env, fn_is_strict)?;

        if let Some(tv) = this_val {
            object_set_key_value(mc, &var_env, "this", tv)?;
            object_set_key_value(
                mc,
                &var_env,
                "__this_initialized",
                &Value::Boolean(!matches!(tv, Value::Uninitialized)),
            )?;
        }

        if let Some(home_obj) = &closure.home_object {
            var_env.borrow_mut(mc).set_home_object(Some(home_obj.clone()));
            // Also set on the parameter environment so frames whose prototype is
            // the param env can still locate [[HomeObject]]
            param_env.borrow_mut(mc).set_home_object(Some(home_obj.clone()));
        } else if let Some(fn_obj_ptr) = fn_obj {
            // If the closure itself has no [[HomeObject]], prefer the function
            // object's [[HomeObject]] (if present). This covers cases like
            // concise methods where the function object wrapper holds the
            // home object but the underlying closure data may not.
            if let Some(home) = fn_obj_ptr.borrow().get_home_object() {
                var_env.borrow_mut(mc).set_home_object(Some(home.clone()));
                param_env.borrow_mut(mc).set_home_object(Some(home.clone()));
            }
        }

        // Ensure the body environment has its own arguments object.
        crate::js_class::create_arguments_object(mc, &var_env, args, None)?;

        // If a function object was provided (Named Function Expression), bind the name
        // into the parameter environment so the function can reference itself by name.
        if let Some(fn_obj_ptr) = fn_obj
            && let Some(name) = fn_obj_ptr.borrow().get_property("name")
        {
            // Skip creating a per-call name binding for functions that were hoisted
            // as declarations on their creation environment (only Named Function
            // Expressions should get a per-call inner name binding).
            let mut should_bind_name = true;
            if let Some(creation_env) = closure.env
                && let Some(existing_cell) = crate::core::env_get_own(&creation_env, &name)
                && let Value::Object(existing_obj_ptr) = &*existing_cell.borrow()
                && Gc::as_ptr(*existing_obj_ptr) == Gc::as_ptr(fn_obj_ptr)
            {
                should_bind_name = false;
            }
            if should_bind_name {
                crate::core::object_set_key_value(mc, &param_env, &name, &Value::Object(fn_obj_ptr))?;
                if fn_is_strict {
                    param_env.borrow_mut(mc).set_const(name.clone());
                }
            }
        }

        var_env
    } else {
        let call_env = prepare_function_call_env(mc, closure.env.as_ref(), this_val, Some(&closure.params[..]), args, None, None)?;

        // Compute strictness inheritance for the call/env chain so we can mark
        // the named binding as const if appropriate (mirror logic from param_env branch)
        let mut env_strict_ancestor = false;
        if closure.enforce_strictness_inheritance {
            let mut proto_iter = closure.env;
            while let Some(cur) = proto_iter {
                if env_get_strictness(&cur) {
                    env_strict_ancestor = true;
                    break;
                }
                proto_iter = cur.borrow().prototype;
            }
        }
        let fn_is_strict = closure.is_strict || env_strict_ancestor;

        // Propagate [[HomeObject]] into the call environment so `super` resolves correctly
        // for generator functions that do not use parameter expressions.
        if let Some(home_obj) = &closure.home_object {
            call_env.borrow_mut(mc).set_home_object(Some(home_obj.clone()));
        } else if let Some(fn_obj_ptr) = fn_obj
            && let Some(home) = fn_obj_ptr.borrow().get_home_object()
        {
            call_env.borrow_mut(mc).set_home_object(Some(home.clone()));
        }

        // If a function object was provided, bind the name into the call env (parameter env alias)
        if let Some(fn_obj_ptr) = fn_obj
            && let Some(name) = fn_obj_ptr.borrow().get_property("name")
        {
            crate::core::object_set_key_value(mc, &call_env, &name, &Value::Object(fn_obj_ptr))?;
            if fn_is_strict {
                call_env.borrow_mut(mc).set_const(name.clone());
            }
        }

        call_env
    };

    // Create a new generator object (internal data)
    let generator = Gc::new(
        mc,
        GcCell::new(crate::core::JSGenerator {
            params: closure.params.clone(),
            body: closure.body.clone(),
            env: func_env,
            this_val: this_val.cloned(),
            // Store call-time arguments so parameter bindings can be created
            // when the generator actually starts executing on the first `next()`.
            args: args.to_vec(),
            state: crate::core::GeneratorState::NotStarted,
            cached_initial_yield: None,
            pending_iterator: None,
            pending_iterator_done: false,
            yield_star_iterator: None,
            pending_for_await: None,
            pending_for_of: None,
            pending_completion: None,
        }),
    );

    // Create a wrapper object for the generator
    let gen_obj = crate::core::new_js_object_data(mc);

    // Store the actual generator data
    slot_set(mc, &gen_obj, InternalSlot::Generator, &Value::Generator(generator));

    slot_set(mc, &gen_obj, InternalSlot::InGenerator, &Value::Boolean(true));

    // DEBUG: Log the generator object pointer so we can inspect its prototype chain
    let proto_ptr = gen_obj.borrow().prototype.map(Gc::as_ptr);
    log::debug!(
        "handle_generator_function_call: gen_obj ptr = {:p} prototype = {:?}",
        Gc::as_ptr(gen_obj),
        proto_ptr
    );

    // DEBUG: Log ctor_prototype and fn_obj pointers (if provided)
    let ctor_ptr = ctor_prototype.map(Gc::as_ptr);
    let fn_ptr = fn_obj.map(Gc::as_ptr);
    log::debug!(
        "handle_generator_function_call: ctor_prototype = {:?}, fn_obj = {:?}",
        ctor_ptr,
        fn_ptr
    );

    // Determine prototype per GetPrototypeFromConstructor semantics. Prefer the
    // constructor's own 'prototype' property (if available after parameter
    // initialization), otherwise fall back to the realm's Generator.prototype intrinsic.
    // If a function object was provided (`fn_obj`), read its 'prototype' now so
    // parameter initializers can mutate it and be observed at the correct time.
    if let Some(fn_obj_ptr) = fn_obj {
        // If a function object was provided (Named Function Expression), read its
        // own 'prototype' property now that parameter initialization has completed
        // so any mutations in default parameter expressions are observed.
        let ctor_proto_opt = if let Some(proto_val_rc) = object_get_key_value(&fn_obj_ptr, "prototype") {
            match &*proto_val_rc.borrow() {
                Value::Object(proto_obj) => Some(*proto_obj),
                Value::Property { value: Some(v), .. } => {
                    if let Value::Object(o) = &*v.borrow() {
                        Some(*o)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        };

        if let Some(proto_obj) = ctor_proto_opt {
            gen_obj.borrow_mut(mc).prototype = Some(proto_obj);
            let new_proto = gen_obj.borrow().prototype.map(Gc::as_ptr);
            log::debug!(
                "handle_generator_function_call: assigned ctor (post-init) prototype, gen_obj.prototype = {:?}",
                new_proto
            );
            log::trace!(
                "handle_generator_function_call: assigned fn_obj.prototype -> gen_obj.ptr={:p} proto={:p} fn_obj={:p}",
                Gc::as_ptr(gen_obj),
                Gc::as_ptr(proto_obj),
                Gc::as_ptr(fn_obj_ptr)
            );
        } else if let Some(ctor_proto_obj) = ctor_prototype {
            gen_obj.borrow_mut(mc).prototype = Some(ctor_proto_obj);
            let new_proto = gen_obj.borrow().prototype.map(Gc::as_ptr);
            log::debug!(
                "handle_generator_function_call: assigned ctor prototype, gen_obj.prototype = {:?}",
                new_proto
            );
            if let Some(p) = gen_obj.borrow().prototype {
                let pp = p.borrow().prototype.map(Gc::as_ptr);
                log::debug!("handle_generator_function_call: ctor_proto.prototype = {:?}", pp);
            }
            log::trace!(
                "handle_generator_function_call: assigned ctor_prototype -> gen_obj.ptr={:p} proto={:p}",
                Gc::as_ptr(gen_obj),
                Gc::as_ptr(ctor_proto_obj)
            );
        } else if let Some(gen_ctor_val) = crate::core::env_get(closure.env.as_ref().expect("Generator needs env"), "Generator")
            && let Value::Object(gen_ctor_obj) = &*gen_ctor_val.borrow()
            && let Some(proto_val_rc) = object_get_key_value(gen_ctor_obj, "prototype")
        {
            // Handle the case where 'prototype' may be stored as a descriptor (Value::Property)
            let proto_obj_opt = match &*proto_val_rc.borrow() {
                Value::Object(o) => Some(*o),
                Value::Property { value: Some(v), .. } => {
                    if let Value::Object(o) = &*v.borrow() {
                        Some(*o)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(proto_obj) = proto_obj_opt {
                gen_obj.borrow_mut(mc).prototype = Some(proto_obj);
                let new_proto = gen_obj.borrow().prototype.map(Gc::as_ptr);
                log::debug!(
                    "handle_generator_function_call: assigned Generator.prototype (fallback), gen_obj.prototype = {:?}",
                    new_proto
                );
                log::trace!(
                    "handle_generator_function_call: assigned Generator.prototype fallback -> gen_obj.ptr={:p} proto={:p}",
                    Gc::as_ptr(gen_obj),
                    Gc::as_ptr(proto_obj)
                );
            }
        }
    } else if let Some(ctor_proto_obj) = ctor_prototype {
        gen_obj.borrow_mut(mc).prototype = Some(ctor_proto_obj);
        let new_proto = gen_obj.borrow().prototype.map(Gc::as_ptr);
        log::debug!(
            "handle_generator_function_call: assigned ctor prototype, gen_obj.prototype = {:?}",
            new_proto
        );
        if let Some(p) = gen_obj.borrow().prototype {
            let pp = p.borrow().prototype.map(Gc::as_ptr);
            log::debug!("handle_generator_function_call: ctor_proto.prototype = {:?}", pp);
        }
    } else if let Some(gen_ctor_val) = crate::core::env_get(closure.env.as_ref().expect("Generator needs env"), "Generator")
        && let Value::Object(gen_ctor_obj) = &*gen_ctor_val.borrow()
        && let Some(proto_val_rc) = object_get_key_value(gen_ctor_obj, "prototype")
    {
        // Handle the case where 'prototype' may be stored as a descriptor (Value::Property)
        let proto_obj_opt = match &*proto_val_rc.borrow() {
            Value::Object(o) => Some(*o),
            Value::Property { value: Some(v), .. } => {
                if let Value::Object(o) = &*v.borrow() {
                    Some(*o)
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(proto_obj) = proto_obj_opt {
            gen_obj.borrow_mut(mc).prototype = Some(proto_obj);
            let new_proto = gen_obj.borrow().prototype.map(Gc::as_ptr);
            log::debug!(
                "handle_generator_function_call: assigned Generator.prototype, gen_obj.prototype = {:?}",
                new_proto
            );
        }
    } else {
        // DEBUG: Could not find Generator.prototype via constructor lookup. Log environment pointer and whether the global env contains 'Generator'
        if let Some(env_ptr) = closure.env {
            let has_gen = crate::core::env_get(&env_ptr, "Generator").is_some();
            log::debug!(
                "handle_generator_function_call: failed to locate Generator.prototype. closure.env ptr = {:p} has_Generator={}",
                Gc::as_ptr(env_ptr),
                has_gen
            );
        } else {
            log::debug!("handle_generator_function_call: failed to locate Generator.prototype and closure.env is None");
        }
    }

    Ok(Value::Object(gen_obj))
}

/// Handle generator instance method calls (like `gen.next()`, `gen.return()`, etc.)
pub fn handle_generator_instance_method<'gc>(
    mc: &MutationContext<'gc>,
    generator: &crate::core::GcPtr<'gc, crate::core::JSGenerator<'gc>>,
    method: &str,
    args: &[Value<'gc>],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match method {
        "next" => {
            // Get optional value to send to the generator
            let send_value = if args.is_empty() { Value::Undefined } else { args[0].clone() };

            generator_next(mc, generator, &send_value)
        }
        "return" => {
            // Return a value and close the generator
            let return_value = if args.is_empty() { Value::Undefined } else { args[0].clone() };

            Ok(generator_return(mc, generator, &return_value)?)
        }
        "throw" => {
            // Throw an exception into the generator
            let throw_value = if args.is_empty() { Value::Undefined } else { args[0].clone() };
            generator_throw(mc, generator, &throw_value)
        }
        _ => Err(raise_eval_error!(format!("Generator.prototype.{} is not implemented", method)).into()),
    }
}

// Helper to replace the first `yield` occurrence inside an Expr with a
// provided `send_value`. `replaced` becomes true once a replacement is made.
fn replace_first_yield_in_expr(expr: &Expr, var_name: &str, replaced: &mut bool) -> Expr {
    // log::trace!("replace_first_yield_in_expr visiting: {:?}, replaced={}", expr, replaced);
    match expr {
        Expr::Yield(inner) => {
            let new_inner = inner.as_ref().map(|i| Box::new(replace_first_yield_in_expr(i, var_name, replaced)));
            if *replaced {
                Expr::Yield(new_inner)
            } else {
                log::trace!("replace_first_yield_in_expr: Replacing Yield {:?} with var='{}'", inner, var_name);
                *replaced = true;
                Expr::Var(var_name.to_string(), None, None)
            }
        }
        Expr::YieldStar(inner) => {
            let new_inner = Box::new(replace_first_yield_in_expr(inner, var_name, replaced));
            if *replaced {
                Expr::YieldStar(new_inner)
            } else {
                log::trace!(
                    "replace_first_yield_in_expr: Replacing YieldStar {:?} with var='{}'",
                    inner,
                    var_name
                );
                *replaced = true;
                Expr::Var(var_name.to_string(), None, None)
            }
        }
        Expr::Await(inner) => {
            let new_inner = Box::new(replace_first_yield_in_expr(inner, var_name, replaced));
            if *replaced {
                Expr::Await(new_inner)
            } else {
                log::trace!("replace_first_yield_in_expr: Replacing Await {:?} with var='{}'", inner, var_name);
                *replaced = true;
                Expr::Var(var_name.to_string(), None, None)
            }
        }
        Expr::Binary(a, op, b) => Expr::Binary(
            Box::new(replace_first_yield_in_expr(a, var_name, replaced)),
            *op,
            Box::new(replace_first_yield_in_expr(b, var_name, replaced)),
        ),
        Expr::Assign(a, b) => Expr::Assign(
            Box::new(replace_first_yield_in_expr(a, var_name, replaced)),
            Box::new(replace_first_yield_in_expr(b, var_name, replaced)),
        ),
        Expr::Index(a, b) => Expr::Index(
            Box::new(replace_first_yield_in_expr(a, var_name, replaced)),
            Box::new(replace_first_yield_in_expr(b, var_name, replaced)),
        ),
        Expr::Property(a, s) => Expr::Property(Box::new(replace_first_yield_in_expr(a, var_name, replaced)), s.clone()),
        Expr::Call(a, args) => Expr::Call(
            Box::new(replace_first_yield_in_expr(a, var_name, replaced)),
            args.iter()
                .map(|arg| replace_first_yield_in_expr(arg, var_name, replaced))
                .collect(),
        ),
        Expr::Object(pairs) => Expr::Object(
            pairs
                .iter()
                .map(|(k, v, is_method, _)| {
                    (
                        replace_first_yield_in_expr(k, var_name, replaced),
                        replace_first_yield_in_expr(v, var_name, replaced),
                        *is_method,
                        false,
                    )
                })
                .collect(),
        ),
        Expr::Array(items) => Expr::Array(
            items
                .iter()
                .map(|it| it.as_ref().map(|e| replace_first_yield_in_expr(e, var_name, replaced)))
                .collect(),
        ),
        Expr::LogicalNot(a) => Expr::LogicalNot(Box::new(replace_first_yield_in_expr(a, var_name, replaced))),
        Expr::TypeOf(a) => Expr::TypeOf(Box::new(replace_first_yield_in_expr(a, var_name, replaced))),
        Expr::Delete(a) => Expr::Delete(Box::new(replace_first_yield_in_expr(a, var_name, replaced))),
        Expr::Void(a) => Expr::Void(Box::new(replace_first_yield_in_expr(a, var_name, replaced))),
        Expr::Increment(a) => Expr::Increment(Box::new(replace_first_yield_in_expr(a, var_name, replaced))),
        Expr::Decrement(a) => Expr::Decrement(Box::new(replace_first_yield_in_expr(a, var_name, replaced))),
        Expr::PostIncrement(a) => Expr::PostIncrement(Box::new(replace_first_yield_in_expr(a, var_name, replaced))),
        Expr::PostDecrement(a) => Expr::PostDecrement(Box::new(replace_first_yield_in_expr(a, var_name, replaced))),
        Expr::LogicalAnd(a, b) => Expr::LogicalAnd(
            Box::new(replace_first_yield_in_expr(a, var_name, replaced)),
            Box::new(replace_first_yield_in_expr(b, var_name, replaced)),
        ),
        Expr::LogicalOr(a, b) => Expr::LogicalOr(
            Box::new(replace_first_yield_in_expr(a, var_name, replaced)),
            Box::new(replace_first_yield_in_expr(b, var_name, replaced)),
        ),
        Expr::Comma(a, b) => Expr::Comma(
            Box::new(replace_first_yield_in_expr(a, var_name, replaced)),
            Box::new(replace_first_yield_in_expr(b, var_name, replaced)),
        ),
        Expr::Spread(a) => Expr::Spread(Box::new(replace_first_yield_in_expr(a, var_name, replaced))),
        Expr::OptionalCall(a, args) => Expr::OptionalCall(
            Box::new(replace_first_yield_in_expr(a, var_name, replaced)),
            args.iter()
                .map(|arg| replace_first_yield_in_expr(arg, var_name, replaced))
                .collect(),
        ),
        Expr::OptionalIndex(a, b) => Expr::OptionalIndex(
            Box::new(replace_first_yield_in_expr(a, var_name, replaced)),
            Box::new(replace_first_yield_in_expr(b, var_name, replaced)),
        ),
        Expr::Conditional(a, b, c) => Expr::Conditional(
            Box::new(replace_first_yield_in_expr(a, var_name, replaced)),
            Box::new(replace_first_yield_in_expr(b, var_name, replaced)),
            Box::new(replace_first_yield_in_expr(c, var_name, replaced)),
        ),
        Expr::DynamicImport(specifier, options) => Expr::DynamicImport(
            Box::new(replace_first_yield_in_expr(specifier, var_name, replaced)),
            options
                .as_ref()
                .map(|o| Box::new(replace_first_yield_in_expr(o, var_name, replaced))),
        ),
        Expr::Class(class_def) => Expr::Class(Box::new(replace_first_yield_in_class_def(class_def, var_name, replaced))),
        _ => expr.clone(),
    }
}

fn replace_first_yield_in_class_def(class_def: &ClassDefinition, var_name: &str, replaced: &mut bool) -> ClassDefinition {
    let mut def = class_def.clone();

    if let Some(extends_expr) = def.extends.as_mut() {
        let replaced_extends = replace_first_yield_in_expr(extends_expr, var_name, replaced);
        *extends_expr = replaced_extends;
        if *replaced {
            return def;
        }
    }

    for member in def.members.iter_mut() {
        replace_first_yield_in_class_member(member, var_name, replaced);
        if *replaced {
            break;
        }
    }

    def
}

fn replace_first_yield_in_class_member(member: &mut ClassMember, var_name: &str, replaced: &mut bool) {
    match member {
        ClassMember::MethodComputed(key_expr, ..)
        | ClassMember::MethodComputedGenerator(key_expr, ..)
        | ClassMember::MethodComputedAsync(key_expr, ..)
        | ClassMember::MethodComputedAsyncGenerator(key_expr, ..)
        | ClassMember::StaticMethodComputed(key_expr, ..)
        | ClassMember::StaticMethodComputedGenerator(key_expr, ..)
        | ClassMember::StaticMethodComputedAsync(key_expr, ..)
        | ClassMember::StaticMethodComputedAsyncGenerator(key_expr, ..)
        | ClassMember::GetterComputed(key_expr, ..)
        | ClassMember::SetterComputed(key_expr, ..)
        | ClassMember::StaticGetterComputed(key_expr, ..)
        | ClassMember::StaticSetterComputed(key_expr, ..)
        | ClassMember::PropertyComputed(key_expr, ..)
        | ClassMember::StaticPropertyComputed(key_expr, ..) => {
            *key_expr = replace_first_yield_in_expr(key_expr, var_name, replaced);
        }
        ClassMember::StaticProperty(_, value_expr) | ClassMember::PrivateStaticProperty(_, value_expr) => {
            *value_expr = replace_first_yield_in_expr(value_expr, var_name, replaced);
        }
        ClassMember::StaticBlock(body) => {
            for stmt in body.iter_mut() {
                replace_first_yield_in_statement(stmt, var_name, replaced);
                if *replaced {
                    break;
                }
            }
        }
        _ => {}
    }
}

fn find_yield_in_destructuring_element(elem: &DestructuringElement) -> Option<(YieldKind, Option<Box<Expr>>)> {
    match elem {
        DestructuringElement::Variable(_, default_opt) => default_opt.as_ref().and_then(|e| find_yield_in_expr(e)),
        DestructuringElement::Property(_, inner)
        | DestructuringElement::ComputedProperty(_, inner)
        | DestructuringElement::RestPattern(inner) => find_yield_in_destructuring_element(inner),
        DestructuringElement::NestedArray(inner, default_opt) => default_opt
            .as_ref()
            .and_then(|e| find_yield_in_expr(e))
            .or_else(|| inner.iter().find_map(find_yield_in_destructuring_element)),
        DestructuringElement::NestedObject(inner, default_opt) => default_opt
            .as_ref()
            .and_then(|e| find_yield_in_expr(e))
            .or_else(|| inner.iter().find_map(find_yield_in_destructuring_element)),
        DestructuringElement::Empty | DestructuringElement::Rest(_) => None,
    }
}

fn find_yield_in_object_destructuring_element(elem: &ObjectDestructuringElement) -> Option<(YieldKind, Option<Box<Expr>>)> {
    match elem {
        ObjectDestructuringElement::Property { value, .. } => find_yield_in_destructuring_element(value),
        ObjectDestructuringElement::ComputedProperty { key, value } => {
            find_yield_in_expr(key).or_else(|| find_yield_in_destructuring_element(value))
        }
        ObjectDestructuringElement::Rest(_) => None,
    }
}

fn replace_first_yield_in_destructuring_element(elem: &mut DestructuringElement, var_name: &str, replaced: &mut bool) {
    if *replaced {
        return;
    }
    match elem {
        DestructuringElement::Variable(_, default_opt) => {
            if let Some(def) = default_opt.as_mut() {
                let replaced_expr = replace_first_yield_in_expr(def, var_name, replaced);
                **def = replaced_expr;
            }
        }
        DestructuringElement::Property(_, inner)
        | DestructuringElement::ComputedProperty(_, inner)
        | DestructuringElement::RestPattern(inner) => {
            replace_first_yield_in_destructuring_element(inner, var_name, replaced);
        }
        DestructuringElement::NestedArray(inner, default_opt) | DestructuringElement::NestedObject(inner, default_opt) => {
            if let Some(def) = default_opt.as_mut() {
                let replaced_expr = replace_first_yield_in_expr(def, var_name, replaced);
                **def = replaced_expr;
                if *replaced {
                    return;
                }
            }
            for e in inner.iter_mut() {
                replace_first_yield_in_destructuring_element(e, var_name, replaced);
                if *replaced {
                    return;
                }
            }
        }
        DestructuringElement::Empty | DestructuringElement::Rest(_) => {}
    }
}

fn replace_first_yield_in_object_destructuring_element(elem: &mut ObjectDestructuringElement, var_name: &str, replaced: &mut bool) {
    if *replaced {
        return;
    }
    match elem {
        ObjectDestructuringElement::Property { value, .. } => {
            replace_first_yield_in_destructuring_element(value, var_name, replaced);
        }
        ObjectDestructuringElement::ComputedProperty { key, value } => {
            *key = replace_first_yield_in_expr(key, var_name, replaced);
            if *replaced {
                return;
            }
            replace_first_yield_in_destructuring_element(value, var_name, replaced);
        }
        ObjectDestructuringElement::Rest(_) => {}
    }
}

pub(crate) fn replace_first_yield_in_statement(stmt: &mut Statement, var_name: &str, replaced: &mut bool) {
    match stmt.kind.as_mut() {
        StatementKind::Expr(e) => {
            *e = replace_first_yield_in_expr(e, var_name, replaced);
        }
        StatementKind::Let(decls) | StatementKind::Var(decls) => {
            for (_, expr_opt) in decls.iter_mut() {
                if let Some(expr) = expr_opt {
                    *expr = replace_first_yield_in_expr(expr, var_name, replaced);
                }
            }
        }
        StatementKind::Const(decls) => {
            for (_, expr) in decls.iter_mut() {
                *expr = replace_first_yield_in_expr(expr, var_name, replaced);
            }
        }
        StatementKind::Return(Some(expr)) => {
            *expr = replace_first_yield_in_expr(expr, var_name, replaced);
        }
        StatementKind::If(if_stmt) => {
            let if_stmt = if_stmt.as_mut();
            let cond = if_stmt.condition.clone();
            if_stmt.condition = replace_first_yield_in_expr(&cond, var_name, replaced);
            for s in if_stmt.then_body.iter_mut() {
                replace_first_yield_in_statement(s, var_name, replaced);
                if *replaced {
                    return;
                }
            }
            if let Some(else_body) = if_stmt.else_body.as_mut() {
                for s in else_body.iter_mut() {
                    replace_first_yield_in_statement(s, var_name, replaced);
                    if *replaced {
                        return;
                    }
                }
            }
        }
        StatementKind::For(for_stmt) => {
            let for_stmt = for_stmt.as_mut();
            if let Some(init) = for_stmt.init.as_mut() {
                replace_first_yield_in_statement(init, var_name, replaced);
                if *replaced {
                    return;
                }
            }
            if let Some(cond) = for_stmt.test.as_mut() {
                *cond = replace_first_yield_in_expr(cond, var_name, replaced);
                if *replaced {
                    return;
                }
            }
            if let Some(update) = for_stmt.update.as_mut() {
                replace_first_yield_in_statement(update, var_name, replaced);
                if *replaced {
                    return;
                }
            }
            for s in for_stmt.body.iter_mut() {
                replace_first_yield_in_statement(s, var_name, replaced);
                if *replaced {
                    return;
                }
            }
        }
        StatementKind::While(cond, body) => {
            *cond = replace_first_yield_in_expr(cond, var_name, replaced);
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, var_name, replaced);
                if *replaced {
                    return;
                }
            }
        }
        StatementKind::DoWhile(body, cond) => {
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, var_name, replaced);
                if *replaced {
                    return;
                }
            }
            *cond = replace_first_yield_in_expr(cond, var_name, replaced);
        }
        StatementKind::ForOf(_, _, iterable, body) | StatementKind::ForIn(_, _, iterable, body) => {
            *iterable = replace_first_yield_in_expr(iterable, var_name, replaced);
            if *replaced {
                return;
            }
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, var_name, replaced);
                if *replaced {
                    return;
                }
            }
        }
        StatementKind::ForOfDestructuringObject(_, pattern, iterable, body)
        | StatementKind::ForInDestructuringObject(_, pattern, iterable, body)
        | StatementKind::ForAwaitOfDestructuringObject(_, pattern, iterable, body) => {
            for p in pattern.iter_mut() {
                replace_first_yield_in_object_destructuring_element(p, var_name, replaced);
                if *replaced {
                    return;
                }
            }
            *iterable = replace_first_yield_in_expr(iterable, var_name, replaced);
            if *replaced {
                return;
            }
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, var_name, replaced);
                if *replaced {
                    return;
                }
            }
        }
        StatementKind::ForOfDestructuringArray(_, pattern, iterable, body)
        | StatementKind::ForInDestructuringArray(_, pattern, iterable, body)
        | StatementKind::ForAwaitOfDestructuringArray(_, pattern, iterable, body) => {
            for p in pattern.iter_mut() {
                replace_first_yield_in_destructuring_element(p, var_name, replaced);
                if *replaced {
                    return;
                }
            }
            *iterable = replace_first_yield_in_expr(iterable, var_name, replaced);
            if *replaced {
                return;
            }
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, var_name, replaced);
                if *replaced {
                    return;
                }
            }
        }
        StatementKind::ForAwaitOfExpr(lhs, iterable, body)
        | StatementKind::ForOfExpr(lhs, iterable, body)
        | StatementKind::ForInExpr(lhs, iterable, body) => {
            let lhs_replaced = replace_first_yield_in_expr(lhs, var_name, replaced);
            *lhs = lhs_replaced;
            if *replaced {
                return;
            }

            let iterable_replaced = replace_first_yield_in_expr(iterable, var_name, replaced);
            *iterable = iterable_replaced;
            if *replaced {
                return;
            }

            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, var_name, replaced);
                if *replaced {
                    return;
                }
            }
        }
        StatementKind::Block(stmts) => {
            for s in stmts.iter_mut() {
                replace_first_yield_in_statement(s, var_name, replaced);
                if *replaced {
                    return;
                }
            }
        }
        StatementKind::TryCatch(tc_stmt) => {
            let tc_stmt = tc_stmt.as_mut();
            for s in tc_stmt.try_body.iter_mut() {
                replace_first_yield_in_statement(s, var_name, replaced);
                if *replaced {
                    return;
                }
            }
            if let Some(catch_body) = tc_stmt.catch_body.as_mut() {
                for s in catch_body.iter_mut() {
                    replace_first_yield_in_statement(s, var_name, replaced);
                    if *replaced {
                        return;
                    }
                }
            }
            if let Some(finally_body) = tc_stmt.finally_body.as_mut() {
                for s in finally_body.iter_mut() {
                    replace_first_yield_in_statement(s, var_name, replaced);
                    if *replaced {
                        return;
                    }
                }
            }
        }
        StatementKind::Class(class_def) => {
            let replaced_def = replace_first_yield_in_class_def(class_def, var_name, replaced);
            **class_def = replaced_def;
        }
        _ => {}
    }
}

fn expr_contains_yield(e: &Expr) -> bool {
    match e {
        Expr::Yield(_) | Expr::YieldStar(_) | Expr::Await(_) => true,
        Expr::Binary(a, _, b) => expr_contains_yield(a) || expr_contains_yield(b),
        Expr::Assign(a, b) => expr_contains_yield(a) || expr_contains_yield(b),
        Expr::Index(a, b) => expr_contains_yield(a) || expr_contains_yield(b),
        Expr::Property(a, _) => expr_contains_yield(a),
        Expr::Call(a, args) => expr_contains_yield(a) || args.iter().any(expr_contains_yield),
        Expr::Object(pairs) => pairs.iter().any(|(k, v, _, _)| expr_contains_yield(k) || expr_contains_yield(v)),
        Expr::Array(items) => items.iter().any(|it| it.as_ref().is_some_and(expr_contains_yield)),
        Expr::UnaryNeg(a)
        | Expr::LogicalNot(a)
        | Expr::TypeOf(a)
        | Expr::Delete(a)
        | Expr::Void(a)
        | Expr::Spread(a)
        | Expr::PostIncrement(a)
        | Expr::PostDecrement(a)
        | Expr::Increment(a)
        | Expr::Decrement(a) => expr_contains_yield(a),
        Expr::LogicalAnd(a, b) | Expr::LogicalOr(a, b) | Expr::Comma(a, b) | Expr::Conditional(a, b, _) => {
            expr_contains_yield(a) || expr_contains_yield(b)
        }
        Expr::OptionalCall(a, args) => expr_contains_yield(a) || args.iter().any(expr_contains_yield),
        Expr::OptionalIndex(a, b) => expr_contains_yield(a) || expr_contains_yield(b),
        Expr::DynamicImport(specifier, options) => {
            expr_contains_yield(specifier) || options.as_ref().map(|o| expr_contains_yield(o)).unwrap_or(false)
        }
        Expr::Class(class_def) => expr_contains_yield_in_class_def(class_def),
        _ => false,
    }
}

fn eval_prefix_until_first_yield<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, expr: &Expr) -> Result<bool, EvalError<'gc>> {
    match expr {
        Expr::Yield(_) | Expr::YieldStar(_) | Expr::Await(_) => Ok(true),
        Expr::Comma(left, right) => {
            if eval_prefix_until_first_yield(mc, env, left)? {
                return Ok(true);
            }
            eval_prefix_until_first_yield(mc, env, right)
        }
        Expr::DynamicImport(specifier, options) => {
            if eval_prefix_until_first_yield(mc, env, specifier)? {
                return Ok(true);
            }
            if let Some(options_expr) = options
                && eval_prefix_until_first_yield(mc, env, options_expr)?
            {
                return Ok(true);
            }
            let _ = crate::core::evaluate_expr(mc, env, expr)?;
            Ok(false)
        }
        Expr::Call(callee, args) | Expr::OptionalCall(callee, args) | Expr::New(callee, args) => {
            if eval_prefix_until_first_yield(mc, env, callee)? {
                return Ok(true);
            }
            for arg in args {
                if eval_prefix_until_first_yield(mc, env, arg)? {
                    return Ok(true);
                }
            }
            let _ = crate::core::evaluate_expr(mc, env, expr)?;
            Ok(false)
        }
        _ => {
            if expr_contains_yield(expr) {
                Ok(true)
            } else {
                let _ = crate::core::evaluate_expr(mc, env, expr)?;
                Ok(false)
            }
        }
    }
}

fn eval_statement_prefix_until_first_yield<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    stmt: &Statement,
) -> Result<(), EvalError<'gc>> {
    match &*stmt.kind {
        StatementKind::Expr(expr) => {
            let _ = eval_prefix_until_first_yield(mc, env, expr)?;
        }
        StatementKind::Block(inner) => {
            eval_statements_prefix_until_first_yield(mc, env, inner)?;
        }
        StatementKind::TryCatch(tc_stmt) => {
            if find_first_yield_in_statements(&tc_stmt.try_body).is_some() {
                eval_statements_prefix_until_first_yield(mc, env, &tc_stmt.try_body)?;
                return Ok(());
            }

            let mut thrown_value: Option<Value> = None;
            if !tc_stmt.try_body.is_empty() {
                match crate::core::evaluate_statements_with_context_and_last_value(mc, env, &tc_stmt.try_body, &[]) {
                    Ok((cf, _)) => {
                        if let crate::core::ControlFlow::Throw(v, _, _) = cf {
                            thrown_value = Some(v);
                        }
                    }
                    Err(EvalError::Throw(v, _, _)) => {
                        thrown_value = Some(v);
                    }
                    Err(EvalError::Js(js_err)) => {
                        thrown_value = Some(crate::core::js_error_to_value(mc, env, &js_err));
                    }
                }
            }

            if let Some(catch_body) = &tc_stmt.catch_body
                && find_first_yield_in_statements(catch_body).is_some()
            {
                let catch_env = crate::core::new_js_object_data(mc);
                catch_env.borrow_mut(mc).prototype = Some(*env);
                if let Some(catch_val) = thrown_value
                    && let Some(CatchParamPattern::Identifier(name)) = &tc_stmt.catch_param
                {
                    object_set_key_value(mc, &catch_env, name, &catch_val)?;
                }
                eval_statements_prefix_until_first_yield(mc, &catch_env, catch_body)?;
                return Ok(());
            }

            if let Some(finally_body) = &tc_stmt.finally_body
                && find_first_yield_in_statements(finally_body).is_some()
            {
                let finally_env = crate::core::new_js_object_data(mc);
                finally_env.borrow_mut(mc).prototype = Some(*env);
                eval_statements_prefix_until_first_yield(mc, &finally_env, finally_body)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn eval_statements_prefix_until_first_yield<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    stmts: &[Statement],
) -> Result<(), EvalError<'gc>> {
    if let Some((idx, _inner_idx_opt, _yield_kind, _yield_inner)) = find_first_yield_in_statements(stmts) {
        if idx > 0 {
            crate::core::evaluate_statements(mc, env, &stmts[0..idx])?;
        }
        if let Some(stmt) = stmts.get(idx) {
            eval_statement_prefix_until_first_yield(mc, env, stmt)?;
        }
    }

    Ok(())
}

fn trim_statement_to_post_first_yield(stmt: &mut Statement) {
    match &mut *stmt.kind {
        StatementKind::Block(inner) => {
            trim_statements_to_post_first_yield(inner);
        }
        StatementKind::TryCatch(tc_stmt) => {
            if find_first_yield_in_statements(&tc_stmt.try_body).is_some() {
                trim_statements_to_post_first_yield(&mut tc_stmt.try_body);
            } else if let Some(catch_body) = tc_stmt.catch_body.as_mut()
                && find_first_yield_in_statements(catch_body).is_some()
            {
                trim_statements_to_post_first_yield(catch_body);
            } else if let Some(finally_body) = tc_stmt.finally_body.as_mut()
                && find_first_yield_in_statements(finally_body).is_some()
            {
                trim_statements_to_post_first_yield(finally_body);
            }
        }
        _ => {}
    }
}

fn trim_statements_to_post_first_yield(stmts: &mut Vec<Statement>) {
    if let Some((idx, _inner_idx_opt, _yield_kind, _yield_inner)) = find_first_yield_in_statements(stmts) {
        if idx > 0 {
            stmts.drain(0..idx);
        }
        if let Some(first_stmt) = stmts.first_mut() {
            trim_statement_to_post_first_yield(first_stmt);
        }
    }
}

fn expr_contains_yield_in_class_def(class_def: &ClassDefinition) -> bool {
    if let Some(extends_expr) = &class_def.extends
        && expr_contains_yield(extends_expr)
    {
        return true;
    }

    class_def.members.iter().any(expr_contains_yield_in_class_member)
}

fn expr_contains_yield_in_class_member(member: &ClassMember) -> bool {
    match member {
        ClassMember::MethodComputed(key_expr, ..)
        | ClassMember::MethodComputedGenerator(key_expr, ..)
        | ClassMember::MethodComputedAsync(key_expr, ..)
        | ClassMember::MethodComputedAsyncGenerator(key_expr, ..)
        | ClassMember::StaticMethodComputed(key_expr, ..)
        | ClassMember::StaticMethodComputedGenerator(key_expr, ..)
        | ClassMember::StaticMethodComputedAsync(key_expr, ..)
        | ClassMember::StaticMethodComputedAsyncGenerator(key_expr, ..)
        | ClassMember::GetterComputed(key_expr, ..)
        | ClassMember::SetterComputed(key_expr, ..)
        | ClassMember::StaticGetterComputed(key_expr, ..)
        | ClassMember::StaticSetterComputed(key_expr, ..)
        | ClassMember::PropertyComputed(key_expr, ..)
        | ClassMember::StaticPropertyComputed(key_expr, ..) => expr_contains_yield(key_expr),
        ClassMember::StaticProperty(_, value_expr) | ClassMember::PrivateStaticProperty(_, value_expr) => expr_contains_yield(value_expr),
        ClassMember::StaticBlock(body) => find_first_yield_in_statements(body).is_some(),
        _ => false,
    }
}

// Replace the first nested statement containing a yield with a Throw statement
// holding `throw_value`. Returns true if a replacement was performed.
pub(crate) fn replace_first_yield_statement_with_throw(stmt: &mut Statement, _throw_value: &Value) -> bool {
    match stmt.kind.as_mut() {
        StatementKind::Expr(e) => {
            if expr_contains_yield(e) {
                *stmt.kind = StatementKind::Throw(Expr::Var("__gen_throw_val".to_string(), None, None));
                return true;
            }
            false
        }
        StatementKind::Let(decls) | StatementKind::Var(decls) => {
            for (_, expr_opt) in decls {
                if let Some(expr) = expr_opt
                    && expr_contains_yield(expr)
                {
                    *stmt.kind = StatementKind::Throw(Expr::Var("__gen_throw_val".to_string(), None, None));
                    return true;
                }
            }
            false
        }
        StatementKind::Const(decls) => {
            for (_, expr) in decls {
                if expr_contains_yield(expr) {
                    *stmt.kind = StatementKind::Throw(Expr::Var("__gen_throw_val".to_string(), None, None));
                    return true;
                }
            }
            false
        }
        StatementKind::Return(Some(expr)) => {
            if expr_contains_yield(expr) {
                *stmt.kind = StatementKind::Throw(Expr::Var("__gen_throw_val".to_string(), None, None));
                return true;
            }
            false
        }
        StatementKind::Class(class_def) => {
            if expr_contains_yield_in_class_def(class_def) {
                *stmt.kind = StatementKind::Throw(Expr::Var("__gen_throw_val".to_string(), None, None));
                return true;
            }
            false
        }
        StatementKind::If(if_stmt) => {
            let if_stmt = if_stmt.as_mut();
            for s in if_stmt.then_body.iter_mut() {
                if replace_first_yield_statement_with_throw(s, _throw_value) {
                    return true;
                }
            }
            if let Some(else_body) = if_stmt.else_body.as_mut() {
                for s in else_body.iter_mut() {
                    if replace_first_yield_statement_with_throw(s, _throw_value) {
                        return true;
                    }
                }
            }
            false
        }
        StatementKind::Block(stmts) => {
            for s in stmts.iter_mut() {
                if replace_first_yield_statement_with_throw(s, _throw_value) {
                    return true;
                }
            }
            false
        }
        StatementKind::For(for_stmt) => {
            for s in for_stmt.as_mut().body.iter_mut() {
                if replace_first_yield_statement_with_throw(s, _throw_value) {
                    return true;
                }
            }
            false
        }
        StatementKind::ForOf(_, _, _, body)
        | StatementKind::ForIn(_, _, _, body)
        | StatementKind::ForOfDestructuringObject(_, _, _, body)
        | StatementKind::ForOfDestructuringArray(_, _, _, body)
        | StatementKind::While(_, body) => {
            for s in body.iter_mut() {
                if replace_first_yield_statement_with_throw(s, _throw_value) {
                    return true;
                }
            }
            false
        }
        StatementKind::DoWhile(body, _) => {
            for s in body.iter_mut() {
                if replace_first_yield_statement_with_throw(s, _throw_value) {
                    return true;
                }
            }
            false
        }
        StatementKind::TryCatch(tc_stmt) => {
            let tc_stmt = tc_stmt.as_mut();
            for s in tc_stmt.try_body.iter_mut() {
                if replace_first_yield_statement_with_throw(s, _throw_value) {
                    return true;
                }
            }
            if let Some(catch_body) = tc_stmt.catch_body.as_mut() {
                for s in catch_body.iter_mut() {
                    if replace_first_yield_statement_with_throw(s, _throw_value) {
                        return true;
                    }
                }
            }
            if let Some(finally) = tc_stmt.finally_body.as_mut() {
                for s in finally.iter_mut() {
                    if replace_first_yield_statement_with_throw(s, _throw_value) {
                        return true;
                    }
                }
            }
            false
        }
        _ => false,
    }
}

// Replace the first nested statement containing a yield with a Return statement
// returning `__gen_throw_val`. Returns true if a replacement was performed.
pub(crate) fn replace_first_yield_statement_with_return(stmt: &mut Statement) -> bool {
    match stmt.kind.as_mut() {
        StatementKind::Expr(e) => {
            if expr_contains_yield(e) {
                *stmt.kind = StatementKind::Return(Some(Expr::Var("__gen_throw_val".to_string(), None, None)));
                return true;
            }
            false
        }
        StatementKind::Let(decls) | StatementKind::Var(decls) => {
            for (_, expr_opt) in decls {
                if let Some(expr) = expr_opt
                    && expr_contains_yield(expr)
                {
                    *stmt.kind = StatementKind::Return(Some(Expr::Var("__gen_throw_val".to_string(), None, None)));
                    return true;
                }
            }
            false
        }
        StatementKind::Const(decls) => {
            for (_, expr) in decls {
                if expr_contains_yield(expr) {
                    *stmt.kind = StatementKind::Return(Some(Expr::Var("__gen_throw_val".to_string(), None, None)));
                    return true;
                }
            }
            false
        }
        StatementKind::Return(Some(expr)) => {
            if expr_contains_yield(expr) {
                *stmt.kind = StatementKind::Return(Some(Expr::Var("__gen_throw_val".to_string(), None, None)));
                return true;
            }
            false
        }
        StatementKind::Class(class_def) => {
            if expr_contains_yield_in_class_def(class_def) {
                *stmt.kind = StatementKind::Return(Some(Expr::Var("__gen_throw_val".to_string(), None, None)));
                return true;
            }
            false
        }
        StatementKind::If(if_stmt) => {
            let if_stmt = if_stmt.as_mut();
            for s in if_stmt.then_body.iter_mut() {
                if replace_first_yield_statement_with_return(s) {
                    return true;
                }
            }
            if let Some(else_body) = if_stmt.else_body.as_mut() {
                for s in else_body.iter_mut() {
                    if replace_first_yield_statement_with_return(s) {
                        return true;
                    }
                }
            }
            false
        }
        StatementKind::Block(stmts) => {
            for s in stmts.iter_mut() {
                if replace_first_yield_statement_with_return(s) {
                    return true;
                }
            }
            false
        }
        StatementKind::For(for_stmt) => {
            for s in for_stmt.as_mut().body.iter_mut() {
                if replace_first_yield_statement_with_return(s) {
                    return true;
                }
            }
            false
        }
        StatementKind::ForOf(_, _, _, body)
        | StatementKind::ForIn(_, _, _, body)
        | StatementKind::ForOfDestructuringObject(_, _, _, body)
        | StatementKind::ForOfDestructuringArray(_, _, _, body)
        | StatementKind::While(_, body) => {
            for s in body.iter_mut() {
                if replace_first_yield_statement_with_return(s) {
                    return true;
                }
            }
            false
        }
        StatementKind::DoWhile(body, _) => {
            for s in body.iter_mut() {
                if replace_first_yield_statement_with_return(s) {
                    return true;
                }
            }
            false
        }
        StatementKind::TryCatch(tc_stmt) => {
            let tc_stmt = tc_stmt.as_mut();
            for s in tc_stmt.try_body.iter_mut() {
                if replace_first_yield_statement_with_return(s) {
                    return true;
                }
            }
            if let Some(catch_body) = tc_stmt.catch_body.as_mut() {
                for s in catch_body.iter_mut() {
                    if replace_first_yield_statement_with_return(s) {
                        return true;
                    }
                }
            }
            if let Some(finally) = tc_stmt.finally_body.as_mut() {
                for s in finally.iter_mut() {
                    if replace_first_yield_statement_with_return(s) {
                        return true;
                    }
                }
            }
            false
        }
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YieldKind {
    Yield,
    YieldStar,
    Await,
}

fn find_yield_in_expr(e: &Expr) -> Option<(YieldKind, Option<Box<Expr>>)> {
    match e {
        Expr::Yield(inner) => {
            if let Some(res) = inner.as_ref().and_then(|i| find_yield_in_expr(i)) {
                return Some(res);
            }
            Some((YieldKind::Yield, inner.clone()))
        }
        Expr::YieldStar(inner) => Some((YieldKind::YieldStar, Some(inner.clone()))),
        Expr::Await(inner) => {
            if let Some(res) = find_yield_in_expr(inner) {
                return Some(res);
            }
            Some((YieldKind::Await, Some(inner.clone())))
        }
        Expr::Binary(a, _, b) => find_yield_in_expr(a).or_else(|| find_yield_in_expr(b)),
        Expr::Assign(a, b) => find_yield_in_expr(a).or_else(|| find_yield_in_expr(b)),
        Expr::Index(a, b) => find_yield_in_expr(a).or_else(|| find_yield_in_expr(b)),
        Expr::Property(a, _) => find_yield_in_expr(a),
        Expr::Call(a, args) => find_yield_in_expr(a).or_else(|| args.iter().find_map(find_yield_in_expr)),
        Expr::Object(pairs) => pairs
            .iter()
            .find_map(|(k, v, _, _)| find_yield_in_expr(k).or_else(|| find_yield_in_expr(v))),
        Expr::Array(items) => items.iter().find_map(|it| it.as_ref().and_then(find_yield_in_expr)),
        Expr::UnaryNeg(a)
        | Expr::LogicalNot(a)
        | Expr::TypeOf(a)
        | Expr::Delete(a)
        | Expr::Void(a)
        | Expr::Increment(a)
        | Expr::Decrement(a)
        | Expr::PostIncrement(a)
        | Expr::PostDecrement(a)
        | Expr::Spread(a) => find_yield_in_expr(a),
        Expr::LogicalAnd(a, b) | Expr::LogicalOr(a, b) | Expr::Comma(a, b) => find_yield_in_expr(a).or_else(|| find_yield_in_expr(b)),
        Expr::Conditional(a, b, c) => find_yield_in_expr(a)
            .or_else(|| find_yield_in_expr(b))
            .or_else(|| find_yield_in_expr(c)),
        Expr::OptionalCall(a, args) => find_yield_in_expr(a).or_else(|| args.iter().find_map(find_yield_in_expr)),
        Expr::OptionalIndex(a, b) => find_yield_in_expr(a).or_else(|| find_yield_in_expr(b)),
        Expr::DynamicImport(specifier, options) => {
            find_yield_in_expr(specifier).or_else(|| options.as_ref().and_then(|o| find_yield_in_expr(o)))
        }
        Expr::Class(class_def) => find_yield_in_class_def(class_def),
        _ => None,
    }
}

fn find_yield_in_class_def(class_def: &ClassDefinition) -> Option<(YieldKind, Option<Box<Expr>>)> {
    if let Some(extends_expr) = &class_def.extends
        && let Some(found) = find_yield_in_expr(extends_expr)
    {
        return Some(found);
    }

    for member in &class_def.members {
        if let Some(found) = find_yield_in_class_member(member) {
            return Some(found);
        }
    }

    None
}

fn find_yield_in_class_member(member: &ClassMember) -> Option<(YieldKind, Option<Box<Expr>>)> {
    match member {
        ClassMember::MethodComputed(key_expr, ..)
        | ClassMember::MethodComputedGenerator(key_expr, ..)
        | ClassMember::MethodComputedAsync(key_expr, ..)
        | ClassMember::MethodComputedAsyncGenerator(key_expr, ..)
        | ClassMember::StaticMethodComputed(key_expr, ..)
        | ClassMember::StaticMethodComputedGenerator(key_expr, ..)
        | ClassMember::StaticMethodComputedAsync(key_expr, ..)
        | ClassMember::StaticMethodComputedAsyncGenerator(key_expr, ..)
        | ClassMember::GetterComputed(key_expr, ..)
        | ClassMember::SetterComputed(key_expr, ..)
        | ClassMember::StaticGetterComputed(key_expr, ..)
        | ClassMember::StaticSetterComputed(key_expr, ..)
        | ClassMember::PropertyComputed(key_expr, ..)
        | ClassMember::StaticPropertyComputed(key_expr, ..) => find_yield_in_expr(key_expr),
        ClassMember::StaticProperty(_, value_expr) | ClassMember::PrivateStaticProperty(_, value_expr) => find_yield_in_expr(value_expr),
        ClassMember::StaticBlock(body) => find_first_yield_in_statements(body).map(|(_, _, kind, inner)| (kind, inner)),
        _ => None,
    }
}

fn find_array_assign(expr: &Expr) -> Option<(&Expr, &Expr)> {
    match expr {
        Expr::Assign(lhs, rhs) => {
            if matches!(&**lhs, Expr::Array(_)) {
                Some((&**lhs, &**rhs))
            } else {
                find_array_assign(rhs)
            }
        }
        Expr::Comma(_, rhs) => find_array_assign(rhs),
        Expr::Conditional(_, then_expr, else_expr) => find_array_assign(then_expr).or_else(|| find_array_assign(else_expr)),
        _ => None,
    }
}

#[allow(dead_code)]
fn find_rightmost_assign_rhs(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Assign(_, rhs) => find_rightmost_assign_rhs(rhs).or(Some(&**rhs)),
        Expr::Comma(_, rhs) => find_rightmost_assign_rhs(rhs),
        Expr::Conditional(_, then_expr, else_expr) => find_rightmost_assign_rhs(then_expr).or_else(|| find_rightmost_assign_rhs(else_expr)),
        _ => None,
    }
}

fn seed_simple_decl_bindings<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    stmts: &[Statement],
) -> Result<(), EvalError<'gc>> {
    for stmt in stmts {
        match &*stmt.kind {
            StatementKind::Const(decls) => {
                for (name, expr) in decls {
                    if env_get(env, name).is_none()
                        && let Expr::Var(_, _, _) = expr
                    {
                        let val = evaluate_expr(mc, env, expr)?;
                        env_set(mc, env, name, &val)?;
                    }
                }
            }
            StatementKind::Let(decls) | StatementKind::Var(decls) => {
                for (name, expr_opt) in decls {
                    if env_get(env, name).is_none() {
                        let expr = match expr_opt {
                            Some(expr) => expr,
                            None => continue,
                        };
                        if let Expr::Var(_, _, _) = expr {
                            let val = evaluate_expr(mc, env, expr)?;
                            env_set(mc, env, name, &val)?;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn bind_replaced_yield_decl<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    stmt: &Statement,
    yield_var: &str,
) -> Result<(), EvalError<'gc>> {
    match &*stmt.kind {
        StatementKind::Const(decls) => {
            for (name, expr) in decls {
                if let Expr::Var(var_name, _, _) = expr
                    && var_name == yield_var
                    && env_get(env, name).is_none()
                {
                    let val = evaluate_expr(mc, env, expr)?;
                    env_set(mc, env, name, &val)?;
                }
            }
        }
        StatementKind::Let(decls) | StatementKind::Var(decls) => {
            for (name, expr_opt) in decls {
                let expr = match expr_opt {
                    Some(expr) => expr,
                    None => continue,
                };
                if let Expr::Var(var_name, _, _) = expr
                    && var_name == yield_var
                    && env_get(env, name).is_none()
                {
                    let val = evaluate_expr(mc, env, expr)?;
                    env_set(mc, env, name, &val)?;
                }
            }
        }
        StatementKind::TryCatch(tc_stmt) => {
            for inner in &tc_stmt.try_body {
                bind_replaced_yield_decl(mc, env, inner, yield_var)?;
            }
        }
        StatementKind::Block(stmts) => {
            for inner in stmts {
                bind_replaced_yield_decl(mc, env, inner, yield_var)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn for_init_needs_execution<'gc>(env: &JSObjectDataPtr<'gc>, init_stmt: &Statement) -> bool {
    match &*init_stmt.kind {
        StatementKind::Let(decls) | StatementKind::Var(decls) => decls.iter().any(|(name, _)| object_get_key_value(env, name).is_none()),
        StatementKind::Const(decls) => decls.iter().any(|(name, _)| object_get_key_value(env, name).is_none()),
        _ => true,
    }
}

fn prepare_pending_iterator_for_yield<'gc>(
    mc: &MutationContext<'gc>,
    eval_env: &JSObjectDataPtr<'gc>,
    stmt: &Statement,
) -> Result<Option<(JSObjectDataPtr<'gc>, bool)>, EvalError<'gc>> {
    let mut pre_steps: usize = 0;

    let rhs_val: Value<'gc> = match &*stmt.kind {
        StatementKind::Expr(expr) => {
            let (lhs, rhs) = if let Some(pair) = find_array_assign(expr) {
                pair
            } else {
                return Ok(None);
            };

            if let Expr::Array(elements) = lhs {
                let mut consumed = 0usize;
                for elem_opt in elements.iter() {
                    match elem_opt {
                        None => consumed += 1,
                        Some(elem) => match elem {
                            Expr::Spread(inner) => {
                                if find_yield_in_expr(inner).is_some() {
                                    pre_steps = consumed;
                                }
                                break;
                            }
                            _ => {
                                if find_yield_in_expr(elem).is_some() {
                                    pre_steps = consumed + 1;
                                    break;
                                }
                                consumed += 1;
                            }
                        },
                    }
                }
            } else {
                return Ok(None);
            }

            crate::core::evaluate_expr(mc, eval_env, rhs)?
        }
        StatementKind::ForOfExpr(lhs, iterable, _) => {
            let elements = if let Expr::Array(elements) = lhs {
                elements
            } else {
                return Ok(None);
            };

            let mut consumed = 0usize;
            for elem_opt in elements.iter() {
                match elem_opt {
                    None => consumed += 1,
                    Some(elem) => match elem {
                        Expr::Spread(inner) => {
                            if find_yield_in_expr(inner).is_some() {
                                pre_steps = consumed;
                            }
                            break;
                        }
                        _ => {
                            if find_yield_in_expr(elem).is_some() {
                                pre_steps = consumed + 1;
                                break;
                            }
                            consumed += 1;
                        }
                    },
                }
            }

            let iter_val = crate::core::evaluate_expr(mc, eval_env, iterable)?;
            let outer_iter = get_iterator(mc, &iter_val, eval_env)?;
            let next_method = crate::core::get_property_with_accessors(mc, eval_env, &outer_iter, "next")?;
            if matches!(next_method, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Iterator has no next method").into());
            }
            let next_res_val = evaluate_call_dispatch(mc, eval_env, &next_method, Some(&Value::Object(outer_iter)), &[])?;
            let next_res = if let Value::Object(next_res) = next_res_val {
                next_res
            } else {
                return Err(raise_type_error!("Iterator result is not an object").into());
            };
            let done_val = crate::core::get_property_with_accessors(mc, eval_env, &next_res, "done")?;
            if matches!(done_val, Value::Boolean(true)) {
                return Ok(None);
            }
            crate::core::get_property_with_accessors(mc, eval_env, &next_res, "value")?
        }
        _ => return Ok(None),
    };

    if matches!(rhs_val, Value::Undefined | Value::Null) {
        return Ok(None);
    }

    let iter_obj = get_iterator(mc, &rhs_val, eval_env)?;
    let mut done = false;
    if pre_steps > 0 {
        let next_method = crate::core::get_property_with_accessors(mc, eval_env, &iter_obj, "next")?;
        if !matches!(next_method, Value::Undefined | Value::Null) {
            for _ in 0..pre_steps {
                let next_res_val = evaluate_call_dispatch(mc, eval_env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                if let Value::Object(next_res) = next_res_val {
                    let done_val = crate::core::get_property_with_accessors(mc, eval_env, &next_res, "done")?;
                    if matches!(done_val, Value::Boolean(true)) {
                        done = true;
                        break;
                    }
                }
            }
        }
    }
    Ok(Some((iter_obj, done)))
}

// Helper to find a yield expression within statements. Returns the
// index of the containing top-level statement, an optional inner index if
// the yield is found inside a nested block/body, and the inner yield
// expression (the Expr inside the yield/await).
#[allow(clippy::type_complexity)]
pub(crate) fn find_first_yield_in_statements(stmts: &[Statement]) -> Option<(usize, Option<usize>, YieldKind, Option<Box<Expr>>)> {
    for (i, s) in stmts.iter().enumerate() {
        match &*s.kind {
            StatementKind::Expr(e) => {
                if let Some((kind, inner)) = find_yield_in_expr(e) {
                    return Some((i, None, kind, inner));
                }
            }
            StatementKind::Return(Some(e)) => {
                if let Some((kind, inner)) = find_yield_in_expr(e) {
                    return Some((i, None, kind, inner));
                }
            }
            StatementKind::Let(decls) | StatementKind::Var(decls) => {
                for (_, expr_opt) in decls {
                    if let Some(expr) = expr_opt
                        && let Some((kind, inner)) = find_yield_in_expr(expr)
                    {
                        return Some((i, None, kind, inner));
                    }
                }
            }
            StatementKind::Const(decls) => {
                for (_, expr) in decls {
                    if let Some((kind, inner)) = find_yield_in_expr(expr) {
                        return Some((i, None, kind, inner));
                    }
                }
            }
            StatementKind::Block(inner_stmts) => {
                if let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(inner_stmts) {
                    return Some((i, Some(inner_idx), kind, found));
                }
            }
            StatementKind::If(if_stmt) => {
                if let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(&if_stmt.then_body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
                if let Some(else_body) = &if_stmt.else_body
                    && let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(else_body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
            }
            StatementKind::For(for_stmt) => {
                if let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(&for_stmt.body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
            }
            StatementKind::TryCatch(tc_stmt) => {
                if let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(&tc_stmt.try_body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
                if let Some(catch_body) = &tc_stmt.catch_body
                    && let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(catch_body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
                if let Some(finally_body) = &tc_stmt.finally_body
                    && let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(finally_body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
            }
            StatementKind::Class(class_def) => {
                if let Some((kind, inner)) = find_yield_in_class_def(class_def) {
                    return Some((i, None, kind, inner));
                }
            }
            StatementKind::While(_, body) | StatementKind::DoWhile(body, _) => {
                if let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
            }
            StatementKind::ForOf(_, _, iterable, body)
            | StatementKind::ForIn(_, _, iterable, body)
            | StatementKind::ForAwaitOf(_, _, iterable, body) => {
                if let Some((kind, found)) = find_yield_in_expr(iterable)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, None, kind, found));
                }
                if let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
            }
            StatementKind::ForOfDestructuringObject(_, pattern, iterable, body)
            | StatementKind::ForInDestructuringObject(_, pattern, iterable, body)
            | StatementKind::ForAwaitOfDestructuringObject(_, pattern, iterable, body) => {
                if let Some((kind, found)) = pattern.iter().find_map(find_yield_in_object_destructuring_element)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, None, kind, found));
                }
                if let Some((kind, found)) = find_yield_in_expr(iterable)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, None, kind, found));
                }
                if let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
            }
            StatementKind::ForOfDestructuringArray(_, pattern, iterable, body)
            | StatementKind::ForInDestructuringArray(_, pattern, iterable, body)
            | StatementKind::ForAwaitOfDestructuringArray(_, pattern, iterable, body) => {
                if let Some((kind, found)) = pattern.iter().find_map(find_yield_in_destructuring_element)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, None, kind, found));
                }
                if let Some((kind, found)) = find_yield_in_expr(iterable)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, None, kind, found));
                }
                if let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
            }
            StatementKind::ForAwaitOfExpr(lhs, iterable, body)
            | StatementKind::ForOfExpr(lhs, iterable, body)
            | StatementKind::ForInExpr(lhs, iterable, body) => {
                if let Some((kind, found)) = find_yield_in_expr(lhs)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, None, kind, found));
                }
                if let Some((kind, found)) = find_yield_in_expr(iterable)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, None, kind, found));
                }
                if let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
            }
            StatementKind::FunctionDeclaration(..) => {
                // don't search nested function declarations
            }
            _ => {}
        }
    }
    None
}

/// Execute generator.next()
pub fn generator_next<'gc>(
    mc: &MutationContext<'gc>,
    generator: &GcPtr<'gc, JSGenerator<'gc>>,
    send_value: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut gen_obj = match generator.try_borrow_mut(mc) {
        Ok(g) => g,
        Err(_) => return Err(raise_type_error!("Generator is already running").into()),
    };

    if matches!(gen_obj.state, GeneratorState::Running { .. }) {
        return Err(raise_type_error!("Generator is already running").into());
    }

    // Take ownership of the generator state so we don't hold a long-lived
    // mutable borrow while we clone/prepare the execution tail and env.
    let orig_state = std::mem::replace(&mut gen_obj.state, GeneratorState::Running { pc: 0, stack: vec![] });
    match orig_state {
        GeneratorState::NotStarted => {
            // Start executing the generator function. Attempt to find the first
            // `yield` expression in the function body and return its value.
            gen_obj.state = GeneratorState::Suspended {
                pc: 0,
                stack: vec![],
                pre_env: None,
            };

            let func_env = gen_obj.env;

            // Ensure `arguments` object exists for generator function body so
            // parameter accesses (and `arguments.length`) reflect the passed args.
            crate::js_class::create_arguments_object(mc, &func_env, &gen_obj.args, None)?;

            slot_set(mc, &func_env, InternalSlot::InGenerator, &Value::Boolean(true));

            if let Some((idx, decl_kind_opt, var_name, iterable, body)) = find_first_for_await_in_statements(&gen_obj.body)
                && idx == 0
            {
                let iter_val = evaluate_expr(mc, &func_env, &iterable)?;
                let (iter_obj, is_async_iter) = get_for_await_iterator(mc, &func_env, &iter_val)?;

                let next_method = object_get_key_value(&iter_obj, "next")
                    .ok_or(raise_type_error!("Iterator has no next method"))?
                    .borrow()
                    .clone();
                let next_res_val = evaluate_call_dispatch(mc, &func_env, &next_method, Some(&Value::Object(iter_obj)), &[])?;

                gen_obj.pending_for_await = Some(crate::core::GeneratorForAwaitState {
                    iterator: iter_obj,
                    is_async: is_async_iter,
                    decl_kind: decl_kind_opt,
                    var_name,
                    body,
                    resume_pc: idx + 1,
                    awaiting_value: false,
                });

                gen_obj.state = GeneratorState::Suspended {
                    pc: idx,
                    stack: vec![],
                    pre_env: Some(func_env),
                };

                return Ok(create_iterator_result(mc, &func_env, &next_res_val, false)?);
            }

            if let Some((idx, inner_idx_opt, yield_kind, yield_inner)) = find_first_yield_in_statements(&gen_obj.body) {
                if let Some(stmt) = gen_obj.body.get(idx)
                    && let StatementKind::ForOf(decl_kind_opt, var_name, iterable, body) = &*stmt.kind
                    && inner_idx_opt.is_some()
                {
                    if idx > 0 {
                        let pre_stmts = gen_obj.body[0..idx].to_vec();
                        crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    }

                    let iter_val = crate::core::evaluate_expr(mc, &func_env, iterable)?;
                    let iter_obj = get_iterator(mc, &iter_val, &func_env)?;
                    let next_method = crate::core::get_property_with_accessors(mc, &func_env, &iter_obj, "next")?;
                    if matches!(next_method, Value::Undefined | Value::Null) {
                        return Err(raise_type_error!("Iterator has no next method").into());
                    }

                    let next_res_val = evaluate_call_dispatch(mc, &func_env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
                    let next_res = if let Value::Object(o) = next_res_val {
                        o
                    } else {
                        return Err(raise_type_error!("Iterator result is not an object").into());
                    };

                    let done_val = crate::core::get_property_with_accessors(mc, &func_env, &next_res, "done")?;
                    if done_val.to_truthy() {
                        gen_obj.state = GeneratorState::Suspended {
                            pc: idx + 1,
                            stack: vec![],
                            pre_env: Some(func_env),
                        };
                        drop(gen_obj);
                        return generator_next(mc, generator, &Value::Undefined);
                    }

                    let value = crate::core::get_property_with_accessors(mc, &func_env, &next_res, "value")?;
                    let iter_env = bind_for_of_iteration_env(mc, &func_env, *decl_kind_opt, var_name, &value)?;
                    let (body_yield_kind, body_yield_inner) = evaluate_for_of_body_first_yield(mc, &iter_env, body)?;

                    gen_obj.pending_for_of = Some(crate::core::GeneratorForOfState {
                        iterator: iter_obj,
                        decl_kind: *decl_kind_opt,
                        var_name: var_name.clone(),
                        body: body.clone(),
                        resume_pc: idx + 1,
                        iter_env,
                    });

                    gen_obj.state = GeneratorState::Suspended {
                        pc: idx,
                        stack: vec![],
                        pre_env: Some(func_env),
                    };

                    slot_set(mc, &func_env, InternalSlot::GenThrowVal, &Value::Undefined);
                    let effective_kind = if matches!(yield_kind, YieldKind::YieldStar | YieldKind::Yield) {
                        body_yield_kind
                    } else {
                        yield_kind
                    };
                    let effective_inner = if yield_inner.is_some() { yield_inner } else { body_yield_inner };

                    if let Some(inner_expr_box) = effective_inner {
                        if effective_kind == YieldKind::Yield && expr_contains_yield(&inner_expr_box) {
                            gen_obj.cached_initial_yield = Some(Value::Undefined);
                            return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?);
                        }

                        match crate::core::evaluate_expr(mc, &iter_env, &inner_expr_box) {
                            Ok(val) => {
                                if matches!(effective_kind, YieldKind::YieldStar) {
                                    let delegated = (|| -> Result<Value<'gc>, EvalError<'gc>> {
                                        let iterator = get_iterator(mc, &val, &iter_env)?;
                                        let next_method = crate::core::get_property_with_accessors(mc, &iter_env, &iterator, "next")?;
                                        let iter_res = evaluate_call_dispatch(
                                            mc,
                                            &iter_env,
                                            &next_method,
                                            Some(&Value::Object(iterator)),
                                            std::slice::from_ref(&Value::Undefined),
                                        )?;

                                        if let Value::Object(res_obj) = iter_res {
                                            let done_val = crate::core::get_property_with_accessors(mc, &iter_env, &res_obj, "done")?;
                                            let done = done_val.to_truthy();

                                            if !done {
                                                gen_obj.yield_star_iterator = Some(iterator);
                                                let yielded = if let Some(v) = object_get_key_value(&res_obj, "value") {
                                                    v.borrow().clone()
                                                } else {
                                                    Value::Undefined
                                                };
                                                gen_obj.cached_initial_yield = Some(yielded.clone());
                                                return Ok(create_iterator_result_with_done(mc, &func_env, &yielded, &done_val)?);
                                            }
                                            let value = crate::core::get_property_with_accessors(mc, &iter_env, &res_obj, "value")?;
                                            Ok(value)
                                        } else {
                                            Err(raise_type_error!("Iterator result is not an object").into())
                                        }
                                    })();

                                    match delegated {
                                        Ok(v) => {
                                            if let Some(_iter) = gen_obj.yield_star_iterator {
                                                return Ok(v);
                                            }
                                            drop(gen_obj);
                                            return generator_next(mc, generator, &v);
                                        }
                                        Err(e) => {
                                            let throw_val = eval_error_to_value(mc, &iter_env, e);
                                            drop(gen_obj);
                                            return generator_throw(mc, generator, &throw_val);
                                        }
                                    }
                                }

                                gen_obj.cached_initial_yield = Some(val.clone());
                                return Ok(create_iterator_result(mc, &func_env, &val, false)?);
                            }
                            Err(e) => {
                                let throw_val = eval_error_to_value(mc, &iter_env, e);
                                drop(gen_obj);
                                return generator_throw(mc, generator, &throw_val);
                            }
                        }
                    }

                    gen_obj.cached_initial_yield = Some(Value::Undefined);
                    return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?);
                }

                if let Some(stmt) = gen_obj.body.get(idx)
                    && let StatementKind::If(if_stmt) = &*stmt.kind
                    && !expr_contains_yield(&if_stmt.condition)
                {
                    let cond_val = crate::core::evaluate_expr(mc, &func_env, &if_stmt.condition)?;
                    let chosen_body = if cond_val.to_truthy() {
                        &if_stmt.then_body
                    } else {
                        if_stmt.else_body.as_deref().unwrap_or(&[])
                    };

                    if find_first_yield_in_statements(chosen_body).is_none() {
                        crate::core::evaluate_statements(mc, &func_env, std::slice::from_ref(stmt))?;
                        gen_obj.state = GeneratorState::Suspended {
                            pc: idx + 1,
                            stack: vec![],
                            pre_env: Some(func_env),
                        };
                        drop(gen_obj);
                        return generator_next(mc, generator, &Value::Undefined);
                    }
                }

                log::debug!(
                    "generator_next: found first yield at idx={} inner_idx={:?} body_len={} func_env_pre={} ",
                    idx,
                    inner_idx_opt,
                    gen_obj.body.len(),
                    gen_obj.args.len()
                );
                // Suspend at the containing top-level statement index so
                // that resumed execution re-evaluates the statement with
                // the sent-in value substituted for the `yield`.
                // Execute statements *before* the one containing the first yield so
                // that variable bindings and side-effects (e.g., Promise constructor
                // executors) occur before we evaluate the inner yield expression.
                // Prepare the function activation environment with the captured
                // call-time arguments so that parameter bindings exist even if the
                // generator suspends before executing any pre-statements.
                // If the yield is inside a nested block/branch we may need to
                // execute pre-statements that are inside that block before
                // evaluating the inner expression. For example, when the
                // function body is a single Block, and the yield is in the
                // block's second statement, we must run the block's first
                // statement so that side-effects happen in order.
                let pre_env_opt: Option<JSObjectDataPtr> = if idx > 0 {
                    let pre_stmts = gen_obj.body[0..idx].to_vec();
                    // Execute pre-yield statements in the function env so that
                    // bindings and side-effects are recorded on that environment.
                    crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    Some(func_env)
                } else if let Some(inner_idx) = inner_idx_opt {
                    if inner_idx > 0 {
                        let pre_key = format!("__gen_pre_exec_{}_{}", idx, inner_idx);
                        if env_get_own(&func_env, &pre_key).is_none() {
                            // Execute pre-statements before the yield for inner containers.
                            match &*gen_obj.body[idx].kind {
                                StatementKind::Block(inner_stmts) => {
                                    let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                                    let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                }
                                StatementKind::TryCatch(tc_stmt) => {
                                    // Determine which sub-body the yield is in
                                    let yield_in_try = find_first_yield_in_statements(&tc_stmt.try_body).is_some();
                                    let yield_in_catch = !yield_in_try
                                        && tc_stmt
                                            .catch_body
                                            .as_ref()
                                            .is_some_and(|cb| find_first_yield_in_statements(cb).is_some());
                                    let yield_in_finally = !yield_in_try
                                        && !yield_in_catch
                                        && tc_stmt
                                            .finally_body
                                            .as_ref()
                                            .is_some_and(|fb| find_first_yield_in_statements(fb).is_some());

                                    if yield_in_try && inner_idx <= tc_stmt.try_body.len() {
                                        let pre_stmts = tc_stmt.try_body[0..inner_idx].to_vec();
                                        let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                        seed_simple_decl_bindings(mc, &func_env, &pre_stmts)?;
                                    } else if yield_in_catch || yield_in_finally {
                                        // Yield is in catch or finally body.
                                        // Run the entire try body within TryCatch semantics first.
                                        if !tc_stmt.try_body.is_empty() {
                                            let try_result = crate::core::evaluate_statements_with_context_and_last_value(
                                                mc,
                                                &func_env,
                                                &tc_stmt.try_body,
                                                &[],
                                            );
                                            match try_result {
                                                Ok((crate::core::ControlFlow::Throw(v, _, _), _)) | Err(EvalError::Throw(v, _, _)) => {
                                                    if let Some(cb) = &tc_stmt.catch_body {
                                                        if let Some(CatchParamPattern::Identifier(name)) = &tc_stmt.catch_param {
                                                            let _ = env_set(mc, &func_env, name, &v);
                                                        }
                                                        if !yield_in_catch {
                                                            let _ = crate::core::evaluate_statements(mc, &func_env, cb);
                                                        }
                                                    }
                                                }
                                                Err(EvalError::Js(js_err)) => {
                                                    let v = crate::core::js_error_to_value(mc, &func_env, &js_err);
                                                    if let Some(cb) = &tc_stmt.catch_body {
                                                        if let Some(CatchParamPattern::Identifier(name)) = &tc_stmt.catch_param {
                                                            let _ = env_set(mc, &func_env, name, &v);
                                                        }
                                                        if !yield_in_catch {
                                                            let _ = crate::core::evaluate_statements(mc, &func_env, cb);
                                                        }
                                                    }
                                                }
                                                _ => {} // try completed normally
                                            }
                                        }
                                        // Run pre-yield stmts from the appropriate body
                                        if yield_in_finally {
                                            if let Some(fb) = &tc_stmt.finally_body
                                                && inner_idx > 0
                                                && inner_idx <= fb.len()
                                            {
                                                let pre_fin = fb[0..inner_idx].to_vec();
                                                let _ = crate::core::evaluate_statements(mc, &func_env, &pre_fin)?;
                                            }
                                        } else if yield_in_catch
                                            && let Some(cb) = &tc_stmt.catch_body
                                            && inner_idx > 0
                                            && inner_idx <= cb.len()
                                        {
                                            let pre_stmts = cb[0..inner_idx].to_vec();
                                            let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                        }
                                    }
                                }
                                StatementKind::ForOf(_, _, _, body)
                                | StatementKind::ForIn(_, _, _, body)
                                | StatementKind::ForAwaitOf(_, _, _, body)
                                | StatementKind::ForOfExpr(_, _, body)
                                | StatementKind::ForInExpr(_, _, body)
                                | StatementKind::ForAwaitOfExpr(_, _, body)
                                | StatementKind::ForOfDestructuringObject(_, _, _, body)
                                | StatementKind::ForInDestructuringObject(_, _, _, body)
                                | StatementKind::ForAwaitOfDestructuringObject(_, _, _, body)
                                | StatementKind::ForOfDestructuringArray(_, _, _, body)
                                | StatementKind::ForInDestructuringArray(_, _, _, body)
                                | StatementKind::ForAwaitOfDestructuringArray(_, _, _, body) => {
                                    if inner_idx > 0 {
                                        let pre_stmts = body[0..inner_idx].to_vec();
                                        let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                    } else if let Some(first_body_stmt) = body.first() {
                                        eval_statement_prefix_until_first_yield(mc, &func_env, first_body_stmt)?;
                                    }
                                }
                                _ => {}
                            }
                            if let Err(e) = env_set(mc, &func_env, &pre_key, &Value::Boolean(true)) {
                                log::warn!("Error setting pre-execution env key: {e}");
                            }
                        }
                    }

                    // ---- Nested TryCatch pre-execution ----
                    // If the yield is inside a nested TryCatch (within the
                    // outer TryCatch's try body), run the inner TryCatch's
                    // try body and/or catch pre-yield stmts. This is needed
                    // regardless of inner_idx value.
                    if let StatementKind::TryCatch(tc_stmt) = &*gen_obj.body[idx].kind
                        && let Some(inner_stmt) = tc_stmt.try_body.get(inner_idx)
                        && let StatementKind::TryCatch(inner_tc) = &*inner_stmt.kind
                    {
                        let nested_key = format!("__gen_nested_tc_{}_{}", idx, inner_idx);
                        if env_get_own(&func_env, &nested_key).is_none() {
                            let yield_in_inner_try = find_first_yield_in_statements(&inner_tc.try_body).is_some();
                            let yield_in_inner_catch = !yield_in_inner_try
                                && inner_tc
                                    .catch_body
                                    .as_ref()
                                    .is_some_and(|cb| find_first_yield_in_statements(cb).is_some());

                            if yield_in_inner_catch {
                                // Run inner try body (which throws), bind catch param
                                let inner_try_result =
                                    crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &inner_tc.try_body, &[]);
                                match inner_try_result {
                                    Ok((crate::core::ControlFlow::Throw(v, _, _), _)) | Err(EvalError::Throw(v, _, _)) => {
                                        if let Some(CatchParamPattern::Identifier(name)) = &inner_tc.catch_param {
                                            env_set(mc, &func_env, name, &v)?;
                                        }
                                    }
                                    Err(EvalError::Js(js_err)) => {
                                        let v = crate::core::js_error_to_value(mc, &func_env, &js_err);
                                        if let Some(CatchParamPattern::Identifier(name)) = &inner_tc.catch_param {
                                            env_set(mc, &func_env, name, &v)?;
                                        }
                                    }
                                    Ok(_) => {}
                                }
                                // Run catch pre-yield stmts
                                if let Some(cb) = &inner_tc.catch_body
                                    && let Some((cyi, _, _, _)) = find_first_yield_in_statements(cb)
                                    && cyi > 0
                                {
                                    let pre_catch = cb[0..cyi].to_vec();
                                    crate::core::evaluate_statements(mc, &func_env, &pre_catch)?;
                                }
                            } else if yield_in_inner_try {
                                // Run inner try body pre-yield stmts
                                if let Some((inner_try_idx, _, _, _)) = find_first_yield_in_statements(&inner_tc.try_body)
                                    && inner_try_idx > 0
                                {
                                    let pre_stmts = inner_tc.try_body[0..inner_try_idx].to_vec();
                                    crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                }
                            }
                            let _ = env_set(mc, &func_env, &nested_key, &Value::Boolean(true));
                        }
                    }

                    Some(func_env)
                } else {
                    // Even when there are no pre-statements, we need the function
                    // env to hold parameter bindings for later resume.
                    Some(func_env)
                };

                // For yields nested inside expression statements (for example,
                // `a(), import(x, yield), b()`), execute left-to-right side effects
                // up to the first yield before suspending.
                if idx < gen_obj.body.len()
                    && let StatementKind::Expr(expr_stmt) = &*gen_obj.body[idx].kind
                    && yield_inner.is_none()
                {
                    let _ = eval_prefix_until_first_yield(mc, &func_env, expr_stmt)?;
                }

                // Suspend at the containing top-level statement index and store the
                // pre-execution environment so that resumed execution can reuse it.
                gen_obj.state = GeneratorState::Suspended {
                    pc: idx,
                    stack: vec![],
                    pre_env: pre_env_opt,
                };

                // Prepare pending iterators for destructuring assignments that contain a yield.
                // This is a narrow fix for assignment patterns that start iteration before
                // the yield expression is evaluated.
                if let Some((iter_obj, done)) = prepare_pending_iterator_for_yield(mc, &func_env, &gen_obj.body[idx])? {
                    gen_obj.pending_iterator = Some(iter_obj);
                    gen_obj.pending_iterator_done = done;
                } else {
                    gen_obj.pending_iterator = None;
                    gen_obj.pending_iterator_done = false;
                }

                // If the yield has an inner expression, evaluate it in a fresh
                // function-like frame whose prototype is the captured env (so it
                // can see bindings from the pre-stmts execution) and return that
                // value as the yielded value.
                if let Some(inner_expr_box) = yield_inner {
                    // If the yield is inside a `for` loop body, ensure the loop
                    // initializer and any body pre-statements execute so loop
                    // bindings (e.g., `let i`) exist before evaluating the yield.
                    if let StatementKind::For(for_stmt) = &*gen_obj.body[idx].kind {
                        if let Some(init_stmt) = &for_stmt.init {
                            crate::core::evaluate_statements(mc, &func_env, std::slice::from_ref(init_stmt))?;
                        }
                        if let Some(inner_idx) = inner_idx_opt
                            && inner_idx > 0
                        {
                            let pre_stmts = for_stmt.body[0..inner_idx].to_vec();
                            let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                        }
                    }

                    // Evaluate inner expression in the current function env so
                    // loop bindings are visible.
                    slot_set(mc, &func_env, InternalSlot::GenThrowVal, &Value::Undefined);
                    if yield_kind == YieldKind::Yield && expr_contains_yield(&inner_expr_box) {
                        gen_obj.cached_initial_yield = Some(Value::Undefined);
                        return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?);
                    }

                    let func_home = func_env.borrow().get_home_object().map(|h| Gc::as_ptr(*h.borrow()));
                    log::trace!(
                        "generator_next: NotStarted inner eval env_ptr={:p} env.home={:?} gen_ptr={:p}",
                        Gc::as_ptr(func_env),
                        func_home,
                        Gc::as_ptr(*generator)
                    );
                    match crate::core::evaluate_expr(mc, &func_env, &inner_expr_box) {
                        Ok(val) => {
                            if matches!(yield_kind, YieldKind::YieldStar) {
                                let delegated = (|| -> Result<Value<'gc>, EvalError<'gc>> {
                                    let iterator = get_iterator(mc, &val, &func_env)?;
                                    let next_method = crate::core::get_property_with_accessors(mc, &func_env, &iterator, "next")?;
                                    let iter_res = evaluate_call_dispatch(
                                        mc,
                                        &func_env,
                                        &next_method,
                                        Some(&Value::Object(iterator)),
                                        std::slice::from_ref(&Value::Undefined),
                                    )?;

                                    if let Value::Object(res_obj) = iter_res {
                                        // Use accessor-aware reads for 'done' and 'value' per spec so getters
                                        // on the iterator result may throw and side-effects are observed.
                                        let done_val = crate::core::get_property_with_accessors(mc, &func_env, &res_obj, "done")?;
                                        let done = done_val.to_truthy();

                                        if !done {
                                            gen_obj.yield_star_iterator = Some(iterator);
                                            let yielded = if let Some(v) = object_get_key_value(&res_obj, "value") {
                                                v.borrow().clone()
                                            } else {
                                                Value::Undefined
                                            };
                                            gen_obj.cached_initial_yield = Some(yielded.clone());
                                            return Ok(create_iterator_result_with_done(mc, &func_env, &yielded, &done_val)?);
                                        }
                                        let value = crate::core::get_property_with_accessors(mc, &func_env, &res_obj, "value")?;
                                        Ok(value)
                                    } else {
                                        Err(raise_type_error!("Iterator result is not an object").into())
                                    }
                                })();

                                match delegated {
                                    Ok(v) => {
                                        if let Some(_iter) = gen_obj.yield_star_iterator {
                                            return Ok(v);
                                        }
                                        drop(gen_obj);
                                        return generator_next(mc, generator, &v);
                                    }
                                    Err(e) => {
                                        let throw_val = eval_error_to_value(mc, &func_env, e);
                                        drop(gen_obj);
                                        return generator_throw(mc, generator, &throw_val);
                                    }
                                }
                            }

                            // Cache the value so re-entry/resume paths can use it
                            gen_obj.cached_initial_yield = Some(val.clone());
                            return Ok(create_iterator_result(mc, &func_env, &val, false)?);
                        }
                        Err(e) => {
                            let throw_val = eval_error_to_value(mc, &func_env, e);
                            drop(gen_obj);
                            return generator_throw(mc, generator, &throw_val);
                        }
                    }
                }

                // No inner expression -> yield undefined
                Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?)
            } else {
                // No yields found: execute the whole function body in a freshly
                // prepared function activation environment using the captured
                // call-time arguments, then complete the generator with the
                // returned value.
                // NOTE: We now create the environment eagerly in handle_generator_function_call,
                // so we just use the stored environment.
                let func_env = gen_obj.env;
                let body = gen_obj.body.clone();
                let args = gen_obj.args.clone();

                // Ensure `arguments` exists for the no-yield completion path too.
                crate::js_class::create_arguments_object(mc, &func_env, &args, None)?;
                slot_set(mc, &func_env, InternalSlot::InGenerator, &Value::Boolean(true));

                // Evaluate the function body and interpret completion per spec:
                // - If a Return occurred, use its value
                // - If normal completion (no Return), completion value is undefined
                // - If a Throw occurred, propagate the throw
                let func_home = func_env.borrow().get_home_object().map(|h| Gc::as_ptr(*h.borrow()));
                log::trace!(
                    "generator_next: NotStarted executing body func_env={:p} env.home={:?} gen_ptr={:p}",
                    Gc::as_ptr(func_env),
                    func_home,
                    Gc::as_ptr(*generator)
                );
                // Set state back to Running before releasing the lock so that
                // re-entrant calls (from within the body) observe "executing" state
                // and correctly throw TypeError per spec.
                gen_obj.state = GeneratorState::Running { pc: 0, stack: vec![] };
                // Drop the borrow before evaluating user code so that re-entrant
                // calls to generator_next (from within the body) can borrow again
                // and correctly observe the generator as "running" (spec: throw TypeError).
                drop(gen_obj);
                let cf_result = crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &body, &[]);
                // After execution, mark generator as completed.
                generator.borrow_mut(mc).state = GeneratorState::Completed;
                let (cf, _last) = cf_result?;
                match cf {
                    crate::core::ControlFlow::Return(v) => Ok(create_iterator_result(mc, &func_env, &v, true)?),
                    crate::core::ControlFlow::Normal(_) => Ok(create_iterator_result(mc, &func_env, &Value::Undefined, true)?),
                    crate::core::ControlFlow::Throw(v, l, c) => Err(crate::core::EvalError::Throw(v, l, c)),
                    _ => Err(raise_eval_error!("Unexpected control flow after evaluating generator body").into()),
                }
            }
        }
        GeneratorState::Suspended { pc, stack: _, pre_env } => {
            // Restore generator state to Suspended early so evaluations that may
            // call back into JavaScript (and possibly call generator.next()) do
            // not observe the generator as `Running` and trigger re-entrancy errors.
            let saved_pre_env = pre_env;
            gen_obj.state = GeneratorState::Suspended {
                pc,
                stack: vec![],
                pre_env: saved_pre_env,
            };
            log::trace!("generator_next: restored Suspended state pc={}", pc);

            // ---- Pending completion from finally-body yield ----
            // When the generator was suspended at a yield inside a finally
            // block (from generator_throw or generator_return), the remaining
            // finally statements were flattened into body[pc]. Execute them
            // and then fire the parked completion (throw or return).
            if gen_obj.pending_completion.is_some() {
                let func_env = if let GeneratorState::Suspended { pre_env: Some(env), .. } = &gen_obj.state {
                    *env
                } else {
                    gen_obj.env
                };
                let pc_val = pc;

                // Bind the send_value — the result of `yield` in the finally
                // body is not typically used but we bind it for correctness.
                env_set(mc, &func_env, "__gen_finally_send", send_value)?;

                // Execute remaining finally statements at body[pc]
                if pc_val < gen_obj.body.len() {
                    let remaining_stmt = gen_obj.body[pc_val].clone();
                    let result = crate::core::evaluate_statements_with_context_and_last_value(
                        mc,
                        &func_env,
                        std::slice::from_ref(&remaining_stmt),
                        &[],
                    );
                    match result {
                        Ok((cf, _)) => match cf {
                            crate::core::ControlFlow::Normal(_) => {
                                let pending = gen_obj.pending_completion.take().unwrap();
                                gen_obj.state = GeneratorState::Completed;
                                match pending {
                                    GeneratorPendingCompletion::Throw(v) => return Err(EvalError::Throw(v, None, None)),
                                    GeneratorPendingCompletion::Return(v) => return Ok(create_iterator_result(mc, &func_env, &v, true)?),
                                }
                            }
                            crate::core::ControlFlow::Throw(v, l, c) => {
                                gen_obj.pending_completion = None;
                                gen_obj.state = GeneratorState::Completed;
                                return Err(EvalError::Throw(v, l, c));
                            }
                            crate::core::ControlFlow::Return(v) => {
                                gen_obj.pending_completion = None;
                                gen_obj.state = GeneratorState::Completed;
                                return Ok(create_iterator_result(mc, &func_env, &v, true)?);
                            }
                            _ => {
                                gen_obj.pending_completion = None;
                                gen_obj.state = GeneratorState::Completed;
                                return Err(raise_eval_error!("Unexpected control flow after finally body").into());
                            }
                        },
                        Err(e) => {
                            gen_obj.pending_completion = None;
                            gen_obj.state = GeneratorState::Completed;
                            return Err(e);
                        }
                    }
                } else {
                    let pending = gen_obj.pending_completion.take().unwrap();
                    gen_obj.state = GeneratorState::Completed;
                    match pending {
                        GeneratorPendingCompletion::Throw(v) => return Err(EvalError::Throw(v, None, None)),
                        GeneratorPendingCompletion::Return(v) => return Ok(create_iterator_result(mc, &func_env, &v, true)?),
                    }
                }
            }

            if let Some(mut for_await) = gen_obj.pending_for_await.take() {
                let func_env = gen_obj.env;
                let pc_val = pc;

                if for_await.awaiting_value {
                    let iter_env = if let Some(VarDeclKind::Let) | Some(VarDeclKind::Const) = for_await.decl_kind {
                        let e = crate::core::new_js_object_data(mc);
                        e.borrow_mut(mc).prototype = Some(func_env);
                        env_set(mc, &e, &for_await.var_name, send_value)?;
                        e
                    } else {
                        env_set_recursive(mc, &func_env, &for_await.var_name, send_value)?;
                        func_env
                    };

                    crate::core::evaluate_statements(mc, &iter_env, &for_await.body)?;

                    let next_method = object_get_key_value(&for_await.iterator, "next")
                        .ok_or(raise_type_error!("Iterator has no next method"))?
                        .borrow()
                        .clone();
                    let next_res_val =
                        evaluate_call_dispatch(mc, &func_env, &next_method, Some(&Value::Object(for_await.iterator)), &Vec::new())?;

                    for_await.awaiting_value = false;
                    gen_obj.pending_for_await = Some(for_await);
                    gen_obj.state = GeneratorState::Suspended {
                        pc: pc_val,
                        stack: vec![],
                        pre_env,
                    };

                    return Ok(create_iterator_result(mc, &func_env, &next_res_val, false)?);
                }

                let next_res = if let Value::Object(obj) = send_value {
                    obj
                } else {
                    return Err(raise_type_error!("Iterator result is not an object").into());
                };

                let done = if let Some(done_val) = object_get_key_value(next_res, "done") {
                    done_val.borrow().to_truthy()
                } else {
                    false
                };

                if done {
                    gen_obj.pending_for_await = None;
                    if for_await.resume_pc >= gen_obj.body.len() {
                        gen_obj.state = GeneratorState::Completed;
                        return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, true)?);
                    }
                    gen_obj.state = GeneratorState::Suspended {
                        pc: for_await.resume_pc,
                        stack: vec![],
                        pre_env: Some(func_env),
                    };
                    drop(gen_obj);
                    return generator_next(mc, generator, &Value::Undefined);
                }

                let value = if let Some(val) = object_get_key_value(next_res, "value") {
                    val.borrow().clone()
                } else {
                    Value::Undefined
                };

                for_await.awaiting_value = true;
                gen_obj.pending_for_await = Some(for_await);
                gen_obj.state = GeneratorState::Suspended {
                    pc: pc_val,
                    stack: vec![],
                    pre_env,
                };

                return Ok(create_iterator_result(mc, &func_env, &value, false)?);
            }

            if let Some(iter) = gen_obj.yield_star_iterator {
                let next_method = crate::core::get_property_with_accessors(mc, &gen_obj.env, &iter, "next")?;
                let iter_res = crate::core::evaluate_call_dispatch(
                    mc,
                    &gen_obj.env,
                    &next_method,
                    Some(&Value::Object(iter)),
                    std::slice::from_ref(send_value),
                )?;
                if let Value::Object(res_obj) = iter_res {
                    // Use accessor-aware reads for 'done' and 'value' per spec so getters
                    // on the iterator result may throw and side-effects are observed.
                    let done_val = crate::core::get_property_with_accessors(mc, &gen_obj.env, &res_obj, "done")?;
                    let done = done_val.to_truthy();

                    if !done {
                        gen_obj.state = GeneratorState::Suspended {
                            pc,
                            stack: vec![],
                            pre_env,
                        };
                        let yielded = if let Some(v) = object_get_key_value(&res_obj, "value") {
                            v.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        return Ok(create_iterator_result_with_done(mc, &gen_obj.env, &yielded, &done_val)?);
                    } else {
                        let value = crate::core::get_property_with_accessors(mc, &gen_obj.env, &res_obj, "value")?;
                        gen_obj.yield_star_iterator = None;
                        gen_obj.state = GeneratorState::Suspended {
                            pc,
                            stack: vec![],
                            pre_env,
                        };
                        drop(gen_obj);
                        return generator_next(mc, generator, &value);
                    }
                } else {
                    return Err(raise_type_error!("Iterator result is not an object").into());
                }
            }

            if let Some(mut for_of) = gen_obj.pending_for_of.take() {
                let func_env = gen_obj.env;
                let mut resumed_body = for_of.body.clone();
                trim_statements_to_post_first_yield(&mut resumed_body);
                let send_var = "__gen_forof_send";
                env_set(mc, &for_of.iter_env, send_var, send_value)?;
                for stmt in &mut resumed_body {
                    let mut did_replace = false;
                    replace_first_yield_in_statement(stmt, send_var, &mut did_replace);
                    if did_replace {
                        break;
                    }
                }

                let (cf, _last) = crate::core::evaluate_statements_with_context_and_last_value(mc, &for_of.iter_env, &resumed_body, &[])?;
                match cf {
                    crate::core::ControlFlow::Normal(_) => {}
                    crate::core::ControlFlow::Return(v) => {
                        gen_obj.state = GeneratorState::Completed;
                        return Ok(create_iterator_result(mc, &func_env, &v, true)?);
                    }
                    crate::core::ControlFlow::Throw(v, l, c) => {
                        return Err(crate::core::EvalError::Throw(v, l, c));
                    }
                    crate::core::ControlFlow::Break(_) | crate::core::ControlFlow::Continue(_) => {
                        gen_obj.state = GeneratorState::Suspended {
                            pc: for_of.resume_pc,
                            stack: vec![],
                            pre_env: Some(func_env),
                        };
                        drop(gen_obj);
                        return generator_next(mc, generator, &Value::Undefined);
                    }
                }

                let next_method = crate::core::get_property_with_accessors(mc, &func_env, &for_of.iterator, "next")?;
                if matches!(next_method, Value::Undefined | Value::Null) {
                    return Err(raise_type_error!("Iterator has no next method").into());
                }
                let next_res_val = evaluate_call_dispatch(mc, &func_env, &next_method, Some(&Value::Object(for_of.iterator)), &[])?;
                let next_res = if let Value::Object(o) = next_res_val {
                    o
                } else {
                    return Err(raise_type_error!("Iterator result is not an object").into());
                };
                let done_val = crate::core::get_property_with_accessors(mc, &func_env, &next_res, "done")?;
                if done_val.to_truthy() {
                    gen_obj.pending_for_of = None;
                    gen_obj.state = GeneratorState::Suspended {
                        pc: for_of.resume_pc,
                        stack: vec![],
                        pre_env: Some(func_env),
                    };
                    drop(gen_obj);
                    return generator_next(mc, generator, &Value::Undefined);
                }

                let next_value = crate::core::get_property_with_accessors(mc, &func_env, &next_res, "value")?;
                let iter_env = bind_for_of_iteration_env(mc, &func_env, for_of.decl_kind, &for_of.var_name, &next_value)?;
                let (yield_kind, yield_inner) = evaluate_for_of_body_first_yield(mc, &iter_env, &for_of.body)?;
                for_of.iter_env = iter_env;
                gen_obj.pending_for_of = Some(for_of);
                gen_obj.state = GeneratorState::Suspended {
                    pc,
                    stack: vec![],
                    pre_env: Some(func_env),
                };
                slot_set(mc, &func_env, InternalSlot::GenThrowVal, &Value::Undefined);

                if let Some(inner_expr_box) = yield_inner {
                    if yield_kind == YieldKind::Yield && expr_contains_yield(&inner_expr_box) {
                        gen_obj.cached_initial_yield = Some(Value::Undefined);
                        return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?);
                    }

                    match crate::core::evaluate_expr(mc, &iter_env, &inner_expr_box) {
                        Ok(val) => {
                            if matches!(yield_kind, YieldKind::YieldStar) {
                                let delegated = (|| -> Result<Value<'gc>, EvalError<'gc>> {
                                    let iterator = get_iterator(mc, &val, &iter_env)?;
                                    let next_method = crate::core::get_property_with_accessors(mc, &iter_env, &iterator, "next")?;
                                    let iter_res = evaluate_call_dispatch(
                                        mc,
                                        &iter_env,
                                        &next_method,
                                        Some(&Value::Object(iterator)),
                                        std::slice::from_ref(&Value::Undefined),
                                    )?;

                                    if let Value::Object(res_obj) = iter_res {
                                        let done_val = crate::core::get_property_with_accessors(mc, &iter_env, &res_obj, "done")?;
                                        let done = done_val.to_truthy();

                                        if !done {
                                            gen_obj.yield_star_iterator = Some(iterator);
                                            let yielded = if let Some(v) = object_get_key_value(&res_obj, "value") {
                                                v.borrow().clone()
                                            } else {
                                                Value::Undefined
                                            };
                                            gen_obj.cached_initial_yield = Some(yielded.clone());
                                            return Ok(create_iterator_result_with_done(mc, &func_env, &yielded, &done_val)?);
                                        }

                                        let value = crate::core::get_property_with_accessors(mc, &iter_env, &res_obj, "value")?;
                                        Ok(value)
                                    } else {
                                        Err(raise_type_error!("Iterator result is not an object").into())
                                    }
                                })();

                                match delegated {
                                    Ok(v) => {
                                        if let Some(_iter) = gen_obj.yield_star_iterator {
                                            return Ok(v);
                                        }
                                        drop(gen_obj);
                                        return generator_next(mc, generator, &v);
                                    }
                                    Err(e) => {
                                        let throw_val = eval_error_to_value(mc, &iter_env, e);
                                        drop(gen_obj);
                                        return generator_throw(mc, generator, &throw_val);
                                    }
                                }
                            }

                            gen_obj.cached_initial_yield = Some(val.clone());
                            return Ok(create_iterator_result(mc, &func_env, &val, false)?);
                        }
                        Err(e) => {
                            let throw_val = eval_error_to_value(mc, &iter_env, e);
                            drop(gen_obj);
                            return generator_throw(mc, generator, &throw_val);
                        }
                    }
                }

                gen_obj.cached_initial_yield = Some(Value::Undefined);
                return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?);
            }

            // On resume, execute from the suspended statement index. If a
            // `send_value` was provided to `next(value)`, substitute the
            // first `yield` in that statement with the sent value before
            // executing.
            let pc_val = pc;
            log::debug!("DEBUG: generator_next Suspended. pc={}, send_value={:?}", pc_val, send_value);
            gen_obj.pending_iterator = None;
            gen_obj.pending_iterator_done = false;
            if pc_val >= gen_obj.body.len() {
                gen_obj.state = GeneratorState::Completed;
                return Ok(create_iterator_result(mc, &gen_obj.env, &Value::Undefined, true)?);
            }
            let mut for_stmt_snapshot = if let Some(stmt) = gen_obj.body.get(pc_val) {
                if matches!(&*stmt.kind, StatementKind::For(_)) {
                    Some(stmt.clone())
                } else {
                    None
                }
            } else {
                None
            };
            // Create a base name for yield variables for this PC; replace ALL yield
            // occurrences in the containing top-level statement with distinct
            // temporary variables so nested yields are placeholders for later
            // send-value binding.
            let base_name = format!("__gen_yield_val_{}_", pc_val);

            // Prepare the function environment early so we can evaluate
            // conditional sub-expressions before mutating the AST.
            let func_env = if let Some(env) = pre_env.as_ref() {
                *env
            } else {
                prepare_function_call_env(mc, Some(&gen_obj.env), gen_obj.this_val.clone().as_ref(), None, &[], None, None)?
            };

            if let Some(stmt) = gen_obj.body.get_mut(pc_val)
                && let StatementKind::For(for_stmt) = stmt.kind.as_mut()
                && for_stmt.body.len() == 1
                && let StatementKind::Expr(Expr::Yield(yield_inner_opt)) = &*for_stmt.body[0].kind
            {
                if let Some(init_stmt) = for_stmt.init.clone() {
                    if for_init_needs_execution(&func_env, &init_stmt) {
                        crate::core::evaluate_statements(mc, &func_env, std::slice::from_ref(&init_stmt))?;
                    }
                    for_stmt.init = None;
                }

                if let Some(update_stmt) = &for_stmt.update {
                    crate::core::evaluate_statements(mc, &func_env, std::slice::from_ref(update_stmt))?;
                }

                if let Some(test_expr) = &for_stmt.test {
                    let test_val = crate::core::evaluate_expr(mc, &func_env, test_expr)?;
                    if !test_val.to_truthy() {
                        gen_obj.state = GeneratorState::Completed;
                        return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, true)?);
                    }
                }

                let yielded = if let Some(inner_expr) = yield_inner_opt {
                    crate::core::evaluate_expr(mc, &func_env, inner_expr)?
                } else {
                    Value::Undefined
                };

                gen_obj.state = GeneratorState::Suspended {
                    pc: pc_val,
                    stack: vec![],
                    pre_env: Some(func_env),
                };
                gen_obj.cached_initial_yield = Some(yielded.clone());
                return Ok(create_iterator_result(mc, &func_env, &yielded, false)?);
            }

            // Special case: While loop containing a yield in its body.
            // On resume, execute post-yield statements from the previous
            // iteration, re-check the condition, and if true execute
            // pre-yield statements and evaluate the yield inner expression
            // to produce the next value.
            if let Some(stmt) = gen_obj.body.get(pc_val)
                && let StatementKind::While(cond, while_body) = &*stmt.kind
                && let Some((body_yield_idx, None, _yield_kind, yield_inner)) = find_first_yield_in_statements(while_body)
            {
                // Execute post-yield statements from current iteration
                if body_yield_idx + 1 < while_body.len() {
                    let post_stmts = while_body[body_yield_idx + 1..].to_vec();
                    crate::core::evaluate_statements(mc, &func_env, &post_stmts)?;
                }

                // Re-check loop condition
                let cond_val = crate::core::evaluate_expr(mc, &func_env, cond)?;
                if !cond_val.to_truthy() {
                    // Loop done, advance to next statement
                    if pc_val + 1 >= gen_obj.body.len() {
                        gen_obj.state = GeneratorState::Completed;
                        return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, true)?);
                    }
                    gen_obj.state = GeneratorState::Suspended {
                        pc: pc_val + 1,
                        stack: vec![],
                        pre_env: Some(func_env),
                    };
                    drop(gen_obj);
                    return generator_next(mc, generator, &Value::Undefined);
                }

                // Execute pre-yield statements for next iteration
                if body_yield_idx > 0 {
                    let pre_stmts = while_body[0..body_yield_idx].to_vec();
                    crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                }

                // Evaluate yield inner expression
                let yielded = if let Some(inner) = yield_inner {
                    crate::core::evaluate_expr(mc, &func_env, &inner)?
                } else {
                    Value::Undefined
                };

                // Suspend at the same while-loop statement
                gen_obj.state = GeneratorState::Suspended {
                    pc: pc_val,
                    stack: vec![],
                    pre_env: Some(func_env),
                };
                gen_obj.cached_initial_yield = Some(yielded.clone());
                return Ok(create_iterator_result(mc, &func_env, &yielded, false)?);
            }

            // Special-case: precompute conditional branch inner yield's operand
            // value so we can return it now while still installing placeholders
            // for subsequent resumes. Also record which branch was chosen so we
            // can ensure we replace the chosen branch's yield in the AST to
            // avoid re-yielding it on the next resume.
            let mut precomputed_yield: Option<Value> = None;
            let mut precomputed_choice_is_then: Option<bool> = None;
            if let Some(prestmt) = gen_obj.body.get(pc_val)
                && let StatementKind::Expr(e) = &*prestmt.kind
                && let Expr::Conditional(cond, then_expr, else_expr) = e
            {
                // Only precompute the chosen branch inner yield if the
                // condition can be evaluated without encountering a
                // `yield`. Evaluating a `Yield` node directly via
                // `evaluate_expr` is invalid and will throw, so skip
                // precomputation when the condition contains yields.
                if !expr_contains_yield(cond) {
                    let cond_val = crate::core::evaluate_expr(mc, &func_env, cond)?;
                    let (chosen, is_then) = if cond_val.to_truthy() {
                        (then_expr, true)
                    } else {
                        (else_expr, false)
                    };
                    if let Some((_, chosen_inner)) = find_yield_in_expr(chosen)
                        && let Some(inner_expr) = chosen_inner
                    {
                        let val = crate::core::evaluate_expr(mc, &func_env, &inner_expr)?;
                        precomputed_yield = Some(val);
                        precomputed_choice_is_then = Some(is_then);
                        log::trace!(
                            "generator_next: Precomputed conditional-branch inner yield value -> {:?}",
                            precomputed_yield
                        );
                    }
                }
            }

            // Collect name of the first replaced var (install placeholder for the yield being resumed)
            let mut replaced_vars: Vec<String> = vec![];
            if let Some(first_stmt) = gen_obj.body.get_mut(pc_val) {
                // If we precomputed a chosen branch in a conditional, prefer to
                // replace the yield in that chosen branch so that the yielded
                // value does not remain in the AST and get re-yielded on the
                // following resume. The `precomputed_choice_is_then` value is
                // only set when the condition could be evaluated without
                // encountering `yield` nodes.
                if let Some(is_then) = precomputed_choice_is_then {
                    // Compute an index for naming the placeholder now (avoid holding
                    // simultaneous mutable/immutable borrows of the statement).
                    let next_idx = count_yield_vars_in_statement(first_stmt, &base_name);
                    let candidate = format!("{}{}", base_name, next_idx);
                    if let StatementKind::Expr(e) = &mut *first_stmt.kind
                        && let Expr::Conditional(_cond, then_expr, else_expr) = e
                    {
                        let mut did_replace = false;
                        if is_then {
                            **then_expr = replace_first_yield_in_expr(then_expr, &candidate, &mut did_replace);
                        } else {
                            **else_expr = replace_first_yield_in_expr(else_expr, &candidate, &mut did_replace);
                        }
                        if did_replace {
                            replaced_vars.push(candidate.clone());
                            log::debug!(
                                "DEBUG: body[{}] after chosen-branch replacement, kind={:?}",
                                pc_val,
                                first_stmt.kind
                            );
                        }
                    }
                }

                // Special-case: if the first top-level statement is a conditional
                // whose condition contains a `yield`, *do not* perform the generic
                // first-yield replacement here. Instead let the conditional-specific
                // resume logic re-evaluate the condition and replace the chosen
                // branch's yield so we don't replace the wrong branch (and cause
                // extra yields later).
                let skip_initial_replacement = if let StatementKind::Expr(e) = &*first_stmt.kind
                    && let Expr::Conditional(cond, _, _) = e
                    && expr_contains_yield(cond)
                {
                    true
                } else {
                    false
                };

                if replaced_vars.is_empty() && !skip_initial_replacement {
                    let next_idx = count_yield_vars_in_statement(first_stmt, &base_name);
                    let candidate = format!("{}{}", base_name, next_idx);
                    let mut did_replace = false;
                    replace_first_yield_in_statement(first_stmt, &candidate, &mut did_replace);
                    if did_replace {
                        replaced_vars.push(candidate.clone());
                        log::debug!("DEBUG: body[{}] after next replacement, kind={:?}", pc_val, first_stmt.kind);
                    }
                } else if skip_initial_replacement {
                    log::trace!("generator_next: skip_initial_replacement=true for pc={}", pc_val);
                }

                // Handle the case where the first top-level statement is a
                // Conditional whose *condition* contains a `yield`. In this
                // resume path we must replace the yield inside the condition
                // with a placeholder bound to the provided `send_value`, then
                // re-evaluate the condition to determine the chosen branch and
                // process that branch's yield accordingly. This ensures the
                // condition's yield is not re-yielded on the next resume.
                if skip_initial_replacement && let Some(first_stmt) = gen_obj.body.get_mut(pc_val) {
                    // First, check immutably whether the first statement is a
                    // conditional whose condition contains a yield. We avoid
                    // taking a mutable borrow until we've computed indices and
                    // other values needed to perform replacement to satisfy
                    // the borrow checker.
                    if let StatementKind::Expr(e_imm) = &*first_stmt.kind
                        && let Expr::Conditional(cond_imm, _then_imm, _else_imm) = e_imm
                        && expr_contains_yield(cond_imm)
                    {
                        // Compute an index for naming the placeholder now
                        // (avoid holding conflicting mutable/immutable borrows).
                        let next_idx = count_yield_vars_in_statement(first_stmt, &base_name);
                        let candidate = format!("{}{}", base_name, next_idx);

                        // Now take a mutable borrow to replace the first yield
                        // inside the condition and bind it to the provided
                        // send value for this resume.
                        // Perform the replacement within a limited mutable
                        // borrow scope so we can bind the placeholder without
                        // holding on to mutable borrows while calling other
                        // helpers that require immutable access.
                        let mut did_replace = false;
                        {
                            if let StatementKind::Expr(e_mut) = &mut *first_stmt.kind
                                && let Expr::Conditional(cond_mut, _then_mut, _else_mut) = e_mut
                            {
                                **cond_mut = replace_first_yield_in_expr(cond_mut, &candidate, &mut did_replace);
                                if did_replace {
                                    env_set(mc, &func_env, &candidate, send_value)?;
                                }
                            }
                        }

                        if did_replace {
                            bind_replaced_yield_decl(mc, &func_env, first_stmt, &candidate)?;

                            // Re-evaluate the newly-modified condition so we
                            // can inspect the chosen branch.
                            if let StatementKind::Expr(e_ref2) = &*first_stmt.kind
                                && let Expr::Conditional(cond_ref2, then_ref2, else_ref2) = e_ref2
                            {
                                let cond_val = crate::core::evaluate_expr(mc, &func_env, cond_ref2)?;
                                let chosen_ref: &Expr = if cond_val.to_truthy() { then_ref2 } else { else_ref2 };

                                if let Some((_, chosen_inner)) = find_yield_in_expr(chosen_ref) {
                                    // If the chosen branch has a bare yield, install a
                                    // placeholder for it and return undefined as the
                                    // yielded value so subsequent resumes won't re-yield.
                                    if chosen_inner.is_none() {
                                        let next_idx = count_yield_vars_in_statement(first_stmt, &base_name);
                                        let candidate2 = format!("{}{}", base_name, next_idx);
                                        if let StatementKind::Expr(e) = &mut *first_stmt.kind
                                            && let Expr::Conditional(_cond, then_expr2, else_expr2) = e
                                        {
                                            let mut did_replace2 = false;
                                            if cond_val.to_truthy() {
                                                **then_expr2 = replace_first_yield_in_expr(then_expr2, &candidate2, &mut did_replace2);
                                            } else {
                                                **else_expr2 = replace_first_yield_in_expr(else_expr2, &candidate2, &mut did_replace2);
                                            }
                                            if did_replace2 {
                                                env_set(mc, &func_env, &candidate2, &Value::Undefined)?;
                                                bind_replaced_yield_decl(mc, &func_env, first_stmt, &candidate2)?;
                                            }
                                        }

                                        gen_obj.cached_initial_yield = Some(Value::Undefined);
                                        return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?);
                                    } else if let Some(inner_expr) = chosen_inner {
                                        // Evaluate the chosen branch's inner expression and
                                        // return it as the yielded value, while also
                                        // replacing that yield in the AST so it won't be
                                        // re-yielded on subsequent resumes.
                                        match crate::core::evaluate_expr(mc, &func_env, &inner_expr) {
                                            Ok(val) => {
                                                // Replace chosen branch's yield with placeholder
                                                let body_idx = pc_val;
                                                if let Some(body_stmt) = gen_obj.body.get_mut(body_idx) {
                                                    let next_idx = count_yield_vars_in_statement(body_stmt, &base_name);
                                                    let candidate = format!("{}{}", base_name, next_idx);
                                                    if let StatementKind::Expr(e) = &mut *body_stmt.kind
                                                        && let Expr::Conditional(_cond, then_expr3, else_expr3) = e
                                                    {
                                                        let mut did_replace3 = false;
                                                        if cond_val.to_truthy() {
                                                            **then_expr3 =
                                                                replace_first_yield_in_expr(then_expr3, &candidate, &mut did_replace3);
                                                        } else {
                                                            **else_expr3 =
                                                                replace_first_yield_in_expr(else_expr3, &candidate, &mut did_replace3);
                                                        }
                                                        if did_replace3 {
                                                            env_set(mc, &func_env, &candidate, &Value::Undefined)?;
                                                            bind_replaced_yield_decl(mc, &func_env, body_stmt, &candidate)?;
                                                        }
                                                    }
                                                }

                                                gen_obj.cached_initial_yield = Some(val.clone());
                                                return Ok(create_iterator_result(mc, &func_env, &val, false)?);
                                            }
                                            Err(e) => return Err(e),
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Clone the tail for execution (now contains all replacements)
            let mut tail: Vec<Statement> = gen_obj.body[pc_val..].to_vec();

            // If we replaced yields, bind the first replaced var to this resume's
            // send value and initialize other placeholders to `undefined` so they
            // are ready to be updated on future resumes. If no replacements
            // occurred, fall back to the old single-var behavior.
            if !replaced_vars.is_empty() {
                env_set(mc, &func_env, &replaced_vars[0], send_value)?;
                log::trace!(
                    "generator_next: bound yield var '{}' -> {:?} in func_env ptr={:p}",
                    replaced_vars[0],
                    send_value,
                    Gc::as_ptr(func_env)
                );
                for v in replaced_vars.iter().skip(1) {
                    env_set(mc, &func_env, v, &Value::Undefined)?;
                    log::trace!(
                        "generator_next: initialised placeholder yield var '{}' = undefined in func_env ptr={:p}",
                        v,
                        Gc::as_ptr(func_env)
                    );
                }
                if let Some(stmt) = gen_obj.body.get(pc_val) {
                    for v in &replaced_vars {
                        bind_replaced_yield_decl(mc, &func_env, stmt, v)?;
                    }
                }

                // If the top-level statement is a Conditional whose condition
                // contains a yield, evaluate the condition now and replace the
                // chosen branch's yield with a placeholder so we don't replace
                // the wrong branch later during resume.
                if let Some(first_stmt) = gen_obj.body.get_mut(pc_val) {
                    // Snapshot yield vars count before taking mutable borrows so we
                    // don't create conflicting simultaneous borrows.
                    let snapshot_next_idx = count_yield_vars_in_statement(&*first_stmt, &base_name);

                    // If the statement is a conditional, examine it. Compute the
                    // condition value first, then look for yields only in the
                    // chosen branch. Do not hold conflicting borrows across calls.
                    if let StatementKind::Expr(e) = &mut *first_stmt.kind
                        && let Expr::Conditional(cond, then_expr, else_expr) = e
                    {
                        let cond_val = crate::core::evaluate_expr(mc, &func_env, cond)?;
                        log::trace!(
                            "generator_next: early conditional handling cond_val.truthy={}",
                            cond_val.to_truthy()
                        );

                        // Create an immutable reference to the chosen branch's expr
                        // for inspection so we don't move the mutable references.
                        let chosen_ref: &Expr = if cond_val.to_truthy() { then_expr } else { else_expr };

                        if let Some((_, chosen_inner)) = find_yield_in_expr(chosen_ref) {
                            // Use the previously computed snapshot index for candidate naming
                            let candidate = format!("{}{}", base_name, snapshot_next_idx);
                            let mut did_replace = false;
                            if cond_val.to_truthy() {
                                **then_expr = replace_first_yield_in_expr(then_expr, &candidate, &mut did_replace);
                            } else {
                                **else_expr = replace_first_yield_in_expr(else_expr, &candidate, &mut did_replace);
                            }
                            if did_replace {
                                env_set(mc, &func_env, &candidate, &Value::Undefined)?;
                                bind_replaced_yield_decl(mc, &func_env, first_stmt, &candidate)?;
                            }

                            // If the chosen branch's yield is bare, return undefined
                            // immediately as the yielded value so later resumes won't re-yield.
                            if chosen_inner.is_none() {
                                gen_obj.cached_initial_yield = Some(Value::Undefined);
                                return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?);
                            } else if let Some(inner_expr) = chosen_inner {
                                match crate::core::evaluate_expr(mc, &func_env, &inner_expr) {
                                    Ok(val) => {
                                        gen_obj.cached_initial_yield = Some(val.clone());
                                        return Ok(create_iterator_result(mc, &func_env, &val, false)?);
                                    }
                                    Err(e) => return Err(e),
                                }
                            }
                        }
                    }
                }
            } else {
                // Fallback: no fresh yields found to replace. Try to locate an
                // existing placeholder (from a prior resume) and update it with
                // the current send value. If none found, allocate a new var.
                let mut updated_existing = false;
                for i in 0..128usize {
                    let candidate = format!("{}{}", base_name, i);
                    if let Some(val_rc) = env_get_own(&func_env, &candidate)
                        && matches!(&*val_rc.borrow(), Value::Undefined)
                    {
                        env_set(mc, &func_env, &candidate, send_value)?;
                        log::trace!(
                            "generator_next: updated existing placeholder '{}' -> {:?} in func_env ptr={:p}",
                            candidate,
                            send_value,
                            Gc::as_ptr(func_env)
                        );
                        if let Some(stmt) = gen_obj.body.get(pc_val) {
                            bind_replaced_yield_decl(mc, &func_env, stmt, &candidate)?;
                        }
                        updated_existing = true;
                        break;
                    }
                }

                if !updated_existing {
                    let next_idx = if let Some(s) = gen_obj.body.get(pc_val) {
                        count_yield_vars_in_statement(s, &base_name)
                    } else {
                        0
                    };
                    let var_name = format!("{}{}", base_name, next_idx);
                    env_set(mc, &func_env, &var_name, send_value)?;
                    log::trace!(
                        "generator_next: bound yield var '{}' -> {:?} in func_env ptr={:p} (fallback new)",
                        var_name,
                        send_value,
                        Gc::as_ptr(func_env)
                    );
                    if let Some(stmt) = gen_obj.body.get(pc_val) {
                        bind_replaced_yield_decl(mc, &func_env, stmt, &var_name)?;
                    }
                }
            }

            // If we precomputed a yield value (conditional branch case), return it
            // immediately as the yielded value while leaving placeholders
            // installed for subsequent resumes.
            if let Some(val) = precomputed_yield {
                gen_obj.cached_initial_yield = Some(val.clone());
                return Ok(create_iterator_result(mc, &func_env, &val, false)?);
            }

            if let Some((idx, inner_idx_opt, yield_kind, yield_inner)) = find_first_yield_in_statements(&tail) {
                // ---- Catch-body yield special case ----
                // When the yield found by find_first_yield is inside the catch
                // body of a TryCatch whose try_body no longer contains yields
                // (they were replaced by placeholders), we must run the try body
                // first so that the throw fires, the catch param is bound, and
                // then the yield's inner expression (e.g., `e`) can be evaluated.
                let is_catch_body_yield = if let StatementKind::TryCatch(tc) = &*tail[idx].kind {
                    find_first_yield_in_statements(&tc.try_body).is_none()
                        && tc
                            .catch_body
                            .as_ref()
                            .is_some_and(|cb| find_first_yield_in_statements(cb).is_some())
                } else {
                    false
                };
                if is_catch_body_yield {
                    let tc = if let StatementKind::TryCatch(tc) = &*tail[idx].kind {
                        tc.clone()
                    } else {
                        unreachable!()
                    };

                    // Run pre-TryCatch statements
                    if idx > 0 {
                        let pre_stmts = tail[0..idx].to_vec();
                        crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    }

                    // Run the try body (should throw due to remaining throw stmts
                    // or previously-injected throw from generator_throw)
                    let try_result = crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &tc.try_body, &[]);

                    let thrown_value = match try_result {
                        Ok((crate::core::ControlFlow::Throw(v, _, _), _)) => Some(v),
                        Err(EvalError::Throw(v, _, _)) => Some(v),
                        Err(EvalError::Js(js_err)) => Some(crate::core::js_error_to_value(mc, &func_env, &js_err)),
                        Ok(_) => None, // try completed normally
                    };

                    if let Some(thrown) = thrown_value {
                        // Bind catch parameter in func_env
                        if let Some(CatchParamPattern::Identifier(name)) = &tc.catch_param {
                            env_set(mc, &func_env, name, &thrown)?;
                        }

                        let catch_body = tc.catch_body.as_ref().unwrap();
                        let (catch_yield_idx, _, _, catch_yield_inner) = find_first_yield_in_statements(catch_body).unwrap();

                        // Run pre-yield catch body statements
                        if catch_yield_idx > 0 {
                            let pre_catch = catch_body[0..catch_yield_idx].to_vec();
                            crate::core::evaluate_statements(mc, &func_env, &pre_catch)?;
                        }

                        // Evaluate the yield's inner expression
                        let yield_val = if let Some(inner) = catch_yield_inner {
                            crate::core::evaluate_expr(mc, &func_env, &inner)?
                        } else {
                            Value::Undefined
                        };

                        // Suspend at the TryCatch position. The yield in catch is
                        // NOT replaced yet -- on the next resume, replace_first_yield
                        // will replace it with a placeholder, then the TryCatch runs
                        // as a pre-stmt for the subsequent yield.
                        gen_obj.state = GeneratorState::Suspended {
                            pc: pc_val + idx,
                            stack: vec![],
                            pre_env: Some(func_env),
                        };
                        gen_obj.cached_initial_yield = Some(yield_val.clone());
                        return Ok(create_iterator_result(mc, &func_env, &yield_val, false)?);
                    } else {
                        // Try completed normally, catch not entered
                        if let Some(finally_stmts) = &tc.finally_body {
                            crate::core::evaluate_statements(mc, &func_env, finally_stmts)?;
                        }
                        gen_obj.state = GeneratorState::Suspended {
                            pc: pc_val + idx + 1,
                            stack: vec![],
                            pre_env: Some(func_env),
                        };
                        drop(gen_obj);
                        return generator_next(mc, generator, &Value::Undefined);
                    }
                }

                if let StatementKind::If(if_stmt) = &*tail[idx].kind
                    && !expr_contains_yield(&if_stmt.condition)
                {
                    let cond_val = crate::core::evaluate_expr(mc, &func_env, &if_stmt.condition)?;
                    let chosen_body = if cond_val.to_truthy() {
                        &if_stmt.then_body
                    } else {
                        if_stmt.else_body.as_deref().unwrap_or(&[])
                    };

                    if find_first_yield_in_statements(chosen_body).is_none() {
                        crate::core::evaluate_statements(mc, &func_env, std::slice::from_ref(&tail[idx]))?;
                        gen_obj.state = GeneratorState::Suspended {
                            pc: pc_val + idx + 1,
                            stack: vec![],
                            pre_env: Some(func_env),
                        };
                        drop(gen_obj);
                        return generator_next(mc, generator, &Value::Undefined);
                    }
                }

                let pre_env_opt: Option<JSObjectDataPtr> = if idx > 0 {
                    let pre_stmts = tail[0..idx].to_vec();
                    crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    Some(func_env)
                } else if let Some(inner_idx) = inner_idx_opt {
                    if inner_idx > 0 {
                        let pre_key = format!("__gen_pre_exec_{}_{}", pc_val, inner_idx);
                        if env_get_own(&func_env, &pre_key).is_none() {
                            match &*tail[idx].kind {
                                StatementKind::Block(inner_stmts) => {
                                    let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                                    let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                }
                                StatementKind::TryCatch(tc_stmt) => {
                                    // Determine which sub-body has the yield
                                    let yield_in_try = find_first_yield_in_statements(&tc_stmt.try_body).is_some();
                                    let yield_in_catch = !yield_in_try
                                        && tc_stmt
                                            .catch_body
                                            .as_ref()
                                            .is_some_and(|cb| find_first_yield_in_statements(cb).is_some());
                                    let yield_in_finally = !yield_in_try
                                        && !yield_in_catch
                                        && tc_stmt
                                            .finally_body
                                            .as_ref()
                                            .is_some_and(|fb| find_first_yield_in_statements(fb).is_some());

                                    if yield_in_try && inner_idx <= tc_stmt.try_body.len() {
                                        let pre_stmts = tc_stmt.try_body[0..inner_idx].to_vec();
                                        // Use evaluate_statements_with_context to catch throws
                                        // that should be handled by the TryCatch's catch/finally.
                                        let pre_result =
                                            crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &pre_stmts, &[]);
                                        match pre_result {
                                            Ok((crate::core::ControlFlow::Normal(_), _)) => {
                                                seed_simple_decl_bindings(mc, &func_env, &pre_stmts)?;
                                            }
                                            Ok((crate::core::ControlFlow::Throw(v, _, _), _)) | Err(EvalError::Throw(v, _, _)) => {
                                                // Pre-stmts threw inside try body. Handle via finally.
                                                if let Some(fb) = &tc_stmt.finally_body
                                                    && let Some((fin_idx, _, _, fin_inner)) = find_first_yield_in_statements(fb)
                                                {
                                                    if fin_idx > 0 {
                                                        crate::core::evaluate_statements(mc, &func_env, &fb[0..fin_idx])?;
                                                    }
                                                    let yield_val = if let Some(inner) = fin_inner {
                                                        crate::core::evaluate_expr(mc, &func_env, &inner)?
                                                    } else {
                                                        Value::Undefined
                                                    };
                                                    gen_obj.pending_completion = Some(GeneratorPendingCompletion::Throw(v));
                                                    let remaining_finally = if fin_idx + 1 < fb.len() {
                                                        fb[fin_idx + 1..].to_vec()
                                                    } else {
                                                        vec![]
                                                    };
                                                    gen_obj.body[pc_val + idx] = StatementKind::Block(remaining_finally).into();
                                                    gen_obj.state = GeneratorState::Suspended {
                                                        pc: pc_val + idx,
                                                        stack: vec![],
                                                        pre_env: Some(func_env),
                                                    };
                                                    gen_obj.cached_initial_yield = Some(yield_val.clone());
                                                    return Ok(create_iterator_result(mc, &func_env, &yield_val, false)?);
                                                }
                                                return Err(EvalError::Throw(v, None, None));
                                            }
                                            Err(EvalError::Js(js_err)) => {
                                                let v = crate::core::js_error_to_value(mc, &func_env, &js_err);
                                                if let Some(fb) = &tc_stmt.finally_body
                                                    && let Some((fin_idx, _, _, fin_inner)) = find_first_yield_in_statements(fb)
                                                {
                                                    if fin_idx > 0 {
                                                        crate::core::evaluate_statements(mc, &func_env, &fb[0..fin_idx])?;
                                                    }
                                                    let yield_val = if let Some(inner) = fin_inner {
                                                        crate::core::evaluate_expr(mc, &func_env, &inner)?
                                                    } else {
                                                        Value::Undefined
                                                    };
                                                    gen_obj.pending_completion = Some(GeneratorPendingCompletion::Throw(v));
                                                    let remaining_finally = if fin_idx + 1 < fb.len() {
                                                        fb[fin_idx + 1..].to_vec()
                                                    } else {
                                                        vec![]
                                                    };
                                                    gen_obj.body[pc_val + idx] = StatementKind::Block(remaining_finally).into();
                                                    gen_obj.state = GeneratorState::Suspended {
                                                        pc: pc_val + idx,
                                                        stack: vec![],
                                                        pre_env: Some(func_env),
                                                    };
                                                    gen_obj.cached_initial_yield = Some(yield_val.clone());
                                                    return Ok(create_iterator_result(mc, &func_env, &yield_val, false)?);
                                                }
                                                return Err(EvalError::Throw(v, None, None));
                                            }
                                            _ => {
                                                seed_simple_decl_bindings(mc, &func_env, &pre_stmts)?;
                                            }
                                        }
                                    } else if yield_in_catch || yield_in_finally {
                                        // Run the entire try body within TryCatch semantics.
                                        if !tc_stmt.try_body.is_empty() {
                                            let try_result = crate::core::evaluate_statements_with_context_and_last_value(
                                                mc,
                                                &func_env,
                                                &tc_stmt.try_body,
                                                &[],
                                            );
                                            match try_result {
                                                Ok((crate::core::ControlFlow::Throw(v, _, _), _)) | Err(EvalError::Throw(v, _, _)) => {
                                                    if let Some(cb) = &tc_stmt.catch_body {
                                                        if let Some(CatchParamPattern::Identifier(name)) = &tc_stmt.catch_param {
                                                            let _ = env_set(mc, &func_env, name, &v);
                                                        }
                                                        if !yield_in_catch {
                                                            let _ = crate::core::evaluate_statements(mc, &func_env, cb);
                                                        }
                                                    }
                                                }
                                                Err(EvalError::Js(js_err)) => {
                                                    let v = crate::core::js_error_to_value(mc, &func_env, &js_err);
                                                    if let Some(cb) = &tc_stmt.catch_body {
                                                        if let Some(CatchParamPattern::Identifier(name)) = &tc_stmt.catch_param {
                                                            let _ = env_set(mc, &func_env, name, &v);
                                                        }
                                                        if !yield_in_catch {
                                                            let _ = crate::core::evaluate_statements(mc, &func_env, cb);
                                                        }
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                        if yield_in_finally {
                                            if let Some(fb) = &tc_stmt.finally_body
                                                && inner_idx > 0
                                                && inner_idx <= fb.len()
                                            {
                                                let pre_fin = fb[0..inner_idx].to_vec();
                                                let _ = crate::core::evaluate_statements(mc, &func_env, &pre_fin)?;
                                            }
                                        } else if yield_in_catch
                                            && let Some(cb) = &tc_stmt.catch_body
                                            && inner_idx > 0
                                            && inner_idx <= cb.len()
                                        {
                                            let pre_stmts = cb[0..inner_idx].to_vec();
                                            let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                        }
                                    }
                                }
                                _ => {}
                            }
                            if let Err(e) = env_set(mc, &func_env, &pre_key, &Value::Boolean(true)) {
                                log::warn!("Error setting pre-execution env key: {e}");
                            }
                        }

                        // Nested TryCatch catch-body yield: if the statement
                        // at try_body[inner_idx] is a TryCatch with no yields
                        // in its try body but yields in catch, run the inner
                        // try body and bind the catch param. This runs once per
                        // resume (keyed separately from pre_stmts above).
                        if let StatementKind::TryCatch(tc_stmt) = &*tail[idx].kind {
                            let nested_key = format!("__gen_nested_tc_{}_{}", pc_val, inner_idx);
                            if env_get_own(&func_env, &nested_key).is_none()
                                && let Some(inner_stmt) = tc_stmt.try_body.get(inner_idx)
                                && let StatementKind::TryCatch(inner_tc) = &*inner_stmt.kind
                                && find_first_yield_in_statements(&inner_tc.try_body).is_none()
                                && inner_tc
                                    .catch_body
                                    .as_ref()
                                    .is_some_and(|cb| find_first_yield_in_statements(cb).is_some())
                            {
                                let inner_tc_clone = inner_tc.clone();
                                let inner_try_result = crate::core::evaluate_statements_with_context_and_last_value(
                                    mc,
                                    &func_env,
                                    &inner_tc_clone.try_body,
                                    &[],
                                );
                                match inner_try_result {
                                    Ok((crate::core::ControlFlow::Throw(thrown, _, _), _)) | Err(EvalError::Throw(thrown, _, _)) => {
                                        if let Some(CatchParamPattern::Identifier(name)) = &inner_tc_clone.catch_param {
                                            env_set(mc, &func_env, name, &thrown)?;
                                        }
                                    }
                                    Err(EvalError::Js(js_err)) => {
                                        let thrown = crate::core::js_error_to_value(mc, &func_env, &js_err);
                                        if let Some(CatchParamPattern::Identifier(name)) = &inner_tc_clone.catch_param {
                                            env_set(mc, &func_env, name, &thrown)?;
                                        }
                                    }
                                    Ok(_) => {} // inner try completed normally
                                }
                                // Run catch pre-yield stmts
                                if let Some(cb) = &inner_tc_clone.catch_body
                                    && let Some((cyi, _, _, _)) = find_first_yield_in_statements(cb)
                                    && cyi > 0
                                {
                                    let pre_catch = cb[0..cyi].to_vec();
                                    crate::core::evaluate_statements(mc, &func_env, &pre_catch)?;
                                }
                                let _ = env_set(mc, &func_env, &nested_key, &Value::Boolean(true));
                            }
                        }
                    }
                    Some(func_env)
                } else {
                    Some(func_env)
                };

                gen_obj.state = GeneratorState::Suspended {
                    pc: pc_val + idx,
                    stack: vec![],
                    pre_env: pre_env_opt,
                };

                if let Some((iter_obj, done)) = prepare_pending_iterator_for_yield(mc, &func_env, &tail[idx])? {
                    gen_obj.pending_iterator = Some(iter_obj);
                    gen_obj.pending_iterator_done = done;
                } else {
                    gen_obj.pending_iterator = None;
                    gen_obj.pending_iterator_done = false;
                }

                if let Some(inner_expr_box) = yield_inner {
                    // If the yield is inside a `for` loop body, ensure the loop
                    // initializer and any body pre-statements execute so loop
                    // bindings (e.g., `let i`) exist before evaluating the yield.
                    if let StatementKind::For(for_stmt) = tail[idx].kind.as_mut() {
                        if let Some(init_stmt) = for_stmt.init.clone() {
                            if for_init_needs_execution(&func_env, &init_stmt) {
                                crate::core::evaluate_statements(mc, &func_env, std::slice::from_ref(&init_stmt))?;
                            }
                            for_stmt.init = None;
                            if let Some(body_stmt) = gen_obj.body.get_mut(pc_val + idx)
                                && let StatementKind::For(body_for_stmt) = body_stmt.kind.as_mut()
                            {
                                body_for_stmt.init = None;
                            }
                            if let Some(snapshot_stmt) = for_stmt_snapshot.as_mut()
                                && let StatementKind::For(snapshot_for_stmt) = snapshot_stmt.kind.as_mut()
                            {
                                snapshot_for_stmt.init = None;
                            }
                        }

                        if let Some(inner_idx) = inner_idx_opt
                            && inner_idx > 0
                        {
                            let pre_stmts = for_stmt.body[0..inner_idx].to_vec();
                            let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                        }
                    }
                    if let StatementKind::TryCatch(tc_stmt) = tail[idx].kind.as_mut()
                        && let Some(inner_idx) = inner_idx_opt
                        && inner_idx > 0
                    {
                        tc_stmt.try_body.drain(0..inner_idx);
                        // NOTE: Do NOT drain from gen_obj.body — the pre_key
                        // guard prevents re-execution, and draining from the
                        // persistent body would change try_body indices and
                        // invalidate count_yield_vars_in_statement on future
                        // resumes.
                    }

                    slot_set(mc, &func_env, InternalSlot::GenThrowVal, &Value::Undefined);

                    // Special-case conditional expressions: the next yield in the
                    // statement may be inside the consequent or alternate. Re-evaluate
                    // the condition to determine which branch will be taken and only
                    // evaluate the yield's inner expression for the selected branch.
                    if let StatementKind::Expr(e) = &*tail[idx].kind
                        && let Expr::Conditional(cond, then_expr, else_expr) = e
                    {
                        let cond_val = crate::core::evaluate_expr(mc, &func_env, cond)?;
                        let chosen: &Expr = if cond_val.to_truthy() { then_expr } else { else_expr };
                        if let Some((_, chosen_inner)) = find_yield_in_expr(chosen) {
                            let func_home = func_env.borrow().get_home_object().map(|h| Gc::as_ptr(*h.borrow()));
                            log::trace!(
                                "generator_next: Resume inner eval for conditional branch env_ptr={:p} env.home={:?} gen_ptr={:p} branch_chosen={}",
                                Gc::as_ptr(func_env),
                                func_home,
                                Gc::as_ptr(*generator),
                                if cond_val.to_truthy() { "then" } else { "else" }
                            );

                            // If the chosen branch has a nested expression for the yield
                            // (e.g., `yield <expr>`), evaluate it and return that value.
                            if let Some(inner_expr) = chosen_inner {
                                match crate::core::evaluate_expr(mc, &func_env, &inner_expr) {
                                    Ok(val) => {
                                        // Replace the chosen branch's yield with a placeholder so
                                        // the yielded value is not re-yielded on the next resume.
                                        let body_idx = pc_val + idx;
                                        if let Some(body_stmt) = gen_obj.body.get_mut(body_idx) {
                                            let next_idx = count_yield_vars_in_statement(body_stmt, &base_name);
                                            let candidate = format!("{}{}", base_name, next_idx);
                                            if let StatementKind::Expr(e) = &mut *body_stmt.kind
                                                && let Expr::Conditional(_cond, then_expr, else_expr) = e
                                            {
                                                let mut did_replace = false;
                                                if cond_val.to_truthy() {
                                                    **then_expr = replace_first_yield_in_expr(then_expr, &candidate, &mut did_replace);
                                                } else {
                                                    **else_expr = replace_first_yield_in_expr(else_expr, &candidate, &mut did_replace);
                                                }
                                                if did_replace {
                                                    env_set(mc, &func_env, &candidate, &Value::Undefined)?;
                                                    bind_replaced_yield_decl(mc, &func_env, body_stmt, &candidate)?;
                                                }
                                            }
                                        }

                                        gen_obj.cached_initial_yield = Some(val.clone());
                                        return Ok(create_iterator_result(mc, &func_env, &val, false)?);
                                    }
                                    Err(e) => {
                                        let throw_val = eval_error_to_value(mc, &func_env, e);
                                        drop(gen_obj);
                                        return generator_throw(mc, generator, &throw_val);
                                    }
                                }
                            } else {
                                // Chosen branch contains a bare `yield` (no inner expr). Install
                                // a placeholder for that yield, initialize it to `undefined`,
                                // and return `undefined` as the yielded value so subsequent
                                // resumes don't re-yield the same yield.
                                let body_idx = pc_val + idx;
                                if let Some(body_stmt) = gen_obj.body.get_mut(body_idx) {
                                    let next_idx = count_yield_vars_in_statement(body_stmt, &base_name);
                                    let candidate = format!("{}{}", base_name, next_idx);
                                    if let StatementKind::Expr(e) = &mut *body_stmt.kind
                                        && let Expr::Conditional(_cond, then_expr, else_expr) = e
                                    {
                                        let mut did_replace = false;
                                        if cond_val.to_truthy() {
                                            **then_expr = replace_first_yield_in_expr(then_expr, &candidate, &mut did_replace);
                                        } else {
                                            **else_expr = replace_first_yield_in_expr(else_expr, &candidate, &mut did_replace);
                                        }
                                        if did_replace {
                                            env_set(mc, &func_env, &candidate, &Value::Undefined)?;
                                            bind_replaced_yield_decl(mc, &func_env, body_stmt, &candidate)?;
                                        }
                                    }
                                }

                                gen_obj.cached_initial_yield = Some(Value::Undefined);
                                return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?);
                            }
                        }
                    }

                    if yield_kind == YieldKind::Yield && expr_contains_yield(&inner_expr_box) {
                        gen_obj.cached_initial_yield = Some(Value::Undefined);
                        return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?);
                    }

                    let func_home = func_env.borrow().get_home_object().map(|h| Gc::as_ptr(*h.borrow()));
                    log::trace!(
                        "generator_next: Resume inner eval env_ptr={:p} env.home={:?} gen_ptr={:p}",
                        Gc::as_ptr(func_env),
                        func_home,
                        Gc::as_ptr(*generator)
                    );
                    match crate::core::evaluate_expr(mc, &func_env, &inner_expr_box) {
                        Ok(val) => {
                            if matches!(yield_kind, YieldKind::YieldStar) {
                                let delegated = (|| -> Result<Value<'gc>, EvalError<'gc>> {
                                    let iterator = get_iterator(mc, &val, &func_env)?;
                                    let next_method = crate::core::get_property_with_accessors(mc, &func_env, &iterator, "next")?;
                                    let iter_res = evaluate_call_dispatch(
                                        mc,
                                        &func_env,
                                        &next_method,
                                        Some(&Value::Object(iterator)),
                                        std::slice::from_ref(&Value::Undefined),
                                    )?;

                                    if let Value::Object(res_obj) = iter_res {
                                        let done_val = crate::core::get_property_with_accessors(mc, &func_env, &res_obj, "done")?;
                                        let done = done_val.to_truthy();

                                        if !done {
                                            gen_obj.yield_star_iterator = Some(iterator);
                                            let yielded = if let Some(v) = object_get_key_value(&res_obj, "value") {
                                                v.borrow().clone()
                                            } else {
                                                Value::Undefined
                                            };
                                            gen_obj.cached_initial_yield = Some(yielded.clone());
                                            return Ok(create_iterator_result_with_done(mc, &func_env, &yielded, &done_val)?);
                                        }

                                        let value = crate::core::get_property_with_accessors(mc, &func_env, &res_obj, "value")?;
                                        Ok(value)
                                    } else {
                                        Err(raise_type_error!("Iterator result is not an object").into())
                                    }
                                })();

                                match delegated {
                                    Ok(v) => {
                                        if let Some(_iter) = gen_obj.yield_star_iterator {
                                            return Ok(v);
                                        }
                                        drop(gen_obj);
                                        return generator_next(mc, generator, &v);
                                    }
                                    Err(e) => {
                                        let throw_val = eval_error_to_value(mc, &func_env, e);
                                        drop(gen_obj);
                                        return generator_throw(mc, generator, &throw_val);
                                    }
                                }
                            }

                            if idx == 0
                                && let Some(snapshot_stmt) = for_stmt_snapshot.clone()
                                && let Some(body_stmt) = gen_obj.body.get_mut(pc_val)
                                && matches!(&*body_stmt.kind, StatementKind::For(_))
                            {
                                *body_stmt = snapshot_stmt;
                            }
                            gen_obj.cached_initial_yield = Some(val.clone());
                            return Ok(create_iterator_result(mc, &func_env, &val, false)?);
                        }
                        Err(e) => {
                            let throw_val = eval_error_to_value(mc, &func_env, e);
                            drop(gen_obj);
                            return generator_throw(mc, generator, &throw_val);
                        }
                    }
                }

                return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, false)?);
            }

            // Execute the (possibly modified) tail and interpret completion per spec
            let tail_result = crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &tail, &[]);
            log::trace!("DEBUG: evaluate_statements result: {:?}", tail_result);
            gen_obj.state = GeneratorState::Completed;

            // Per GeneratorStart spec step 4.i-j: dispose resources on generator
            // body's LexicalEnvironment before returning.
            let (cf, _last) = match tail_result {
                Ok(pair) => pair,
                Err(e) => {
                    // Tail evaluation threw via EvalError — try to dispose, then propagate.
                    let thrown = eval_error_to_value(mc, &func_env, e);
                    let _ = crate::core::dispose_resources_with_completion(mc, &func_env, Some(thrown.clone()));
                    return Err(crate::core::EvalError::Throw(thrown, None, None));
                }
            };

            // Extract throw value if the body ended with a throw completion.
            let thrown_val = if let crate::core::ControlFlow::Throw(ref v, _, _) = cf {
                Some(v.clone())
            } else {
                None
            };

            // Dispose resources, merging any disposal errors with the body completion.
            let dispose_result = crate::core::dispose_resources_with_completion(mc, &func_env, thrown_val.clone());

            match (&cf, dispose_result) {
                // Body threw AND disposal threw → disposal already produced SuppressedError
                (crate::core::ControlFlow::Throw(_, _, _), Err(crate::core::EvalError::Throw(v, l, c))) => {
                    Err(crate::core::EvalError::Throw(v, l, c))
                }
                // Body threw, disposal ok → propagate original throw
                (crate::core::ControlFlow::Throw(v, l, c), _) => Err(crate::core::EvalError::Throw(v.clone(), *l, *c)),
                // Body normal, disposal threw → propagate disposal error
                (_, Err(crate::core::EvalError::Throw(v, l, c))) => Err(crate::core::EvalError::Throw(v, l, c)),
                // Body returned, disposal ok → normal return
                (crate::core::ControlFlow::Return(v), Ok(())) => Ok(create_iterator_result(mc, &func_env, v, true)?),
                // Body normal, disposal ok → normal completion
                (crate::core::ControlFlow::Normal(_), Ok(())) => Ok(create_iterator_result(mc, &func_env, &Value::Undefined, true)?),
                // Other disposal errors
                (_, Err(e)) => Err(e),
                // Other control flows
                _ => Err(raise_eval_error!("Unexpected control flow after evaluating generator tail").into()),
            }
        }
        GeneratorState::Running { .. } => Err(raise_type_error!("Generator is already running").into()),
        GeneratorState::Completed => {
            // Restore state to Completed (std::mem::replace set it to Running above)
            gen_obj.state = GeneratorState::Completed;
            Ok(create_iterator_result(mc, &gen_obj.env, &Value::Undefined, true)?)
        }
    }
}

/// Execute generator.return()
fn resume_with_return_completion<'gc>(
    mc: &MutationContext<'gc>,
    generator: &crate::core::GcPtr<'gc, crate::core::JSGenerator<'gc>>,
    return_value: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut gen_obj = generator.borrow_mut(mc);
    match &mut gen_obj.state {
        GeneratorState::NotStarted | GeneratorState::Completed => {
            gen_obj.state = GeneratorState::Completed;
            Ok(create_iterator_result(mc, &gen_obj.env, return_value, true)?)
        }
        GeneratorState::Running { .. } => Err(raise_type_error!("Generator is already running").into()),
        GeneratorState::Suspended { pc, .. } => {
            let pc_val = *pc;
            if pc_val >= gen_obj.body.len() {
                gen_obj.state = GeneratorState::Completed;
                return Ok(create_iterator_result(mc, &gen_obj.env, return_value, true)?);
            }

            let mut tail: Vec<Statement> = gen_obj.body[pc_val..].to_vec();
            let tail_before = tail.clone();

            let mut replaced = false;
            for s in tail.iter_mut() {
                if replace_first_yield_statement_with_return(s) {
                    replaced = true;
                    break;
                }
            }
            if !replaced {
                tail[0] = StatementKind::Return(Some(Expr::Var("__gen_throw_val".to_string(), None, None))).into();
            }

            let func_env = if let GeneratorState::Suspended { pre_env: Some(env), .. } = &gen_obj.state {
                *env
            } else {
                prepare_function_call_env(mc, Some(&gen_obj.env), gen_obj.this_val.clone().as_ref(), None, &[], None, None)?
            };

            if let Some((idx, inner_idx_opt, _yield_kind, _yield_inner)) = find_first_yield_in_statements(&tail_before) {
                if idx > 0 {
                    let pre_stmts = tail_before[0..idx].to_vec();
                    crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                } else if let Some(inner_idx) = inner_idx_opt
                    && inner_idx > 0
                    && let StatementKind::Block(inner_stmts) = &*tail_before[idx].kind
                {
                    let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                    let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                }
            }

            slot_set(mc, &func_env, InternalSlot::GenThrowVal, return_value);

            // ---- Finally-body yield special case for .return() ----
            // When .return() is called while suspended in a try-finally, the
            // yield in try is replaced with `return __gen_throw_val`. If the
            // finally body has a yield, we need to suspend at that yield and
            // park the return completion for re-fire after finally.
            if let Some((tc_idx, _, _, _)) = find_first_yield_in_statements(&tail) {
                let is_finally_yield = if let StatementKind::TryCatch(tc) = &*tail[tc_idx].kind {
                    find_first_yield_in_statements(&tc.try_body).is_none()
                        && (tc.catch_body.is_none()
                            || tc
                                .catch_body
                                .as_ref()
                                .is_some_and(|cb| find_first_yield_in_statements(cb).is_none()))
                        && tc
                            .finally_body
                            .as_ref()
                            .is_some_and(|fb| find_first_yield_in_statements(fb).is_some())
                } else {
                    false
                };

                if is_finally_yield {
                    let tc = if let StatementKind::TryCatch(tc) = &*tail[tc_idx].kind {
                        tc.clone()
                    } else {
                        unreachable!()
                    };

                    // Run pre-TryCatch statements from the modified tail
                    if tc_idx > 0 {
                        let pre_stmts = tail[0..tc_idx].to_vec();
                        crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    }

                    // Run try body (which now has `return __gen_throw_val`)
                    let try_result = crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &tc.try_body, &[]);

                    // Determine the abrupt completion from try/catch
                    let abrupt_completion: Option<GeneratorPendingCompletion> = match try_result {
                        Ok((crate::core::ControlFlow::Return(v), _)) => Some(GeneratorPendingCompletion::Return(v)),
                        Ok((crate::core::ControlFlow::Throw(v, _, _), _)) | Err(EvalError::Throw(v, _, _)) => {
                            if let Some(catch_body) = &tc.catch_body {
                                if let Some(CatchParamPattern::Identifier(name)) = &tc.catch_param {
                                    env_set(mc, &func_env, name, &v)?;
                                }
                                match crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, catch_body, &[]) {
                                    Ok((crate::core::ControlFlow::Normal(_), _)) => None,
                                    Ok((crate::core::ControlFlow::Return(rv), _)) => Some(GeneratorPendingCompletion::Return(rv)),
                                    Ok((crate::core::ControlFlow::Throw(tv, _, _), _)) => Some(GeneratorPendingCompletion::Throw(tv)),
                                    Err(EvalError::Throw(tv, _, _)) => Some(GeneratorPendingCompletion::Throw(tv)),
                                    _ => None,
                                }
                            } else {
                                Some(GeneratorPendingCompletion::Throw(v))
                            }
                        }
                        Err(EvalError::Js(js_err)) => {
                            let v = crate::core::js_error_to_value(mc, &func_env, &js_err);
                            Some(GeneratorPendingCompletion::Throw(v))
                        }
                        Ok(_) => None, // try completed normally
                    };

                    // Enter finally body: find yield, run pre-yield stmts, evaluate yield
                    let finally_body = tc.finally_body.as_ref().unwrap();
                    let (fin_yield_idx, _, _, fin_yield_inner) = find_first_yield_in_statements(finally_body).unwrap();

                    if fin_yield_idx > 0 {
                        let pre_fin = finally_body[0..fin_yield_idx].to_vec();
                        crate::core::evaluate_statements(mc, &func_env, &pre_fin)?;
                    }

                    let yield_val = if let Some(inner) = fin_yield_inner {
                        crate::core::evaluate_expr(mc, &func_env, &inner)?
                    } else {
                        Value::Undefined
                    };

                    // Store pending completion for re-fire after finally
                    gen_obj.pending_completion = abrupt_completion;

                    // Replace the TryCatch in body with remaining finally stmts
                    let remaining_finally = if fin_yield_idx + 1 < finally_body.len() {
                        finally_body[fin_yield_idx + 1..].to_vec()
                    } else {
                        vec![]
                    };
                    gen_obj.body[pc_val + tc_idx] = StatementKind::Block(remaining_finally).into();

                    gen_obj.state = GeneratorState::Suspended {
                        pc: pc_val + tc_idx,
                        stack: vec![],
                        pre_env: Some(func_env),
                    };
                    gen_obj.cached_initial_yield = Some(yield_val.clone());
                    return Ok(create_iterator_result(mc, &func_env, &yield_val, false)?);
                }

                // ---- Catch-body yield special case for .return() ----
                // When .return() is called while suspended in a try-catch, the
                // yield in try is replaced with `return __gen_throw_val`. If
                // the catch body has a yield (e.g. nested try-catch within catch),
                // we must handle it to avoid evaluate_statements failing on yield.
                let is_catch_yield = if let StatementKind::TryCatch(tc) = &*tail[tc_idx].kind {
                    find_first_yield_in_statements(&tc.try_body).is_none()
                        && tc
                            .catch_body
                            .as_ref()
                            .is_some_and(|cb| find_first_yield_in_statements(cb).is_some())
                } else {
                    false
                };

                if is_catch_yield {
                    // For .return(), the try body has `return __gen_throw_val`.
                    // The try body returns normally (or the return is caught?).
                    // Since the return exits the try and there's no throw,
                    // the catch is not entered. Run try + finally and return.
                    let tc = if let StatementKind::TryCatch(tc) = &*tail[tc_idx].kind {
                        tc.clone()
                    } else {
                        unreachable!()
                    };

                    if tc_idx > 0 {
                        let pre_stmts = tail[0..tc_idx].to_vec();
                        crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    }

                    let try_result = crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &tc.try_body, &[]);
                    match try_result {
                        Ok((crate::core::ControlFlow::Return(v), _)) => {
                            if let Some(finally_stmts) = &tc.finally_body {
                                crate::core::evaluate_statements(mc, &func_env, finally_stmts)?;
                            }
                            gen_obj.state = GeneratorState::Completed;
                            return Ok(create_iterator_result(mc, &func_env, &v, true)?);
                        }
                        Ok((crate::core::ControlFlow::Throw(v, _, _), _)) | Err(EvalError::Throw(v, _, _)) => {
                            // Try threw; bind catch param and run catch body
                            if let Some(CatchParamPattern::Identifier(name)) = &tc.catch_param {
                                env_set(mc, &func_env, name, &v)?;
                            }
                            if let Some(catch_body) = &tc.catch_body {
                                // Catch body has yields — but for .return() we just
                                // need to complete the generator and return the value.
                                // The catch body's yield is not reached since we're
                                // doing a return completion, not throwing.
                                let _ = crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, catch_body, &[]);
                            }
                            if let Some(finally_stmts) = &tc.finally_body {
                                crate::core::evaluate_statements(mc, &func_env, finally_stmts)?;
                            }
                            gen_obj.state = GeneratorState::Completed;
                            return Ok(create_iterator_result(mc, &func_env, return_value, true)?);
                        }
                        Ok(_) => {
                            if let Some(finally_stmts) = &tc.finally_body {
                                crate::core::evaluate_statements(mc, &func_env, finally_stmts)?;
                            }
                            gen_obj.state = GeneratorState::Completed;
                            return Ok(create_iterator_result(mc, &func_env, return_value, true)?);
                        }
                        Err(e) => return Err(e),
                    }
                }
            }

            let result = crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &tail, &[]);
            gen_obj.state = GeneratorState::Completed;

            match result {
                Ok((cf, _last)) => match cf {
                    crate::core::ControlFlow::Return(v) => Ok(create_iterator_result(mc, &func_env, &v, true)?),
                    crate::core::ControlFlow::Normal(_) => Ok(create_iterator_result(mc, &func_env, &Value::Undefined, true)?),
                    crate::core::ControlFlow::Throw(v, l, c) => Err(crate::core::EvalError::Throw(v, l, c)),
                    _ => Err(raise_eval_error!("Unexpected control flow after generator return handling").into()),
                },
                Err(e) => Err(e),
            }
        }
    }
}

fn generator_return<'gc>(
    mc: &MutationContext<'gc>,
    generator: &crate::core::GcPtr<'gc, crate::core::JSGenerator<'gc>>,
    return_value: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut gen_obj = match generator.try_borrow_mut(mc) {
        Ok(g) => g,
        Err(_) => return Err(raise_type_error!("Generator is already running").into()),
    };

    if matches!(gen_obj.state, GeneratorState::Running { .. }) {
        return Err(raise_type_error!("Generator is already running").into());
    }

    if let Some(iter_obj) = gen_obj.yield_star_iterator {
        let ret_method = match crate::core::get_property_with_accessors(mc, &gen_obj.env, &iter_obj, "return") {
            Ok(v) => v,
            Err(e) => {
                gen_obj.yield_star_iterator = None;
                let throw_val = eval_error_to_value(mc, &gen_obj.env, e);
                drop(gen_obj);
                return generator_throw(mc, generator, &throw_val);
            }
        };

        if matches!(ret_method, Value::Undefined | Value::Null) {
            gen_obj.yield_star_iterator = None;
            drop(gen_obj);
            return resume_with_return_completion(mc, generator, return_value);
        }

        let ret_res = match crate::core::evaluate_call_dispatch(
            mc,
            &gen_obj.env,
            &ret_method,
            Some(&Value::Object(iter_obj)),
            std::slice::from_ref(return_value),
        ) {
            Ok(v) => v,
            Err(e) => {
                gen_obj.yield_star_iterator = None;
                let throw_val = eval_error_to_value(mc, &gen_obj.env, e);
                drop(gen_obj);
                return generator_throw(mc, generator, &throw_val);
            }
        };

        let ret_obj = match ret_res {
            Value::Object(o) => o,
            _ => {
                gen_obj.yield_star_iterator = None;
                let throw_val = eval_error_to_value(
                    mc,
                    &gen_obj.env,
                    raise_type_error!("Iterator return did not return an object").into(),
                );
                drop(gen_obj);
                return generator_throw(mc, generator, &throw_val);
            }
        };
        let done_val = match crate::core::get_property_with_accessors(mc, &gen_obj.env, &ret_obj, "done") {
            Ok(v) => v,
            Err(e) => {
                gen_obj.yield_star_iterator = None;
                let throw_val = eval_error_to_value(mc, &gen_obj.env, e);
                drop(gen_obj);
                return generator_throw(mc, generator, &throw_val);
            }
        };
        let done = done_val.to_truthy();
        if done {
            let value = match crate::core::get_property_with_accessors(mc, &gen_obj.env, &ret_obj, "value") {
                Ok(v) => v,
                Err(e) => {
                    gen_obj.yield_star_iterator = None;
                    let throw_val = eval_error_to_value(mc, &gen_obj.env, e);
                    drop(gen_obj);
                    return generator_throw(mc, generator, &throw_val);
                }
            };
            gen_obj.yield_star_iterator = None;
            drop(gen_obj);
            return resume_with_return_completion(mc, generator, &value);
        }
        let value = if let Some(v) = object_get_key_value(&ret_obj, "value") {
            v.borrow().clone()
        } else {
            Value::Undefined
        };
        return Ok(create_iterator_result_with_done(mc, &gen_obj.env, &value, &done_val)?);
    }

    if let Some(iter_obj) = gen_obj.pending_iterator {
        if !gen_obj.pending_iterator_done {
            close_pending_iterator(mc, &gen_obj.env, &iter_obj)?;
        }
        gen_obj.pending_iterator = None;
        gen_obj.pending_iterator_done = false;
    }

    match gen_obj.state {
        GeneratorState::NotStarted | GeneratorState::Completed => {
            gen_obj.state = GeneratorState::Completed;
            Ok(create_iterator_result(mc, &gen_obj.env, return_value, true)?)
        }
        GeneratorState::Running { .. } => Err(raise_eval_error!("Generator is already running").into()),
        GeneratorState::Suspended { .. } => {
            drop(gen_obj);
            resume_with_return_completion(mc, generator, return_value)
        }
    }
}

/// Execute generator.throw()
pub fn generator_throw<'gc>(
    mc: &MutationContext<'gc>,
    generator: &crate::core::GcPtr<'gc, crate::core::JSGenerator<'gc>>,
    throw_value: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut gen_obj = match generator.try_borrow_mut(mc) {
        Ok(g) => g,
        Err(_) => return Err(raise_type_error!("Generator is already running").into()),
    };

    if matches!(gen_obj.state, GeneratorState::Running { .. }) {
        return Err(raise_type_error!("Generator is already running").into());
    }

    if let Some(iter_obj) = gen_obj.yield_star_iterator {
        let throw_method = match crate::core::get_property_with_accessors(mc, &gen_obj.env, &iter_obj, "throw") {
            Ok(v) => v,
            Err(e) => {
                gen_obj.yield_star_iterator = None;
                let throw_val = eval_error_to_value(mc, &gen_obj.env, e);
                drop(gen_obj);
                return generator_throw(mc, generator, &throw_val);
            }
        };
        if matches!(throw_method, Value::Undefined | Value::Null) {
            let return_method = match crate::core::get_property_with_accessors(mc, &gen_obj.env, &iter_obj, "return") {
                Ok(v) => v,
                Err(e) => {
                    gen_obj.yield_star_iterator = None;
                    let throw_val = eval_error_to_value(mc, &gen_obj.env, e);
                    drop(gen_obj);
                    return generator_throw(mc, generator, &throw_val);
                }
            };
            if !matches!(return_method, Value::Undefined | Value::Null) {
                let return_res = crate::core::evaluate_call_dispatch(mc, &gen_obj.env, &return_method, Some(&Value::Object(iter_obj)), &[]);
                match return_res {
                    Ok(Value::Object(_)) => {}
                    Ok(_) => {
                        gen_obj.yield_star_iterator = None;
                        let throw_val = eval_error_to_value(
                            mc,
                            &gen_obj.env,
                            raise_type_error!("Iterator return did not return an object").into(),
                        );
                        drop(gen_obj);
                        return generator_throw(mc, generator, &throw_val);
                    }
                    Err(e) => {
                        gen_obj.yield_star_iterator = None;
                        let throw_val = eval_error_to_value(mc, &gen_obj.env, e);
                        drop(gen_obj);
                        return generator_throw(mc, generator, &throw_val);
                    }
                }
            }
            gen_obj.yield_star_iterator = None;
            let throw_val = eval_error_to_value(
                mc,
                &gen_obj.env,
                raise_type_error!("Iterator does not provide a throw method").into(),
            );
            drop(gen_obj);
            return generator_throw(mc, generator, &throw_val);
        }

        let throw_res = crate::core::evaluate_call_dispatch(
            mc,
            &gen_obj.env,
            &throw_method,
            Some(&Value::Object(iter_obj)),
            std::slice::from_ref(throw_value),
        );
        let throw_res = match throw_res {
            Ok(v) => v,
            Err(e) => {
                gen_obj.yield_star_iterator = None;
                let throw_val = eval_error_to_value(mc, &gen_obj.env, e);
                drop(gen_obj);
                return generator_throw(mc, generator, &throw_val);
            }
        };
        let throw_obj = match throw_res {
            Value::Object(o) => o,
            _ => {
                gen_obj.yield_star_iterator = None;
                let throw_val = eval_error_to_value(
                    mc,
                    &gen_obj.env,
                    raise_type_error!("Iterator throw did not return an object").into(),
                );
                drop(gen_obj);
                return generator_throw(mc, generator, &throw_val);
            }
        };
        let done_val = match crate::core::get_property_with_accessors(mc, &gen_obj.env, &throw_obj, "done") {
            Ok(v) => v,
            Err(e) => {
                gen_obj.yield_star_iterator = None;
                let throw_val = eval_error_to_value(mc, &gen_obj.env, e);
                drop(gen_obj);
                return generator_throw(mc, generator, &throw_val);
            }
        };
        let done = done_val.to_truthy();
        if done {
            let value = match crate::core::get_property_with_accessors(mc, &gen_obj.env, &throw_obj, "value") {
                Ok(v) => v,
                Err(e) => {
                    gen_obj.yield_star_iterator = None;
                    let throw_val = eval_error_to_value(mc, &gen_obj.env, e);
                    drop(gen_obj);
                    return generator_throw(mc, generator, &throw_val);
                }
            };
            gen_obj.yield_star_iterator = None;
            drop(gen_obj);
            return generator_next(mc, generator, &value);
        }
        let value = if let Some(v) = object_get_key_value(&throw_obj, "value") {
            v.borrow().clone()
        } else {
            Value::Undefined
        };
        return Ok(create_iterator_result_with_done(mc, &gen_obj.env, &value, &done_val)?);
    }

    match &mut gen_obj.state {
        GeneratorState::NotStarted => {
            // Throwing into a not-started generator marks it completed and throws synchronously
            gen_obj.state = GeneratorState::Completed;
            Err(EvalError::Throw(throw_value.clone(), None, None))
        }
        GeneratorState::Suspended { pc, .. } => {
            // Replace the suspended statement with a Throw containing the thrown value
            let pc_val = *pc;
            if pc_val >= gen_obj.body.len() {
                gen_obj.state = GeneratorState::Completed;
                return Err(EvalError::Throw(throw_value.clone(), None, None));
            }

            // ---- Pending completion from finally-body yield ----
            // When the generator is paused at a yield inside a finally block,
            // calling .throw(val) fires the throw at the yield point. The
            // remaining finally stmts DO NOT execute (the throw is abrupt).
            // The new throw overrides the pending completion.
            if gen_obj.pending_completion.is_some() {
                gen_obj.pending_completion = None;
                gen_obj.state = GeneratorState::Completed;
                return Err(EvalError::Throw(throw_value.clone(), None, None));
            }

            let mut tail: Vec<Statement> = gen_obj.body[pc_val..].to_vec();

            // If resuming from a pre-executed environment, the enclosing `for`
            // initializer may already have run. Prevent re-running it during
            // throw handling by removing the init expression when present.
            if let GeneratorState::Suspended { pre_env, .. } = &gen_obj.state
                && pre_env.is_some()
                && let Some(first_stmt) = tail.get_mut(0)
                && let StatementKind::For(for_stmt) = first_stmt.kind.as_mut()
            {
                for_stmt.init = None;
            }

            // Record the location of the first yield so we can execute any
            // pre-statements (those before the yield) in the function env
            // prior to executing the modified tail with a thrown value.
            let tail_before = tail.clone();

            // Attempt to replace the first nested statement containing a `yield`
            // with a Throw so that surrounding try/catch blocks can catch it.
            let mut replaced = false;
            for s in tail.iter_mut() {
                if replace_first_yield_statement_with_throw(s, throw_value) {
                    replaced = true;
                    break;
                }
            }
            if !replaced {
                // fallback: replace the top-level statement
                tail[0] = StatementKind::Throw(Expr::Var("__gen_throw_val".to_string(), None, None)).into();
            }

            let func_env = if let GeneratorState::Suspended { pre_env: Some(env), .. } = &gen_obj.state {
                *env
            } else {
                prepare_function_call_env(mc, Some(&gen_obj.env), gen_obj.this_val.clone().as_ref(), None, &[], None, None)?
            };

            // Execute pre-statements in the function env so bindings created
            // before the original yield are present when evaluating the throw.
            if let Some((idx, inner_idx_opt, _yield_kind, _yield_inner)) = find_first_yield_in_statements(&tail_before) {
                if idx > 0 {
                    let pre_stmts = tail_before[0..idx].to_vec();
                    crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                } else if let Some(inner_idx) = inner_idx_opt
                    && inner_idx > 0
                    && let StatementKind::Block(inner_stmts) = &*tail_before[idx].kind
                {
                    let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                    let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                }
            }

            slot_set(mc, &func_env, InternalSlot::GenThrowVal, &throw_value.clone());

            // If the modified tail still contains yields (e.g., in catch/finally
            // bodies after the injected throw), evaluate_statements would fail
            // on those yield expressions. Handle TryCatch with catch-body yields
            // directly: run try body, bind catch param, evaluate catch yield.
            if find_first_yield_in_statements(&tail).is_some() {
                // Check if the first yield is in a catch body of a TryCatch
                if let Some((tc_idx, _, _, _)) = find_first_yield_in_statements(&tail) {
                    let is_catch_yield = if let StatementKind::TryCatch(tc) = &*tail[tc_idx].kind {
                        find_first_yield_in_statements(&tc.try_body).is_none()
                            && tc
                                .catch_body
                                .as_ref()
                                .is_some_and(|cb| find_first_yield_in_statements(cb).is_some())
                    } else {
                        false
                    };

                    if is_catch_yield {
                        let tc = if let StatementKind::TryCatch(tc) = &*tail[tc_idx].kind {
                            tc.clone()
                        } else {
                            unreachable!()
                        };

                        // Run pre-TryCatch statements from the modified tail
                        if tc_idx > 0 {
                            let pre_stmts = tail[0..tc_idx].to_vec();
                            crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                        }

                        // Run try body (which contains the injected throw)
                        let try_result = crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &tc.try_body, &[]);

                        let thrown_value_opt = match try_result {
                            Ok((crate::core::ControlFlow::Throw(v, _, _), _)) => Some(v),
                            Err(EvalError::Throw(v, _, _)) => Some(v),
                            Err(EvalError::Js(js_err)) => Some(crate::core::js_error_to_value(mc, &func_env, &js_err)),
                            Ok(_) => None,
                        };

                        if let Some(thrown) = thrown_value_opt {
                            // Bind catch parameter
                            if let Some(CatchParamPattern::Identifier(name)) = &tc.catch_param {
                                env_set(mc, &func_env, name, &thrown)?;
                            }

                            let catch_body = tc.catch_body.as_ref().unwrap();
                            let (catch_yield_idx, _, _, catch_yield_inner) = find_first_yield_in_statements(catch_body).unwrap();

                            if catch_yield_idx > 0 {
                                let pre_catch = catch_body[0..catch_yield_idx].to_vec();
                                crate::core::evaluate_statements(mc, &func_env, &pre_catch)?;
                            }

                            let yield_val = if let Some(inner) = catch_yield_inner {
                                crate::core::evaluate_expr(mc, &func_env, &inner)?
                            } else {
                                Value::Undefined
                            };

                            // Inject the throw into the ACTUAL body so that on next
                            // resume, the TryCatch's try body will re-throw (the
                            // yield in try was replaced with throw __gen_throw_val).
                            let mut replaced_in_body = false;
                            for s in gen_obj.body[pc_val..].iter_mut() {
                                if replace_first_yield_statement_with_throw(s, throw_value) {
                                    replaced_in_body = true;
                                    break;
                                }
                            }
                            if !replaced_in_body {
                                gen_obj.body[pc_val] = StatementKind::Throw(Expr::Var("__gen_throw_val".to_string(), None, None)).into();
                            }

                            gen_obj.state = GeneratorState::Suspended {
                                pc: pc_val + tc_idx,
                                stack: vec![],
                                pre_env: Some(func_env),
                            };
                            gen_obj.cached_initial_yield = Some(yield_val.clone());
                            return Ok(create_iterator_result(mc, &func_env, &yield_val, false)?);
                        }

                        // Try completed normally (shouldn't happen with injected throw)
                        if let Some(finally_stmts) = &tc.finally_body {
                            crate::core::evaluate_statements(mc, &func_env, finally_stmts)?;
                        }
                        gen_obj.state = GeneratorState::Completed;
                        return Ok(create_iterator_result(mc, &func_env, &Value::Undefined, true)?);
                    }

                    // ---- Finally-body yield special case ----
                    // Scan for any TryCatch in the tail whose finally body
                    // has yields. This handles nested structures where the
                    // try body may have unreachable yields (the injected throw
                    // fires before they are reached).
                    let finally_tc_idx = tail.iter().position(|s| {
                        if let StatementKind::TryCatch(tc) = &*s.kind {
                            tc.finally_body
                                .as_ref()
                                .is_some_and(|fb| find_first_yield_in_statements(fb).is_some())
                                && (tc.catch_body.is_none()
                                    || tc
                                        .catch_body
                                        .as_ref()
                                        .is_some_and(|cb| find_first_yield_in_statements(cb).is_none()))
                        } else {
                            false
                        }
                    });

                    if let Some(ftc_idx) = finally_tc_idx {
                        // Check if the throw was injected BEFORE the TryCatch.
                        // If so, the TryCatch is never reached — skip this handler.
                        let throw_before_tc = if let Some((orig_idx, _, _, _)) = find_first_yield_in_statements(&tail_before) {
                            orig_idx < ftc_idx
                        } else {
                            false
                        };

                        if !throw_before_tc {
                            let tc = if let StatementKind::TryCatch(tc) = &*tail[ftc_idx].kind {
                                tc.clone()
                            } else {
                                unreachable!()
                            };

                            // Check if there's a nested TryCatch in the try body
                            // where the inner catch has yields (the throw is caught
                            // by the inner catch, so we should yield from inner catch
                            // rather than proceeding to the finally body).
                            let nested_catch_yield_info: Option<(usize, Box<crate::core::TryCatchStatement>)> =
                                tc.try_body.iter().enumerate().find_map(|(i, s)| {
                                    if let StatementKind::TryCatch(inner_tc) = &*s.kind
                                        && find_first_yield_in_statements(&inner_tc.try_body).is_none()
                                        && inner_tc
                                            .catch_body
                                            .as_ref()
                                            .is_some_and(|cb| find_first_yield_in_statements(cb).is_some())
                                    {
                                        return Some((i, inner_tc.clone()));
                                    }
                                    None
                                });

                            if let Some((inner_tc_idx, inner_tc)) = nested_catch_yield_info {
                                // ---- Nested catch-body yield ----
                                // The throw is caught by inner catch, yield from inner catch.
                                if ftc_idx > 0 {
                                    let pre_stmts = tail[0..ftc_idx].to_vec();
                                    crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                }

                                // Run outer try body stmts before the inner TryCatch
                                if inner_tc_idx > 0 {
                                    let pre_inner = tc.try_body[0..inner_tc_idx].to_vec();
                                    crate::core::evaluate_statements(mc, &func_env, &pre_inner)?;
                                }

                                // Run inner TryCatch try body (which throws)
                                let try_result =
                                    crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &inner_tc.try_body, &[]);
                                let thrown = match try_result {
                                    Ok((crate::core::ControlFlow::Throw(v, _, _), _)) | Err(EvalError::Throw(v, _, _)) => Some(v),
                                    Err(EvalError::Js(js_err)) => Some(crate::core::js_error_to_value(mc, &func_env, &js_err)),
                                    Ok(_) => None,
                                };

                                if let Some(thrown_val) = thrown {
                                    if let Some(CatchParamPattern::Identifier(name)) = &inner_tc.catch_param {
                                        env_set(mc, &func_env, name, &thrown_val)?;
                                    }
                                    let catch_body = inner_tc.catch_body.as_ref().unwrap();
                                    let (catch_yield_idx, _, _, catch_yield_inner) = find_first_yield_in_statements(catch_body).unwrap();
                                    if catch_yield_idx > 0 {
                                        let pre_catch = catch_body[0..catch_yield_idx].to_vec();
                                        crate::core::evaluate_statements(mc, &func_env, &pre_catch)?;
                                    }
                                    let yield_val = if let Some(inner) = catch_yield_inner {
                                        crate::core::evaluate_expr(mc, &func_env, &inner)?
                                    } else {
                                        Value::Undefined
                                    };

                                    // Inject throw into actual body for next resume
                                    let mut replaced_in_body = false;
                                    for s in gen_obj.body[pc_val..].iter_mut() {
                                        if replace_first_yield_statement_with_throw(s, throw_value) {
                                            replaced_in_body = true;
                                            break;
                                        }
                                    }
                                    if !replaced_in_body {
                                        gen_obj.body[pc_val] =
                                            StatementKind::Throw(Expr::Var("__gen_throw_val".to_string(), None, None)).into();
                                    }

                                    gen_obj.state = GeneratorState::Suspended {
                                        pc: pc_val + ftc_idx,
                                        stack: vec![],
                                        pre_env: Some(func_env),
                                    };
                                    gen_obj.cached_initial_yield = Some(yield_val.clone());
                                    return Ok(create_iterator_result(mc, &func_env, &yield_val, false)?);
                                }
                                // Inner try didn't throw — shouldn't happen, fall through
                            } else {
                                // ---- Finally-body yield (throw propagates to finally) ----

                                // Run pre-TryCatch statements from the modified tail
                                if ftc_idx > 0 {
                                    let pre_stmts = tail[0..ftc_idx].to_vec();
                                    crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                }

                                // Run try body (which contains the injected throw)
                                let try_result =
                                    crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &tc.try_body, &[]);

                                // Determine the abrupt completion from try/catch
                                let abrupt_completion: Option<GeneratorPendingCompletion> = match try_result {
                                    Ok((crate::core::ControlFlow::Throw(v, _, _), _)) | Err(EvalError::Throw(v, _, _)) => {
                                        if let Some(catch_body) = &tc.catch_body {
                                            // Bind catch param and run catch body
                                            if let Some(CatchParamPattern::Identifier(name)) = &tc.catch_param {
                                                env_set(mc, &func_env, name, &v)?;
                                            }
                                            match crate::core::evaluate_statements_with_context_and_last_value(
                                                mc,
                                                &func_env,
                                                catch_body,
                                                &[],
                                            ) {
                                                Ok((crate::core::ControlFlow::Normal(_), _)) => None,
                                                Ok((crate::core::ControlFlow::Return(rv), _)) => {
                                                    Some(GeneratorPendingCompletion::Return(rv))
                                                }
                                                Ok((crate::core::ControlFlow::Throw(tv, _, _), _)) => {
                                                    Some(GeneratorPendingCompletion::Throw(tv))
                                                }
                                                Err(EvalError::Throw(tv, _, _)) => Some(GeneratorPendingCompletion::Throw(tv)),
                                                Err(EvalError::Js(js_err)) => Some(GeneratorPendingCompletion::Throw(
                                                    crate::core::js_error_to_value(mc, &func_env, &js_err),
                                                )),
                                                _ => None,
                                            }
                                        } else {
                                            // No catch: the throw is parked for re-fire after finally
                                            Some(GeneratorPendingCompletion::Throw(v))
                                        }
                                    }
                                    Err(EvalError::Js(js_err)) => {
                                        let v = crate::core::js_error_to_value(mc, &func_env, &js_err);
                                        if let Some(catch_body) = &tc.catch_body {
                                            if let Some(CatchParamPattern::Identifier(name)) = &tc.catch_param {
                                                env_set(mc, &func_env, name, &v)?;
                                            }
                                            match crate::core::evaluate_statements_with_context_and_last_value(
                                                mc,
                                                &func_env,
                                                catch_body,
                                                &[],
                                            ) {
                                                Ok((crate::core::ControlFlow::Normal(_), _)) => None,
                                                Ok((crate::core::ControlFlow::Return(rv), _)) => {
                                                    Some(GeneratorPendingCompletion::Return(rv))
                                                }
                                                Ok((crate::core::ControlFlow::Throw(tv, _, _), _)) => {
                                                    Some(GeneratorPendingCompletion::Throw(tv))
                                                }
                                                Err(EvalError::Throw(tv, _, _)) => Some(GeneratorPendingCompletion::Throw(tv)),
                                                _ => None,
                                            }
                                        } else {
                                            Some(GeneratorPendingCompletion::Throw(v))
                                        }
                                    }
                                    Ok((crate::core::ControlFlow::Return(v), _)) => Some(GeneratorPendingCompletion::Return(v)),
                                    Ok(_) => None, // try completed normally
                                };

                                // Enter finally body: find yield, run pre-yield stmts, evaluate yield
                                let finally_body = tc.finally_body.as_ref().unwrap();
                                let (fin_yield_idx, _, _, fin_yield_inner) = find_first_yield_in_statements(finally_body).unwrap();

                                if fin_yield_idx > 0 {
                                    let pre_fin = finally_body[0..fin_yield_idx].to_vec();
                                    crate::core::evaluate_statements(mc, &func_env, &pre_fin)?;
                                }

                                let yield_val = if let Some(inner) = fin_yield_inner {
                                    crate::core::evaluate_expr(mc, &func_env, &inner)?
                                } else {
                                    Value::Undefined
                                };

                                // Store pending completion for re-fire after finally
                                gen_obj.pending_completion = abrupt_completion;

                                // Replace the TryCatch in body with remaining finally stmts
                                let remaining_finally = if fin_yield_idx + 1 < finally_body.len() {
                                    finally_body[fin_yield_idx + 1..].to_vec()
                                } else {
                                    vec![]
                                };
                                gen_obj.body[pc_val + ftc_idx] = StatementKind::Block(remaining_finally).into();

                                gen_obj.state = GeneratorState::Suspended {
                                    pc: pc_val + ftc_idx,
                                    stack: vec![],
                                    pre_env: Some(func_env),
                                };
                                gen_obj.cached_initial_yield = Some(yield_val.clone());
                                return Ok(create_iterator_result(mc, &func_env, &yield_val, false)?);
                            }
                        } // close if !throw_before_tc
                    } // close if let Some(ftc_idx)
                } // close if let Some((tc_idx, ...))
            } // close if find_first_yield

            // Execute the modified tail. If the throw is uncaught, evaluate_statements
            // will return Err and we should propagate that to the caller.
            let func_home = func_env.borrow().get_home_object().map(|h| Gc::as_ptr(*h.borrow()));
            log::trace!(
                "generator_throw: func_env={:p} env.home={:?} gen_ptr={:p}",
                Gc::as_ptr(func_env),
                func_home,
                Gc::as_ptr(*generator)
            );
            let result = crate::core::evaluate_statements_with_context_and_last_value(mc, &func_env, &tail, &[]);
            // Don't blindly set Completed; check if it returned from a yield or completion
            // NOTE: Current implementation of evaluate_statements does not support Yield.
            // If the generator contains subsequent yields, evaluate_statements may fail.
            // For now, we assume simple throw-catch-return or throw-catch-end behavior.

            gen_obj.state = GeneratorState::Completed;

            match result {
                Ok((cf, _last)) => match cf {
                    crate::core::ControlFlow::Return(v) => Ok(create_iterator_result(mc, &func_env, &v, true)?),
                    crate::core::ControlFlow::Normal(_) => Ok(create_iterator_result(mc, &func_env, &Value::Undefined, true)?),
                    crate::core::ControlFlow::Throw(v, l, c) => Err(crate::core::EvalError::Throw(v, l, c)),
                    _ => Err(raise_eval_error!("Unexpected control flow after generator throw handling").into()),
                },
                Err(e) => Err(e),
            }
        }
        GeneratorState::Running { .. } => Err(raise_eval_error!("Generator is already running").into()),
        GeneratorState::Completed => Err(EvalError::Throw(throw_value.clone(), None, None)),
    }
}

fn get_global_env<'gc>(_mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> JSObjectDataPtr<'gc> {
    let mut global_env = *env;
    loop {
        if global_env
            .borrow()
            .properties
            .contains_key(&crate::core::PropertyKey::String("globalThis".to_string()))
        {
            break;
        }
        let proto = global_env.borrow().prototype;
        if let Some(p) = proto {
            global_env = p;
        } else {
            break;
        }
    }
    global_env
}

/// Create an iterator result object {value: value, done: done}
fn create_iterator_result<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
    done: bool,
) -> Result<Value<'gc>, JSError> {
    // Iterator result objects should be extensible by default
    let obj = crate::core::new_js_object_data(mc);

    // Ensure iterator result inherits from Object.prototype
    let global_env = get_global_env(mc, env);
    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, &global_env, "Object");

    // Debug: report iterator result being created
    log::trace!("create_iterator_result: value={:?} done={}", value, done);

    // Set value property
    object_set_key_value(mc, &obj, "value", value)?;

    // Set done property
    object_set_key_value(mc, &obj, "done", &Value::Boolean(done))?;

    Ok(Value::Object(obj))
}

fn create_iterator_result_with_done<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    value: &Value<'gc>,
    done_value: &Value<'gc>,
) -> Result<Value<'gc>, JSError> {
    let obj = crate::core::new_js_object_data(mc);

    let global_env = get_global_env(mc, env);
    let _ = crate::core::set_internal_prototype_from_constructor(mc, &obj, &global_env, "Object");

    object_set_key_value(mc, &obj, "value", value)?;
    object_set_key_value(mc, &obj, "done", done_value)?;
    Ok(Value::Object(obj))
}

/// Initialize Generator constructor/prototype and attach prototype methods
pub fn initialize_generator<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    // Create constructor object and generator prototype
    let gen_ctor = crate::core::new_js_object_data(mc);
    // Set __proto__ to Function.prototype if present
    if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        gen_ctor.borrow_mut(mc).prototype = Some(*proto_obj);
    }

    let gen_proto = crate::core::new_js_object_data(mc);
    log::debug!("js_generator::init: gen_proto ptr = {:p}", Gc::as_ptr(gen_proto));
    // Per spec, Generator.prototype.[[Prototype]] = %IteratorPrototype%.
    // Fall back to Object.prototype if %IteratorPrototype% is not available.
    if let Some(ip_val) = crate::core::slot_get_chained(env, &InternalSlot::IteratorPrototype)
        && let Value::Object(ip) = &*ip_val.borrow()
    {
        gen_proto.borrow_mut(mc).prototype = Some(*ip);
    } else {
        let _ = crate::core::set_internal_prototype_from_constructor(mc, &gen_proto, env, "Object");
    }
    // DEBUG: report gen_proto and later when GeneratorFunction.prototype is linked
    log::debug!("init_generator: gen_proto created at {:p}", Gc::as_ptr(gen_proto));

    // Attach prototype methods as built-in function objects with proper name/length.
    let create_builtin_method_obj = |native_name: &str, display_name: &str, length: f64| -> Result<JSObjectDataPtr<'gc>, JSError> {
        let method_obj = crate::core::new_js_object_data(mc);
        if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
            && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*proto_val.borrow()
        {
            method_obj.borrow_mut(mc).prototype = Some(*func_proto);
        }
        method_obj
            .borrow_mut(mc)
            .set_closure(Some(crate::core::new_gc_cell_ptr(mc, Value::Function(native_name.to_string()))));

        let name_desc =
            crate::core::create_descriptor_object(mc, &Value::String(crate::unicode::utf8_to_utf16(display_name)), false, false, true)?;
        crate::js_object::define_property_internal(mc, &method_obj, "name", &name_desc)?;

        let len_desc = crate::core::create_descriptor_object(mc, &Value::Number(length), false, false, true)?;
        crate::js_object::define_property_internal(mc, &method_obj, "length", &len_desc)?;
        Ok(method_obj)
    };

    let next_obj = create_builtin_method_obj("Generator.prototype.next", "next", 1.0)?;
    crate::core::object_set_key_value(mc, &gen_proto, "next", &Value::Object(next_obj))?;
    gen_proto
        .borrow_mut(mc)
        .set_non_enumerable(crate::core::PropertyKey::String("next".to_string()));

    let return_obj = create_builtin_method_obj("Generator.prototype.return", "return", 1.0)?;
    crate::core::object_set_key_value(mc, &gen_proto, "return", &Value::Object(return_obj))?;
    gen_proto
        .borrow_mut(mc)
        .set_non_enumerable(crate::core::PropertyKey::String("return".to_string()));

    let throw_obj = create_builtin_method_obj("Generator.prototype.throw", "throw", 1.0)?;
    crate::core::object_set_key_value(mc, &gen_proto, "throw", &Value::Object(throw_obj))?;
    gen_proto
        .borrow_mut(mc)
        .set_non_enumerable(crate::core::PropertyKey::String("throw".to_string()));

    // Register Symbol.iterator on Generator.prototype -> returns the generator object itself
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
    {
        // Create a function name recognized by the call dispatcher
        let iter_obj = create_builtin_method_obj("Generator.prototype.iterator", "[Symbol.iterator]", 0.0)?;
        log::debug!("js_generator::init: registering Symbol.iterator ptr = {:p}", Gc::as_ptr(*iter_sym));
        crate::core::object_set_key_value(mc, &gen_proto, *iter_sym, &Value::Object(iter_obj))?;
        gen_proto
            .borrow_mut(mc)
            .set_non_enumerable(crate::core::PropertyKey::Symbol(*iter_sym));
    }

    // Set Generator.prototype[@@toStringTag] = "Generator"
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let desc_tag =
            crate::core::create_descriptor_object(mc, &Value::String(crate::unicode::utf8_to_utf16("Generator")), false, false, true)?;
        crate::js_object::define_property_internal(mc, &gen_proto, *tag_sym, &desc_tag)?;
    }

    // link prototype to constructor and expose on global env
    let desc = crate::core::create_descriptor_object(mc, &Value::Object(gen_proto), true, false, false)?;
    crate::js_object::define_property_internal(mc, &gen_ctor, "prototype", &desc)?;
    crate::core::env_set(mc, env, "Generator", &Value::Object(gen_ctor))?;

    // Create GeneratorFunction constructor and prototype so that generator
    // function objects inherit from a distinct `GeneratorFunction.prototype`.
    // This prototype object should expose a `prototype` property pointing to
    // the `Generator.prototype` intrinsic (used as generator instances' [[Prototype]]).
    let gen_func_ctor = crate::core::new_js_object_data(mc);
    slot_set(
        mc,
        &gen_func_ctor,
        InternalSlot::NativeCtor,
        &Value::String(crate::unicode::utf8_to_utf16("GeneratorFunction")),
    );
    // Set internal prototype of constructor to Function when available
    if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
    {
        gen_func_ctor.borrow_mut(mc).prototype = Some(*func_ctor);
    }

    // Mark constructor as constructable
    slot_set(mc, &gen_func_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));

    // GeneratorFunction.length = 1 (non-writable, non-enumerable, configurable)
    let desc_len = crate::core::create_descriptor_object(mc, &Value::Number(1.0), false, false, true)?;
    crate::js_object::define_property_internal(mc, &gen_func_ctor, "length", &desc_len)?;

    // GeneratorFunction.name = "GeneratorFunction" (non-writable, non-enumerable, configurable)
    let desc_name = crate::core::create_descriptor_object(
        mc,
        &Value::String(crate::unicode::utf8_to_utf16("GeneratorFunction")),
        false,
        false,
        true,
    )?;
    crate::js_object::define_property_internal(mc, &gen_func_ctor, "name", &desc_name)?;

    let gen_func_proto = crate::core::new_js_object_data(mc);
    // GeneratorFunction.prototype should inherit from Function.prototype
    if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        gen_func_proto.borrow_mut(mc).prototype = Some(*func_proto);
    }

    // GeneratorFunction.prototype.prototype -> %Generator.prototype%
    // writable=false, enumerable=false, configurable=true
    let desc_proto = crate::core::create_descriptor_object(mc, &Value::Object(gen_proto), false, false, true)?;
    crate::js_object::define_property_internal(mc, &gen_func_proto, "prototype", &desc_proto)?;

    // GeneratorFunction.prototype.constructor -> GeneratorFunction
    // writable=false, enumerable=false, configurable=true
    let desc_proto_ctor = crate::core::create_descriptor_object(mc, &Value::Object(gen_func_ctor), false, false, true)?;
    crate::js_object::define_property_internal(mc, &gen_func_proto, "constructor", &desc_proto_ctor)?;

    // %GeneratorPrototype%.constructor -> %GeneratorFunction.prototype%
    // writable=false, enumerable=false, configurable=true
    let gen_proto_ctor_desc = crate::core::create_descriptor_object(mc, &Value::Object(gen_func_proto), false, false, true)?;
    crate::js_object::define_property_internal(mc, &gen_proto, "constructor", &gen_proto_ctor_desc)?;
    // DEBUG: report whether `gen_func_proto` now has a 'prototype' property and where it points
    if let Some(proto_rc) = crate::core::object_get_key_value(&gen_func_proto, "prototype") {
        let proto_val = proto_rc.borrow().clone();
        match proto_val {
            Value::Object(o) => {
                log::trace!(
                    "init_generator: gen_func_proto.ptr={:p} .prototype -> {:p}",
                    Gc::as_ptr(gen_func_proto),
                    Gc::as_ptr(o)
                );
            }
            Value::Property { value: Some(v), .. } => {
                let inner = v.borrow().clone();
                if let Value::Object(o2) = inner {
                    log::trace!(
                        "init_generator: gen_func_proto.ptr={:p} .prototype (descriptor) -> {:p}",
                        Gc::as_ptr(gen_func_proto),
                        Gc::as_ptr(o2)
                    );
                } else {
                    log::trace!(
                        "init_generator: gen_func_proto.ptr={:p} .prototype (descriptor) value not object",
                        Gc::as_ptr(gen_func_proto)
                    );
                }
            }
            _ => {
                log::trace!(
                    "init_generator: gen_func_proto.ptr={:p} .prototype present but not object",
                    Gc::as_ptr(gen_func_proto)
                );
            }
        }
    } else {
        log::trace!(
            "init_generator: gen_func_proto.ptr={:p} .prototype MISSING",
            Gc::as_ptr(gen_func_proto)
        );
    }
    // Set GeneratorFunction.prototype[@@toStringTag] = "GeneratorFunction"
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let desc_tag = crate::core::create_descriptor_object(
            mc,
            &Value::String(crate::unicode::utf8_to_utf16("GeneratorFunction")),
            false,
            false,
            true,
        )?;
        crate::js_object::define_property_internal(mc, &gen_func_proto, *tag_sym, &desc_tag)?;
    }

    // GeneratorFunction.prototype property descriptor:
    // writable=false, enumerable=false, configurable=false
    let desc_ctor_proto = crate::core::create_descriptor_object(mc, &Value::Object(gen_func_proto), false, false, false)?;
    crate::js_object::define_property_internal(mc, &gen_func_ctor, "prototype", &desc_ctor_proto)?;
    // Expose GeneratorFunction in the global environment
    crate::core::env_set(mc, env, "GeneratorFunction", &Value::Object(gen_func_ctor))?;

    Ok(())
}

pub(crate) fn count_yield_vars_in_statement(stmt: &Statement, prefix: &str) -> usize {
    match &*stmt.kind {
        StatementKind::Expr(e) => count_yield_vars_in_expr(e, prefix),
        StatementKind::Let(decls) | StatementKind::Var(decls) => decls
            .iter()
            .map(|(_, expr_opt)| expr_opt.as_ref().map_or(0, |e| count_yield_vars_in_expr(e, prefix)))
            .sum(),
        StatementKind::Const(decls) => decls.iter().map(|(_, e)| count_yield_vars_in_expr(e, prefix)).sum(),
        StatementKind::Return(Some(expr)) => count_yield_vars_in_expr(expr, prefix),
        StatementKind::If(if_stmt) => {
            count_yield_vars_in_expr(&if_stmt.condition, prefix)
                + if_stmt
                    .then_body
                    .iter()
                    .map(|s| count_yield_vars_in_statement(s, prefix))
                    .sum::<usize>()
                + if_stmt
                    .else_body
                    .as_ref()
                    .map_or(0, |b| b.iter().map(|s| count_yield_vars_in_statement(s, prefix)).sum())
        }
        StatementKind::For(for_stmt) => {
            for_stmt.init.as_ref().map_or(0, |s| count_yield_vars_in_statement(s, prefix))
                + for_stmt.test.as_ref().map_or(0, |e| count_yield_vars_in_expr(e, prefix))
                + for_stmt.update.as_ref().map_or(0, |s| count_yield_vars_in_statement(s, prefix))
                + for_stmt
                    .body
                    .iter()
                    .map(|s| count_yield_vars_in_statement(s, prefix))
                    .sum::<usize>()
        }
        StatementKind::While(cond, body) => {
            count_yield_vars_in_expr(cond, prefix) + body.iter().map(|s| count_yield_vars_in_statement(s, prefix)).sum::<usize>()
        }
        StatementKind::DoWhile(body, cond) => {
            count_yield_vars_in_expr(cond, prefix) + body.iter().map(|s| count_yield_vars_in_statement(s, prefix)).sum::<usize>()
        }
        StatementKind::Block(stmts) => stmts.iter().map(|s| count_yield_vars_in_statement(s, prefix)).sum(),
        StatementKind::TryCatch(tc) => {
            tc.try_body.iter().map(|s| count_yield_vars_in_statement(s, prefix)).sum::<usize>()
                + tc.catch_body
                    .as_ref()
                    .map_or(0, |b| b.iter().map(|s| count_yield_vars_in_statement(s, prefix)).sum())
                + tc.finally_body
                    .as_ref()
                    .map_or(0, |b| b.iter().map(|s| count_yield_vars_in_statement(s, prefix)).sum())
        }
        StatementKind::ForOf(_, _, expr, body)
        | StatementKind::ForIn(_, _, expr, body)
        | StatementKind::ForOfDestructuringObject(_, _, expr, body)
        | StatementKind::ForOfDestructuringArray(_, _, expr, body) => {
            count_yield_vars_in_expr(expr, prefix) + body.iter().map(|s| count_yield_vars_in_statement(s, prefix)).sum::<usize>()
        }
        StatementKind::Class(class_def) => count_yield_vars_in_class_def(class_def, prefix),
        _ => 0,
    }
}

fn count_yield_vars_in_expr(expr: &Expr, prefix: &str) -> usize {
    let mut count = 0;
    if let Expr::Var(name, _, _) = expr
        && name.starts_with(prefix)
    {
        count = 1;
    }
    count
        + match expr {
            Expr::Binary(a, _, b)
            | Expr::Assign(a, b)
            | Expr::Index(a, b)
            | Expr::LogicalAnd(a, b)
            | Expr::LogicalOr(a, b)
            | Expr::Comma(a, b)
            | Expr::OptionalIndex(a, b) => count_yield_vars_in_expr(a, prefix) + count_yield_vars_in_expr(b, prefix),
            Expr::UnaryNeg(a)
            | Expr::UnaryPlus(a)
            | Expr::LogicalNot(a)
            | Expr::TypeOf(a)
            | Expr::Void(a)
            | Expr::Delete(a)
            | Expr::BitNot(a)
            | Expr::Increment(a)
            | Expr::Decrement(a)
            | Expr::PostIncrement(a)
            | Expr::PostDecrement(a)
            | Expr::Spread(a)
            | Expr::Await(a)
            | Expr::YieldStar(a) => count_yield_vars_in_expr(a, prefix),
            Expr::Yield(opt) => opt.as_ref().map_or(0, |a| count_yield_vars_in_expr(a, prefix)),
            Expr::Call(a, args) | Expr::New(a, args) | Expr::OptionalCall(a, args) => {
                count_yield_vars_in_expr(a, prefix) + args.iter().map(|x| count_yield_vars_in_expr(x, prefix)).sum::<usize>()
            }
            Expr::Object(pairs) => pairs
                .iter()
                .map(|(k, v, _, _)| count_yield_vars_in_expr(k, prefix) + count_yield_vars_in_expr(v, prefix))
                .sum(),
            Expr::Array(items) => items
                .iter()
                .map(|i| i.as_ref().map_or(0, |x| count_yield_vars_in_expr(x, prefix)))
                .sum(),
            Expr::Conditional(a, b, c) => {
                count_yield_vars_in_expr(a, prefix) + count_yield_vars_in_expr(b, prefix) + count_yield_vars_in_expr(c, prefix)
            }
            Expr::Property(a, _) => count_yield_vars_in_expr(a, prefix),
            Expr::Class(class_def) => count_yield_vars_in_class_def(class_def, prefix),
            _ => 0,
        }
}

fn count_yield_vars_in_class_def(class_def: &ClassDefinition, prefix: &str) -> usize {
    let mut count = 0;
    if let Some(extends_expr) = &class_def.extends {
        count += count_yield_vars_in_expr(extends_expr, prefix);
    }

    for member in &class_def.members {
        count += count_yield_vars_in_class_member(member, prefix);
    }

    count
}

fn count_yield_vars_in_class_member(member: &ClassMember, prefix: &str) -> usize {
    match member {
        ClassMember::MethodComputed(key_expr, ..)
        | ClassMember::MethodComputedGenerator(key_expr, ..)
        | ClassMember::MethodComputedAsync(key_expr, ..)
        | ClassMember::MethodComputedAsyncGenerator(key_expr, ..)
        | ClassMember::StaticMethodComputed(key_expr, ..)
        | ClassMember::StaticMethodComputedGenerator(key_expr, ..)
        | ClassMember::StaticMethodComputedAsync(key_expr, ..)
        | ClassMember::StaticMethodComputedAsyncGenerator(key_expr, ..)
        | ClassMember::GetterComputed(key_expr, ..)
        | ClassMember::SetterComputed(key_expr, ..)
        | ClassMember::StaticGetterComputed(key_expr, ..)
        | ClassMember::StaticSetterComputed(key_expr, ..)
        | ClassMember::PropertyComputed(key_expr, ..)
        | ClassMember::StaticPropertyComputed(key_expr, ..) => count_yield_vars_in_expr(key_expr, prefix),
        ClassMember::StaticProperty(_, value_expr) | ClassMember::PrivateStaticProperty(_, value_expr) => {
            count_yield_vars_in_expr(value_expr, prefix)
        }
        ClassMember::StaticBlock(body) => body.iter().map(|s| count_yield_vars_in_statement(s, prefix)).sum(),
        _ => 0,
    }
}
