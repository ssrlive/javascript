use crate::{
    core::{Expr, JSObjectDataPtr, PropertyKey, Statement, Value, evaluate_expr},
    error::JSError,
};

use std::cell::RefCell;
use std::rc::Rc;

/// Handle generator function constructor (when called as `new GeneratorFunction(...)`)
pub fn _handle_generator_function_constructor(_args: &[Expr], _env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Generator functions cannot be constructed with `new`
    Err(raise_eval_error!("GeneratorFunction is not a constructor"))
}

/// Handle generator function calls (creating generator objects)
pub fn handle_generator_function_call(
    params: &[(String, Option<Box<Expr>>)],
    body: &[Statement],
    _args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    // Create a new generator object
    let generator = Rc::new(RefCell::new(crate::core::JSGenerator {
        params: params.to_vec(),
        body: body.to_vec(),
        env: env.clone(),
        state: crate::core::GeneratorState::NotStarted,
    }));

    // Create a wrapper object for the generator
    let gen_obj = Rc::new(RefCell::new(crate::core::JSObjectData::new()));
    // Store the actual generator data
    gen_obj.borrow_mut().insert(
        crate::core::PropertyKey::String("__generator__".to_string()),
        Rc::new(RefCell::new(Value::Generator(generator))),
    );

    Ok(Value::Object(gen_obj))
}

/// Handle generator instance method calls (like `gen.next()`, `gen.return()`, etc.)
pub fn handle_generator_instance_method(
    generator: &Rc<RefCell<crate::core::JSGenerator>>,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    match method {
        "next" => {
            // Get optional value to send to the generator
            let send_value = if args.is_empty() {
                Value::Undefined
            } else {
                evaluate_expr(env, &args[0])?
            };

            generator_next(generator, send_value)
        }
        "return" => {
            // Return a value and close the generator
            let return_value = if args.is_empty() {
                Value::Undefined
            } else {
                evaluate_expr(env, &args[0])?
            };

            generator_return(generator, return_value)
        }
        "throw" => {
            // Throw an exception into the generator
            let throw_value = if args.is_empty() {
                Value::Undefined
            } else {
                evaluate_expr(env, &args[0])?
            };

            generator_throw(generator, throw_value)
        }
        _ => Err(raise_eval_error!(format!("Generator.prototype.{} is not implemented", method))),
    }
}

