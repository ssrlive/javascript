use crate::core::{Gc, GcCell, GeneratorState, MutationContext};
use crate::{
    core::{
        Expr, JSObjectDataPtr, PropertyKey, Statement, StatementKind, Value, obj_set_key_value, object_get_key_value,
        prepare_function_call_env,
    },
    error::JSError,
};

/// Handle generator function constructor (when called as `new GeneratorFunction(...)`)
pub fn _handle_generator_function_constructor<'gc>(
    _mc: &MutationContext<'gc>,
    _args: &[Expr],
    _env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, JSError> {
    // Generator functions cannot be constructed with `new`
    Err(raise_eval_error!("GeneratorFunction is not a constructor"))
}

/// Handle generator function calls (creating generator objects)
pub fn handle_generator_function_call<'gc>(
    mc: &MutationContext<'gc>,
    closure: &crate::core::ClosureData<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, JSError> {
    // Create a new generator object (internal data)
    let generator = Gc::new(
        mc,
        GcCell::new(crate::core::JSGenerator {
            params: closure.params.clone(),
            body: closure.body.clone(),
            env: closure.env,
            // Store call-time arguments so parameter bindings can be created
            // when the generator actually starts executing on the first `next()`.
            args: args.to_vec(),
            state: crate::core::GeneratorState::NotStarted,
        }),
    );

    // Create a wrapper object for the generator
    let gen_obj = crate::core::new_js_object_data(mc);

    // Store the actual generator data
    crate::core::obj_set_key_value(mc, &gen_obj, &"__generator__".into(), Value::Generator(generator))?;

    // Set prototype to Generator.prototype if available
    if let Some(gen_ctor_val) = crate::core::env_get(&closure.env, "Generator")
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
) -> Result<Value<'gc>, JSError> {
    match method {
        "next" => {
            // Get optional value to send to the generator
            let send_value = if args.is_empty() { Value::Undefined } else { args[0].clone() };

            generator_next(mc, generator, send_value)
        }
        "return" => {
            // Return a value and close the generator
            let return_value = if args.is_empty() { Value::Undefined } else { args[0].clone() };

            generator_return(mc, generator, return_value)
        }
        "throw" => {
            // Throw an exception into the generator
            let throw_value = if args.is_empty() { Value::Undefined } else { args[0].clone() };

            generator_throw(mc, generator, throw_value)
        }
        _ => Err(raise_eval_error!(format!("Generator.prototype.{} is not implemented", method))),
    }
}

