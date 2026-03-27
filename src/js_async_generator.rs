use crate::core::{
    AsyncGeneratorRequest, ClosureData, EvalError, Expr, InternalSlot, JSAsyncGenerator, JSObjectDataPtr, JSPromise, Statement,
    StatementKind, Value, VarDeclKind, env_set, env_set_recursive, evaluate_call_dispatch, evaluate_expr, get_own_property,
    new_js_object_data, object_get_key_value, object_set_key_value, prepare_function_call_env, prepare_function_call_env_with_home,
    slot_get, slot_get_chained, slot_set,
};
use crate::core::{Gc, GcContext, GcPtr, new_gc_cell_ptr};
use crate::error::{JSError, JSErrorKind};
use crate::js_generator::YieldKind;
use crate::js_promise::{make_promise_js_object, perform_promise_then, reject_promise, resolve_promise};

fn js_error_to_value<'gc>(ctx: &GcContext<'gc>, env: &JSObjectDataPtr<'gc>, j: &JSError) -> Value<'gc> {
    let fallback_msg = j.message();
    let (ctor_name, msg) = match j.kind() {
        JSErrorKind::TypeError { message } => ("TypeError", message.as_str()),
        JSErrorKind::RangeError { message } => ("RangeError", message.as_str()),
        JSErrorKind::SyntaxError { message } => ("SyntaxError", message.as_str()),
        JSErrorKind::ReferenceError { message } => ("ReferenceError", message.as_str()),
        JSErrorKind::RuntimeError { message } => ("Error", message.as_str()),
        JSErrorKind::URIError { message } => ("URIError", message.as_str()),
        JSErrorKind::EvaluationError { message } => ("Error", message.as_str()),
        JSErrorKind::Throw(message) => ("Error", message.as_str()),
        _ => ("Error", fallback_msg.as_str()),
    };

    let msg_val = Value::from(msg);
    if let Some(ctor_val) = crate::core::env_get(env, ctor_name)
        && let Value::Object(ctor_obj) = &*ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(ctor_obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        return crate::core::create_error(ctx, Some(*proto_obj), &msg_val).unwrap_or(Value::from(msg));
    }

    Value::from(msg)
}

fn eval_error_to_value<'gc>(ctx: &GcContext<'gc>, env: &JSObjectDataPtr<'gc>, err: EvalError<'gc>) -> Value<'gc> {
    match err {
        EvalError::Throw(v, ..) => v,
        EvalError::Js(j) => js_error_to_value(ctx, env, &j),
    }
}

fn await_value<'gc>(ctx: &GcContext<'gc>, _env: &JSObjectDataPtr<'gc>, value: Value<'gc>) -> Result<Value<'gc>, Value<'gc>> {
    if let Value::Object(obj) = &value
        && let Some(promise_ref) = crate::js_promise::get_promise_from_js_object(obj)
    {
        crate::js_promise::mark_promise_handled(ctx, promise_ref, _env).expect("must succeed");
        let state = promise_ref.borrow().state.clone();
        match state {
            crate::core::PromiseState::Pending => {
                // Do not block the event loop inside async generators; return the promise
                // so callers (e.g., for-await-of) can await it.
                return Ok(value.clone());
            }
            crate::core::PromiseState::Fulfilled(v) => return Ok(v.clone()),
            crate::core::PromiseState::Rejected(r) => return Err(r.clone()),
        }
    }
    Ok(value)
}

fn has_async_iterator<'gc>(env: &JSObjectDataPtr<'gc>, obj: &JSObjectDataPtr<'gc>) -> bool {
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(async_iter_sym_val) = object_get_key_value(sym_obj, "asyncIterator")
        && let Value::Symbol(async_iter_sym) = &*async_iter_sym_val.borrow()
    {
        return object_get_key_value(obj, async_iter_sym).is_some();
    }
    false
}

// Create an async generator instance (object) when an async generator function is called.
pub fn handle_async_generator_function_call<'gc>(
    ctx: &GcContext<'gc>,
    closure: &ClosureData<'gc>,
    args: &[Value<'gc>],
    fn_obj: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let closure_env = closure.env.expect("closure env must exist");
    // Prepare call-time environment and bind parameters immediately.
    // This ensures parameter destructuring/defaults throw at call time, matching spec.
    let home_opt = if let Some(home_obj) = &closure.home_object {
        Some(home_obj.clone())
    } else if let Some(fn_obj_ptr) = fn_obj {
        fn_obj_ptr.borrow().get_home_object()
    } else {
        None
    };
    let call_env =
        prepare_function_call_env_with_home(ctx, Some(&closure_env), None, Some(&closure.params[..]), args, None, None, home_opt)?;

    // Ensure an `arguments` object is available on the call environment.
    crate::js_class::create_arguments_object(ctx, &call_env, args, None)?;

    // Create the async generator instance object
    let gen_obj = new_js_object_data(ctx);

    // Create internal async generator struct
    let async_gen = new_gc_cell_ptr(
        ctx,
        JSAsyncGenerator {
            params: closure.params.clone(),
            body: closure.body.clone(),
            env: closure_env,
            call_env: Some(call_env),
            args: args.to_vec(),
            state: crate::core::GeneratorState::NotStarted,
            cached_initial_yield: None,
            pending: Vec::new(),
            pending_for_await: None,
            yield_star_iterator: None,
        },
    );

    // Store it on the object under a hidden key
    slot_set(ctx, &gen_obj, InternalSlot::AsyncGeneratorState, &Value::AsyncGenerator(async_gen));

    // Determine prototype for the async generator object.
    // Prefer the function object's own `prototype` if it's an object; otherwise
    // fall back to AsyncGenerator.prototype from the current realm if present.
    let mut proto_candidate: Option<JSObjectDataPtr<'gc>> = None;
    if let Some(fn_obj) = fn_obj {
        if let Some(proto_val) = get_own_property(&fn_obj, "prototype") {
            let proto_value = match &*proto_val.borrow() {
                Value::Property { value: Some(v), .. } => v.borrow().clone(),
                other => other.clone(),
            };
            if let Value::Object(proto_obj) = proto_value {
                proto_candidate = Some(proto_obj);
            }
        }
        if proto_candidate.is_none()
            && let Some(proto_val) = slot_get(&fn_obj, &InternalSlot::AsyncGeneratorProto)
        {
            let proto_value = match &*proto_val.borrow() {
                Value::Property { value: Some(v), .. } => v.borrow().clone(),
                other => other.clone(),
            };
            if let Value::Object(proto_obj) = proto_value {
                proto_candidate = Some(proto_obj);
            }
        }
    }
    if proto_candidate.is_none()
        && let Some(gen_ctor_val) = crate::core::env_get(closure.env.as_ref().expect("AsyncGenerator needs env"), "AsyncGenerator")
        && let Value::Object(gen_ctor_obj) = &*gen_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(gen_ctor_obj, "prototype")
    {
        let proto_value = match &*proto_val.borrow() {
            Value::Property { value: Some(v), .. } => v.borrow().clone(),
            other => other.clone(),
        };
        if let Value::Object(proto_obj) = proto_value {
            proto_candidate = Some(proto_obj);
        }
    }
    if let Some(mut proto_obj) = proto_candidate {
        if !has_async_iterator(&closure_env, &proto_obj)
            && let Some(gen_ctor_val) = crate::core::env_get(&closure_env, "AsyncGenerator")
            && let Value::Object(gen_ctor_obj) = &*gen_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(gen_ctor_obj, "prototype")
        {
            let proto_value = match &*proto_val.borrow() {
                Value::Property { value: Some(v), .. } => v.borrow().clone(),
                other => other.clone(),
            };
            if let Value::Object(async_proto) = proto_value {
                proto_obj = async_proto;
            }
        }
        gen_obj.borrow_mut(ctx).prototype = Some(proto_obj);
    }

    // Create 'next' function as a native Function; name it so call_native_function can route
    let next_func = Value::Function("AsyncGenerator.prototype.next".to_string());
    object_set_key_value(ctx, &gen_obj, "next", &next_func)?;
    // Create 'throw' and 'return' functions
    let throw_func = Value::Function("AsyncGenerator.prototype.throw".to_string());
    object_set_key_value(ctx, &gen_obj, "throw", &throw_func)?;
    let return_func = Value::Function("AsyncGenerator.prototype.return".to_string());
    object_set_key_value(ctx, &gen_obj, "return", &return_func)?;
    // Return the object
    Ok(Value::Object(gen_obj))
}