// Helper to replace the first `yield` occurrence inside an Expr with a
// provided `send_value`. `replaced` becomes true once a replacement is made.
fn replace_first_yield_in_expr(expr: &Expr, send_value: &Value, replaced: &mut bool) -> Expr {
    use crate::core::Expr;
    match expr {
        Expr::Yield(_) => {
            if !*replaced {
                *replaced = true;
                Expr::Value(send_value.clone())
            } else {
                expr.clone()
            }
        }
        Expr::YieldStar(_) => {
            if !*replaced {
                *replaced = true;
                Expr::Value(send_value.clone())
            } else {
                expr.clone()
            }
        }
        Expr::Binary(a, op, b) => Expr::Binary(
            Box::new(replace_first_yield_in_expr(a, send_value, replaced)),
            op.clone(),
            Box::new(replace_first_yield_in_expr(b, send_value, replaced)),
        ),
        Expr::Assign(a, b) => Expr::Assign(
            Box::new(replace_first_yield_in_expr(a, send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, send_value, replaced)),
        ),
        Expr::Index(a, b) => Expr::Index(
            Box::new(replace_first_yield_in_expr(a, send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, send_value, replaced)),
        ),
        Expr::Property(a, s) => Expr::Property(Box::new(replace_first_yield_in_expr(a, send_value, replaced)), s.clone()),
        Expr::Call(a, args) => Expr::Call(
            Box::new(replace_first_yield_in_expr(a, send_value, replaced)),
            args.iter()
                .map(|arg| replace_first_yield_in_expr(arg, send_value, replaced))
                .collect(),
        ),
        Expr::Object(pairs) => Expr::Object(
            pairs
                .iter()
                .map(|(k, v)| (k.clone(), replace_first_yield_in_expr(v, send_value, replaced)))
                .collect(),
        ),
        Expr::Array(items) => Expr::Array(
            items
                .iter()
                .map(|it| replace_first_yield_in_expr(it, send_value, replaced))
                .collect(),
        ),
        Expr::LogicalNot(a) => Expr::LogicalNot(Box::new(replace_first_yield_in_expr(a, send_value, replaced))),
        Expr::TypeOf(a) => Expr::TypeOf(Box::new(replace_first_yield_in_expr(a, send_value, replaced))),
        Expr::Delete(a) => Expr::Delete(Box::new(replace_first_yield_in_expr(a, send_value, replaced))),
        Expr::Void(a) => Expr::Void(Box::new(replace_first_yield_in_expr(a, send_value, replaced))),
        Expr::Increment(a) => Expr::Increment(Box::new(replace_first_yield_in_expr(a, send_value, replaced))),
        Expr::Decrement(a) => Expr::Decrement(Box::new(replace_first_yield_in_expr(a, send_value, replaced))),
        Expr::PostIncrement(a) => Expr::PostIncrement(Box::new(replace_first_yield_in_expr(a, send_value, replaced))),
        Expr::PostDecrement(a) => Expr::PostDecrement(Box::new(replace_first_yield_in_expr(a, send_value, replaced))),
        Expr::LogicalAnd(a, b) => Expr::LogicalAnd(
            Box::new(replace_first_yield_in_expr(a, send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, send_value, replaced)),
        ),
        Expr::LogicalOr(a, b) => Expr::LogicalOr(
            Box::new(replace_first_yield_in_expr(a, send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, send_value, replaced)),
        ),
        Expr::Comma(a, b) => Expr::Comma(
            Box::new(replace_first_yield_in_expr(a, send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, send_value, replaced)),
        ),
        Expr::Spread(a) => Expr::Spread(Box::new(replace_first_yield_in_expr(a, send_value, replaced))),
        Expr::OptionalCall(a, args) => Expr::OptionalCall(
            Box::new(replace_first_yield_in_expr(a, send_value, replaced)),
            args.iter()
                .map(|arg| replace_first_yield_in_expr(arg, send_value, replaced))
                .collect(),
        ),
        Expr::OptionalIndex(a, b) => Expr::OptionalIndex(
            Box::new(replace_first_yield_in_expr(a, send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, send_value, replaced)),
        ),
        Expr::Conditional(a, b, c) => Expr::Conditional(
            Box::new(replace_first_yield_in_expr(a, send_value, replaced)),
            Box::new(replace_first_yield_in_expr(b, send_value, replaced)),
            Box::new(replace_first_yield_in_expr(c, send_value, replaced)),
        ),
        _ => expr.clone(),
    }
}

