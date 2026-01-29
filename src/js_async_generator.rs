use crate::core::{
    AsyncGeneratorRequest, ClosureData, Expr, JSAsyncGenerator, JSObjectDataPtr, JSPromise, Statement, StatementKind, Value,
    evaluate_statements, new_js_object_data, object_get_key_value, object_set_key_value, prepare_function_call_env,
};
use crate::core::{GcPtr, MutationContext, new_gc_cell_ptr};
use crate::error::JSError;
use crate::js_promise::{make_promise_js_object, reject_promise, resolve_promise};

// Create an async generator instance (object) when an async generator function is called.
pub fn handle_async_generator_function_call<'gc>(
    mc: &MutationContext<'gc>,
    closure: &ClosureData<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, JSError> {
    // Create the async generator instance object
    let gen_obj = new_js_object_data(mc);

    // Create internal async generator struct
    let async_gen = new_gc_cell_ptr(
        mc,
        JSAsyncGenerator {
            params: closure.params.clone(),
            body: closure.body.clone(),
            env: closure.env.expect("closure env must exist"),
            args: args.to_vec(),
            state: crate::core::GeneratorState::NotStarted,
            cached_initial_yield: None,
            pending: Vec::new(),
        },
    );

    // Store it on the object under a hidden key
    object_set_key_value(mc, &gen_obj, "__async_generator__", Value::AsyncGenerator(async_gen))?;

    // Set prototype to AsyncGenerator.prototype if available
    if let Some(gen_ctor_val) = crate::core::env_get(closure.env.as_ref().expect("AsyncGenerator needs env"), "AsyncGenerator")
        && let Value::Object(gen_ctor_obj) = &*gen_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(gen_ctor_obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        gen_obj.borrow_mut(mc).prototype = Some(*proto_obj);
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
        | StatementKind::ForOfDestructuringArray(_, _, _, body) => body.iter().any(stmt_contains_yield_or_await),
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
                if let Some((idx, inner_idx_opt, yield_inner)) = crate::js_generator::find_first_yield_in_statements(&gen_ptr_mut.body) {
                    let func_env = crate::core::prepare_function_call_env(
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
                    crate::js_class::create_arguments_object(mc, &func_env, &gen_ptr_mut.args, None)?;

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
                            Ok(val) => {
                                gen_ptr_mut.cached_initial_yield = Some(val.clone());
                                let res_obj = create_iterator_result_obj(mc, val, false)?;
                                crate::js_promise::resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                                // suspended: stop processing further requests
                                return Ok(());
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
                    let func_env = prepare_function_call_env(
                        mc,
                        Some(&gen_ptr_mut.env),
                        None,
                        Some(&gen_ptr_mut.params[..]),
                        &gen_ptr_mut.args,
                        None,
                        None,
                    )?;

                    // Ensure `arguments` exists for the no-yield completion path too.
                    crate::js_class::create_arguments_object(mc, &func_env, &gen_ptr_mut.args, None)?;

                    match evaluate_statements(mc, &func_env, &gen_ptr_mut.body) {
                        Ok(v) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            let res_obj = create_iterator_result_obj(mc, v, true)?;
                            resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                            continue;
                        }
                        Err(e) => {
                            gen_ptr_mut.state = GeneratorState::Completed;
                            reject_promise(mc, &promise_cell, Value::String(crate::unicode::utf8_to_utf16(&e.message())), env);
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
                // Resume execution from pc: run remaining tail to completion, but
                // first substitute the `yield` with the provided send value (or
                // cached initial yield) so the suspended point receives the value.
                let pc_val = *pc;
                let mut tail: Vec<crate::core::Statement> = if pc_val < gen_ptr_mut.body.len() {
                    gen_ptr_mut.body[pc_val..].to_vec()
                } else {
                    vec![]
                };

                // Replace first yield occurrence in the first statement with the
                // special variable `__gen_throw_val` so we can inject the send value
                // into it when executing.
                let mut replaced = false;
                if let Some(first_stmt) = tail.get_mut(0) {
                    crate::js_generator::replace_first_yield_in_statement(first_stmt, &_send_value, &mut replaced);
                }

                // Use the pre-execution environment if available so bindings created
                // by pre-statements remain visible when we resume execution.
                let parent_env = if let Some(env) = pre_env.as_ref() { env } else { &gen_ptr_mut.env };
                let func_env = crate::core::prepare_function_call_env(mc, Some(parent_env), None, None, &[], None, None)?;

                // Prefer the queued send value if it is concrete; otherwise fall back
                // to the cached initially-yielded value if present.
                if let Value::Undefined = _send_value {
                    if let Some(cached) = gen_ptr_mut.cached_initial_yield.as_ref() {
                        object_set_key_value(mc, &func_env, "__gen_throw_val", cached.clone())?;
                    } else {
                        object_set_key_value(mc, &func_env, "__gen_throw_val", _send_value.clone())?;
                    }
                } else {
                    object_set_key_value(mc, &func_env, "__gen_throw_val", _send_value.clone())?;
                }

                // Execute the (possibly modified) tail
                match crate::core::evaluate_statements(mc, &func_env, &tail) {
                    Ok(v) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        let res_obj = create_iterator_result_obj(mc, v, true)?;
                        resolve_promise(mc, &promise_cell, Value::Object(res_obj), env);
                        continue;
                    }
                    Err(e) => {
                        gen_ptr_mut.state = GeneratorState::Completed;
                        reject_promise(mc, &promise_cell, Value::String(crate::unicode::utf8_to_utf16(&e.message())), env);
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

                let parent_env = if let Some(env) = pre_env.as_ref() { env } else { &gen_ptr_mut.env };
                let func_env = crate::core::prepare_function_call_env(mc, Some(parent_env), None, None, &[], None, None)?;
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
                        reject_promise(mc, &promise_cell, Value::String(crate::unicode::utf8_to_utf16(&e.message())), env);
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

                let parent_env = if let Some(env) = pre_env.as_ref() { env } else { &gen_ptr_mut.env };
                let func_env = crate::core::prepare_function_call_env(mc, Some(parent_env), None, None, &[], None, None)?;
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
                        reject_promise(mc, &promise_cell, Value::String(crate::unicode::utf8_to_utf16(&e.message())), env);
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