/// Initialize AsyncGenerator constructor/prototype and attach prototype methods
pub fn initialize_async_generator<'gc>(ctx: &GcContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    // Create constructor object and async generator prototype
    let async_gen_ctor = crate::core::new_js_object_data(ctx);
    // Set __proto__ to Function.prototype if present
    if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        async_gen_ctor.borrow_mut(ctx).prototype = Some(*proto_obj);
    }

    let async_gen_proto = crate::core::new_js_object_data(ctx);
    // Create an AsyncIteratorPrototype object and make AsyncGenerator.prototype
    // inherit from it (instead of directly from Object.prototype).
    let async_iter_proto = crate::core::new_js_object_data(ctx);
    let _ = crate::core::set_internal_prototype_from_constructor(ctx, &async_iter_proto, env, "Object");
    async_gen_proto.borrow_mut(ctx).prototype = Some(async_iter_proto);

    // Attach prototype methods as proper function objects so they expose
    // correct `length` and `name` properties with the right descriptors.
    let func_proto_opt: Option<JSObjectDataPtr<'gc>> = if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(func_proto) = &*proto_val.borrow()
    {
        Some(*func_proto)
    } else {
        None
    };
    for (method_name, dispatch_name, length) in [
        ("next", "AsyncGenerator.prototype.next", 1),
        ("return", "AsyncGenerator.prototype.return", 1),
        ("throw", "AsyncGenerator.prototype.throw", 1),
    ] {
        let fn_obj = new_js_object_data(ctx);
        fn_obj
            .borrow_mut(ctx)
            .set_closure(Some(new_gc_cell_ptr(ctx, Value::Function(dispatch_name.to_string()))));
        if let Some(fp) = func_proto_opt {
            fn_obj.borrow_mut(ctx).prototype = Some(fp);
        }
        let desc_name = crate::core::create_descriptor_object(ctx, &Value::from(method_name), false, false, true)?;
        crate::js_object::define_property_internal(ctx, &fn_obj, "name", &desc_name)?;
        let desc_len = crate::core::create_descriptor_object(ctx, &Value::Number(length as f64), false, false, true)?;
        crate::js_object::define_property_internal(ctx, &fn_obj, "length", &desc_len)?;
        let desc_method = crate::core::create_descriptor_object(ctx, &Value::Object(fn_obj), true, false, true)?;
        crate::js_object::define_property_internal(ctx, &async_gen_proto, method_name, &desc_method)?;
    }

    // Register internal helpers for awaits
    crate::core::env_set(
        ctx,
        env,
        "__internal_async_gen_await_resolve",
        &Value::Function("__internal_async_gen_await_resolve".to_string()),
    )?;
    crate::core::env_set(
        ctx,
        env,
        "__internal_async_gen_await_reject",
        &Value::Function("__internal_async_gen_await_reject".to_string()),
    )?;

    crate::core::env_set(
        ctx,
        env,
        "__internal_async_gen_yield_resolve",
        &Value::Function("__internal_async_gen_yield_resolve".to_string()),
    )?;
    crate::core::env_set(
        ctx,
        env,
        "__internal_async_gen_yield_reject",
        &Value::Function("__internal_async_gen_yield_reject".to_string()),
    )?;

    crate::core::env_set(
        ctx,
        env,
        "__internal_async_gen_yield_star_resolve",
        &Value::Function("__internal_async_gen_yield_star_resolve".to_string()),
    )?;
    crate::core::env_set(
        ctx,
        env,
        "__internal_async_gen_yield_star_reject",
        &Value::Function("__internal_async_gen_yield_star_reject".to_string()),
    )?;

    crate::core::env_set(
        ctx,
        env,
        "__internal_async_gen_return_resolve",
        &Value::Function("__internal_async_gen_return_resolve".to_string()),
    )?;
    crate::core::env_set(
        ctx,
        env,
        "__internal_async_gen_return_reject",
        &Value::Function("__internal_async_gen_return_reject".to_string()),
    )?;

    // Register Symbol.asyncIterator on %AsyncIteratorPrototype% as a proper function object
    // Per spec: %AsyncIteratorPrototype% [ @@asyncIterator ] ( ) — returns `this`
    // name = "[Symbol.asyncIterator]", length = 0
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(async_iter_sym_val) = object_get_key_value(sym_obj, "asyncIterator")
        && let Value::Symbol(async_iter_sym) = &*async_iter_sym_val.borrow()
    {
        let fn_obj = new_js_object_data(ctx);
        fn_obj.borrow_mut(ctx).set_closure(Some(new_gc_cell_ptr(
            ctx,
            Value::Function("AsyncGenerator.prototype.asyncIterator".to_string()),
        )));
        if let Some(fp) = func_proto_opt {
            fn_obj.borrow_mut(ctx).prototype = Some(fp);
        }
        let desc_name = crate::core::create_descriptor_object(ctx, &Value::from("[Symbol.asyncIterator]"), false, false, true)?;
        crate::js_object::define_property_internal(ctx, &fn_obj, "name", &desc_name)?;
        let desc_len = crate::core::create_descriptor_object(ctx, &Value::Number(0.0), false, false, true)?;
        crate::js_object::define_property_internal(ctx, &fn_obj, "length", &desc_len)?;

        // Set on AsyncIteratorPrototype (writable: true, enumerable: false, configurable: true)
        let desc_method = crate::core::create_descriptor_object(ctx, &Value::Object(fn_obj), true, false, true)?;
        crate::js_object::define_property_internal(ctx, &async_iter_proto, *async_iter_sym, &desc_method)?;
    }

    // Register Symbol.asyncDispose on %AsyncIteratorPrototype%
    // Per spec: %AsyncIteratorPrototype% [ @@asyncDispose ] ( ) — returns a Promise
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(async_dispose_sym_val) = object_get_key_value(sym_obj, "asyncDispose")
        && let Value::Symbol(async_dispose_sym) = &*async_dispose_sym_val.borrow()
    {
        let fn_obj = new_js_object_data(ctx);
        fn_obj.borrow_mut(ctx).set_closure(Some(new_gc_cell_ptr(
            ctx,
            Value::Function("AsyncIteratorPrototype.asyncDispose".to_string()),
        )));
        slot_set(ctx, &fn_obj, InternalSlot::Callable, &Value::Boolean(true));
        if let Some(fp) = func_proto_opt {
            fn_obj.borrow_mut(ctx).prototype = Some(fp);
        }
        let desc_name = crate::core::create_descriptor_object(ctx, &Value::from("[Symbol.asyncDispose]"), false, false, true)?;
        crate::js_object::define_property_internal(ctx, &fn_obj, "name", &desc_name)?;
        let desc_len = crate::core::create_descriptor_object(ctx, &Value::Number(0.0), false, false, true)?;
        crate::js_object::define_property_internal(ctx, &fn_obj, "length", &desc_len)?;

        let desc_method = crate::core::create_descriptor_object(ctx, &Value::Object(fn_obj), true, false, true)?;
        crate::js_object::define_property_internal(ctx, &async_iter_proto, *async_dispose_sym, &desc_method)?;
    }

    // Set AsyncGenerator.prototype[@@toStringTag] = "AsyncGenerator"
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let desc_tag = crate::core::create_descriptor_object(ctx, &Value::from("AsyncGenerator"), false, false, true)?;
        crate::js_object::define_property_internal(ctx, &async_gen_proto, *tag_sym, &desc_tag)?;
    }

    // Defer setting constructor and env binding until after AsyncGeneratorFunction.prototype
    // is created, because per spec %AsyncGenerator% IS %AsyncGeneratorFunction%.prototype.
    // Set 'prototype' on the temporary async_gen_ctor (used internally)
    let desc_proto = crate::core::create_descriptor_object(ctx, &Value::Object(async_gen_proto), true, false, false)?;
    crate::js_object::define_property_internal(ctx, &async_gen_ctor, "prototype", &desc_proto)?;

    // Create AsyncGeneratorFunction constructor/prototype so async generator
    // function objects inherit from a distinct AsyncGeneratorFunction.prototype,
    // whose own "prototype" points to AsyncGenerator.prototype.
    let async_gen_func_ctor = crate::core::new_js_object_data(ctx);
    let async_gen_func_proto = crate::core::new_js_object_data(ctx);

    // AsyncGeneratorFunction itself inherits from Function (constructor).
    // AsyncGeneratorFunction.prototype inherits from Function.prototype.
    if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
    {
        // [[Prototype]] of AsyncGeneratorFunction is Function
        async_gen_func_ctor.borrow_mut(ctx).prototype = Some(*func_ctor);
        // AsyncGeneratorFunction.prototype.[[Prototype]] is Function.prototype
        if let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
            && let Value::Object(func_proto) = &*proto_val.borrow()
        {
            async_gen_func_proto.borrow_mut(ctx).prototype = Some(*func_proto);
        }
    }

    slot_set(
        ctx,
        &async_gen_func_ctor,
        InternalSlot::NativeCtor,
        &Value::from("AsyncGeneratorFunction"),
    );
    // Mark as constructor so typeof returns "function" and isConstructor is true
    slot_set(ctx, &async_gen_func_ctor, InternalSlot::IsConstructor, &Value::Boolean(true));

    // AsyncGeneratorFunction.length = 1 (non-writable, non-enumerable, configurable)
    let desc_len = crate::core::create_descriptor_object(ctx, &Value::Number(1.0), false, false, true)?;
    crate::js_object::define_property_internal(ctx, &async_gen_func_ctor, "length", &desc_len)?;

    // AsyncGeneratorFunction.name = "AsyncGeneratorFunction" (non-writable, non-enumerable, configurable)
    let desc_name = crate::core::create_descriptor_object(ctx, &Value::from("AsyncGeneratorFunction"), false, false, true)?;
    crate::js_object::define_property_internal(ctx, &async_gen_func_ctor, "name", &desc_name)?;

    // AsyncGeneratorFunction.prototype.prototype → %AsyncGenerator.prototype%
    // writable=false, enumerable=false, configurable=true
    let desc_fn_proto = crate::core::create_descriptor_object(ctx, &Value::Object(async_gen_proto), false, false, true)?;
    crate::js_object::define_property_internal(ctx, &async_gen_func_proto, "prototype", &desc_fn_proto)?;

    // AsyncGeneratorFunction.prototype.constructor → AsyncGeneratorFunction
    // writable=false, enumerable=false, configurable=true
    let desc_fn_ctor = crate::core::create_descriptor_object(ctx, &Value::Object(async_gen_func_ctor), false, false, true)?;
    crate::js_object::define_property_internal(ctx, &async_gen_func_proto, "constructor", &desc_fn_ctor)?;

    // Set AsyncGeneratorFunction.prototype[@@toStringTag] = "AsyncGeneratorFunction"
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(tag_sym_val) = object_get_key_value(sym_obj, "toStringTag")
        && let Value::Symbol(tag_sym) = &*tag_sym_val.borrow()
    {
        let desc_tag = crate::core::create_descriptor_object(ctx, &Value::from("AsyncGeneratorFunction"), false, false, true)?;
        crate::js_object::define_property_internal(ctx, &async_gen_func_proto, *tag_sym, &desc_tag)?;
    }

    // AsyncGeneratorFunction.prototype: non-writable, non-enumerable, non-configurable
    let desc_ctor_proto = crate::core::create_descriptor_object(ctx, &Value::Object(async_gen_func_proto), false, false, false)?;
    crate::js_object::define_property_internal(ctx, &async_gen_func_ctor, "prototype", &desc_ctor_proto)?;

    // Per spec, %AsyncGenerator% IS %AsyncGeneratorFunction%.prototype.
    // Set 'constructor' on AsyncGenerator.prototype → async_gen_func_proto
    // { [[Writable]]: false, [[Enumerable]]: false, [[Configurable]]: true }
    let desc_ctor = crate::core::create_descriptor_object(ctx, &Value::Object(async_gen_func_proto), false, false, true)?;
    crate::js_object::define_property_internal(ctx, &async_gen_proto, "constructor", &desc_ctor)?;
    // Expose %AsyncGenerator% on the environment as async_gen_func_proto (not the old async_gen_ctor)
    crate::core::env_set(ctx, env, "AsyncGenerator", &Value::Object(async_gen_func_proto))?;

    // Store as hidden intrinsic (NOT a global) via internal slot
    slot_set(
        ctx,
        env,
        InternalSlot::AsyncGeneratorFunctionCtor,
        &Value::Object(async_gen_func_ctor),
    );

    // Stamp with OriginGlobal so evaluate_new can discover the constructor's realm
    slot_set(ctx, &async_gen_func_ctor, InternalSlot::OriginGlobal, &Value::Object(*env));

    Ok(())
}

// Small helper: detect if an Expr contains a yield/await
#[allow(dead_code)]
fn expr_contains_yield_or_await(e: &Expr) -> bool {
    match e {
        Expr::Yield(_) | Expr::YieldStar(_) | Expr::Await(_) => true,
        Expr::Binary(a, _, b) => expr_contains_yield_or_await(a) || expr_contains_yield_or_await(b),
        Expr::Assign(a, b) => expr_contains_yield_or_await(a) || expr_contains_yield_or_await(b),
        Expr::Index(a, b) => expr_contains_yield_or_await(a) || expr_contains_yield_or_await(b),
        Expr::Property(a, _) => expr_contains_yield_or_await(a),
        Expr::Call(a, args) => expr_contains_yield_or_await(a) || args.iter().any(expr_contains_yield_or_await),
        Expr::Object(pairs) => pairs
            .iter()
            .any(|(k, v, _, _)| expr_contains_yield_or_await(k) || expr_contains_yield_or_await(v)),
        Expr::Array(items) => items.iter().any(|it| it.as_ref().is_some_and(expr_contains_yield_or_await)),
        Expr::UnaryNeg(a)
        | Expr::LogicalNot(a)
        | Expr::TypeOf(a)
        | Expr::Delete(a)
        | Expr::Void(a)
        | Expr::PostIncrement(a)
        | Expr::PostDecrement(a)
        | Expr::Increment(a)
        | Expr::Decrement(a) => expr_contains_yield_or_await(a),
        Expr::LogicalAnd(a, b) | Expr::LogicalOr(a, b) | Expr::Comma(a, b) | Expr::Conditional(a, b, _) => {
            expr_contains_yield_or_await(a) || expr_contains_yield_or_await(b)
        }
        Expr::OptionalCall(a, args) => expr_contains_yield_or_await(a) || args.iter().any(expr_contains_yield_or_await),
        Expr::OptionalIndex(a, b) => expr_contains_yield_or_await(a) || expr_contains_yield_or_await(b),
        _ => false,
    }
}

#[allow(dead_code)]
fn stmt_contains_yield_or_await(s: &Statement) -> bool {
    match &*s.kind {
        StatementKind::Expr(e) => expr_contains_yield_or_await(e),
        StatementKind::Let(decls) | StatementKind::Var(decls) => decls
            .iter()
            .any(|(_, e_opt)| e_opt.as_ref().map(expr_contains_yield_or_await).unwrap_or(false)),
        StatementKind::Const(decls) => decls.iter().any(|(_, e)| expr_contains_yield_or_await(e)),
        StatementKind::If(if_stmt) => {
            let if_stmt = if_stmt.as_ref();
            if expr_contains_yield_or_await(&if_stmt.condition) {
                return true;
            }
            if if_stmt.then_body.iter().any(stmt_contains_yield_or_await) {
                return true;
            }
            if if_stmt
                .else_body
                .as_ref()
                .map(|b| b.iter().any(stmt_contains_yield_or_await))
                .unwrap_or(false)
            {
                return true;
            }
            false
        }
        StatementKind::Block(stmts) => stmts.iter().any(stmt_contains_yield_or_await),
        StatementKind::For(for_stmt) => for_stmt.body.iter().any(stmt_contains_yield_or_await),
        StatementKind::While(cond, body) => expr_contains_yield_or_await(cond) || body.iter().any(stmt_contains_yield_or_await),
        StatementKind::DoWhile(body, cond) => body.iter().any(stmt_contains_yield_or_await) || expr_contains_yield_or_await(cond),
        StatementKind::ForOf(_, _, _, body)
        | StatementKind::ForIn(_, _, _, body)
        | StatementKind::ForOfDestructuringObject(_, _, _, body)
        | StatementKind::ForOfDestructuringArray(_, _, _, body)
        | StatementKind::ForAwaitOf(_, _, _, body)
        | StatementKind::ForAwaitOfDestructuringObject(_, _, _, body)
        | StatementKind::ForAwaitOfDestructuringArray(_, _, _, body)
        | StatementKind::ForAwaitOfExpr(_, _, body)
        | StatementKind::ForOfExpr(_, _, body) => body.iter().any(stmt_contains_yield_or_await),
        StatementKind::TryCatch(tc_stmt) => {
            let tc = tc_stmt.as_ref();
            if tc.try_body.iter().any(stmt_contains_yield_or_await) {
                return true;
            }
            if tc
                .catch_body
                .as_ref()
                .map(|b| b.iter().any(stmt_contains_yield_or_await))
                .unwrap_or(false)
            {
                return true;
            }
            if tc
                .finally_body
                .as_ref()
                .map(|b| b.iter().any(stmt_contains_yield_or_await))
                .unwrap_or(false)
            {
                return true;
            }
            false
        }
        _ => false,
    }
}

fn create_iterator_result_obj<'gc>(ctx: &GcContext<'gc>, value: Value<'gc>, done: bool) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(ctx);
    object_set_key_value(ctx, &obj, "value", &value)?;
    object_set_key_value(ctx, &obj, "done", &Value::Boolean(done))?;
    Ok(obj)
}

enum AsyncGeneratorCompletion<'gc> {
    Normal,
    Return(Value<'gc>),
}

fn evaluate_async_generator_completion<'gc>(
    ctx: &GcContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
) -> Result<AsyncGeneratorCompletion<'gc>, EvalError<'gc>> {
    match crate::core::evaluate_statements_with_labels(ctx, env, statements, &[], &[])? {
        crate::core::ControlFlow::Normal(_) => Ok(AsyncGeneratorCompletion::Normal),
        crate::core::ControlFlow::Return(v) => Ok(AsyncGeneratorCompletion::Return(v)),
        crate::core::ControlFlow::Throw(v, line, column) => Err(EvalError::Throw(v, line, column)),
        crate::core::ControlFlow::Break(_) => Err(raise_syntax_error!("break statement not in loop or switch").into()),
        crate::core::ControlFlow::Continue(_) => Err(raise_syntax_error!("continue statement not in loop").into()),
    }
}