fn replace_first_yield_in_statement(stmt: &mut Statement, send_value: &Value, replaced: &mut bool) {
    use crate::core::Statement;
    match stmt {
        Statement::Expr(e) => {
            *e = replace_first_yield_in_expr(e, send_value, replaced);
        }
        Statement::Let(_, Some(expr)) | Statement::Var(_, Some(expr)) => {
            *expr = replace_first_yield_in_expr(expr, send_value, replaced);
        }
        Statement::Const(_, expr) => {
            *expr = replace_first_yield_in_expr(expr, send_value, replaced);
        }
        Statement::Return(Some(expr)) => {
            *expr = replace_first_yield_in_expr(expr, send_value, replaced);
        }
        Statement::If(cond, then_body, else_body_opt) => {
            *cond = replace_first_yield_in_expr(cond, send_value, replaced);
            for s in then_body.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
                }
            }
            if let Some(else_body) = else_body_opt {
                for s in else_body.iter_mut() {
                    replace_first_yield_in_statement(s, send_value, replaced);
                    if *replaced {
                        return;
                    }
                }
            }
        }
        Statement::For(_, cond_opt, _, body) => {
            if let Some(cond) = cond_opt {
                *cond = replace_first_yield_in_expr(cond, send_value, replaced);
            }
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
                }
            }
        }
        Statement::While(cond, body) => {
            *cond = replace_first_yield_in_expr(cond, send_value, replaced);
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
                }
            }
        }
        Statement::DoWhile(body, cond) => {
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
                }
            }
            *cond = replace_first_yield_in_expr(cond, send_value, replaced);
        }
        Statement::ForOf(_, _, body)
        | Statement::ForIn(_, _, body)
        | Statement::ForOfDestructuringObject(_, _, body)
        | Statement::ForOfDestructuringArray(_, _, body) => {
            for s in body.iter_mut() {
                replace_first_yield_in_statement(s, send_value, replaced);
                if *replaced {
                    return;
                }
            }
        }
        Statement::Block(stmts) => {
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
        Expr::Yield(_) | Expr::YieldStar(_) => true,
        Expr::Binary(a, _, b) => expr_contains_yield(a) || expr_contains_yield(b),
        Expr::Assign(a, b) => expr_contains_yield(a) || expr_contains_yield(b),
        Expr::Index(a, b) => expr_contains_yield(a) || expr_contains_yield(b),
        Expr::Property(a, _) => expr_contains_yield(a),
        Expr::Call(a, args) => expr_contains_yield(a) || args.iter().any(expr_contains_yield),
        Expr::Object(pairs) => pairs.iter().any(|(_, v)| expr_contains_yield(v)),
        Expr::Array(items) => items.iter().any(expr_contains_yield),
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
fn replace_first_yield_statement_with_throw(stmt: &mut Statement, throw_value: &Value) -> bool {
    match stmt {
        Statement::Expr(e) => {
            if expr_contains_yield(e) {
                *stmt = Statement::Throw(Expr::Value(throw_value.clone()));
                return true;
            }
            false
        }
        Statement::Let(_, Some(expr)) | Statement::Var(_, Some(expr)) | Statement::Const(_, expr) => {
            if expr_contains_yield(expr) {
                *stmt = Statement::Throw(Expr::Value(throw_value.clone()));
                return true;
            }
            false
        }
        Statement::If(_, then_body, else_body_opt) => {
            for s in then_body.iter_mut() {
                if replace_first_yield_statement_with_throw(s, throw_value) {
                    return true;
                }
            }
            if let Some(else_body) = else_body_opt {
                for s in else_body.iter_mut() {
                    if replace_first_yield_statement_with_throw(s, throw_value) {
                        return true;
                    }
                }
            }
            false
        }
        Statement::Block(stmts) => {
            for s in stmts.iter_mut() {
                if replace_first_yield_statement_with_throw(s, throw_value) {
                    return true;
                }
            }
            false
        }
        Statement::For(_, _, _, body)
        | Statement::ForOf(_, _, body)
        | Statement::ForIn(_, _, body)
        | Statement::ForOfDestructuringObject(_, _, body)
        | Statement::ForOfDestructuringArray(_, _, body)
        | Statement::While(_, body) => {
            for s in body.iter_mut() {
                if replace_first_yield_statement_with_throw(s, throw_value) {
                    return true;
                }
            }
            false
        }
        Statement::DoWhile(body, _) => {
            for s in body.iter_mut() {
                if replace_first_yield_statement_with_throw(s, throw_value) {
                    return true;
                }
            }
            false
        }
        Statement::TryCatch(try_body, _, catch_body, finally_body_opt) => {
            for s in try_body.iter_mut() {
                if replace_first_yield_statement_with_throw(s, throw_value) {
                    return true;
                }
            }
            for s in catch_body.iter_mut() {
                if replace_first_yield_statement_with_throw(s, throw_value) {
                    return true;
                }
            }
            if let Some(finally) = finally_body_opt {
                for s in finally.iter_mut() {
                    if replace_first_yield_statement_with_throw(s, throw_value) {
                        return true;
                    }
                }
            }
            false
        }
        _ => false,
    }
}

// Helper to find a yield expression within statements. Returns the
// index of the containing top-level statement and the inner yield
// expression if found.
fn find_first_yield_in_statements(stmts: &[Statement]) -> Option<(usize, Option<Box<Expr>>)> {
    use crate::core::Statement;
    for (i, s) in stmts.iter().enumerate() {
        match s {
            Statement::Expr(e) => match e {
                Expr::Yield(inner) => return Some((i, inner.clone())),
                Expr::YieldStar(inner) => return Some((i, Some(inner.clone()))),
                _ => {}
            },
            Statement::Block(inner_stmts) => {
                if let Some((_inner_idx, found)) = find_first_yield_in_statements(inner_stmts) {
                    return Some((i, found));
                }
            }
            Statement::If(_, then_body, else_body_opt) => {
                if let Some((_inner_idx, found)) = find_first_yield_in_statements(then_body) {
                    return Some((i, found));
                }
                if let Some(else_body) = else_body_opt
                    && let Some((_inner_idx, found)) = find_first_yield_in_statements(else_body)
                {
                    return Some((i, found));
                }
            }
            Statement::For(_, _, _, body) | Statement::While(_, body) | Statement::DoWhile(body, _) => {
                if let Some((_inner_idx, found)) = find_first_yield_in_statements(body) {
                    return Some((i, found));
                }
            }
            Statement::ForOf(_, _, body)
            | Statement::ForIn(_, _, body)
            | Statement::ForOfDestructuringObject(_, _, body)
            | Statement::ForOfDestructuringArray(_, _, body) => {
                if let Some((_inner_idx, found)) = find_first_yield_in_statements(body) {
                    return Some((i, found));
                }
            }
            Statement::FunctionDeclaration(_, _, _, _) => {
                // don't search nested function declarations
            }
            _ => {}
        }
    }
    None
}

/// Execute generator.next()
fn generator_next(generator: &Rc<RefCell<crate::core::JSGenerator>>, _send_value: Value) -> Result<Value, JSError> {
    let mut gen_obj = generator.borrow_mut();

    match &mut gen_obj.state {
        crate::core::GeneratorState::NotStarted => {
            // Start executing the generator function. Attempt to find the first
            // `yield` expression in the function body and return its value.
            gen_obj.state = crate::core::GeneratorState::Suspended { pc: 0, stack: vec![] };

            if let Some((idx, yield_inner)) = find_first_yield_in_statements(&gen_obj.body) {
                // Suspend at the containing top-level statement index so
                // that resumed execution re-evaluates the statement with
                // the sent-in value substituted for the `yield`.
                gen_obj.state = crate::core::GeneratorState::Suspended { pc: idx, stack: vec![] };

                // If the yield has an inner expression, evaluate it in a fresh
                // function-like frame whose prototype is the captured env.
                if let Some(inner_expr_box) = yield_inner {
                    let func_env = crate::core::new_js_object_data();
                    func_env.borrow_mut().prototype = Some(gen_obj.env.clone());
                    func_env.borrow_mut().is_function_scope = true;
                    match crate::core::evaluate_expr(&func_env, &inner_expr_box) {
                        Ok(val) => return Ok(create_iterator_result(val, false)),
                        Err(_) => return Ok(create_iterator_result(Value::Undefined, false)),
                    }
                }

                // No inner expression -> yield undefined
                Ok(create_iterator_result(Value::Undefined, false))
            } else {
                // Fallback to previous placeholder behavior
                Ok(create_iterator_result(Value::Number(42.0), false))
            }
        }
        crate::core::GeneratorState::Suspended { pc, stack: _ } => {
            // On resume, execute from the suspended statement index. If a
            // `send_value` was provided to `next(value)`, substitute the
            // first `yield` in that statement with the sent value before
            // executing.
            let pc_val = *pc;
            if pc_val >= gen_obj.body.len() {
                gen_obj.state = crate::core::GeneratorState::Completed;
                return Ok(create_iterator_result(Value::Undefined, true));
            }
            // Clone the tail and replace first yield in the first statement
            let mut tail: Vec<Statement> = gen_obj.body[pc_val..].to_vec();
            let mut replaced = false;
            if let Some(first_stmt) = tail.get_mut(0) {
                replace_first_yield_in_statement(first_stmt, &_send_value, &mut replaced);
            }

            let func_env = crate::core::new_js_object_data();
            func_env.borrow_mut().prototype = Some(gen_obj.env.clone());
            func_env.borrow_mut().is_function_scope = true;
            // Execute the (possibly modified) tail
            let result = crate::core::evaluate_statements(&func_env, &tail);
            gen_obj.state = crate::core::GeneratorState::Completed;
            match result {
                Ok(val) => Ok(create_iterator_result(val, true)),
                Err(_) => Ok(create_iterator_result(Value::Undefined, true)),
            }
        }
        crate::core::GeneratorState::Running { .. } => Err(raise_eval_error!("Generator is already running")),
        crate::core::GeneratorState::Completed => Ok(create_iterator_result(Value::Undefined, true)),
    }
}

/// Execute generator.return()
fn generator_return(generator: &Rc<RefCell<crate::core::JSGenerator>>, return_value: Value) -> Result<Value, JSError> {
    let mut gen_obj = generator.borrow_mut();
    gen_obj.state = crate::core::GeneratorState::Completed;
    Ok(create_iterator_result(return_value, true))
}

/// Execute generator.throw()
fn generator_throw(generator: &Rc<RefCell<crate::core::JSGenerator>>, throw_value: Value) -> Result<Value, JSError> {
    let mut gen_obj = generator.borrow_mut();
    match &mut gen_obj.state {
        crate::core::GeneratorState::NotStarted => {
            // Throwing into a not-started generator throws synchronously
            Err(raise_throw_error!(throw_value))
        }
        crate::core::GeneratorState::Suspended { pc, .. } => {
            // Replace the suspended statement with a Throw containing the thrown value
            let pc_val = *pc;
            if pc_val >= gen_obj.body.len() {
                gen_obj.state = crate::core::GeneratorState::Completed;
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
                tail[0] = Statement::Throw(Expr::Value(throw_value.clone()));
            }

            let func_env = crate::core::new_js_object_data();
            func_env.borrow_mut().prototype = Some(gen_obj.env.clone());
            func_env.borrow_mut().is_function_scope = true;

            // Execute the modified tail. If the throw is uncaught, evaluate_statements
            // will return Err and we should propagate that to the caller.
            let result = crate::core::evaluate_statements(&func_env, &tail);
            gen_obj.state = crate::core::GeneratorState::Completed;
            match result {
                Ok(val) => Ok(create_iterator_result(val, true)),
                Err(e) => Err(e),
            }
        }
        crate::core::GeneratorState::Running { .. } => Err(raise_eval_error!("Generator is already running")),
        crate::core::GeneratorState::Completed => Err(raise_eval_error!("Generator has already completed")),
    }
}

/// Create an iterator result object {value: value, done: done}
fn create_iterator_result(value: Value, done: bool) -> Value {
    let obj = Rc::new(RefCell::new(crate::core::JSObjectData::default()));

    // Set value property
    obj.borrow_mut()
        .properties
        .insert(PropertyKey::String("value".to_string()), Rc::new(RefCell::new(value)));

    // Set done property
    obj.borrow_mut()
        .properties
        .insert(PropertyKey::String("done".to_string()), Rc::new(RefCell::new(Value::Boolean(done))));

    Value::Object(obj)
}
