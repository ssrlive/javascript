use crate::core::{Gc, GcCell, GeneratorState, MutationContext};
use crate::{
    core::{
        EvalError, Expr, JSObjectDataPtr, PropertyKey, Statement, StatementKind, Value, VarDeclKind, env_get, env_set, env_set_recursive,
        evaluate_call_dispatch, evaluate_expr, object_get_key_value, object_set_key_value, prepare_function_call_env,
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

    let call_res = crate::core::evaluate_call_dispatch(mc, env, return_val, Some(Value::Object(*iter_obj)), vec![])?;
    if !matches!(call_res, Value::Object(_)) {
        return Err(raise_type_error!("Iterator return did not return an object").into());
    }

    Ok(())
}

fn get_iterator<'gc>(
    mc: &MutationContext<'gc>,
    val: Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<JSObjectDataPtr<'gc>, EvalError<'gc>> {
    if let Some(sym_ctor_val) = crate::core::env_get(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor_val.borrow()
        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
        && let Value::Object(o) = val
    {
        let method = crate::core::get_property_with_accessors(mc, env, &o, PropertyKey::Symbol(*iter_sym))?;
        if !matches!(method, Value::Undefined | Value::Null) {
            let iter = crate::core::evaluate_call_dispatch(mc, env, method, Some(val), vec![])?;
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
            let res = evaluate_call_dispatch(mc, env, method, Some(iter_val.clone()), vec![])?;
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
    this_val: Option<Value<'gc>>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    // Eagerly initialize the function environment to enforce argument destructuring/defaults
    // errors at call time (per spec), rather than delaying to the first .next() call.
    let func_env = prepare_function_call_env(
        mc,
        closure.env.as_ref(),
        this_val.clone(),
        Some(&closure.params[..]),
        args,
        None,
        None,
    )?;

    // Create a new generator object (internal data)
    let generator = Gc::new(
        mc,
        GcCell::new(crate::core::JSGenerator {
            params: closure.params.clone(),
            body: closure.body.clone(),
            env: func_env,
            this_val,
            // Store call-time arguments so parameter bindings can be created
            // when the generator actually starts executing on the first `next()`.
            args: args.to_vec(),
            state: crate::core::GeneratorState::NotStarted,
            cached_initial_yield: None,
            pending_iterator: None,
            pending_iterator_done: false,
            yield_star_iterator: None,
            pending_for_await: None,
        }),
    );

    // Create a wrapper object for the generator
    let gen_obj = crate::core::new_js_object_data(mc);

    // Store the actual generator data
    object_set_key_value(mc, &gen_obj, "__generator__", Value::Generator(generator))?;

    object_set_key_value(mc, &gen_obj, "__in_generator", Value::Boolean(true))?;

    // Set prototype to Generator.prototype if available
    if let Some(gen_ctor_val) = crate::core::env_get(closure.env.as_ref().expect("Generator needs env"), "Generator")
        && let Value::Object(gen_ctor_obj) = &*gen_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(gen_ctor_obj, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        gen_obj.borrow_mut(mc).prototype = Some(*proto_obj);
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

            generator_next(mc, generator, send_value)
        }
        "return" => {
            // Return a value and close the generator
            let return_value = if args.is_empty() { Value::Undefined } else { args[0].clone() };

            Ok(generator_return(mc, generator, return_value)?)
        }
        "throw" => {
            // Throw an exception into the generator
            let throw_value = if args.is_empty() { Value::Undefined } else { args[0].clone() };

            // If generator_throw indicates a thrown JS value we should propagate
            match generator_throw(mc, generator, throw_value.clone()) {
                Ok(v) => Ok(v),
                Err(_e) => {
                    // If the generator threw a JS value, it is represented via "throw_value" already
                    // generator_throw returns a JSError for thrown values via raise_throw_error, but to preserve
                    // the original thrown JS value we convert to EvalError::Throw with the original value here.
                    // Inspect e.kind to detect Throw; but since generator_throw was passed the actual thrown value
                    // we can simply propagate it.
                    Err(crate::core::js_error::EvalError::Throw(throw_value, None, None))
                }
            }
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
                log::trace!("Replacing Yield");
                *replaced = true;
                Expr::Var(var_name.to_string(), None, None)
            }
        }
        Expr::YieldStar(inner) => {
            let new_inner = Box::new(replace_first_yield_in_expr(inner, var_name, replaced));
            if *replaced {
                *new_inner
            } else {
                log::trace!("Replacing YieldStar");
                *replaced = true;
                Expr::Var(var_name.to_string(), None, None)
            }
        }
        Expr::Await(inner) => {
            let new_inner = Box::new(replace_first_yield_in_expr(inner, var_name, replaced));
            if *replaced {
                Expr::Await(new_inner)
            } else {
                log::trace!("Replacing Await");
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
                .map(|(k, v, is_method)| {
                    (
                        replace_first_yield_in_expr(k, var_name, replaced),
                        replace_first_yield_in_expr(v, var_name, replaced),
                        *is_method,
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
        _ => expr.clone(),
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
        StatementKind::ForOf(_, _, _, body)
        | StatementKind::ForIn(_, _, _, body)
        | StatementKind::ForOfDestructuringObject(_, _, _, body)
        | StatementKind::ForOfDestructuringArray(_, _, _, body) => {
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
        Expr::Object(pairs) => pairs.iter().any(|(k, v, _)| expr_contains_yield(k) || expr_contains_yield(v)),
        Expr::Array(items) => items.iter().any(|it| it.as_ref().is_some_and(expr_contains_yield)),
        Expr::UnaryNeg(a)
        | Expr::LogicalNot(a)
        | Expr::TypeOf(a)
        | Expr::Delete(a)
        | Expr::Void(a)
        | Expr::PostIncrement(a)
        | Expr::PostDecrement(a)
        | Expr::Increment(a)
        | Expr::Decrement(a) => expr_contains_yield(a),
        Expr::LogicalAnd(a, b) | Expr::LogicalOr(a, b) | Expr::Comma(a, b) | Expr::Conditional(a, b, _) => {
            expr_contains_yield(a) || expr_contains_yield(b)
        }
        Expr::OptionalCall(a, args) => expr_contains_yield(a) || args.iter().any(expr_contains_yield),
        Expr::OptionalIndex(a, b) => expr_contains_yield(a) || expr_contains_yield(b),
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
            .find_map(|(k, v, _)| find_yield_in_expr(k).or_else(|| find_yield_in_expr(v))),
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
                        env_set(mc, env, name, val)?;
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
                            env_set(mc, env, name, val)?;
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
                    env_set(mc, env, name, val)?;
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
                    env_set(mc, env, name, val)?;
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
    let expr = if let StatementKind::Expr(e) = &*stmt.kind {
        e
    } else {
        return Ok(None);
    };

    let mut pre_steps: usize = 0;
    let rhs = if let Some((lhs, rhs)) = find_array_assign(expr) {
        if let Expr::Array(elements) = lhs {
            let mut consumed = 0usize;
            for elem_opt in elements.iter() {
                match elem_opt {
                    None => {
                        consumed += 1;
                    }
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
                            } else {
                                consumed += 1;
                            }
                        }
                    },
                }
            }
        }
        rhs
    } else if let Some(rhs) = find_rightmost_assign_rhs(expr) {
        rhs
    } else {
        return Ok(None);
    };

    let rhs_val = crate::core::evaluate_expr(mc, eval_env, rhs)?;
    if matches!(rhs_val, Value::Undefined | Value::Null) {
        return Ok(None);
    }

    let mut iterator: Option<JSObjectDataPtr<'gc>> = None;
    if let Some(sym_ctor) = crate::core::env_get(eval_env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(iter_sym) = object_get_key_value(sym_obj, "iterator")
        && let Value::Symbol(iter_sym_data) = &*iter_sym.borrow()
    {
        let method = if let Value::Object(obj) = &rhs_val {
            crate::core::get_property_with_accessors(mc, eval_env, obj, iter_sym_data)?
        } else {
            Value::Undefined
        };
        if !matches!(method, Value::Undefined | Value::Null) {
            let res = crate::core::evaluate_call_dispatch(mc, eval_env, method, Some(rhs_val.clone()), vec![])?;
            if let Value::Object(iter_obj) = res {
                iterator = Some(iter_obj);
            }
        }
    }

    if let Some(iter_obj) = iterator {
        let mut done = false;
        if pre_steps > 0 {
            let next_method = crate::core::get_property_with_accessors(mc, eval_env, &iter_obj, "next")?;
            if !matches!(next_method, Value::Undefined | Value::Null) {
                for _ in 0..pre_steps {
                    let next_res_val = evaluate_call_dispatch(mc, eval_env, next_method.clone(), Some(Value::Object(iter_obj)), vec![])?;
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
        return Ok(Some((iter_obj, done)));
    }

    Ok(None)
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
            StatementKind::While(_, body) | StatementKind::DoWhile(body, _) => {
                if let Some((inner_idx, _inner_opt, kind, found)) = find_first_yield_in_statements(body)
                    && matches!(kind, YieldKind::Yield | YieldKind::YieldStar | YieldKind::Await)
                {
                    return Some((i, Some(inner_idx), kind, found));
                }
            }
            StatementKind::ForOf(_, _, _, body)
            | StatementKind::ForIn(_, _, _, body)
            | StatementKind::ForOfDestructuringObject(_, _, _, body)
            | StatementKind::ForOfDestructuringArray(_, _, _, body)
            | StatementKind::ForAwaitOf(_, _, _, body)
            | StatementKind::ForAwaitOfDestructuringObject(_, _, _, body)
            | StatementKind::ForAwaitOfDestructuringArray(_, _, _, body)
            | StatementKind::ForAwaitOfExpr(_, _, body)
            | StatementKind::ForOfExpr(_, _, body) => {
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
    generator: &crate::core::GcPtr<'gc, crate::core::JSGenerator<'gc>>,
    _send_value: Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut gen_obj = generator.borrow_mut(mc);

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

            object_set_key_value(mc, &func_env, "__in_generator", Value::Boolean(true))?;

            if let Some((idx, decl_kind_opt, var_name, iterable, body)) = find_first_for_await_in_statements(&gen_obj.body)
                && idx == 0
            {
                let iter_val = evaluate_expr(mc, &func_env, &iterable)?;
                let (iter_obj, is_async_iter) = get_for_await_iterator(mc, &func_env, &iter_val)?;

                let next_method = object_get_key_value(&iter_obj, "next")
                    .ok_or(raise_type_error!("Iterator has no next method"))?
                    .borrow()
                    .clone();
                let next_res_val = evaluate_call_dispatch(mc, &func_env, next_method, Some(Value::Object(iter_obj)), vec![])?;

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

                return Ok(create_iterator_result(mc, next_res_val, false)?);
            }

            if let Some((idx, inner_idx_opt, yield_kind, yield_inner)) = find_first_yield_in_statements(&gen_obj.body) {
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
                        if object_get_key_value(&func_env, &pre_key).is_none() {
                            // Execute pre-statements before the yield for inner containers.
                            match &*gen_obj.body[idx].kind {
                                StatementKind::Block(inner_stmts) => {
                                    let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                                    let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                }
                                StatementKind::TryCatch(tc_stmt) => {
                                    let pre_stmts = tc_stmt.try_body[0..inner_idx].to_vec();
                                    let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                    seed_simple_decl_bindings(mc, &func_env, &pre_stmts)?;
                                }
                                _ => {}
                            }
                            if let Err(e) = object_set_key_value(mc, &func_env, &pre_key, Value::Boolean(true)) {
                                log::warn!("Error setting pre-execution env key: {e}");
                            }
                        }
                    }
                    Some(func_env)
                } else {
                    // Even when there are no pre-statements, we need the function
                    // env to hold parameter bindings for later resume.
                    Some(func_env)
                };

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
                    object_set_key_value(mc, &func_env, "__gen_throw_val", Value::Undefined)?;
                    match crate::core::evaluate_expr(mc, &func_env, &inner_expr_box) {
                        Ok(val) => {
                            if matches!(yield_kind, YieldKind::YieldStar) {
                                let iterator = get_iterator(mc, val, &func_env)?;
                                let next_method = crate::core::get_property_with_accessors(mc, &func_env, &iterator, "next")?;
                                let iter_res =
                                    crate::core::evaluate_call_dispatch(mc, &func_env, next_method, Some(Value::Object(iterator)), vec![])?;

                                if let Value::Object(res_obj) = iter_res {
                                    let done_val = object_get_key_value(&res_obj, "done")
                                        .map(|v| v.borrow().clone())
                                        .unwrap_or(Value::Boolean(false));
                                    let done = matches!(done_val, Value::Boolean(true));
                                    let value = object_get_key_value(&res_obj, "value")
                                        .map(|v| v.borrow().clone())
                                        .unwrap_or(Value::Undefined);

                                    if !done {
                                        gen_obj.yield_star_iterator = Some(iterator);
                                        gen_obj.cached_initial_yield = Some(value.clone());
                                        return Ok(create_iterator_result(mc, value, false)?);
                                    } else {
                                        // Done immediately. Recurse with done value.
                                        drop(gen_obj);
                                        return generator_next(mc, generator, value);
                                    }
                                } else {
                                    return Err(raise_type_error!("Iterator result is not an object").into());
                                }
                            }

                            // Cache the value so re-entry/resume paths can use it
                            gen_obj.cached_initial_yield = Some(val.clone());
                            return Ok(create_iterator_result(mc, val, false)?);
                        }
                        Err(e) => return Err(e),
                    }
                }

                // No inner expression -> yield undefined
                Ok(create_iterator_result(mc, Value::Undefined, false)?)
            } else {
                // No yields found: execute the whole function body in a freshly
                // prepared function activation environment using the captured
                // call-time arguments, then complete the generator with the
                // returned value.
                // NOTE: We now create the environment eagerly in handle_generator_function_call,
                // so we just use the stored environment.
                let func_env = gen_obj.env;

                // Ensure `arguments` exists for the no-yield completion path too.
                crate::js_class::create_arguments_object(mc, &func_env, &gen_obj.args, None)?;
                object_set_key_value(mc, &func_env, "__in_generator", Value::Boolean(true))?;

                let res = crate::core::evaluate_statements(mc, &func_env, &gen_obj.body);
                gen_obj.state = GeneratorState::Completed;
                Ok(create_iterator_result(mc, res?, true)?)
            }
        }
        GeneratorState::Suspended { pc, stack: _, pre_env } => {
            if let Some(mut for_await) = gen_obj.pending_for_await.take() {
                let func_env = gen_obj.env;
                let pc_val = pc;

                if for_await.awaiting_value {
                    let iter_env = if let Some(VarDeclKind::Let) | Some(VarDeclKind::Const) = for_await.decl_kind {
                        let e = crate::core::new_js_object_data(mc);
                        e.borrow_mut(mc).prototype = Some(func_env);
                        env_set(mc, &e, &for_await.var_name, _send_value.clone())?;
                        e
                    } else {
                        env_set_recursive(mc, &func_env, &for_await.var_name, _send_value.clone())?;
                        func_env
                    };

                    crate::core::evaluate_statements(mc, &iter_env, &for_await.body)?;

                    let next_method = object_get_key_value(&for_await.iterator, "next")
                        .ok_or(raise_type_error!("Iterator has no next method"))?
                        .borrow()
                        .clone();
                    let next_res_val = evaluate_call_dispatch(mc, &func_env, next_method, Some(Value::Object(for_await.iterator)), vec![])?;

                    for_await.awaiting_value = false;
                    gen_obj.pending_for_await = Some(for_await);
                    gen_obj.state = GeneratorState::Suspended {
                        pc: pc_val,
                        stack: vec![],
                        pre_env,
                    };

                    return Ok(create_iterator_result(mc, next_res_val, false)?);
                }

                let next_res = if let Value::Object(obj) = _send_value {
                    obj
                } else {
                    return Err(raise_type_error!("Iterator result is not an object").into());
                };

                let done = if let Some(done_val) = object_get_key_value(&next_res, "done") {
                    done_val.borrow().to_truthy()
                } else {
                    false
                };

                if done {
                    gen_obj.pending_for_await = None;
                    if for_await.resume_pc >= gen_obj.body.len() {
                        gen_obj.state = GeneratorState::Completed;
                        return Ok(create_iterator_result(mc, Value::Undefined, true)?);
                    }
                    gen_obj.state = GeneratorState::Suspended {
                        pc: for_await.resume_pc,
                        stack: vec![],
                        pre_env: Some(func_env),
                    };
                    drop(gen_obj);
                    return generator_next(mc, generator, Value::Undefined);
                }

                let value = if let Some(val) = object_get_key_value(&next_res, "value") {
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

                return Ok(create_iterator_result(mc, value, false)?);
            }

            if let Some(iter) = gen_obj.yield_star_iterator {
                let next_method = crate::core::get_property_with_accessors(mc, &gen_obj.env, &iter, "next")?;
                let iter_res = crate::core::evaluate_call_dispatch(
                    mc,
                    &gen_obj.env,
                    next_method,
                    Some(Value::Object(iter)),
                    vec![_send_value.clone()],
                )?;
                if let Value::Object(res_obj) = iter_res {
                    let done_val = object_get_key_value(&res_obj, "done")
                        .map(|v| v.borrow().clone())
                        .unwrap_or(Value::Boolean(false));
                    let done = matches!(done_val, Value::Boolean(true));
                    let value = object_get_key_value(&res_obj, "value")
                        .map(|v| v.borrow().clone())
                        .unwrap_or(Value::Undefined);

                    if !done {
                        gen_obj.state = GeneratorState::Suspended {
                            pc,
                            stack: vec![],
                            pre_env,
                        };
                        return Ok(create_iterator_result(mc, value, false)?);
                    } else {
                        gen_obj.yield_star_iterator = None;
                        gen_obj.state = GeneratorState::Suspended {
                            pc,
                            stack: vec![],
                            pre_env,
                        };
                        drop(gen_obj);
                        return generator_next(mc, generator, value);
                    }
                } else {
                    return Err(raise_type_error!("Iterator result is not an object").into());
                }
            }

            // On resume, execute from the suspended statement index. If a
            // `send_value` was provided to `next(value)`, substitute the
            // first `yield` in that statement with the sent value before
            // executing.
            let pc_val = pc;
            log::debug!("DEBUG: generator_next Suspended. pc={}, send_value={:?}", pc_val, _send_value);
            gen_obj.pending_iterator = None;
            gen_obj.pending_iterator_done = false;
            if pc_val >= gen_obj.body.len() {
                gen_obj.state = GeneratorState::Completed;
                return Ok(create_iterator_result(mc, Value::Undefined, true)?);
            }
            // Generate a unique variable name for this yield point (based on PC and count)
            // This ensures that subsequent yields don't overwrite the value of this yield in the environment
            let base_name = format!("__gen_yield_val_{}_", pc_val);
            let next_idx = if let Some(s) = gen_obj.body.get(pc_val) {
                count_yield_vars_in_statement(s, &base_name)
            } else {
                0
            };
            let var_name = format!("{}{}", base_name, next_idx);

            // Modify the AST in place to reflect progress (remove init, replace yield)
            let mut replaced = false;
            if let Some(first_stmt) = gen_obj.body.get_mut(pc_val) {
                replace_first_yield_in_statement(first_stmt, &var_name, &mut replaced);
                log::debug!("DEBUG: body[{}] after: replaced={}, kind={:?}", pc_val, replaced, first_stmt.kind);
            }

            // Clone the tail for execution (now contains modifications)
            let mut tail: Vec<Statement> = gen_obj.body[pc_val..].to_vec();

            // Use the pre-execution environment if available so bindings created
            // by pre-statements remain visible when we resume execution.
            let func_env = if let Some(env) = pre_env.as_ref() {
                *env
            } else {
                prepare_function_call_env(mc, Some(&gen_obj.env), gen_obj.this_val.clone(), None, &[], None, None)?
            };

            object_set_key_value(mc, &func_env, &var_name, _send_value.clone())?;
            if let Some(stmt) = gen_obj.body.get(pc_val) {
                bind_replaced_yield_decl(mc, &func_env, stmt, &var_name)?;
            }

            if let Some((idx, inner_idx_opt, _yield_kind, yield_inner)) = find_first_yield_in_statements(&tail) {
                let pre_env_opt: Option<JSObjectDataPtr> = if idx > 0 {
                    let pre_stmts = tail[0..idx].to_vec();
                    crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                    Some(func_env)
                } else if let Some(inner_idx) = inner_idx_opt {
                    if inner_idx > 0 {
                        let pre_key = format!("__gen_pre_exec_{}_{}", var_name, inner_idx);
                        if object_get_key_value(&func_env, &pre_key).is_none() {
                            match &*tail[idx].kind {
                                StatementKind::Block(inner_stmts) => {
                                    let pre_stmts = inner_stmts[0..inner_idx].to_vec();
                                    let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                }
                                StatementKind::TryCatch(tc_stmt) => {
                                    let pre_stmts = tc_stmt.try_body[0..inner_idx].to_vec();
                                    let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
                                    seed_simple_decl_bindings(mc, &func_env, &pre_stmts)?;
                                }
                                _ => {}
                            }
                            if let Err(e) = object_set_key_value(mc, &func_env, &pre_key, Value::Boolean(true)) {
                                log::warn!("Error setting pre-execution env key: {e}");
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
                        if let Some(body_stmt) = gen_obj.body.get_mut(pc_val + idx)
                            && let StatementKind::TryCatch(body_tc_stmt) = body_stmt.kind.as_mut()
                        {
                            body_tc_stmt.try_body.drain(0..inner_idx);
                        }
                    }

                    object_set_key_value(mc, &func_env, "__gen_throw_val", Value::Undefined)?;
                    match crate::core::evaluate_expr(mc, &func_env, &inner_expr_box) {
                        Ok(val) => {
                            gen_obj.cached_initial_yield = Some(val.clone());
                            return Ok(create_iterator_result(mc, val, false)?);
                        }
                        Err(e) => return Err(e),
                    }
                }

                return Ok(create_iterator_result(mc, Value::Undefined, false)?);
            }

            // Execute the (possibly modified) tail
            let result = crate::core::evaluate_statements(mc, &func_env, &tail);
            log::trace!("DEBUG: evaluate_statements result: {:?}", result);
            gen_obj.state = GeneratorState::Completed;
            match result {
                Ok(val) => match create_iterator_result(mc, val, true) {
                    Ok(r) => Ok(r),
                    Err(e) => Err(e.into()),
                },
                Err(e) => Err(e),
            }
        }
        GeneratorState::Running { .. } => Err(raise_eval_error!("Generator is already running").into()),
        GeneratorState::Completed => Ok(create_iterator_result(mc, Value::Undefined, true)?),
    }
}

/// Execute generator.return()
fn generator_return<'gc>(
    mc: &MutationContext<'gc>,
    generator: &crate::core::GcPtr<'gc, crate::core::JSGenerator<'gc>>,
    return_value: Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut gen_obj = generator.borrow_mut(mc);
    if let Some(iter_obj) = gen_obj.pending_iterator {
        if !gen_obj.pending_iterator_done {
            close_pending_iterator(mc, &gen_obj.env, &iter_obj)?;
        }
        gen_obj.pending_iterator = None;
        gen_obj.pending_iterator_done = false;
    }
    gen_obj.state = GeneratorState::Completed;
    Ok(create_iterator_result(mc, return_value, true)?)
}

/// Execute generator.throw()
pub fn generator_throw<'gc>(
    mc: &MutationContext<'gc>,
    generator: &crate::core::GcPtr<'gc, crate::core::JSGenerator<'gc>>,
    throw_value: Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let mut gen_obj = generator.borrow_mut(mc);
    match &mut gen_obj.state {
        GeneratorState::NotStarted => {
            // Throwing into a not-started generator throws synchronously
            Err(EvalError::Throw(throw_value, None, None))
        }
        GeneratorState::Suspended { pc, .. } => {
            // Replace the suspended statement with a Throw containing the thrown value
            let pc_val = *pc;
            if pc_val >= gen_obj.body.len() {
                gen_obj.state = GeneratorState::Completed;
                return Err(EvalError::Throw(throw_value, None, None));
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
                if replace_first_yield_statement_with_throw(s, &throw_value) {
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
                prepare_function_call_env(mc, Some(&gen_obj.env), gen_obj.this_val.clone(), None, &[], None, None)?
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

            object_set_key_value(mc, &func_env, "__gen_throw_val", throw_value.clone())?;

            // Execute the modified tail. If the throw is uncaught, evaluate_statements
            // will return Err and we should propagate that to the caller.
            let result = crate::core::evaluate_statements(mc, &func_env, &tail);
            // Don't blindly set Completed; check if it returned from a yield or completion
            // NOTE: Current implementation of evaluate_statements does not support Yield.
            // If the generator contains subsequent yields, evaluate_statements may fail.
            // For now, we assume simple throw-catch-return or throw-catch-end behavior.

            gen_obj.state = GeneratorState::Completed;

            match result {
                Ok(val) => Ok(create_iterator_result(mc, val, true)?),
                Err(e) => Err(e),
            }
        }
        GeneratorState::Running { .. } => Err(raise_eval_error!("Generator is already running").into()),
        GeneratorState::Completed => Err(raise_eval_error!("Generator has already completed").into()),
    }
}

/// Create an iterator result object {value: value, done: done}
fn create_iterator_result<'gc>(mc: &MutationContext<'gc>, value: Value<'gc>, done: bool) -> Result<Value<'gc>, JSError> {
    // Iterator result objects should be extensible by default
    let obj = crate::core::new_js_object_data(mc);

    // Set value property
    object_set_key_value(mc, &obj, "value", value)?;

    // Set done property
    object_set_key_value(mc, &obj, "done", Value::Boolean(done))?;

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

    // Attach prototype methods as named functions that dispatch to the generator handler
    let val = Value::Function("Generator.prototype.next".to_string());
    object_set_key_value(mc, &gen_proto, "next", val)?;

    let val = Value::Function("Generator.prototype.return".to_string());
    object_set_key_value(mc, &gen_proto, "return", val)?;

    let val = Value::Function("Generator.prototype.throw".to_string());
    object_set_key_value(mc, &gen_proto, "throw", val)?;

    // Register Symbol.iterator on Generator.prototype -> returns the generator object itself
    if let Some(sym_ctor) = object_get_key_value(env, "Symbol")
        && let Value::Object(sym_obj) = &*sym_ctor.borrow()
        && let Some(iter_sym_val) = object_get_key_value(sym_obj, "iterator")
        && let Value::Symbol(iter_sym) = &*iter_sym_val.borrow()
    {
        // Create a function name recognized by the call dispatcher
        let val = Value::Function("Generator.prototype.iterator".to_string());
        object_set_key_value(mc, &gen_proto, iter_sym, val)?;
        gen_proto
            .borrow_mut(mc)
            .set_non_enumerable(crate::core::PropertyKey::Symbol(*iter_sym));
    }

    // link prototype to constructor and expose on global env
    object_set_key_value(mc, &gen_ctor, "prototype", Value::Object(gen_proto))?;
    crate::core::env_set(mc, env, "Generator", Value::Object(gen_ctor))?;
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
                .map(|(k, v, _)| count_yield_vars_in_expr(k, prefix) + count_yield_vars_in_expr(v, prefix))
                .sum(),
            Expr::Array(items) => items
                .iter()
                .map(|i| i.as_ref().map_or(0, |x| count_yield_vars_in_expr(x, prefix)))
                .sum(),
            Expr::Conditional(a, b, c) => {
                count_yield_vars_in_expr(a, prefix) + count_yield_vars_in_expr(b, prefix) + count_yield_vars_in_expr(c, prefix)
            }
            Expr::Property(a, _) => count_yield_vars_in_expr(a, prefix),
            _ => 0,
        }
}