// Helper to create a new internal JSPromise cell and corresponding JS Promise object
fn create_promise_cell_and_obj<'gc>(ctx: &GcContext<'gc>, env: &JSObjectDataPtr<'gc>) -> (GcPtr<'gc, JSPromise<'gc>>, Value<'gc>) {
    let promise_cell = new_gc_cell_ptr(ctx, crate::core::JSPromise::new());
    let promise_obj = make_promise_js_object(ctx, promise_cell, Some(*env)).unwrap();
    (promise_cell, Value::Object(promise_obj))
}

fn extract_simple_yield_expr(body: &[Statement]) -> Option<Expr> {
    if body.len() == 1
        && let StatementKind::Expr(Expr::Yield(Some(inner))) = &*body[0].kind
    {
        return Some((**inner).clone());
    }
    None
}

fn eval_yield_inner_expr<'gc>(
    ctx: &GcContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    yield_kind: crate::js_generator::YieldKind,
    inner_expr: &Expr,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let expr = if yield_kind == crate::js_generator::YieldKind::YieldStar {
        if let Expr::Await(inner) = inner_expr {
            inner.as_ref()
        } else {
            inner_expr
        }
    } else {
        inner_expr
    };

    crate::core::evaluate_expr(ctx, env, expr)
}

fn allow_loop_fallback(stmt: &Statement) -> bool {
    match &*stmt.kind {
        StatementKind::While(..) | StatementKind::DoWhile(..) | StatementKind::For(..) => true,
        StatementKind::Block(stmts) => stmts.iter().any(|s| {
            matches!(
                &*s.kind,
                StatementKind::While(..) | StatementKind::DoWhile(..) | StatementKind::For(..)
            ) && stmt_contains_yield_or_await(s)
        }),
        _ => false,
    }
}

