use crate::core::{
    AsyncGeneratorRequest, ClosureData, EvalError, Expr, JSAsyncGenerator, JSObjectDataPtr, JSPromise, Statement, StatementKind, Value,
    VarDeclKind, env_set, env_set_recursive, evaluate_call_dispatch, evaluate_expr, evaluate_statements, get_own_property,
    new_js_object_data, object_get_key_value, object_set_key_value, prepare_function_call_env,
};
use crate::core::{GcPtr, MutationContext, new_gc_cell_ptr};
use crate::error::JSError;
use crate::js_promise::{make_promise_js_object, reject_promise, resolve_promise};

fn eval_error_to_value<'gc>(err: EvalError<'gc>) -> Value<'gc> {
    match err {
        EvalError::Throw(v, ..) => v,
        EvalError::Js(j) => Value::String(crate::unicode::utf8_to_utf16(&j.message())),
    }
}

fn await_value<'gc>(mc: &MutationContext<'gc>, _env: &JSObjectDataPtr<'gc>, value: Value<'gc>) -> Result<Value<'gc>, Value<'gc>> {
    if let Value::Object(obj) = &value
        && let Some(promise_ref) = crate::js_promise::get_promise_from_js_object(obj)
    {
        crate::js_promise::mark_promise_handled(mc, promise_ref, _env).expect("must succeed");
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
    mc: &MutationContext<'gc>,
    closure: &ClosureData<'gc>,
    args: &[Value<'gc>],
    fn_obj: Option<JSObjectDataPtr<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let closure_env = closure.env.expect("closure env must exist");
    // Prepare call-time environment and bind parameters immediately.
    // This ensures parameter destructuring/defaults throw at call time, matching spec.
    let call_env = prepare_function_call_env(mc, Some(&closure_env), None, Some(&closure.params[..]), args, None, None)?;

    // Ensure an `arguments` object is available on the call environment.
    crate::js_class::create_arguments_object(mc, &call_env, args, None)?;

    // Create the async generator instance object
    let gen_obj = new_js_object_data(mc);

    // Create internal async generator struct
    let async_gen = new_gc_cell_ptr(
        mc,
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
        },
    );

    // Store it on the object under a hidden key
    object_set_key_value(mc, &gen_obj, "__async_generator__", Value::AsyncGenerator(async_gen))?;

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
            && let Some(proto_val) = get_own_property(&fn_obj, "__async_generator_proto")
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
        gen_obj.borrow_mut(mc).prototype = Some(proto_obj);
    }

    // Create 'next' function as a native Function; name it so call_native_function can route
    let next_func = Value::Function("AsyncGenerator.prototype.next".to_string());
    object_set_key_value(mc, &gen_obj, "next", next_func)?;
    // Create 'throw' and 'return' functions
    let throw_func = Value::Function("AsyncGenerator.prototype.throw".to_string());
    object_set_key_value(mc, &gen_obj, "throw", throw_func)?;
    let return_func = Value::Function("AsyncGenerator.prototype.return".to_string());
    object_set_key_value(mc, &gen_obj, "return", return_func)?;
    // Return the object
    Ok(Value::Object(gen_obj))
}