// Helper to replace the first `yield` occurrence inside an Expr with a
// provided `send_value`. `replaced` becomes true once a replacement is made.
fn replace_first_yield_in_expr(expr: &Expr, _send_value: &Value, replaced: &mut bool) -> Expr {
    use crate::core::Expr;
    match expr {
        Expr::Yield(_) => {
            if !*replaced {
                *replaced = true;
                Expr::Var("__gen_throw_val".to_string(), None, None)
            } else {
                expr.clone()
            }
        }
        Expr::YieldStar(_) => {
            if !*replaced {
                *replaced = true;
                Expr::Var("__gen_throw_val".to_string(), None, None)
            } else {
                expr.clone()
            }
        }
        Expr::Await(_) => {
            if !*replaced {
                *replaced = true;
                Expr::Var("__gen_throw_val".to_string(), None, None)
            } else {
                expr.clone()
            }
        }
        Expr::Binary(a, op, b) => Expr::Binary(
            Box::new(replace_first_yield_in_expr(a, _send_value, replaced)),
            *op,
            Box::new(replace_first_yield_in_expr(b, _send_value, replaced)),
        ),
        Expr::Assign(a, b) => Expr::Assign(
            Box::new(replace_first_yield_in_expr(a, _send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, _send_value, replaced)),
        ),
        Expr::Index(a, b) => Expr::Index(
            Box::new(replace_first_yield_in_expr(a, _send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, _send_value, replaced)),
        ),
        Expr::Property(a, s) => Expr::Property(Box::new(replace_first_yield_in_expr(a, _send_value, replaced)), s.clone()),
        Expr::Call(a, args) => Expr::Call(
            Box::new(replace_first_yield_in_expr(a, _send_value, replaced)),
            args.iter()
                .map(|arg| replace_first_yield_in_expr(arg, _send_value, replaced))
                .collect(),
        ),
        Expr::Object(pairs) => Expr::Object(
            pairs
                .iter()
                .map(|(k, v, is_method)| {
                    (
                        replace_first_yield_in_expr(k, _send_value, replaced),
                        replace_first_yield_in_expr(v, _send_value, replaced),
                        *is_method,
                    )
                })
                .collect(),
        ),
        Expr::Array(items) => Expr::Array(
            items
                .iter()
                .map(|it| it.as_ref().map(|e| replace_first_yield_in_expr(e, _send_value, replaced)))
                .collect(),
        ),
        Expr::LogicalNot(a) => Expr::LogicalNot(Box::new(replace_first_yield_in_expr(a, _send_value, replaced))),
        Expr::TypeOf(a) => Expr::TypeOf(Box::new(replace_first_yield_in_expr(a, _send_value, replaced))),
        Expr::Delete(a) => Expr::Delete(Box::new(replace_first_yield_in_expr(a, _send_value, replaced))),
        Expr::Void(a) => Expr::Void(Box::new(replace_first_yield_in_expr(a, _send_value, replaced))),
        Expr::Increment(a) => Expr::Increment(Box::new(replace_first_yield_in_expr(a, _send_value, replaced))),
        Expr::Decrement(a) => Expr::Decrement(Box::new(replace_first_yield_in_expr(a, _send_value, replaced))),
        Expr::PostIncrement(a) => Expr::PostIncrement(Box::new(replace_first_yield_in_expr(a, _send_value, replaced))),
        Expr::PostDecrement(a) => Expr::PostDecrement(Box::new(replace_first_yield_in_expr(a, _send_value, replaced))),
        Expr::LogicalAnd(a, b) => Expr::LogicalAnd(
            Box::new(replace_first_yield_in_expr(a, _send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, _send_value, replaced)),
        ),
        Expr::LogicalOr(a, b) => Expr::LogicalOr(
            Box::new(replace_first_yield_in_expr(a, _send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, _send_value, replaced)),
        ),
        Expr::Comma(a, b) => Expr::Comma(
            Box::new(replace_first_yield_in_expr(a, _send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, _send_value, replaced)),
        ),
        Expr::Spread(a) => Expr::Spread(Box::new(replace_first_yield_in_expr(a, _send_value, replaced))),
        Expr::OptionalCall(a, args) => Expr::OptionalCall(
            Box::new(replace_first_yield_in_expr(a, _send_value, replaced)),
            args.iter()
                .map(|arg| replace_first_yield_in_expr(arg, _send_value, replaced))
                .collect(),
        ),
        Expr::OptionalIndex(a, b) => Expr::OptionalIndex(
            Box::new(replace_first_yield_in_expr(a, _send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, _send_value, replaced)),
        ),
        Expr::Conditional(a, b, c) => Expr::Conditional(
            Box::new(replace_first_yield_in_expr(a, _send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, _send_value, replaced)),
            Box::new(replace_first_yield_in_expr(c, _send_value, replaced)),
        ),
        _ => expr.clone(),
    }
}