fn get_for_await_iterator<'gc>(
    ctx: &GcContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iter_val: &Value<'gc>,
) -> Result<(JSObjectDataPtr<'gc>, bool), EvalError<'gc>> {
    let mut iterator: Option<JSObjectDataPtr<'gc>> = None;
    let mut is_async_iter = false;

    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(async_iter_sym) = object_get_key_value(sym_obj, "asyncIterator")
        && let Value::Symbol(async_iter_sym_data) = &*async_iter_sym.borrow()
        && let Value::Object(obj) = iter_val
    {
        let method = crate::core::get_property_with_accessors(ctx, env, obj, async_iter_sym_data)?;
        let method = match method {
            Value::Property { value: Some(v), .. } => v.borrow().clone(),
            other => other,
        };
        if !matches!(method, Value::Undefined | Value::Null) {
            let res = evaluate_call_dispatch(ctx, env, &method, Some(iter_val), &[])?;
            let res = await_value(ctx, env, res).map_err(|v| EvalError::Throw(v, None, None))?;
            if let Value::Object(iter_obj) = res {
                iterator = Some(iter_obj);
                is_async_iter = true;
            }
        }
    }

    if iterator.is_none()
        && let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
        && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
        && let Value::Object(obj) = iter_val
    {
        let method = crate::core::get_property_with_accessors(ctx, env, obj, iter_sym_data)?;
        let method = match method {
            Value::Property { value: Some(v), .. } => v.borrow().clone(),
            other => other,
        };
        if !matches!(method, Value::Undefined | Value::Null) {
            let res = evaluate_call_dispatch(ctx, env, &method, Some(iter_val), &[])?;
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

fn for_await_next_value<'gc>(
    ctx: &GcContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iter_obj: JSObjectDataPtr<'gc>,
    is_async_iter: bool,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    let next_method = crate::core::get_property_with_accessors(ctx, env, &iter_obj, "next")?;
    // unwrap descriptor if necessary
    let next_method = match next_method {
        Value::Property { value: Some(v), .. } => v.borrow().clone(),
        other => other,
    };
    if matches!(next_method, Value::Undefined | Value::Null) {
        return Err(EvalError::Js(raise_type_error!("Iterator has no next method")));
    }

    let mut next_res_val = evaluate_call_dispatch(ctx, env, &next_method, Some(&Value::Object(iter_obj)), &[])?;
    if is_async_iter {
        next_res_val = await_value(ctx, env, next_res_val).map_err(|v| EvalError::Throw(v, None, None))?;
    }

    if let Value::Object(next_res) = next_res_val {
        let done = if let Some(done_val) = object_get_key_value(&next_res, "done") {
            matches!(&*done_val.borrow(), Value::Boolean(true))
        } else {
            false
        };

        if done {
            return Ok(None);
        }

        let mut value = if let Some(val) = object_get_key_value(&next_res, "value") {
            val.borrow().clone()
        } else {
            Value::Undefined
        };

        value = await_value(ctx, env, value).map_err(|v| EvalError::Throw(v, None, None))?;
        Ok(Some(value))
    } else {
        Err(raise_type_error!("Iterator result is not an object").into())
    }
}

// Process pending requests (next/throw/return) for the given async generator
// Processes requests until the generator suspends or the pending queue is empty.
fn process_one_pending<'gc>(
    ctx: &GcContext<'gc>,
    gen_ptr: GcPtr<'gc, JSAsyncGenerator<'gc>>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    use crate::core::GeneratorState;

    loop {
        // Drain any requests that were deferred while the generator was executing.
        drain_deferred_requests(ctx, gen_ptr, env)?;

        let mut gen_ptr_mut_guard = gen_ptr.borrow_mut(ctx);
        let gen_ptr_mut = &mut *gen_ptr_mut_guard;

        if gen_ptr_mut.pending.is_empty() {
            return Ok(());
        }

        // Check if we are delegating to another iterator (yield*)
        if let Some(iter_obj) = gen_ptr_mut.yield_star_iterator {
            let (promise_cell, request) = gen_ptr_mut.pending.remove(0);
            drop(gen_ptr_mut_guard);

            let (method, args) = match request {
                AsyncGeneratorRequest::Next(val) => ("next", vec![val]),
                AsyncGeneratorRequest::Throw(val) => ("throw", vec![val]),
                AsyncGeneratorRequest::Return(val) => ("return", vec![val]),
            };

            handle_yield_star_call(ctx, env, gen_ptr, promise_cell, iter_obj, method, args)?;
            // Delegation handles the request; return to event loop (or wait for callback)
            return Ok(());
        }

        // Pop the next pending entry (front of queue)
        let maybe_entry = gen_ptr_mut.pending.first().cloned();
        let (promise_cell, request) = maybe_entry.unwrap();
        gen_ptr_mut.pending.remove(0);

        match (&mut gen_ptr_mut.state, request) {
            (GeneratorState::NotStarted, AsyncGeneratorRequest::Next(_send_value)) => {
                // Initialize and suspend at first yield (or run to completion)
                if let Some((idx, inner_idx_opt, yield_kind, yield_inner)) =
                    crate::js_generator::find_first_yield_in_statements(&gen_ptr_mut.body)
                {
                    let func_env = if let Some(prep_env) = gen_ptr_mut.call_env.take() {
                        prep_env
                    } else {
                        let env = crate::core::prepare_function_call_env(
                            ctx,
                            Some(&gen_ptr_mut.env),
                            None,
                            Some(&gen_ptr_mut.params[..]),
                            &gen_ptr_mut.args,
                            None,
                            None,
                        )?;

                        // Ensure an `arguments` object is available to the function body so
                        // parameter accesses (and `arguments.length`) reflect the passed args.
                        // This mirrors what `call_closure`/function calls do for ordinary functions.
                        crate::js_class::create_arguments_object(ctx, &env, &gen_ptr_mut.args, None)?;
                        env
                    };

                    if let StatementKind::ForAwaitOf(decl_kind_opt, var_name, iterable, body) = &*gen_ptr_mut.body[idx].kind
                        && inner_idx_opt.is_some()
                        && let Some(yield_expr) = extract_simple_yield_expr(body)
                    {
                        if idx > 0 {
                            let pre_stmts = gen_ptr_mut.body[0..idx].to_vec();
                            let _ = crate::core::evaluate_statements(ctx, &func_env, &pre_stmts)?;
                        }
                        let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
                        if let Some(VarDeclKind::Let) | Some(VarDeclKind::Const) = decl_kind_opt {
                            let he = new_js_object_data(ctx);
                            he.borrow_mut(ctx).prototype = Some(func_env);
                            env_set(ctx, &he, var_name, &Value::Uninitialized)?;
                            head_env = Some(he);
                        }
                        let iter_eval_env = head_env.as_ref().unwrap_or(&func_env);
                        let iter_val = evaluate_expr(ctx, iter_eval_env, iterable)?;
                        let (iter_obj, is_async_iter) = match get_for_await_iterator(ctx, &func_env, &iter_val) {
                            Ok(v) => v,
                            Err(EvalError::Throw(v, _, _)) => {
                                gen_ptr_mut.state = GeneratorState::Completed;
                                reject_promise(ctx, &promise_cell, v, env);
                                return Ok(());
                            }
                            Err(e) => {
                                gen_ptr_mut.state = GeneratorState::Completed;
                                reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                                return Ok(());
                            }
                        };

                        match for_await_next_value(ctx, &func_env, iter_obj, is_async_iter) {
                            Ok(Some(value)) => {
                                let iter_env = if let Some(VarDeclKind::Let) | Some(VarDeclKind::Const) = decl_kind_opt {
                                    let e = new_js_object_data(ctx);
                                    e.borrow_mut(ctx).prototype = Some(func_env);
                                    env_set(ctx, &e, var_name, &value.clone())?;
                                    e
                                } else {
                                    env_set_recursive(ctx, &func_env, var_name, &value.clone())?;
                                    func_env
                                };

                                let mut yielded = evaluate_expr(ctx, &iter_env, &yield_expr)?;
                                match await_value(ctx, env, yielded.clone()) {
                                    Ok(awaited) => {
                                        yielded = awaited;
                                    }
                                    Err(reason) => {
                                        gen_ptr_mut.state = GeneratorState::Completed;
                                        reject_promise(ctx, &promise_cell, reason, env);
                                        return Ok(());
                                    }
                                }

                                gen_ptr_mut.state = GeneratorState::Suspended {
                                    pc: idx,
                                    stack: vec![],
                                    pre_env: Some(func_env),
                                };
                                gen_ptr_mut.pending_for_await = Some(crate::core::AsyncForAwaitState {
                                    iterator: iter_obj,
                                    is_async: is_async_iter,
                                    decl_kind: *decl_kind_opt,
                                    var_name: var_name.clone(),
                                    yield_expr,
                                });

                                let res_obj = create_iterator_result_obj(ctx, yielded, false)?;
                                resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                                return Ok(());
                            }
                            Ok(None) => {
                                gen_ptr_mut.state = GeneratorState::Completed;
                                let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                                resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                                return Ok(());
                            }
                            Err(e) => {
                                gen_ptr_mut.state = GeneratorState::Completed;
                                reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                                return Ok(());
                            }
                        }
                    }

                    // Handle regular For loop with yield in body (NotStarted)
                    if let StatementKind::For(for_stmt) = &*gen_ptr_mut.body[idx].kind
                        && inner_idx_opt.is_some()
                    {
                        // Execute pre-loop statements
                        if idx > 0 {
                            let pre_stmts = gen_ptr_mut.body[0..idx].to_vec();
                            crate::core::evaluate_statements(ctx, &func_env, &pre_stmts)?;
                        }

                        // Execute for-loop init
                        if let Some(init_stmt) = &for_stmt.init {
                            let init_clone = init_stmt.clone();
                            crate::core::evaluate_statements(ctx, &func_env, std::slice::from_ref(&init_clone))?;
                        }

                        // Check test condition
                        if let Some(test_expr) = &for_stmt.test {
                            let test_val = crate::core::evaluate_expr(ctx, &func_env, test_expr)?;
                            if !test_val.to_truthy() {
                                gen_ptr_mut.state = GeneratorState::Completed;
                                let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                                resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                                return Ok(());
                            }
                        }

                        // Evaluate pre-yield body statements
                        let body_clone = for_stmt.body.clone();
                        if let Some(inner_idx) = inner_idx_opt
                            && inner_idx > 0
                        {
                            let pre_stmts = body_clone[0..inner_idx].to_vec();
                            crate::core::evaluate_statements(ctx, &func_env, &pre_stmts)?;
                        }

                        // Evaluate yield expression
                        let yielded = if let Some(ref inner_expr) = yield_inner {
                            match crate::core::evaluate_expr(ctx, &func_env, inner_expr) {
                                Ok(v) => v,
                                Err(e) => {
                                    gen_ptr_mut.state = GeneratorState::Completed;
                                    reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                                    return Ok(());
                                }
                            }
                        } else {
                            Value::Undefined
                        };

                        // Clear init so it doesn't re-run on resume
                        if let StatementKind::For(for_stmt_m) = gen_ptr_mut.body[idx].kind.as_mut() {
                            for_stmt_m.init = None;
                        }

                        gen_ptr_mut.state = GeneratorState::Suspended {
                            pc: idx,
                            stack: vec![],
                            pre_env: Some(func_env),
                        };
                        gen_ptr_mut.cached_initial_yield = Some(yielded.clone());
                        let res_obj = create_iterator_result_obj(ctx, yielded, false)?;
                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                        return Ok(());
                    }

                    if idx > 0 {
                        let pre_stmts = gen_ptr_mut.body[0..idx].to_vec();
                        let _ = crate::core::evaluate_statements(ctx, &func_env, &pre_stmts)?;
                    } else if let Some(inner_idx) = inner_idx_opt
                        && inner_idx > 0
                        && let StatementKind::Block(inner_stmts) = &*gen_ptr_mut.body[idx].kind
                    {
                        let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                        let _ = crate::core::evaluate_statements(ctx, &func_env, &pre_stmts)?;
                    }

                    gen_ptr_mut.state = GeneratorState::Suspended {
                        pc: idx,
                        stack: vec![],
                        pre_env: Some(func_env),
                    };

                    if let Some(inner_expr_box) = yield_inner {
                        let parent_env = &func_env;
                        let inner_eval_env = crate::core::prepare_function_call_env(ctx, Some(parent_env), None, None, &[], None, None)?;
                        slot_set(ctx, &inner_eval_env, InternalSlot::GenThrowVal, &Value::Undefined);
                        match eval_yield_inner_expr(ctx, &inner_eval_env, yield_kind, &inner_expr_box) {
                            Ok(mut val) => {
                                if yield_kind == crate::js_generator::YieldKind::YieldStar {
                                    let mut should_await = false;
                                    let mut promise_ref_opt = None;
                                    match &val {
                                        Value::Object(o) => {
                                            if let Some(p) = crate::js_promise::get_promise_from_js_object(o) {
                                                should_await = true;
                                                promise_ref_opt = Some(p);
                                            }
                                        }
                                        Value::Promise(p) => {
                                            should_await = true;
                                            promise_ref_opt = Some(*p);
                                        }
                                        _ => {}
                                    }

                                    if should_await {
                                        let promise = promise_ref_opt.unwrap();
                                        crate::js_promise::mark_promise_handled(ctx, promise, env).ok();
                                        let state = promise.borrow().state.clone();
                                        match state {
                                            crate::core::PromiseState::Pending => {
                                                let gen_val = Value::AsyncGenerator(gen_ptr);
                                                let promise_cell_val = Value::Promise(promise_cell);

                                                let on_fulfilled = Value::Closure(crate::core::Gc::new(
                                                    ctx,
                                                    ClosureData::new(
                                                        &[crate::core::DestructuringElement::Variable("value".to_string(), None)],
                                                        &[Statement::from(StatementKind::Expr(Expr::Call(
                                                            Box::new(Expr::Var(
                                                                "__internal_async_gen_await_resolve".to_string(),
                                                                None,
                                                                None,
                                                            )),
                                                            vec![
                                                                Expr::Var("value".to_string(), None, None),
                                                                Expr::Var("__gen".to_string(), None, None),
                                                                Expr::Var("__p".to_string(), None, None),
                                                            ],
                                                        )))],
                                                        {
                                                            let e = new_js_object_data(ctx);
                                                            e.borrow_mut(ctx).prototype = Some(*env);
                                                            slot_set(ctx, &e, InternalSlot::Gen, &gen_val.clone());
                                                            slot_set(ctx, &e, InternalSlot::P, &promise_cell_val.clone());
                                                            Some(e)
                                                        },
                                                        None,
                                                    ),
                                                ));

                                                let on_rejected = Value::Closure(crate::core::Gc::new(
                                                    ctx,
                                                    ClosureData::new(
                                                        &[crate::core::DestructuringElement::Variable("reason".to_string(), None)],
                                                        &[Statement::from(StatementKind::Expr(Expr::Call(
                                                            Box::new(Expr::Var(
                                                                "__internal_async_gen_await_reject".to_string(),
                                                                None,
                                                                None,
                                                            )),
                                                            vec![
                                                                Expr::Var("reason".to_string(), None, None),
                                                                Expr::Var("__gen".to_string(), None, None),
                                                                Expr::Var("__p".to_string(), None, None),
                                                            ],
                                                        )))],
                                                        {
                                                            let e = new_js_object_data(ctx);
                                                            e.borrow_mut(ctx).prototype = Some(*env);
                                                            slot_set(ctx, &e, InternalSlot::Gen, &gen_val);
                                                            slot_set(ctx, &e, InternalSlot::P, &promise_cell_val);
                                                            Some(e)
                                                        },
                                                        None,
                                                    ),
                                                ));

                                                perform_promise_then(ctx, promise, Some(on_fulfilled), Some(on_rejected), None, env)?;
                                                return Ok(());
                                            }
                                            crate::core::PromiseState::Fulfilled(v) => {
                                                val = v;
                                            }
                                            crate::core::PromiseState::Rejected(r) => {
                                                gen_ptr_mut.state = GeneratorState::Completed;
                                                reject_promise(ctx, &promise_cell, r, env);
                                                return Ok(());
                                            }
                                        }
                                    }

                                    let (iter_obj, _) = match get_for_await_iterator(ctx, env, &val) {
                                        Ok(v) => v,
                                        Err(EvalError::Throw(v, _, _)) => {
                                            gen_ptr_mut.state = GeneratorState::Completed;
                                            reject_promise(ctx, &promise_cell, v, env);
                                            return Ok(());
                                        }
                                        Err(e) => {
                                            gen_ptr_mut.state = GeneratorState::Completed;
                                            reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                                            return Ok(());
                                        }
                                    };
                                    gen_ptr_mut.yield_star_iterator = Some(iter_obj);
                                    drop(gen_ptr_mut_guard);

                                    handle_yield_star_call(ctx, env, gen_ptr, promise_cell, iter_obj, "next", vec![Value::Undefined])?;
                                    return Ok(());
                                }

                                // Helper to immediately resume if Await on non-promise
                                if matches!(
                                    yield_kind,
                                    crate::js_generator::YieldKind::Await | crate::js_generator::YieldKind::Yield
                                ) {
                                    let mut should_await = false;
                                    let mut promise_ref_opt = None;
                                    match &val {
                                        Value::Object(o) => {
                                            if let Some(p) = crate::js_promise::get_promise_from_js_object(o) {
                                                should_await = true;
                                                promise_ref_opt = Some(p);
                                            }
                                        }
                                        Value::Promise(p) => {
                                            should_await = true;
                                            promise_ref_opt = Some(*p);
                                        }
                                        _ => {}
                                    }

                                    if should_await {
                                        // It is a promise. Check state.
                                        let promise = promise_ref_opt.unwrap();
                                        crate::js_promise::mark_promise_handled(ctx, promise, env).ok();

                                        let state = promise.borrow().state.clone();
                                        match state {
                                            crate::core::PromiseState::Pending => {
                                                // SUSPEND

                                                let (resolve_name, reject_name) = if yield_kind == crate::js_generator::YieldKind::Yield {
                                                    ("__internal_async_gen_yield_resolve", "__internal_async_gen_yield_reject")
                                                } else {
                                                    ("__internal_async_gen_await_resolve", "__internal_async_gen_await_reject")
                                                };

                                                // Prepare args for internal helpers
                                                let gen_val = Value::AsyncGenerator(gen_ptr);
                                                let promise_cell_val = Value::Promise(promise_cell);

                                                // Create on_fulfilled callback
                                                let on_fulfilled = Value::Closure(crate::core::Gc::new(
                                                    ctx,
                                                    ClosureData::new(
                                                        &[crate::core::DestructuringElement::Variable("value".to_string(), None)],
                                                        &[Statement::from(StatementKind::Expr(Expr::Call(
                                                            Box::new(Expr::Var(resolve_name.to_string(), None, None)),
                                                            vec![
                                                                Expr::Var("value".to_string(), None, None),
                                                                Expr::Var("__gen".to_string(), None, None),
                                                                Expr::Var("__p".to_string(), None, None),
                                                            ],
                                                        )))],
                                                        {
                                                            let e = new_js_object_data(ctx);
                                                            e.borrow_mut(ctx).prototype = Some(*env);
                                                            slot_set(ctx, &e, InternalSlot::Gen, &gen_val.clone());
                                                            slot_set(ctx, &e, InternalSlot::P, &promise_cell_val.clone());
                                                            Some(e)
                                                        },
                                                        None,
                                                    ),
                                                ));

                                                let on_rejected = Value::Closure(crate::core::Gc::new(
                                                    ctx,
                                                    ClosureData::new(
                                                        &[crate::core::DestructuringElement::Variable("reason".to_string(), None)],
                                                        &[Statement::from(StatementKind::Expr(Expr::Call(
                                                            Box::new(Expr::Var(reject_name.to_string(), None, None)),
                                                            vec![
                                                                Expr::Var("reason".to_string(), None, None),
                                                                Expr::Var("__gen".to_string(), None, None),
                                                                Expr::Var("__p".to_string(), None, None),
                                                            ],
                                                        )))],
                                                        {
                                                            let e = new_js_object_data(ctx);
                                                            e.borrow_mut(ctx).prototype = Some(*env);
                                                            slot_set(ctx, &e, InternalSlot::Gen, &gen_val);
                                                            slot_set(ctx, &e, InternalSlot::P, &promise_cell_val);
                                                            Some(e)
                                                        },
                                                        None,
                                                    ),
                                                ));

                                                perform_promise_then(ctx, promise, Some(on_fulfilled), Some(on_rejected), None, env)?;
                                                return Ok(());
                                            }
                                            crate::core::PromiseState::Fulfilled(v) => {
                                                val = v; // continue as if awaited
                                            }
                                            crate::core::PromiseState::Rejected(r) => {
                                                gen_ptr_mut.state = GeneratorState::Completed;
                                                reject_promise(ctx, &promise_cell, r, env);
                                                return Ok(());
                                            }
                                        }
                                    }
                                }
                                // Treat val as the result
                                gen_ptr_mut.cached_initial_yield = Some(val.clone());
                                let res_obj = create_iterator_result_obj(ctx, val, false)?;
                                resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                                return Ok(());
                            }
                            Err(_) => {
                                gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                                let res_obj = create_iterator_result_obj(ctx, Value::Undefined, false)?;
                                resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                                return Ok(());
                            }
                        }
                    }

                    gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                    let res_obj = create_iterator_result_obj(ctx, Value::Undefined, false)?;
                    resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                    return Ok(());
                } else {
                    // No yields: run to completion and keep processing next requests
                    let func_env = if let Some(prep_env) = gen_ptr_mut.call_env.take() {
                        prep_env
                    } else {
                        let env = prepare_function_call_env(
                            ctx,
                            Some(&gen_ptr_mut.env),
                            None,
                            Some(&gen_ptr_mut.params[..]),
                            &gen_ptr_mut.args,
                            None,
                            None,
                        )?;

                        // Ensure `arguments` exists for the no-yield completion path too.
                        crate::js_class::create_arguments_object(ctx, &env, &gen_ptr_mut.args, None)?;
                        env
                    };

                    match evaluate_async_generator_completion(ctx, &func_env, &gen_ptr_mut.body) {
                        Ok(AsyncGeneratorCompletion::Normal) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                            resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                        Ok(AsyncGeneratorCompletion::Return(v)) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            let res_obj = create_iterator_result_obj(ctx, v, true)?;
                            resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                        Err(e) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                            continue;
                        }
                    }
                }
            }
            (GeneratorState::NotStarted, AsyncGeneratorRequest::Throw(throw_val)) => {
                reject_promise(ctx, &promise_cell, throw_val, env);
                continue;
            }
            (GeneratorState::NotStarted, AsyncGeneratorRequest::Return(ret_val)) => {
                gen_ptr_mut.state = GeneratorState::Completed;
                let res_obj = create_iterator_result_obj(ctx, ret_val, true)?;
                resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                continue;
            }
            (GeneratorState::Suspended { pc, pre_env, .. }, AsyncGeneratorRequest::Next(_send_value)) => {
                if let Some(for_await) = gen_ptr_mut.pending_for_await.clone() {
                    let func_env = if let Some(env) = pre_env.as_ref() {
                        *env
                    } else {
                        crate::core::prepare_function_call_env(ctx, Some(&gen_ptr_mut.env), None, None, &[], None, None)?
                    };

                    match for_await_next_value(ctx, &func_env, for_await.iterator, for_await.is_async) {
                        Ok(Some(value)) => {
                            let iter_env = if let Some(VarDeclKind::Let) | Some(VarDeclKind::Const) = for_await.decl_kind {
                                let e = new_js_object_data(ctx);
                                e.borrow_mut(ctx).prototype = Some(func_env);
                                env_set(ctx, &e, &for_await.var_name, &value.clone())?;
                                e
                            } else {
                                env_set_recursive(ctx, &func_env, &for_await.var_name, &value.clone())?;
                                func_env
                            };

                            let mut yielded = evaluate_expr(ctx, &iter_env, &for_await.yield_expr)?;
                            match await_value(ctx, env, yielded.clone()) {
                                Ok(awaited) => {
                                    yielded = awaited;
                                }
                                Err(reason) => {
                                    gen_ptr_mut.state = GeneratorState::Completed;
                                    gen_ptr_mut.pending_for_await = None;
                                    reject_promise(ctx, &promise_cell, reason, env);
                                    continue;
                                }
                            }

                            gen_ptr_mut.pending_for_await = Some(for_await);
                            let res_obj = create_iterator_result_obj(ctx, yielded, false)?;
                            resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                        Ok(None) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            gen_ptr_mut.pending_for_await = None;
                            let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                            resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                        Err(e) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            gen_ptr_mut.pending_for_await = None;
                            reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                            continue;
                        }
                    }
                }

                // Handle regular For loop with yield in body (Suspended / resume)
                let pc_val = *pc;
                if pc_val < gen_ptr_mut.body.len() && matches!(&*gen_ptr_mut.body[pc_val].kind, StatementKind::For(_)) {
                    let func_env = if let Some(env) = pre_env.as_ref() {
                        *env
                    } else {
                        crate::core::prepare_function_call_env(ctx, Some(&gen_ptr_mut.env), None, None, &[], None, None)?
                    };

                    // Execute init if it still exists (first resume after NotStarted may have cleared it)
                    if let StatementKind::For(for_stmt_m) = gen_ptr_mut.body[pc_val].kind.as_mut()
                        && let Some(init_stmt) = for_stmt_m.init.take()
                    {
                        crate::core::evaluate_statements(ctx, &func_env, std::slice::from_ref(&init_stmt))?;
                    }

                    // Re-borrow immutably for test/update/body access
                    let (test_expr_clone, update_stmt_clone, body_clone) = if let StatementKind::For(fs) = &*gen_ptr_mut.body[pc_val].kind {
                        (fs.test.clone(), fs.update.clone(), fs.body.clone())
                    } else {
                        unreachable!()
                    };

                    // Execute update
                    if let Some(update_stmt) = &update_stmt_clone {
                        crate::core::evaluate_statements(ctx, &func_env, std::slice::from_ref(update_stmt))?;
                    }

                    // Check test condition
                    if let Some(test_expr) = &test_expr_clone {
                        let test_val = crate::core::evaluate_expr(ctx, &func_env, test_expr)?;
                        if !test_val.to_truthy() {
                            // Loop is done — run remaining post-loop statements
                            if pc_val + 1 < gen_ptr_mut.body.len() {
                                let post_stmts = gen_ptr_mut.body[pc_val + 1..].to_vec();
                                drop(gen_ptr_mut_guard);
                                match crate::core::evaluate_statements(ctx, &func_env, &post_stmts) {
                                    Ok(_) => {
                                        gen_ptr.borrow_mut(ctx).state = GeneratorState::Completed;
                                        let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                                        return Ok(());
                                    }
                                    Err(e) => {
                                        gen_ptr.borrow_mut(ctx).state = GeneratorState::Completed;
                                        reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                                        return Ok(());
                                    }
                                }
                            }
                            gen_ptr_mut.state = GeneratorState::Completed;
                            let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                            resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                    }

                    // Find yield in for-body and evaluate
                    if let Some((body_yield_idx, _, _yield_kind, yield_inner_opt)) =
                        crate::js_generator::find_first_yield_in_statements(&body_clone)
                    {
                        // Execute pre-yield body statements
                        if body_yield_idx > 0 {
                            let pre_stmts = body_clone[0..body_yield_idx].to_vec();
                            crate::core::evaluate_statements(ctx, &func_env, &pre_stmts)?;
                        }

                        // Evaluate yield expression
                        let yielded = if let Some(inner_expr) = yield_inner_opt {
                            match crate::core::evaluate_expr(ctx, &func_env, &inner_expr) {
                                Ok(v) => v,
                                Err(e) => {
                                    gen_ptr_mut.state = GeneratorState::Completed;
                                    reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                                    return Ok(());
                                }
                            }
                        } else {
                            Value::Undefined
                        };

                        gen_ptr_mut.state = GeneratorState::Suspended {
                            pc: pc_val,
                            stack: vec![],
                            pre_env: Some(func_env),
                        };
                        gen_ptr_mut.cached_initial_yield = Some(yielded.clone());
                        let res_obj = create_iterator_result_obj(ctx, yielded, false)?;
                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }

                    // No more yields in for body — run to completion
                    gen_ptr_mut.state = GeneratorState::Completed;
                    let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                    resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                    continue;
                }

                // Resume execution from pc: run remaining tail to completion, but
                // first substitute the `yield` with the provided send value (or
                // cached initial yield) so the suspended point receives the value.
                let pc_val = *pc;
                let original_tail: Vec<crate::core::Statement> = if pc_val < gen_ptr_mut.body.len() {
                    gen_ptr_mut.body[pc_val..].to_vec()
                } else {
                    vec![]
                };
                let mut tail = original_tail.clone();
                let allow_fallback = original_tail.first().is_some_and(allow_loop_fallback);

                // Generate a unique variable name for this yield point (based on PC and count)
                let base_name = format!("__gen_yield_val_{}_", pc_val);
                let next_idx = if let Some(s) = tail.first() {
                    crate::js_generator::count_yield_vars_in_statement(s, &base_name)
                } else {
                    0
                };
                let var_name = format!("{}{}", base_name, next_idx);

                // Replace first yield occurrence in the suspended statement so we
                // don't re-yield the same value on subsequent resumes.
                let mut replaced = false;
                if let Some(first_stmt) = tail.get_mut(0) {
                    crate::js_generator::replace_first_yield_in_statement(first_stmt, &var_name, &mut replaced);
                }
                if !allow_fallback {
                    let mut replaced_body = false;
                    if let Some(body_stmt) = gen_ptr_mut.body.get_mut(pc_val) {
                        crate::js_generator::replace_first_yield_in_statement(body_stmt, &var_name, &mut replaced_body);
                    }
                }

                // Use the pre-execution environment if available so bindings created
                // by pre-statements remain visible when we resume execution.
                let func_env = if let Some(env) = pre_env.as_ref() {
                    *env
                } else {
                    crate::core::prepare_function_call_env(ctx, Some(&gen_ptr_mut.env), None, None, &[], None, None)?
                };

                // Prefer the queued send value if it is concrete; otherwise fall back
                // to the cached initially-yielded value if present.
                env_set(ctx, &func_env, &var_name, &_send_value.clone())?;

                if let Some((idx, inner_idx_opt, yield_kind, yield_inner)) = crate::js_generator::find_first_yield_in_statements(&tail) {
                    if idx > 0 {
                        let pre_stmts = tail[0..idx].to_vec();
                        let _ = crate::core::evaluate_statements(ctx, &func_env, &pre_stmts)?;
                    } else if let Some(inner_idx) = inner_idx_opt
                        && inner_idx > 0
                        && let StatementKind::Block(inner_stmts) = &*tail[idx].kind
                    {
                        let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                        let _ = crate::core::evaluate_statements(ctx, &func_env, &pre_stmts)?;
                    }

                    // If the yield is inside a while loop, ensure the loop condition
                    // is still true before yielding again.
                    if let Some(stmt) = tail.get(idx)
                        && let StatementKind::While(while_stmt, _) = &*stmt.kind
                    {
                        let cond_val = crate::core::evaluate_expr(ctx, &func_env, while_stmt)?;
                        let cond_bool = cond_val.to_truthy();
                        if !cond_bool {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                            resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                    }

                    gen_ptr_mut.state = GeneratorState::Suspended {
                        pc: pc_val + idx,
                        stack: vec![],
                        pre_env: Some(func_env),
                    };

                    if let Some(inner_expr_box) = yield_inner {
                        let parent_env = &func_env;
                        let inner_eval_env = crate::core::prepare_function_call_env(ctx, Some(parent_env), None, None, &[], None, None)?;
                        slot_set(ctx, &inner_eval_env, InternalSlot::GenThrowVal, &Value::Undefined);
                        match eval_yield_inner_expr(ctx, &inner_eval_env, yield_kind, &inner_expr_box) {
                            Ok(mut val) => {
                                if yield_kind == crate::js_generator::YieldKind::YieldStar {
                                    let mut should_await = false;
                                    let mut promise_ref_opt = None;
                                    match &val {
                                        Value::Object(o) => {
                                            if let Some(p) = crate::js_promise::get_promise_from_js_object(o) {
                                                should_await = true;
                                                promise_ref_opt = Some(p);
                                            }
                                        }
                                        Value::Promise(p) => {
                                            should_await = true;
                                            promise_ref_opt = Some(*p);
                                        }
                                        _ => {}
                                    }

                                    if should_await {
                                        let promise = promise_ref_opt.unwrap();
                                        crate::js_promise::mark_promise_handled(ctx, promise, env).ok();
                                        let state = promise.borrow().state.clone();
                                        match state {
                                            crate::core::PromiseState::Pending => {
                                                let gen_val = Value::AsyncGenerator(gen_ptr);
                                                let promise_cell_val = Value::Promise(promise_cell);

                                                let on_fulfilled = Value::Closure(crate::core::Gc::new(
                                                    ctx,
                                                    ClosureData::new(
                                                        &[crate::core::DestructuringElement::Variable("value".to_string(), None)],
                                                        &[Statement::from(StatementKind::Expr(Expr::Call(
                                                            Box::new(Expr::Var(
                                                                "__internal_async_gen_await_resolve".to_string(),
                                                                None,
                                                                None,
                                                            )),
                                                            vec![
                                                                Expr::Var("value".to_string(), None, None),
                                                                Expr::Var("__gen".to_string(), None, None),
                                                                Expr::Var("__p".to_string(), None, None),
                                                            ],
                                                        )))],
                                                        {
                                                            let e = new_js_object_data(ctx);
                                                            e.borrow_mut(ctx).prototype = Some(*env);
                                                            slot_set(ctx, &e, InternalSlot::Gen, &gen_val.clone());
                                                            slot_set(ctx, &e, InternalSlot::P, &promise_cell_val.clone());
                                                            Some(e)
                                                        },
                                                        None,
                                                    ),
                                                ));

                                                let on_rejected = Value::Closure(crate::core::Gc::new(
                                                    ctx,
                                                    ClosureData::new(
                                                        &[crate::core::DestructuringElement::Variable("reason".to_string(), None)],
                                                        &[Statement::from(StatementKind::Expr(Expr::Call(
                                                            Box::new(Expr::Var(
                                                                "__internal_async_gen_await_reject".to_string(),
                                                                None,
                                                                None,
                                                            )),
                                                            vec![
                                                                Expr::Var("reason".to_string(), None, None),
                                                                Expr::Var("__gen".to_string(), None, None),
                                                                Expr::Var("__p".to_string(), None, None),
                                                            ],
                                                        )))],
                                                        {
                                                            let e = new_js_object_data(ctx);
                                                            e.borrow_mut(ctx).prototype = Some(*env);
                                                            slot_set(ctx, &e, InternalSlot::Gen, &gen_val);
                                                            slot_set(ctx, &e, InternalSlot::P, &promise_cell_val);
                                                            Some(e)
                                                        },
                                                        None,
                                                    ),
                                                ));

                                                perform_promise_then(ctx, promise, Some(on_fulfilled), Some(on_rejected), None, env)?;
                                                return Ok(());
                                            }
                                            crate::core::PromiseState::Fulfilled(v) => {
                                                val = v;
                                            }
                                            crate::core::PromiseState::Rejected(r) => {
                                                gen_ptr_mut.state = GeneratorState::Completed;
                                                reject_promise(ctx, &promise_cell, r, env);
                                                return Ok(());
                                            }
                                        }
                                    }

                                    let (iter_obj, _) = match get_for_await_iterator(ctx, env, &val) {
                                        Ok(v) => v,
                                        Err(EvalError::Throw(v, _, _)) => {
                                            gen_ptr_mut.state = GeneratorState::Completed;
                                            reject_promise(ctx, &promise_cell, v, env);
                                            return Ok(());
                                        }
                                        Err(e) => {
                                            gen_ptr_mut.state = GeneratorState::Completed;
                                            reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                                            return Ok(());
                                        }
                                    };
                                    gen_ptr_mut.yield_star_iterator = Some(iter_obj);
                                    drop(gen_ptr_mut_guard);

                                    handle_yield_star_call(ctx, env, gen_ptr, promise_cell, iter_obj, "next", vec![Value::Undefined])?;
                                    return Ok(());
                                }
                                // Helper to immediately resume if Await on non-promise
                                if matches!(yield_kind, YieldKind::Await | YieldKind::Yield) {
                                    let mut should_await = false;
                                    let mut promise_ref_opt = None;
                                    match &val {
                                        Value::Object(o) => {
                                            if let Some(p) = crate::js_promise::get_promise_from_js_object(o) {
                                                should_await = true;
                                                promise_ref_opt = Some(p);
                                            }
                                        }
                                        Value::Promise(p) => {
                                            should_await = true;
                                            promise_ref_opt = Some(*p);
                                        }
                                        _ => {}
                                    }

                                    if should_await {
                                        // It is a promise. Check state.
                                        let promise = promise_ref_opt.unwrap();
                                        crate::js_promise::mark_promise_handled(ctx, promise, env).ok();

                                        let state = promise.borrow().state.clone();
                                        match state {
                                            crate::core::PromiseState::Pending => {
                                                // SUSPEND
                                                let (resolve_name, reject_name) = if yield_kind == YieldKind::Yield {
                                                    ("__internal_async_gen_yield_resolve", "__internal_async_gen_yield_reject")
                                                } else {
                                                    ("__internal_async_gen_await_resolve", "__internal_async_gen_await_reject")
                                                };
                                                let gen_val = Value::AsyncGenerator(gen_ptr);
                                                let promise_cell_val = Value::Promise(promise_cell);

                                                // Create on_fulfilled callback
                                                let on_fulfilled = Value::Closure(crate::core::Gc::new(
                                                    ctx,
                                                    ClosureData::new(
                                                        &[crate::core::DestructuringElement::Variable("value".to_string(), None)],
                                                        &[Statement::from(StatementKind::Expr(Expr::Call(
                                                            Box::new(Expr::Var(resolve_name.to_string(), None, None)),
                                                            vec![
                                                                Expr::Var("value".to_string(), None, None),
                                                                Expr::Var("__gen".to_string(), None, None),
                                                                Expr::Var("__p".to_string(), None, None),
                                                            ],
                                                        )))],
                                                        {
                                                            let e = new_js_object_data(ctx);
                                                            e.borrow_mut(ctx).prototype = Some(*env);
                                                            slot_set(ctx, &e, InternalSlot::Gen, &gen_val.clone());
                                                            slot_set(ctx, &e, InternalSlot::P, &promise_cell_val.clone());
                                                            Some(e)
                                                        },
                                                        None,
                                                    ),
                                                ));

                                                let on_rejected = Value::Closure(crate::core::Gc::new(
                                                    ctx,
                                                    ClosureData::new(
                                                        &[crate::core::DestructuringElement::Variable("reason".to_string(), None)],
                                                        &[Statement::from(StatementKind::Expr(Expr::Call(
                                                            Box::new(Expr::Var(reject_name.to_string(), None, None)),
                                                            vec![
                                                                Expr::Var("reason".to_string(), None, None),
                                                                Expr::Var("__gen".to_string(), None, None),
                                                                Expr::Var("__p".to_string(), None, None),
                                                            ],
                                                        )))],
                                                        {
                                                            let e = new_js_object_data(ctx);
                                                            e.borrow_mut(ctx).prototype = Some(*env);
                                                            slot_set(ctx, &e, InternalSlot::Gen, &gen_val);
                                                            slot_set(ctx, &e, InternalSlot::P, &promise_cell_val);
                                                            Some(e)
                                                        },
                                                        None,
                                                    ),
                                                ));

                                                perform_promise_then(ctx, promise, Some(on_fulfilled), Some(on_rejected), None, env)?;
                                                return Ok(());
                                            }
                                            crate::core::PromiseState::Fulfilled(v) => {
                                                val = v; // continue as if awaited
                                            }
                                            crate::core::PromiseState::Rejected(r) => {
                                                gen_ptr_mut.state = GeneratorState::Completed;
                                                reject_promise(ctx, &promise_cell, r, env);
                                                return Ok(());
                                            }
                                        }
                                    }
                                }
                                match await_value(ctx, env, val.clone()) {
                                    Ok(awaited) => {
                                        gen_ptr_mut.cached_initial_yield = Some(awaited.clone());
                                        let res_obj = create_iterator_result_obj(ctx, awaited, false)?;
                                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                                        continue;
                                    }
                                    Err(reason) => {
                                        gen_ptr_mut.state = GeneratorState::Completed;
                                        reject_promise(ctx, &promise_cell, reason, env);
                                        continue;
                                    }
                                }
                            }
                            Err(_) => {
                                gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                                let res_obj = create_iterator_result_obj(ctx, Value::Undefined, false)?;
                                resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                                continue;
                            }
                        }
                    }

                    gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                    let res_obj = create_iterator_result_obj(ctx, Value::Undefined, false)?;
                    resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                    continue;
                }

                // If we replaced the first yield but no yields remain, and no concrete send value
                // was provided, fall back to evaluating the original yield expression. This
                // prevents infinite loops for loop-based generators whose next iteration relies
                // on the yield expression's side effects (e.g. `current--`).
                let yield_already_replaced = replaced || next_idx > 0;

                if allow_fallback
                    && yield_already_replaced
                    && matches!(_send_value, Value::Undefined)
                    && let Some((idx, inner_idx_opt, yield_kind, yield_inner)) =
                        crate::js_generator::find_first_yield_in_statements(&original_tail)
                {
                    if idx > 0 {
                        let pre_stmts = original_tail[0..idx].to_vec();
                        let _ = crate::core::evaluate_statements(ctx, &func_env, &pre_stmts)?;
                    } else if let Some(inner_idx) = inner_idx_opt
                        && inner_idx > 0
                        && let StatementKind::Block(inner_stmts) = &*original_tail[idx].kind
                    {
                        let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                        let _ = crate::core::evaluate_statements(ctx, &func_env, &pre_stmts)?;
                    }

                    if let Some(stmt) = original_tail.get(idx)
                        && let StatementKind::While(while_stmt, _) = &*stmt.kind
                    {
                        let cond_val = crate::core::evaluate_expr(ctx, &func_env, while_stmt)?;
                        let cond_bool = cond_val.to_truthy();
                        if !cond_bool {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                            resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                    }

                    gen_ptr_mut.state = GeneratorState::Suspended {
                        pc: pc_val + idx,
                        stack: vec![],
                        pre_env: Some(func_env),
                    };

                    if let Some(inner_expr_box) = yield_inner {
                        let parent_env = &func_env;
                        let inner_eval_env = crate::core::prepare_function_call_env(ctx, Some(parent_env), None, None, &[], None, None)?;
                        slot_set(ctx, &inner_eval_env, InternalSlot::GenThrowVal, &Value::Undefined);
                        match eval_yield_inner_expr(ctx, &inner_eval_env, yield_kind, &inner_expr_box) {
                            Ok(mut val) => {
                                if yield_kind == YieldKind::YieldStar {
                                    let (iter_obj, _) = match get_for_await_iterator(ctx, env, &val) {
                                        Ok(v) => v,
                                        Err(EvalError::Throw(v, _, _)) => {
                                            gen_ptr_mut.state = GeneratorState::Completed;
                                            reject_promise(ctx, &promise_cell, v, env);
                                            return Ok(());
                                        }
                                        Err(e) => {
                                            gen_ptr_mut.state = GeneratorState::Completed;
                                            reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                                            return Ok(());
                                        }
                                    };
                                    gen_ptr_mut.yield_star_iterator = Some(iter_obj);
                                    drop(gen_ptr_mut_guard);

                                    handle_yield_star_call(ctx, env, gen_ptr, promise_cell, iter_obj, "next", vec![Value::Undefined])?;
                                    return Ok(());
                                }
                                // Helper to immediately resume if Await on non-promise
                                if matches!(yield_kind, YieldKind::Await | YieldKind::Yield) {
                                    let mut should_await = false;
                                    let mut promise_ref_opt = None;
                                    match &val {
                                        Value::Object(o) => {
                                            if let Some(p) = crate::js_promise::get_promise_from_js_object(o) {
                                                should_await = true;
                                                promise_ref_opt = Some(p);
                                            }
                                        }
                                        Value::Promise(p) => {
                                            should_await = true;
                                            promise_ref_opt = Some(*p);
                                        }
                                        _ => {}
                                    }

                                    if should_await {
                                        // It is a promise. Check state.
                                        let promise = promise_ref_opt.unwrap();
                                        crate::js_promise::mark_promise_handled(ctx, promise, env).ok();

                                        let state = promise.borrow().state.clone();
                                        match state {
                                            crate::core::PromiseState::Pending => {
                                                // SUSPEND
                                                let (resolve_name, reject_name) = if yield_kind == crate::js_generator::YieldKind::Yield {
                                                    ("__internal_async_gen_yield_resolve", "__internal_async_gen_yield_reject")
                                                } else {
                                                    ("__internal_async_gen_await_resolve", "__internal_async_gen_await_reject")
                                                };
                                                let gen_val = Value::AsyncGenerator(gen_ptr);
                                                let promise_cell_val = Value::Promise(promise_cell);

                                                // Create on_fulfilled callback
                                                let on_fulfilled = Value::Closure(Gc::new(
                                                    ctx,
                                                    ClosureData::new(
                                                        &[crate::core::DestructuringElement::Variable("value".to_string(), None)],
                                                        &[Statement::from(StatementKind::Expr(Expr::Call(
                                                            Box::new(Expr::Var(resolve_name.to_string(), None, None)),
                                                            vec![
                                                                Expr::Var("value".to_string(), None, None),
                                                                Expr::Var("__gen".to_string(), None, None),
                                                                Expr::Var("__p".to_string(), None, None),
                                                            ],
                                                        )))],
                                                        {
                                                            let e = new_js_object_data(ctx);
                                                            e.borrow_mut(ctx).prototype = Some(*env);
                                                            slot_set(ctx, &e, InternalSlot::Gen, &gen_val.clone());
                                                            slot_set(ctx, &e, InternalSlot::P, &promise_cell_val.clone());
                                                            Some(e)
                                                        },
                                                        None,
                                                    ),
                                                ));

                                                let on_rejected = Value::Closure(Gc::new(
                                                    ctx,
                                                    ClosureData::new(
                                                        &[crate::core::DestructuringElement::Variable("reason".to_string(), None)],
                                                        &[Statement::from(StatementKind::Expr(Expr::Call(
                                                            Box::new(Expr::Var(reject_name.to_string(), None, None)),
                                                            vec![
                                                                Expr::Var("reason".to_string(), None, None),
                                                                Expr::Var("__gen".to_string(), None, None),
                                                                Expr::Var("__p".to_string(), None, None),
                                                            ],
                                                        )))],
                                                        {
                                                            let e = new_js_object_data(ctx);
                                                            e.borrow_mut(ctx).prototype = Some(*env);
                                                            slot_set(ctx, &e, InternalSlot::Gen, &gen_val);
                                                            slot_set(ctx, &e, InternalSlot::P, &promise_cell_val);
                                                            Some(e)
                                                        },
                                                        None,
                                                    ),
                                                ));

                                                perform_promise_then(ctx, promise, Some(on_fulfilled), Some(on_rejected), None, env)?;
                                                return Ok(());
                                            }
                                            crate::core::PromiseState::Fulfilled(v) => {
                                                val = v; // continue as if awaited
                                            }
                                            crate::core::PromiseState::Rejected(r) => {
                                                gen_ptr_mut.state = GeneratorState::Completed;
                                                reject_promise(ctx, &promise_cell, r, env);
                                                return Ok(());
                                            }
                                        }
                                    }
                                }
                                match await_value(ctx, env, val.clone()) {
                                    Ok(awaited) => {
                                        gen_ptr_mut.cached_initial_yield = Some(awaited.clone());
                                        let res_obj = create_iterator_result_obj(ctx, awaited, false)?;
                                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                                        continue;
                                    }
                                    Err(reason) => {
                                        gen_ptr_mut.state = GeneratorState::Completed;
                                        reject_promise(ctx, &promise_cell, reason, env);
                                        continue;
                                    }
                                }
                            }
                            Err(_) => {
                                gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                                let res_obj = create_iterator_result_obj(ctx, Value::Undefined, false)?;
                                resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                                continue;
                            }
                        }
                    }

                    gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                    let res_obj = create_iterator_result_obj(ctx, Value::Undefined, false)?;
                    resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                    continue;
                }

                // No further yields: execute tail to completion
                match evaluate_async_generator_completion(ctx, &func_env, &tail) {
                    Ok(AsyncGeneratorCompletion::Normal) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    Ok(AsyncGeneratorCompletion::Return(v)) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        let res_obj = create_iterator_result_obj(ctx, v, true)?;
                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    Err(e) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                        continue;
                    }
                }
            }
            (GeneratorState::Suspended { pc, pre_env, .. }, AsyncGeneratorRequest::Throw(throw_val)) => {
                // Resume by throwing into the suspended point: replace first yield with a Throw
                let pc_val = *pc;
                if pc_val >= gen_ptr_mut.body.len() {
                    gen_ptr_mut.state = GeneratorState::Completed;
                    reject_promise(ctx, &promise_cell, throw_val, env);
                    continue;
                }
                let mut tail: Vec<Statement> = gen_ptr_mut.body[pc_val..].to_vec();
                let mut replaced = false;
                for s in tail.iter_mut() {
                    if crate::js_generator::replace_first_yield_statement_with_throw(s, &throw_val) {
                        replaced = true;
                        break;
                    }
                }
                if !replaced {
                    tail[0] = StatementKind::Throw(Expr::Var("__gen_throw_val".to_string(), None, None)).into();
                }

                let func_env = if let Some(env) = pre_env.as_ref() {
                    *env
                } else {
                    crate::core::prepare_function_call_env(ctx, Some(&gen_ptr_mut.env), None, None, &[], None, None)?
                };
                slot_set(ctx, &func_env, InternalSlot::GenThrowVal, &throw_val.clone());

                match evaluate_async_generator_completion(ctx, &func_env, &tail) {
                    Ok(AsyncGeneratorCompletion::Normal) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    Ok(AsyncGeneratorCompletion::Return(v)) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        let res_obj = create_iterator_result_obj(ctx, v, true)?;
                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    Err(e) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                        continue;
                    }
                }
            }
            (GeneratorState::Suspended { pc, pre_env, .. }, AsyncGeneratorRequest::Return(ret_val)) => {
                // Inject a `return __gen_throw_val` into the suspended point so
                // that any `finally` blocks execute and the generator completes
                // in a spec-like manner.
                let pc_val = *pc;
                if pc_val >= gen_ptr_mut.body.len() {
                    gen_ptr_mut.state = GeneratorState::Completed;
                    let res_obj = create_iterator_result_obj(ctx, ret_val, true)?;
                    resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                    continue;
                }

                let mut tail: Vec<Statement> = gen_ptr_mut.body[pc_val..].to_vec();
                let mut replaced = false;
                for s in tail.iter_mut() {
                    if crate::js_generator::replace_first_yield_statement_with_return(s) {
                        replaced = true;
                        break;
                    }
                }
                if !replaced {
                    tail[0] = StatementKind::Return(Some(Expr::Var("__gen_throw_val".to_string(), None, None))).into();
                }

                let func_env = if let Some(env) = pre_env.as_ref() {
                    *env
                } else {
                    crate::core::prepare_function_call_env(ctx, Some(&gen_ptr_mut.env), None, None, &[], None, None)?
                };
                slot_set(ctx, &func_env, InternalSlot::GenThrowVal, &ret_val.clone());

                match evaluate_async_generator_completion(ctx, &func_env, &tail) {
                    Ok(AsyncGeneratorCompletion::Normal) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    Ok(AsyncGeneratorCompletion::Return(v)) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        let res_obj = create_iterator_result_obj(ctx, v, true)?;
                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    Err(e) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                        continue;
                    }
                }
            }
            (GeneratorState::Running { .. }, _) => {
                // Shouldn't happen; reject the promise
                let reason = Value::from("Async generator already running");
                reject_promise(ctx, &promise_cell, reason, env);
                // continue processing remaining requests (unlikely)
                continue;
            }
            (GeneratorState::Completed, AsyncGeneratorRequest::Return(ret_val)) => {
                let promise_ctor = crate::core::env_get(env, "Promise")
                    .map(|rc| rc.borrow().clone())
                    .unwrap_or(Value::Undefined);

                let resolved = match crate::js_promise::handle_promise_static_method_val(
                    ctx,
                    "resolve",
                    std::slice::from_ref(&ret_val),
                    Some(&promise_ctor),
                    env,
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                        continue;
                    }
                };

                let resolved_promise = match resolved {
                    Value::Promise(p) => p,
                    Value::Object(obj) => {
                        if let Some(p) = crate::js_promise::get_promise_from_js_object(&obj) {
                            p
                        } else {
                            let res_obj = create_iterator_result_obj(ctx, ret_val, true)?;
                            resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                    }
                    _ => {
                        let res_obj = create_iterator_result_obj(ctx, ret_val, true)?;
                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                };

                match resolved_promise.borrow().state.clone() {
                    crate::core::PromiseState::Pending => {
                        let resolve_env = new_js_object_data(ctx);
                        resolve_env.borrow_mut(ctx).prototype = Some(*env);
                        env_set(ctx, &resolve_env, "__promise_cell", &Value::Promise(promise_cell))?;

                        let on_fulfilled = Value::Closure(crate::core::Gc::new(
                            ctx,
                            ClosureData::new(
                                &[crate::core::DestructuringElement::Variable("v".to_string(), None)],
                                &[Statement::from(StatementKind::Expr(Expr::Call(
                                    Box::new(Expr::Var("__internal_async_gen_return_resolve".to_string(), None, None)),
                                    vec![
                                        Expr::Var("v".to_string(), None, None),
                                        Expr::Var("__promise_cell".to_string(), None, None),
                                    ],
                                )))],
                                Some(resolve_env),
                                None,
                            ),
                        ));

                        let reject_env = new_js_object_data(ctx);
                        reject_env.borrow_mut(ctx).prototype = Some(*env);
                        env_set(ctx, &reject_env, "__promise_cell", &Value::Promise(promise_cell))?;

                        let on_rejected = Value::Closure(crate::core::Gc::new(
                            ctx,
                            ClosureData::new(
                                &[crate::core::DestructuringElement::Variable("reason".to_string(), None)],
                                &[Statement::from(StatementKind::Expr(Expr::Call(
                                    Box::new(Expr::Var("__internal_async_gen_return_reject".to_string(), None, None)),
                                    vec![
                                        Expr::Var("reason".to_string(), None, None),
                                        Expr::Var("__promise_cell".to_string(), None, None),
                                    ],
                                )))],
                                Some(reject_env),
                                None,
                            ),
                        ));

                        if let Err(e) = perform_promise_then(ctx, resolved_promise, Some(on_fulfilled), Some(on_rejected), None, env) {
                            reject_promise(ctx, &promise_cell, js_error_to_value(ctx, env, &e), env);
                        }
                        continue;
                    }
                    crate::core::PromiseState::Fulfilled(v) => {
                        let res_obj = create_iterator_result_obj(ctx, v, true)?;
                        resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    crate::core::PromiseState::Rejected(reason) => {
                        reject_promise(ctx, &promise_cell, reason, env);
                        continue;
                    }
                }
            }
            (GeneratorState::Completed, _) => {
                // Already completed: fulfill with done=true
                let res_obj = create_iterator_result_obj(ctx, Value::Undefined, true)?;
                resolve_promise(ctx, &promise_cell, Value::Object(res_obj), env);
                continue;
            }
        }
    }
}

/// Helper: create a rejected promise with a TypeError for bad `this` values.
/// Per spec, AsyncGeneratorEnqueue returns a rejected promise (not a thrown error)
/// when the generator argument is invalid.
fn reject_with_type_error<'gc>(ctx: &GcContext<'gc>, env: &JSObjectDataPtr<'gc>, message: &str) -> Result<Option<Value<'gc>>, JSError> {
    let (promise_cell, promise_obj_val) = create_promise_cell_and_obj(ctx, env);
    // Build a TypeError value from the current realm's TypeError constructor
    let err_val = {
        let msg_val: Value<'gc> = Value::from(message);
        let mut proto_opt: Option<JSObjectDataPtr<'gc>> = None;
        if let Some(err_ctor_val) = crate::core::env_get(env, "TypeError")
            && let Value::Object(err_ctor) = &*err_ctor_val.borrow()
            && let Some(proto_val) = object_get_key_value(err_ctor, "prototype")
            && let Value::Object(proto) = &*proto_val.borrow()
        {
            proto_opt = Some(*proto);
        }
        crate::core::create_error(ctx, proto_opt, &msg_val).unwrap_or(Value::from(message))
    };
    reject_promise(ctx, &promise_cell, err_val, env);
    Ok(Some(promise_obj_val))
}

/// Helper: enqueue a request on an async generator, using try_borrow_mut to
/// avoid panicking when the generator is already executing.
fn enqueue_async_generator_request<'gc>(
    ctx: &GcContext<'gc>,
    gen_ptr: GcPtr<'gc, JSAsyncGenerator<'gc>>,
    promise_cell: GcPtr<'gc, JSPromise<'gc>>,
    request: AsyncGeneratorRequest<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    match gen_ptr.try_borrow_mut(ctx) {
        Ok(mut gen_mut) => {
            gen_mut.pending.push((promise_cell, request));
            if gen_mut.pending.len() == 1 {
                drop(gen_mut);
                process_one_pending(ctx, gen_ptr, env)?;
            }
        }
        Err(_) => {
            // Generator is currently executing (borrow held by process_one_pending).
            // Park the request in DEFERRED_ASYNC_GEN_REQUESTS; the outer
            // process_one_pending loop drains it after each step.
            DEFERRED_ASYNC_GEN_REQUESTS.with(|q| {
                // Safety: we transmute lifetimes to 'static for thread-local storage.
                // The values are consumed within the same GC arena epoch — before any
                // collection can occur — so the underlying Gc pointers remain valid.
                let entry: DeferredRequest<'static> = unsafe { std::mem::transmute((gen_ptr, promise_cell, request)) };
                q.borrow_mut().push(entry);
            });
        }
    }
    Ok(())
}

type DeferredRequest<'gc> = (
    GcPtr<'gc, JSAsyncGenerator<'gc>>,
    GcPtr<'gc, JSPromise<'gc>>,
    AsyncGeneratorRequest<'gc>,
);

thread_local! {
    static DEFERRED_ASYNC_GEN_REQUESTS: std::cell::RefCell<Vec<DeferredRequest<'static>>> =
       const { std::cell::RefCell::new(Vec::new()) };
}

/// Drain any deferred async generator requests that were parked while the
/// generator was executing. Called from process_one_pending after each step.
fn drain_deferred_requests<'gc>(
    ctx: &GcContext<'gc>,
    target_gen: GcPtr<'gc, JSAsyncGenerator<'gc>>,
    _env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    let entries: Vec<DeferredRequest<'static>> = DEFERRED_ASYNC_GEN_REQUESTS.with(|q| std::mem::take(&mut *q.borrow_mut()));
    for entry in entries {
        let (gen_ptr, promise_cell, request): DeferredRequest<'gc> = unsafe { std::mem::transmute(entry) };
        // Only process entries for the current generator
        if Gc::ptr_eq(gen_ptr, target_gen) {
            let mut gen_mut = gen_ptr.borrow_mut(ctx);
            gen_mut.pending.push((promise_cell, request));
        } else {
            // Put back entries for other generators
            DEFERRED_ASYNC_GEN_REQUESTS.with(|q| {
                let entry: DeferredRequest<'static> = unsafe { std::mem::transmute((gen_ptr, promise_cell, request)) };
                q.borrow_mut().push(entry);
            });
        }
    }
    Ok(())
}