/// Initialize AsyncGenerator constructor/prototype and attach prototype methods
pub fn initialize_async_generator<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> Result<(), JSError> {
    // Create constructor object and async generator prototype
    let async_gen_ctor = crate::core::new_js_object_data(mc);
    // Set __proto__ to Function.prototype if present
    if let Some(func_ctor_val) = crate::core::env_get(env, "Function")
        && let Value::Object(func_ctor) = &*func_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(func_ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        async_gen_ctor.borrow_mut(mc).prototype = Some(*proto_obj);
    }

    let async_gen_proto = crate::core::new_js_object_data(mc);

    // Attach prototype methods as named functions that dispatch to the async generator handler
    let val = Value::Function("AsyncGenerator.prototype.next".to_string());
    object_set_key_value(mc, &async_gen_proto, "next", val)?;

    let val = Value::Function("AsyncGenerator.prototype.return".to_string());
    object_set_key_value(mc, &async_gen_proto, "return", val)?;

    let val = Value::Function("AsyncGenerator.prototype.throw".to_string());
    object_set_key_value(mc, &async_gen_proto, "throw", val)?;

    // Register Symbol.asyncIterator on AsyncGenerator.prototype -> returns the generator object itself
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(async_iter_sym_val) = object_get_key_value(sym_obj, "asyncIterator")
        && let Value::Symbol(async_iter_sym) = &*async_iter_sym_val.borrow()
    {
        let val = Value::Function("AsyncGenerator.prototype.asyncIterator".to_string());
        object_set_key_value(mc, &async_gen_proto, async_iter_sym, val)?;
        async_gen_proto
            .borrow_mut(mc)
            .set_non_enumerable(crate::core::PropertyKey::Symbol(*async_iter_sym));
    }

    // Link prototype to constructor and expose on global env
    object_set_key_value(mc, &async_gen_ctor, "prototype", Value::Object(async_gen_proto))?;
    crate::core::env_set(mc, env, "AsyncGenerator", Value::Object(async_gen_ctor))?;
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
            .any(|(k, v, _)| expr_contains_yield_or_await(k) || expr_contains_yield_or_await(v)),
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

fn create_iterator_result_obj<'gc>(mc: &MutationContext<'gc>, value: Value<'gc>, done: bool) -> Result<JSObjectDataPtr<'gc>, JSError> {
    let obj = new_js_object_data(mc);
    object_set_key_value(mc, &obj, "value", value)?;
    object_set_key_value(mc, &obj, "done", Value::Boolean(done))?;
    Ok(obj)
}

// Helper to create a new internal JSPromise cell and corresponding JS Promise object
fn create_promise_cell_and_obj<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>) -> (GcPtr<'gc, JSPromise<'gc>>, Value<'gc>) {
    let promise_cell = new_gc_cell_ptr(mc, crate::core::JSPromise::new());
    let promise_obj = make_promise_js_object(mc, promise_cell, Some(*env)).unwrap();
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

fn get_for_await_iterator<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iter_val: &Value<'gc>,
) -> Result<(JSObjectDataPtr<'gc>, bool), EvalError<'gc>> {
    let mut iterator: Option<JSObjectDataPtr<'gc>> = None;
    let mut is_async_iter = false;

    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(async_iter_sym) = object_get_key_value(sym_obj, "asyncIterator")
        && let Value::Symbol(async_iter_sym_data) = &*async_iter_sym.borrow()
    {
        let method = if let Value::Object(obj) = iter_val
            && let Some(c) = object_get_key_value(obj, async_iter_sym_data)
        {
            c.borrow().clone()
        } else {
            Value::Undefined
        };

        if !matches!(method, Value::Undefined | Value::Null) {
            let res = evaluate_call_dispatch(mc, env, method, Some(iter_val.clone()), vec![])?;
            let res = await_value(mc, env, res).map_err(|v| EvalError::Throw(v, None, None))?;
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
    {
        let method = if let Value::Object(obj) = iter_val
            && let Some(c) = object_get_key_value(obj, iter_sym_data)
        {
            c.borrow().clone()
        } else {
            Value::Undefined
        };

        if !matches!(method, Value::Undefined | Value::Null) {
            let res = evaluate_call_dispatch(mc, env, method, Some(iter_val.clone()), vec![])?;
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
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    iter_obj: JSObjectDataPtr<'gc>,
    is_async_iter: bool,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    let next_method = object_get_key_value(&iter_obj, "next")
        .ok_or(EvalError::Js(raise_type_error!("Iterator has no next method")))?
        .borrow()
        .clone();

    let mut next_res_val = evaluate_call_dispatch(mc, env, next_method, Some(Value::Object(iter_obj)), vec![])?;
    if is_async_iter {
        next_res_val = await_value(mc, env, next_res_val).map_err(|v| EvalError::Throw(v, None, None))?;
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

        value = await_value(mc, env, value).map_err(|v| EvalError::Throw(v, None, None))?;
        Ok(Some(value))
    } else {
        Err(raise_type_error!("Iterator result is not an object").into())
    }
}

// Process pending requests (next/throw/return) for the given async generator
// Processes requests until the generator suspends or the pending queue is empty.
fn process_one_pending<'gc>(
    mc: &MutationContext<'gc>,
    gen_ptr_mut: &mut JSAsyncGenerator<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<(), JSError> {
    use crate::core::GeneratorState;

    loop {
        // Pop the next pending entry (front of queue)
        let maybe_entry = gen_ptr_mut.pending.first().cloned();
        if maybe_entry.is_none() {
            return Ok(());
        }
        let (promise_cell, request) = maybe_entry.unwrap();
        gen_ptr_mut.pending.remove(0);

        match (&mut gen_ptr_mut.state, request) {
            (GeneratorState::NotStarted, crate::core::AsyncGeneratorRequest::Next(_send_value)) => {
                // Initialize and suspend at first yield (or run to completion)
                if let Some((idx, inner_idx_opt, yield_kind, yield_inner)) =
                    crate::js_generator::find_first_yield_in_statements(&gen_ptr_mut.body)
                {
                    let func_env = if let Some(prep_env) = gen_ptr_mut.call_env.take() {
                        prep_env
                    } else {
                        let env = crate::core::prepare_function_call_env(
                            mc,
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
                        crate::js_class::create_arguments_object(mc, &env, &gen_ptr_mut.args, None)?;
                        env
                    };

                    if let StatementKind::ForAwaitOf(decl_kind_opt, var_name, iterable, body) = &*gen_ptr_mut.body[idx].kind
                        && inner_idx_opt.is_some()
                        && let Some(yield_expr) = extract_simple_yield_expr(body)
                    {
                        if idx > 0 {
                            let pre_stmts = gen_ptr_mut.body[0..idx].to_vec();
                            let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                        }
                        let mut head_env: Option<JSObjectDataPtr<'gc>> = None;
                        if let Some(VarDeclKind::Let) | Some(VarDeclKind::Const) = decl_kind_opt {
                            let he = new_js_object_data(mc);
                            he.borrow_mut(mc).prototype = Some(func_env);
                            env_set(mc, &he, var_name, Value::Uninitialized)?;
                            head_env = Some(he);
                        }
                        let iter_eval_env = head_env.as_ref().unwrap_or(&func_env);
                        let iter_val = evaluate_expr(mc, iter_eval_env, iterable)?;
                        let (iter_obj, is_async_iter) = get_for_await_iterator(mc, &func_env, &iter_val)?;

                        match for_await_next_value(mc, &func_env, iter_obj, is_async_iter) {
                            Ok(Some(value)) => {
                                let iter_env = if let Some(VarDeclKind::Let) | Some(VarDeclKind::Const) = decl_kind_opt {
                                    let e = new_js_object_data(mc);
                                    e.borrow_mut(mc).prototype = Some(func_env);
                                    env_set(mc, &e, var_name, value.clone())?;
                                    e
                                } else {
                                    env_set_recursive(mc, &func_env, var_name, value.clone())?;
                                    func_env
                                };

                                let mut yielded = evaluate_expr(mc, &iter_env, &yield_expr)?;
                                match await_value(mc, env, yielded.clone()) {
                                    Ok(awaited) => {
                                        yielded = awaited;
                                    }
                                    Err(reason) => {
                                        gen_ptr_mut.state = GeneratorState::Completed;
                                        reject_promise(mc, &promise_cell, reason, env);
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

                                let res_obj = create_iterator_result_obj(mc, yielded, false)?;
                                crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                                return Ok(());
                            }
                            Ok(None) => {
                                gen_ptr_mut.state = GeneratorState::Completed;
                                let res_obj = create_iterator_result_obj(mc, Value::Undefined, true)?;
                                resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                                return Ok(());
                            }
                            Err(e) => {
                                gen_ptr_mut.state = GeneratorState::Completed;
                                reject_promise(mc, &promise_cell, eval_error_to_value(e), env);
                                return Ok(());
                            }
                        }
                    }

                    if idx > 0 {
                        let pre_stmts = gen_ptr_mut.body[0..idx].to_vec();
                        let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    } else if let Some(inner_idx) = inner_idx_opt
                        && inner_idx > 0
                        && let StatementKind::Block(inner_stmts) = &*gen_ptr_mut.body[idx].kind
                    {
                        let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                        let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    }

                    gen_ptr_mut.state = GeneratorState::Suspended {
                        pc: idx,
                        stack: vec![],
                        pre_env: Some(func_env),
                    };

                    if let Some(inner_expr_box) = yield_inner {
                        let parent_env = &func_env;
                        let inner_eval_env = crate::core::prepare_function_call_env(mc, Some(parent_env), None, None, &[], None, None)?;
                        object_set_key_value(mc, &inner_eval_env, "__gen_throw_val", Value::Undefined)?;
                        match crate::core::evaluate_expr(mc, &inner_eval_env, &inner_expr_box) {
                            Ok(mut val) => {
                                if yield_kind == crate::js_generator::YieldKind::YieldStar
                                    && let Value::Object(arr_obj) = &val
                                    && crate::js_array::is_array(mc, arr_obj)
                                {
                                    if let Some(first) = object_get_key_value(arr_obj, 0) {
                                        val = first.borrow().clone();
                                    } else {
                                        val = Value::Undefined;
                                    }
                                }
                                match await_value(mc, env, val.clone()) {
                                    Ok(awaited) => {
                                        gen_ptr_mut.cached_initial_yield = Some(awaited.clone());
                                        let res_obj = create_iterator_result_obj(mc, awaited, false)?;
                                        crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                                        return Ok(());
                                    }
                                    Err(reason) => {
                                        gen_ptr_mut.state = GeneratorState::Completed;
                                        reject_promise(mc, &promise_cell, reason, env);
                                        return Ok(());
                                    }
                                }
                            }
                            Err(_) => {
                                gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                                let res_obj = create_iterator_result_obj(mc, Value::Undefined, false)?;
                                crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                                return Ok(());
                            }
                        }
                    }

                    gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                    let res_obj = create_iterator_result_obj(mc, Value::Undefined, false)?;
                    crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                    return Ok(());
                } else {
                    // No yields: run to completion and keep processing next requests
                    let func_env = if let Some(prep_env) = gen_ptr_mut.call_env.take() {
                        prep_env
                    } else {
                        let env = prepare_function_call_env(
                            mc,
                            Some(&gen_ptr_mut.env),
                            None,
                            Some(&gen_ptr_mut.params[..]),
                            &gen_ptr_mut.args,
                            None,
                            None,
                        )?;

                        // Ensure `arguments` exists for the no-yield completion path too.
                        crate::js_class::create_arguments_object(mc, &env, &gen_ptr_mut.args, None)?;
                        env
                    };

                    match evaluate_statements(mc, &func_env, &gen_ptr_mut.body) {
                        Ok(v) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            let res_obj = create_iterator_result_obj(mc, v, true)?;
                            resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                        Err(e) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            reject_promise(mc, &promise_cell, eval_error_to_value(e), env);
                            continue;
                        }
                    }
                }
            }
            (GeneratorState::NotStarted, crate::core::AsyncGeneratorRequest::Throw(throw_val)) => {
                reject_promise(mc, &promise_cell, throw_val, env);
                continue;
            }
            (GeneratorState::NotStarted, crate::core::AsyncGeneratorRequest::Return(ret_val)) => {
                gen_ptr_mut.state = GeneratorState::Completed;
                let res_obj = create_iterator_result_obj(mc, ret_val, true)?;
                resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                continue;
            }
            (GeneratorState::Suspended { pc, pre_env, .. }, crate::core::AsyncGeneratorRequest::Next(_send_value)) => {
                if let Some(for_await) = gen_ptr_mut.pending_for_await.clone() {
                    let func_env = if let Some(env) = pre_env.as_ref() {
                        *env
                    } else {
                        crate::core::prepare_function_call_env(mc, Some(&gen_ptr_mut.env), None, None, &[], None, None)?
                    };

                    match for_await_next_value(mc, &func_env, for_await.iterator, for_await.is_async) {
                        Ok(Some(value)) => {
                            let iter_env = if let Some(VarDeclKind::Let) | Some(VarDeclKind::Const) = for_await.decl_kind {
                                let e = new_js_object_data(mc);
                                e.borrow_mut(mc).prototype = Some(func_env);
                                env_set(mc, &e, &for_await.var_name, value.clone())?;
                                e
                            } else {
                                env_set_recursive(mc, &func_env, &for_await.var_name, value.clone())?;
                                func_env
                            };

                            let mut yielded = evaluate_expr(mc, &iter_env, &for_await.yield_expr)?;
                            match await_value(mc, env, yielded.clone()) {
                                Ok(awaited) => {
                                    yielded = awaited;
                                }
                                Err(reason) => {
                                    gen_ptr_mut.state = GeneratorState::Completed;
                                    gen_ptr_mut.pending_for_await = None;
                                    reject_promise(mc, &promise_cell, reason, env);
                                    continue;
                                }
                            }

                            gen_ptr_mut.pending_for_await = Some(for_await);
                            let res_obj = create_iterator_result_obj(mc, yielded, false)?;
                            crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                        Ok(None) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            gen_ptr_mut.pending_for_await = None;
                            let res_obj = create_iterator_result_obj(mc, Value::Undefined, true)?;
                            crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                        Err(e) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            gen_ptr_mut.pending_for_await = None;
                            reject_promise(mc, &promise_cell, eval_error_to_value(e), env);
                            continue;
                        }
                    }
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

                // Generate a unique variable name for this yield point (based on PC and count)
                let base_name = format!("__gen_yield_val_{}_", pc_val);
                let next_idx = if let Some(s) = tail.first() {
                    crate::js_generator::count_yield_vars_in_statement(s, &base_name)
                } else {
                    0
                };
                let var_name = format!("{}{}", base_name, next_idx);

                // Replace first yield occurrence in the first statement with the
                // special variable so we can inject the send value into it when executing.
                let mut replaced = false;
                if let Some(first_stmt) = tail.get_mut(0) {
                    crate::js_generator::replace_first_yield_in_statement(first_stmt, &var_name, &mut replaced);
                }

                // Use the pre-execution environment if available so bindings created
                // by pre-statements remain visible when we resume execution.
                let func_env = if let Some(env) = pre_env.as_ref() {
                    *env
                } else {
                    crate::core::prepare_function_call_env(mc, Some(&gen_ptr_mut.env), None, None, &[], None, None)?
                };

                // Prefer the queued send value if it is concrete; otherwise fall back
                // to the cached initially-yielded value if present.
                if let Value::Undefined = _send_value {
                    if let Some(cached) = gen_ptr_mut.cached_initial_yield.as_ref() {
                        object_set_key_value(mc, &func_env, &var_name, cached.clone())?;
                    } else {
                        object_set_key_value(mc, &func_env, &var_name, _send_value.clone())?;
                    }
                } else {
                    object_set_key_value(mc, &func_env, &var_name, _send_value.clone())?;
                }

                if let Some((idx, inner_idx_opt, yield_kind, yield_inner)) = crate::js_generator::find_first_yield_in_statements(&tail) {
                    if idx > 0 {
                        let pre_stmts = tail[0..idx].to_vec();
                        let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    } else if let Some(inner_idx) = inner_idx_opt
                        && inner_idx > 0
                        && let StatementKind::Block(inner_stmts) = &*tail[idx].kind
                    {
                        let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                        let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    }

                    // If the yield is inside a while loop, ensure the loop condition
                    // is still true before yielding again.
                    if let Some(stmt) = tail.get(idx)
                        && let StatementKind::While(while_stmt, _) = &*stmt.kind
                    {
                        let cond_val = crate::core::evaluate_expr(mc, &func_env, while_stmt)?;
                        let cond_bool = crate::js_boolean::to_boolean(&cond_val);
                        if !cond_bool {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            let res_obj = create_iterator_result_obj(mc, Value::Undefined, true)?;
                            crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
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
                        let inner_eval_env = crate::core::prepare_function_call_env(mc, Some(parent_env), None, None, &[], None, None)?;
                        object_set_key_value(mc, &inner_eval_env, "__gen_throw_val", Value::Undefined)?;
                        match crate::core::evaluate_expr(mc, &inner_eval_env, &inner_expr_box) {
                            Ok(mut val) => {
                                if yield_kind == crate::js_generator::YieldKind::YieldStar
                                    && let Value::Object(arr_obj) = &val
                                    && crate::js_array::is_array(mc, arr_obj)
                                {
                                    if let Some(first) = object_get_key_value(arr_obj, 0) {
                                        val = first.borrow().clone();
                                    } else {
                                        val = Value::Undefined;
                                    }
                                }
                                match await_value(mc, env, val.clone()) {
                                    Ok(awaited) => {
                                        gen_ptr_mut.cached_initial_yield = Some(awaited.clone());
                                        let res_obj = create_iterator_result_obj(mc, awaited, false)?;
                                        crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                                        continue;
                                    }
                                    Err(reason) => {
                                        gen_ptr_mut.state = GeneratorState::Completed;
                                        reject_promise(mc, &promise_cell, reason, env);
                                        continue;
                                    }
                                }
                            }
                            Err(_) => {
                                gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                                let res_obj = create_iterator_result_obj(mc, Value::Undefined, false)?;
                                crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                                continue;
                            }
                        }
                    }

                    gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                    let res_obj = create_iterator_result_obj(mc, Value::Undefined, false)?;
                    crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                    continue;
                }

                // If we replaced the first yield but no yields remain, and no concrete send value
                // was provided, fall back to evaluating the original yield expression. This
                // prevents infinite loops for loop-based generators whose next iteration relies
                // on the yield expression's side effects (e.g. `current--`).
                if matches!(_send_value, Value::Undefined)
                    && let Some((idx, inner_idx_opt, yield_kind, yield_inner)) =
                        crate::js_generator::find_first_yield_in_statements(&original_tail)
                {
                    if idx > 0 {
                        let pre_stmts = original_tail[0..idx].to_vec();
                        let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    } else if let Some(inner_idx) = inner_idx_opt
                        && inner_idx > 0
                        && let StatementKind::Block(inner_stmts) = &*original_tail[idx].kind
                    {
                        let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                        let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    }

                    if let Some(stmt) = original_tail.get(idx)
                        && let StatementKind::While(while_stmt, _) = &*stmt.kind
                    {
                        let cond_val = crate::core::evaluate_expr(mc, &func_env, while_stmt)?;
                        let cond_bool = crate::js_boolean::to_boolean(&cond_val);
                        if !cond_bool {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            let res_obj = create_iterator_result_obj(mc, Value::Undefined, true)?;
                            crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
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
                        let inner_eval_env = crate::core::prepare_function_call_env(mc, Some(parent_env), None, None, &[], None, None)?;
                        object_set_key_value(mc, &inner_eval_env, "__gen_throw_val", Value::Undefined)?;
                        match crate::core::evaluate_expr(mc, &inner_eval_env, &inner_expr_box) {
                            Ok(mut val) => {
                                if yield_kind == crate::js_generator::YieldKind::YieldStar
                                    && let Value::Object(arr_obj) = &val
                                    && crate::js_array::is_array(mc, arr_obj)
                                {
                                    if let Some(first) = object_get_key_value(arr_obj, 0) {
                                        val = first.borrow().clone();
                                    } else {
                                        val = Value::Undefined;
                                    }
                                }
                                match await_value(mc, env, val.clone()) {
                                    Ok(awaited) => {
                                        gen_ptr_mut.cached_initial_yield = Some(awaited.clone());
                                        let res_obj = create_iterator_result_obj(mc, awaited, false)?;
                                        crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                                        continue;
                                    }
                                    Err(reason) => {
                                        gen_ptr_mut.state = GeneratorState::Completed;
                                        reject_promise(mc, &promise_cell, reason, env);
                                        continue;
                                    }
                                }
                            }
                            Err(_) => {
                                gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                                let res_obj = create_iterator_result_obj(mc, Value::Undefined, false)?;
                                crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                                continue;
                            }
                        }
                    }

                    gen_ptr_mut.cached_initial_yield = Some(Value::Undefined);
                    let res_obj = create_iterator_result_obj(mc, Value::Undefined, false)?;
                    crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                    continue;
                }

                // No further yields: execute tail to completion
                match crate::core::evaluate_statements(mc, &func_env, &tail) {
                    Ok(v) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        let res_obj = create_iterator_result_obj(mc, v, true)?;
                        crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    Err(e) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        reject_promise(mc, &promise_cell, eval_error_to_value(e), env);
                        continue;
                    }
                }
            }
            (GeneratorState::Suspended { pc, pre_env, .. }, crate::core::AsyncGeneratorRequest::Throw(throw_val)) => {
                // Resume by throwing into the suspended point: replace first yield with a Throw
                let pc_val = *pc;
                if pc_val >= gen_ptr_mut.body.len() {
                    gen_ptr_mut.state = GeneratorState::Completed;
                    reject_promise(mc, &promise_cell, throw_val, env);
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
                    crate::core::prepare_function_call_env(mc, Some(&gen_ptr_mut.env), None, None, &[], None, None)?
                };
                object_set_key_value(mc, &func_env, "__gen_throw_val", throw_val.clone())?;

                match crate::core::evaluate_statements(mc, &func_env, &tail) {
                    Ok(v) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        let res_obj = create_iterator_result_obj(mc, v, true)?;
                        resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    Err(e) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        reject_promise(mc, &promise_cell, eval_error_to_value(e), env);
                        continue;
                    }
                }
            }
            (GeneratorState::Suspended { pc, pre_env, .. }, crate::core::AsyncGeneratorRequest::Return(ret_val)) => {
                // Inject a `return __gen_throw_val` into the suspended point so
                // that any `finally` blocks execute and the generator completes
                // in a spec-like manner.
                let pc_val = *pc;
                if pc_val >= gen_ptr_mut.body.len() {
                    gen_ptr_mut.state = GeneratorState::Completed;
                    let res_obj = create_iterator_result_obj(mc, ret_val, true)?;
                    resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
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
                    crate::core::prepare_function_call_env(mc, Some(&gen_ptr_mut.env), None, None, &[], None, None)?
                };
                object_set_key_value(mc, &func_env, "__gen_throw_val", ret_val.clone())?;

                match crate::core::evaluate_statements(mc, &func_env, &tail) {
                    Ok(v) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        let res_obj = create_iterator_result_obj(mc, v, true)?;
                        resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    Err(e) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        reject_promise(mc, &promise_cell, eval_error_to_value(e), env);
                        continue;
                    }
                }
            }
            (GeneratorState::Running { .. }, _) => {
                // Shouldn't happen; reject the promise
                let reason = Value::String(crate::unicode::utf8_to_utf16("Async generator already running"));
                reject_promise(mc, &promise_cell, reason, env);
                // continue processing remaining requests (unlikely)
                continue;
            }
            (GeneratorState::Completed, _) => {
                // Already completed: fulfill with done=true
                let res_obj = create_iterator_result_obj(mc, Value::Undefined, true)?;
                resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                continue;
            }
        }
    }
}

// Native implementation for AsyncGenerator.prototype.next
pub fn handle_async_generator_prototype_next<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, JSError> {
    let send_value = if !args.is_empty() { args[0].clone() } else { Value::Undefined };

    let this = this_val.ok_or_else(|| crate::raise_eval_error!("AsyncGenerator.prototype.next called without this"))?;
    if let Value::Object(obj) = this {
        // Obtain internal async generator struct
        if let Some(inner) = object_get_key_value(&obj, "__async_generator__") {
            match &*inner.borrow() {
                Value::AsyncGenerator(gen_ptr) => {
                    // create a new pending Promise and enqueue it
                    let (promise_cell, promise_obj_val) = create_promise_cell_and_obj(mc, env);
                    // push onto pending
                    {
                        let mut gen_ptr_mut = gen_ptr.borrow_mut(mc);
                        gen_ptr_mut
                            .pending
                            .push((promise_cell, AsyncGeneratorRequest::Next(send_value.clone())));
                        // If this is the only pending request, process it immediately
                        if gen_ptr_mut.pending.len() == 1 {
                            // process one pending (might settle immediately or suspend)
                            process_one_pending(mc, &mut gen_ptr_mut, env)?;
                        }
                    }
                    Ok(Some(promise_obj_val))
                }
                _ => Err(crate::raise_eval_error!("Async generator internal missing")),
            }
        } else {
            Err(crate::raise_eval_error!("Async generator internal missing"))
        }
    } else {
        Err(crate::raise_eval_error!("AsyncGenerator.prototype.next called on non-object"))
    }
}

// Native implementation for AsyncGenerator.prototype.throw
pub fn handle_async_generator_prototype_throw<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, JSError> {
    let throw_val = if !args.is_empty() { args[0].clone() } else { Value::Undefined };

    let this = this_val.ok_or_else(|| crate::raise_eval_error!("AsyncGenerator.prototype.throw called without this"))?;
    if let Value::Object(obj) = this {
        // Obtain internal async generator struct
        if let Some(inner) = object_get_key_value(&obj, "__async_generator__") {
            match &*inner.borrow() {
                Value::AsyncGenerator(gen_ptr) => {
                    // create a new pending Promise and enqueue it
                    let (promise_cell, promise_obj_val) = create_promise_cell_and_obj(mc, env);
                    // push onto pending
                    {
                        let mut gen_ptr_mut = gen_ptr.borrow_mut(mc);
                        gen_ptr_mut
                            .pending
                            .push((promise_cell, AsyncGeneratorRequest::Throw(throw_val.clone())));
                        // If this is the only pending request, process it immediately
                        if gen_ptr_mut.pending.len() == 1 {
                            // process one pending (might settle immediately or suspend)
                            process_one_pending(mc, &mut gen_ptr_mut, env)?;
                        }
                    }
                    Ok(Some(promise_obj_val))
                }
                _ => Err(crate::raise_eval_error!("Async generator internal missing")),
            }
        } else {
            Err(crate::raise_eval_error!("Async generator internal missing"))
        }
    } else {
        Err(crate::raise_eval_error!("AsyncGenerator.prototype.throw called on non-object"))
    }
}

// Native implementation for AsyncGenerator.prototype.return
pub fn handle_async_generator_prototype_return<'gc>(
    mc: &MutationContext<'gc>,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, JSError> {
    let ret_val = if !args.is_empty() { args[0].clone() } else { Value::Undefined };

    let this = this_val.ok_or_else(|| crate::raise_eval_error!("AsyncGenerator.prototype.return called without this"))?;
    if let Value::Object(obj) = this {
        // Obtain internal async generator struct
        if let Some(inner) = object_get_key_value(&obj, "__async_generator__") {
            match &*inner.borrow() {
                Value::AsyncGenerator(gen_ptr) => {
                    // create a new pending Promise and enqueue it
                    let (promise_cell, promise_obj_val) = create_promise_cell_and_obj(mc, env);
                    // push onto pending
                    {
                        let mut gen_ptr_mut = gen_ptr.borrow_mut(mc);
                        gen_ptr_mut
                            .pending
                            .push((promise_cell, AsyncGeneratorRequest::Return(ret_val.clone())));
                        // If this is the only pending request, process it immediately
                        if gen_ptr_mut.pending.len() == 1 {
                            // process one pending (might settle immediately or suspend)
                            process_one_pending(mc, &mut gen_ptr_mut, env)?;
                        }
                    }
                    Ok(Some(promise_obj_val))
                }
                _ => Err(crate::raise_eval_error!("Async generator internal missing")),
            }
        } else {
            Err(crate::raise_eval_error!("Async generator internal missing"))
        }
    } else {
        Err(crate::raise_eval_error!("AsyncGenerator.prototype.return called on non-object"))
    }
}