fn replace_first_yield_in_statement(stmt: &mut Statement, send_value: &Value, replaced: &mut bool) {
    match stmt.kind.as_mut() {
        StatementKind::Expr(e) => {
            *e = replace_first_yield_in_expr(e, send_value, replaced);
        }
        StatementKind::Let(decls) | StatementKind::Var(decls) => {
            for (_, expr_opt) in decls.iter_mut() {
                if let Some(expr) = expr_opt {
                    *expr = replace_first_yield_in_expr(expr, send_value, replaced);
                }
            }
        }
        StatementKind::Const(decls) => {
            for (_, expr) in decls.iter_mut() {
                *expr = replace_first_yield_in_expr(expr, send_value, replaced);
            }
        }
        StatementKind::Return(Some(expr)) => {
            *expr = replace_first_yield_in_expr(expr, send_value, replaced);
        }
        StatementKind::If(if_stmt) => {
            let if_stmt = if_stmt.as_mut();
            let cond = if_stmt.condition.clone();
            if_stmt.condition = replace_first_yield_in_expr(&cond, send_value, replaced);
            for s in if_stmt.then_body.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
                }
            }
            if let Some(else_body) = if_stmt.else_body.as_mut() {
                for s in else_body.iter_mut() {
                    replace_first_yield_in_statement(s, send_value, replaced);
                    if *replaced {
                        return;
                    }
                }
            }
        }
        StatementKind::For(for_stmt) => {
            let for_stmt = for_stmt.as_mut();
            if let Some(cond) = for_stmt.test.as_mut() {
                *cond = replace_first_yield_in_expr(cond, send_value, replaced);
            }
            for s in for_stmt.body.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
                }
            }
        }
        StatementKind::While(cond, body) => {
            *cond = replace_first_yield_in_expr(cond, send_value, replaced);
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
                }
            }
        }
        StatementKind::DoWhile(body, cond) => {
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
                }
            }
            *cond = replace_first_yield_in_expr(cond, send_value, replaced);
        }
        StatementKind::ForOf(_, _, body)
        | StatementKind::ForIn(_, _, body)
        | StatementKind::ForOfDestructuringObject(_, _, body)
        | StatementKind::ForOfDestructuringArray(_, _, body) => {
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
                }
            }
        }
        StatementKind::Block(stmts) => {
            for s in stmts.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
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
fn replace_first_yield_statement_with_throw(stmt: &mut Statement, _throw_value: &Value) -> bool {
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
        StatementKind::ForOf(_, _, body)
        | StatementKind::ForIn(_, _, body)
        | StatementKind::ForOfDestructuringObject(_, _, body)
        | StatementKind::ForOfDestructuringArray(_, _, body)
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

fn find_yield_in_expr(e: &Expr) -> Option<Option<Box<Expr>>> {
    match e {
        Expr::Yield(inner) => Some(inner.clone()),
        Expr::YieldStar(inner) => Some(Some(inner.clone())),
        Expr::Await(inner) => Some(Some(inner.clone())),
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
        | Expr::PostDecrement(a) => find_yield_in_expr(a),
        Expr::LogicalAnd(a, b) | Expr::LogicalOr(a, b) | Expr::Comma(a, b) | Expr::Conditional(a, b, _) => {
            find_yield_in_expr(a).or_else(|| find_yield_in_expr(b))
        } // Conditional: a then b (if matched) or c? We just scan linearly for now
        Expr::OptionalCall(a, args) => find_yield_in_expr(a).or_else(|| args.iter().find_map(find_yield_in_expr)),
        Expr::OptionalIndex(a, b) => find_yield_in_expr(a).or_else(|| find_yield_in_expr(b)),
        _ => None,
    }
}

// Helper to find a yield expression within statements. Returns the
// index of the containing top-level statement and the inner yield
// expression if found.
fn find_first_yield_in_statements(stmts: &[Statement]) -> Option<(usize, Option<Box<Expr>>)> {
    for (i, s) in stmts.iter().enumerate() {
        match &*s.kind {
            StatementKind::Expr(e) => {
                if let Some(inner) = find_yield_in_expr(e) {
                    return Some((i, inner));
                }
            }
            StatementKind::Return(Some(e)) => {
                if let Some(inner) = find_yield_in_expr(e) {
                    return Some((i, inner));
                }
            }
            StatementKind::Let(decls) | StatementKind::Var(decls) => {
                for (_, expr_opt) in decls {
                    if let Some(expr) = expr_opt
                        && let Some(inner) = find_yield_in_expr(expr)
                    {
                        return Some((i, inner));
                    }
                }
            }
            StatementKind::Const(decls) => {
                for (_, expr) in decls {
                    if let Some(inner) = find_yield_in_expr(expr) {
                        return Some((i, inner));
                    }
                }
            }
            StatementKind::Block(inner_stmts) => {
                if let Some((_inner_idx, found)) = find_first_yield_in_statements(inner_stmts) {
                    return Some((i, found));
                }
            }
            StatementKind::If(if_stmt) => {
                let if_stmt = if_stmt.as_ref();
                if let Some((_inner_idx, found)) = find_first_yield_in_statements(&if_stmt.then_body) {
                    return Some((i, found));
                }
                if let Some(else_body) = &if_stmt.else_body
                    && let Some((_inner_idx, found)) = find_first_yield_in_statements(else_body)
                {
                    return Some((i, found));
                }
            }
            StatementKind::For(for_stmt) => {
                if let Some((_inner_idx, found)) = find_first_yield_in_statements(&for_stmt.body) {
                    return Some((i, found));
                }
            }
            StatementKind::TryCatch(tc_stmt) => {
                if let Some((_inner_idx, found)) = find_first_yield_in_statements(&tc_stmt.try_body) {
                    return Some((i, found));
                }
                if let Some(catch_body) = &tc_stmt.as_ref().catch_body
                    && let Some((_inner_idx, found)) = find_first_yield_in_statements(catch_body)
                {
                    return Some((i, found));
                }
                if let Some(finally_body) = &tc_stmt.as_ref().finally_body
                    && let Some((_inner_idx, found)) = find_first_yield_in_statements(finally_body)
                {
                    return Some((i, found));
                }
            }
            StatementKind::While(_, body) | StatementKind::DoWhile(body, _) => {
                if let Some((_inner_idx, found)) = find_first_yield_in_statements(body) {
                    return Some((i, found));
                }
            }
            StatementKind::ForOf(_, _, body)
            | StatementKind::ForIn(_, _, body)
            | StatementKind::ForOfDestructuringObject(_, _, body)
            | StatementKind::ForOfDestructuringArray(_, _, body) => {
                if let Some((_inner_idx, found)) = find_first_yield_in_statements(body) {
                    return Some((i, found));
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
) -> Result<Value<'gc>, JSError> {
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

            if let Some((idx, yield_inner)) = find_first_yield_in_statements(&gen_obj.body) {
                // Suspend at the containing top-level statement index so
                // that resumed execution re-evaluates the statement with
                // the sent-in value substituted for the `yield`.
                // Execute statements *before* the one containing the first yield so
                // that variable bindings and side-effects (e.g., Promise constructor
                // executors) occur before we evaluate the inner yield expression.
                // Prepare the function activation environment with the captured
                // call-time arguments so that parameter bindings exist even if the
                // generator suspends before executing any pre-statements.
                let func_env =
                    prepare_function_call_env(mc, Some(&gen_obj.env), None, Some(&gen_obj.params[..]), &gen_obj.args, None, None)?;

                let pre_env_opt: Option<JSObjectDataPtr> = if idx > 0 {
                    let pre_stmts = gen_obj.body[0..idx].to_vec();
                    // Execute pre-yield statements in the function env so that
                    // bindings and side-effects are recorded on that environment.
                    let _ = crate::core::evaluate_statements(mc, &func_env, &pre_stmts)?;
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

                // If the yield has an inner expression, evaluate it in a fresh
                // function-like frame whose prototype is the captured env (so it
                // can see bindings from the pre-stmts execution) and return that
                // value as the yielded value.
                if let Some(inner_expr_box) = yield_inner {
                    // Use pre_env so the inner expression can access bindings created
                    // in the function activation environment.
                    let parent_env = pre_env_opt.as_ref().unwrap_or(&gen_obj.env);
                    let func_env = prepare_function_call_env(mc, Some(parent_env), None, None, &[], None, None)?;
                    crate::core::obj_set_key_value(mc, &func_env, &"__gen_throw_val".into(), Value::Undefined)?;
                    match crate::core::evaluate_expr(mc, &func_env, &inner_expr_box) {
                        Ok(val) => return Ok(create_iterator_result(mc, val, false)),
                        Err(_) => return Ok(create_iterator_result(mc, Value::Undefined, false)),
                    }
                }

                // No inner expression -> yield undefined
                Ok(create_iterator_result(mc, Value::Undefined, false))
            } else {
                // No yields found: execute the whole function body in a freshly
                // prepared function activation environment using the captured
                // call-time arguments, then complete the generator with the
                // returned value.
                let func_env =
                    prepare_function_call_env(mc, Some(&gen_obj.env), None, Some(&gen_obj.params[..]), &gen_obj.args, None, None)?;
                let res = crate::core::evaluate_statements(mc, &func_env, &gen_obj.body);
                gen_obj.state = GeneratorState::Completed;
                match res {
                    Ok(v) => Ok(create_iterator_result(mc, v, true)),
                    Err(e) => Err(e.into()),
                }
            }
        }
        GeneratorState::Suspended { pc, stack: _, pre_env } => {
            // On resume, execute from the suspended statement index. If a
            // `send_value` was provided to `next(value)`, substitute the
            // first `yield` in that statement with the sent value before
            // executing.
            let pc_val = pc;
            log::trace!("DEBUG: generator_next Suspended. pc={}, send_value={:?}", pc_val, _send_value);
            if pc_val >= gen_obj.body.len() {
                gen_obj.state = GeneratorState::Completed;
                return Ok(create_iterator_result(mc, Value::Undefined, true));
            }
            // Clone the tail and replace first yield in the first statement
            let mut tail: Vec<Statement> = gen_obj.body[pc_val..].to_vec();
            let mut replaced = false;
            log::trace!("DEBUG: tail[0] before: {:?}", tail[0]);
            if let Some(first_stmt) = tail.get_mut(0) {
                replace_first_yield_in_statement(first_stmt, &_send_value, &mut replaced);
            }
            log::trace!("DEBUG: tail[0] after: replaced={}, stmt={:?}", replaced, tail[0]);

            // Use the pre-execution environment if available so bindings created
            // by pre-statements remain visible when we resume execution.
            let parent_env = if let Some(env) = pre_env.as_ref() { env } else { &gen_obj.env };
            let func_env = prepare_function_call_env(mc, Some(parent_env), None, None, &[], None, None)?;
            obj_set_key_value(mc, &func_env, &"__gen_throw_val".into(), _send_value.clone())?;
            // Execute the (possibly modified) tail
            let result = crate::core::evaluate_statements(mc, &func_env, &tail);
            log::trace!("DEBUG: evaluate_statements result: {:?}", result);
            gen_obj.state = GeneratorState::Completed;
            match result {
                Ok(val) => Ok(create_iterator_result(mc, val, true)),
                Err(_) => Ok(create_iterator_result(mc, Value::Undefined, true)),
            }
        }
        GeneratorState::Running { .. } => Err(raise_eval_error!("Generator is already running")),
        GeneratorState::Completed => Ok(create_iterator_result(mc, Value::Undefined, true)),
    }
}

/// Execute generator.return()
fn generator_return<'gc>(
    mc: &MutationContext<'gc>,
    generator: &crate::core::GcPtr<'gc, crate::core::JSGenerator<'gc>>,
    return_value: Value<'gc>,
) -> Result<Value<'gc>, JSError> {
    let mut gen_obj = generator.borrow_mut(mc);
    gen_obj.state = GeneratorState::Completed;
    Ok(create_iterator_result(mc, return_value, true))
}

/// Execute generator.throw()
pub fn generator_throw<'gc>(
    mc: &MutationContext<'gc>,
    generator: &crate::core::GcPtr<'gc, crate::core::JSGenerator<'gc>>,
    throw_value: Value<'gc>,
) -> Result<Value<'gc>, JSError> {
    let mut gen_obj = generator.borrow_mut(mc);
    match &mut gen_obj.state {
        GeneratorState::NotStarted => {
            // Throwing into a not-started generator throws synchronously
            Err(raise_throw_error!(throw_value))
        }
        GeneratorState::Suspended { pc, .. } => {
            // Replace the suspended statement with a Throw containing the thrown value
            let pc_val = *pc;
            if pc_val >= gen_obj.body.len() {
                gen_obj.state = GeneratorState::Completed;
                return Err(raise_throw_error!(throw_value));
            }
            let mut tail: Vec<Statement> = gen_obj.body[pc_val..].to_vec();
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

            let func_env = prepare_function_call_env(mc, Some(&gen_obj.env), None, None, &[], None, None)?;
            crate::core::obj_set_key_value(mc, &func_env, &"__gen_throw_val".into(), throw_value.clone())?;

            // Execute the modified tail. If the throw is uncaught, evaluate_statements
            // will return Err and we should propagate that to the caller.
            let result = crate::core::evaluate_statements(mc, &func_env, &tail);
            // Don't blindly set Completed; check if it returned from a yield or completion
            // NOTE: Current implementation of evaluate_statements does not support Yield.
            // If the generator contains subsequent yields, evaluate_statements may fail.
            // For now, we assume simple throw-catch-return or throw-catch-end behavior.

            gen_obj.state = GeneratorState::Completed;

            match result {
                Ok(val) => Ok(create_iterator_result(mc, val, true)),
                Err(e) => Err(e.into()),
            }
        }
        GeneratorState::Running { .. } => Err(raise_eval_error!("Generator is already running")),
        GeneratorState::Completed => Err(raise_eval_error!("Generator has already completed")),
    }
}