// Native implementation for AsyncGenerator.prototype.next
pub fn handle_async_generator_prototype_next<'gc>(
    ctx: &GcContext<'gc>,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, JSError> {
    let send_value = if !args.is_empty() { args[0].clone() } else { Value::Undefined };

    // Per spec: if this is not an Object or lacks [[AsyncGeneratorState]], reject.
    let this = match this_val {
        Some(Value::Object(obj)) => obj,
        _ => return reject_with_type_error(ctx, env, "AsyncGenerator.prototype.next called on incompatible receiver"),
    };
    let inner = match slot_get_chained(&this, &InternalSlot::AsyncGeneratorState) {
        Some(v) => v,
        None => return reject_with_type_error(ctx, env, "AsyncGenerator.prototype.next called on incompatible receiver"),
    };
    match &*inner.borrow() {
        Value::AsyncGenerator(gen_ptr) => {
            let (promise_cell, promise_obj_val) = create_promise_cell_and_obj(ctx, env);
            enqueue_async_generator_request(ctx, *gen_ptr, promise_cell, AsyncGeneratorRequest::Next(send_value), env)?;
            Ok(Some(promise_obj_val))
        }
        _ => reject_with_type_error(ctx, env, "AsyncGenerator.prototype.next called on incompatible receiver"),
    }
}

// Native implementation for AsyncGenerator.prototype.throw
pub fn handle_async_generator_prototype_throw<'gc>(
    ctx: &GcContext<'gc>,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, JSError> {
    let throw_val = if !args.is_empty() { args[0].clone() } else { Value::Undefined };

    let this = match this_val {
        Some(Value::Object(obj)) => obj,
        _ => return reject_with_type_error(ctx, env, "AsyncGenerator.prototype.throw called on incompatible receiver"),
    };
    let inner = match slot_get_chained(&this, &InternalSlot::AsyncGeneratorState) {
        Some(v) => v,
        None => return reject_with_type_error(ctx, env, "AsyncGenerator.prototype.throw called on incompatible receiver"),
    };
    match &*inner.borrow() {
        Value::AsyncGenerator(gen_ptr) => {
            let (promise_cell, promise_obj_val) = create_promise_cell_and_obj(ctx, env);
            enqueue_async_generator_request(ctx, *gen_ptr, promise_cell, AsyncGeneratorRequest::Throw(throw_val), env)?;
            Ok(Some(promise_obj_val))
        }
        _ => reject_with_type_error(ctx, env, "AsyncGenerator.prototype.throw called on incompatible receiver"),
    }
}

// Native implementation for AsyncGenerator.prototype.return
pub fn handle_async_generator_prototype_return<'gc>(
    ctx: &GcContext<'gc>,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, JSError> {
    let ret_val = if !args.is_empty() { args[0].clone() } else { Value::Undefined };

    let this = match this_val {
        Some(Value::Object(obj)) => obj,
        _ => return reject_with_type_error(ctx, env, "AsyncGenerator.prototype.return called on incompatible receiver"),
    };
    let inner = match slot_get_chained(&this, &InternalSlot::AsyncGeneratorState) {
        Some(v) => v,
        None => return reject_with_type_error(ctx, env, "AsyncGenerator.prototype.return called on incompatible receiver"),
    };
    match &*inner.borrow() {
        Value::AsyncGenerator(gen_ptr) => {
            let (promise_cell, promise_obj_val) = create_promise_cell_and_obj(ctx, env);
            enqueue_async_generator_request(ctx, *gen_ptr, promise_cell, AsyncGeneratorRequest::Return(ret_val), env)?;
            Ok(Some(promise_obj_val))
        }
        _ => reject_with_type_error(ctx, env, "AsyncGenerator.prototype.return called on incompatible receiver"),
    }
}