/// Create an iterator result object {value: value, done: done}
fn create_iterator_result<'gc>(mc: &MutationContext<'gc>, value: Value<'gc>, done: bool) -> Value<'gc> {
    let obj = Gc::new(mc, GcCell::new(crate::core::JSObjectData::default()));

    // Set value property
    obj.borrow_mut(mc)
        .properties
        .insert(PropertyKey::String("value".to_string()), Gc::new(mc, GcCell::new(value)));

    // Set done property
    obj.borrow_mut(mc).properties.insert(
        PropertyKey::String("done".to_string()),
        Gc::new(mc, GcCell::new(Value::Boolean(done))),
    );

    Value::Object(obj)
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
    obj_set_key_value(mc, &gen_proto, &"next".into(), val)?;

    let val = Value::Function("Generator.prototype.return".to_string());
    obj_set_key_value(mc, &gen_proto, &"return".into(), val)?;

    let val = Value::Function("Generator.prototype.throw".to_string());
    obj_set_key_value(mc, &gen_proto, &"throw".into(), val)?;

    // link prototype to constructor and expose on global env
    obj_set_key_value(mc, &gen_ctor, &"prototype".into(), Value::Object(gen_proto))?;
    crate::core::env_set(mc, env, "Generator", Value::Object(gen_ctor))?;
    Ok(())
}