pub fn __internal_async_gen_await_resolve<'gc>(
    ctx: &GcContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // args: [value, gen_ptr, promise_cell]
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let gen_val = args.get(1).expect("arg1 missing");
    let promise_val = args.get(2).expect("arg2 missing");

    if let Value::AsyncGenerator(gen_ptr) = gen_val
        && let Value::Promise(promise_cell) = promise_val
    {
        let mut gen_mut = gen_ptr.borrow_mut(ctx);
        // Push continuation to front
        gen_mut.pending.insert(0, (*promise_cell, AsyncGeneratorRequest::Next(value)));
        drop(gen_mut);

        process_one_pending(ctx, *gen_ptr, env)?;
    }
    Ok(Value::Undefined)
}

pub fn __internal_async_gen_await_reject<'gc>(
    ctx: &GcContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // args: [reason, gen_ptr, promise_cell]
    let reason = args.first().cloned().unwrap_or(Value::Undefined);
    let gen_val = args.get(1).expect("arg1 missing");
    let promise_val = args.get(2).expect("arg2 missing");

    if let Value::AsyncGenerator(gen_ptr) = gen_val
        && let Value::Promise(promise_cell) = promise_val
    {
        let mut gen_mut = gen_ptr.borrow_mut(ctx);
        gen_mut.pending.insert(0, (*promise_cell, AsyncGeneratorRequest::Throw(reason)));
        drop(gen_mut);

        process_one_pending(ctx, *gen_ptr, env)?;
    }
    Ok(Value::Undefined)
}

pub fn __internal_async_gen_yield_resolve<'gc>(
    ctx: &GcContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let gen_val = args.get(1).expect("arg1 missing");
    let promise_val = args.get(2).expect("arg2 missing");

    if let Value::AsyncGenerator(gen_ptr) = gen_val
        && let Value::Promise(promise_cell) = promise_val
    {
        let mut gen_mut = gen_ptr.borrow_mut(ctx);
        gen_mut.cached_initial_yield = Some(value.clone());
        drop(gen_mut);

        let res_obj = create_iterator_result_obj(ctx, value, false)?;
        resolve_promise(ctx, promise_cell, Value::Object(res_obj), env);
    }
    Ok(Value::Undefined)
}

pub fn __internal_async_gen_yield_reject<'gc>(
    ctx: &GcContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let reason = args.first().cloned().unwrap_or(Value::Undefined);
    let gen_val = args.get(1).expect("arg1 missing");
    let promise_val = args.get(2).expect("arg2 missing");

    if let Value::AsyncGenerator(gen_ptr) = gen_val
        && let Value::Promise(promise_cell) = promise_val
    {
        let mut gen_mut = gen_ptr.borrow_mut(ctx);
        gen_mut.state = crate::core::GeneratorState::Completed;
        drop(gen_mut);

        reject_promise(ctx, promise_cell, reason, env);
    }
    Ok(Value::Undefined)
}

pub fn __internal_async_gen_return_resolve<'gc>(
    ctx: &GcContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    if let Some(Value::Promise(promise_cell)) = args.get(1) {
        let res_obj = create_iterator_result_obj(ctx, value, true)?;
        resolve_promise(ctx, promise_cell, Value::Object(res_obj), env);
    }
    Ok(Value::Undefined)
}

pub fn __internal_async_gen_return_reject<'gc>(
    ctx: &GcContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let reason = args.first().cloned().unwrap_or(Value::Undefined);
    if let Some(Value::Promise(promise_cell)) = args.get(1) {
        reject_promise(ctx, promise_cell, reason, env);
    }
    Ok(Value::Undefined)
}

pub fn __internal_async_gen_yield_star_resolve<'gc>(
    ctx: &GcContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let result_obj_val = args.first().cloned().unwrap_or(Value::Undefined);
    let gen_val = args.get(1).expect("arg1 missing");
    let outer_p_val = args.get(2).expect("arg2 missing");

    if let Value::AsyncGenerator(gen_ptr) = gen_val
        && let Value::Promise(outer_p_cell) = outer_p_val
    {
        let mut done = false;
        let mut value = Value::Undefined;
        if let Value::Object(obj) = &result_obj_val {
            let done_val = crate::core::get_property_with_accessors(ctx, env, obj, "done")?;
            done = done_val.to_truthy();
            let value_val = crate::core::get_property_with_accessors(ctx, env, obj, "value")?;
            value = value_val;
        }

        if done {
            let mut gen_mut = gen_ptr.borrow_mut(ctx);
            gen_mut.yield_star_iterator = None;
            gen_mut.pending.insert(0, (*outer_p_cell, AsyncGeneratorRequest::Next(value)));
            drop(gen_mut);
            process_one_pending(ctx, *gen_ptr, env)?;
        } else {
            match await_value(ctx, env, value.clone()) {
                Ok(awaited) => {
                    let res_obj = create_iterator_result_obj(ctx, awaited, false)?;
                    resolve_promise(ctx, outer_p_cell, Value::Object(res_obj), env);
                }
                Err(reason) => {
                    let mut gen_mut = gen_ptr.borrow_mut(ctx);
                    gen_mut.yield_star_iterator = None;
                    gen_mut.state = crate::core::GeneratorState::Completed;
                    drop(gen_mut);
                    reject_promise(ctx, outer_p_cell, reason, env);
                }
            }
        }
    }
    Ok(Value::Undefined)
}

pub fn __internal_async_gen_yield_star_reject<'gc>(
    ctx: &GcContext<'gc>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    let reason = args.first().cloned().unwrap_or(Value::Undefined);
    let gen_val = args.get(1).expect("arg1 missing");
    let outer_p_val = args.get(2).expect("arg2 missing");

    if let Value::AsyncGenerator(gen_ptr) = gen_val
        && let Value::Promise(outer_p_cell) = outer_p_val
    {
        let mut gen_mut = gen_ptr.borrow_mut(ctx);
        gen_mut.yield_star_iterator = None;
        gen_mut.pending.insert(0, (*outer_p_cell, AsyncGeneratorRequest::Throw(reason)));
        drop(gen_mut);

        process_one_pending(ctx, *gen_ptr, env)?;
    }
    Ok(Value::Undefined)
}

fn handle_yield_star_call<'gc>(
    ctx: &GcContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    gen_ptr: GcPtr<'gc, JSAsyncGenerator<'gc>>,
    promise_cell: GcPtr<'gc, JSPromise<'gc>>,
    iter_obj: JSObjectDataPtr<'gc>,
    method: &str,
    args: Vec<Value<'gc>>,
) -> Result<(), JSError> {
    // retrieve the method (next/throw/return) from iterator object
    let method_func = if method == "next" {
        if let Some(cached) = slot_get(&iter_obj, &InternalSlot::YieldStarNextMethod) {
            cached.borrow().clone()
        } else {
            let fetched = match crate::core::get_property_with_accessors(ctx, env, &iter_obj, method) {
                Ok(v) => v,
                Err(e) => {
                    let mut gen_mut = gen_ptr.borrow_mut(ctx);
                    gen_mut.yield_star_iterator = None;
                    gen_mut.state = crate::core::GeneratorState::Completed;
                    drop(gen_mut);
                    reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                    return Ok(());
                }
            };
            // unwrap property descriptor if necessary before caching/calling
            let fetched = match fetched {
                Value::Property { value: Some(v), .. } => v.borrow().clone(),
                other => other,
            };
            if !matches!(fetched, Value::Undefined | Value::Null) {
                slot_set(ctx, &iter_obj, InternalSlot::YieldStarNextMethod, &fetched.clone());
                iter_obj.borrow_mut(ctx).set_non_enumerable("__yield_star_next_method");
            }
            fetched
        }
    } else {
        match crate::core::get_property_with_accessors(ctx, env, &iter_obj, method) {
            Ok(v) => v,
            Err(e) => {
                let mut gen_mut = gen_ptr.borrow_mut(ctx);
                gen_mut.yield_star_iterator = None;
                gen_mut.state = crate::core::GeneratorState::Completed;
                drop(gen_mut);
                reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                return Ok(());
            }
        }
    };
    // debug info to investigate crashing case: classify method_func and whether it's callable
    let method_type = match &method_func {
        Value::Function(name) => format!("Function({})", name),
        Value::Closure(_) => "Closure".to_string(),
        Value::Object(o) => {
            let is_callable = crate::core::slot_get_chained(o, &InternalSlot::Callable).is_some() || o.borrow().get_closure().is_some();
            format!("Object(callable={})", is_callable)
        }
        Value::Property { .. } => "Property".to_string(),
        other => format!("{:?}", other),
    };

    log::warn!(
        "handle_yield_star_call method={} iter_keys={:?} method_type={}",
        method,
        iter_obj
            .borrow()
            .properties
            .keys()
            .map(|k| match k {
                crate::core::PropertyKey::String(s) => s.clone(),
                other => format!("{}", other),
            })
            .collect::<Vec<_>>(),
        method_type
    );
    if !matches!(method_func, Value::Undefined) {
        log::debug!(
            "handle_yield_star_call: about to call method='{}' method_func_variant={:?}",
            method,
            method_func
        );
        let call_res = evaluate_call_dispatch(ctx, env, &method_func, Some(&Value::Object(iter_obj)), &args);

        let res_val = match call_res {
            Ok(v) => v,
            Err(e) => {
                let mut gen_mut = gen_ptr.borrow_mut(ctx);
                gen_mut.yield_star_iterator = None;
                gen_mut.state = crate::core::GeneratorState::Completed;
                drop(gen_mut);
                reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                return Ok(());
            }
        };

        // Normalize to a promise and await its resolution (thenable-aware).
        let res_promise = match res_val {
            Value::Promise(p) => p,
            Value::Object(obj) => {
                if let Some(p) = crate::js_promise::get_promise_from_js_object(&obj) {
                    p
                } else {
                    let then_val = match crate::core::get_property_with_accessors(ctx, env, &obj, "then") {
                        Ok(v) => v,
                        Err(e) => {
                            let mut gen_mut = gen_ptr.borrow_mut(ctx);
                            gen_mut.yield_star_iterator = None;
                            gen_mut.state = crate::core::GeneratorState::Completed;
                            drop(gen_mut);
                            reject_promise(ctx, &promise_cell, eval_error_to_value(ctx, env, e), env);
                            return Ok(());
                        }
                    };
                    let is_then_callable = matches!(then_val, Value::Function(_) | Value::Closure(_) | Value::Object(_));
                    let (p, resolve, reject) = crate::js_promise::create_promise_capability(ctx, env)?;
                    if !matches!(then_val, Value::Undefined | Value::Null) && is_then_callable {
                        let call_env = crate::js_class::prepare_call_env_with_this(
                            ctx,
                            Some(env),
                            Some(&Value::Object(obj)),
                            None,
                            &[],
                            None,
                            Some(env),
                            None,
                        )?;
                        if let Err(e) = evaluate_call_dispatch(ctx, &call_env, &then_val, Some(&Value::Object(obj)), &[resolve, reject]) {
                            reject_promise(ctx, &p, eval_error_to_value(ctx, env, e), env);
                        }
                    } else {
                        crate::js_promise::call_function(ctx, &resolve, &[Value::Object(obj)], env)?;
                    }
                    p
                }
            }
            _ => {
                let (p, r, _) = crate::js_promise::create_promise_capability(ctx, env)?;
                crate::js_promise::call_function(ctx, &r, &[res_val], env)?;
                p
            }
        };

        let gen_val = Value::AsyncGenerator(gen_ptr);
        let p_val = Value::Promise(promise_cell);

        let on_fulfilled = Value::Closure(crate::core::Gc::new(
            ctx,
            ClosureData::new(
                &[crate::core::DestructuringElement::Variable("res".to_string(), None)],
                &[Statement::from(StatementKind::Expr(Expr::Call(
                    Box::new(Expr::Var("__internal_async_gen_yield_star_resolve".to_string(), None, None)),
                    vec![
                        Expr::Var("res".to_string(), None, None),
                        Expr::Var("__gen".to_string(), None, None),
                        Expr::Var("__p".to_string(), None, None),
                    ],
                )))],
                {
                    let e = new_js_object_data(ctx);
                    e.borrow_mut(ctx).prototype = Some(*env);
                    slot_set(ctx, &e, InternalSlot::Gen, &gen_val.clone());
                    slot_set(ctx, &e, InternalSlot::P, &p_val.clone());
                    Some(e)
                },
                None,
            ),
        ));

        let on_rejected = Value::Closure(crate::core::Gc::new(
            ctx,
            ClosureData::new(
                &[crate::core::DestructuringElement::Variable("reason".to_string(), None)],
                &[Statement::from(StatementKind::Expr(Expr::Call(
                    Box::new(Expr::Var("__internal_async_gen_yield_star_reject".to_string(), None, None)),
                    vec![
                        Expr::Var("reason".to_string(), None, None),
                        Expr::Var("__gen".to_string(), None, None),
                        Expr::Var("__p".to_string(), None, None),
                    ],
                )))],
                {
                    let e = new_js_object_data(ctx);
                    e.borrow_mut(ctx).prototype = Some(*env);
                    slot_set(ctx, &e, InternalSlot::Gen, &gen_val.clone());
                    slot_set(ctx, &e, InternalSlot::P, &p_val.clone());
                    Some(e)
                },
                None,
            ),
        ));

        perform_promise_then(ctx, res_promise, Some(on_fulfilled), Some(on_rejected), None, env)?;
    } else if method == "return" {
        let arg = args.first().cloned().unwrap_or(Value::Undefined);
        let mut gen_mut = gen_ptr.borrow_mut(ctx);
        gen_mut.yield_star_iterator = None;
        gen_mut.state = crate::core::GeneratorState::Completed;
        drop(gen_mut);
        let res = create_iterator_result_obj(ctx, arg, true)?;
        resolve_promise(ctx, &promise_cell, Value::Object(res), env);
    } else if method == "throw" {
        let arg = args.first().cloned().unwrap_or(Value::Undefined);
        let mut gen_mut = gen_ptr.borrow_mut(ctx);
        gen_mut.yield_star_iterator = None;
        drop(gen_mut);
        reject_promise(ctx, &promise_cell, arg, env);
    } else {
        let mut gen_mut = gen_ptr.borrow_mut(ctx);
        gen_mut.yield_star_iterator = None;
        gen_mut.state = crate::core::GeneratorState::Completed;
        drop(gen_mut);
        let err = Value::from("TypeError: Iterator has no next method");
        reject_promise(ctx, &promise_cell, err, env);
    }
    Ok(())
}
