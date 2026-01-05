#![allow(dead_code, unused_variables)]

use crate::{
    JSError, JSErrorKind, PropertyKey, Value,
    core::{
        BinaryOp, ClosureData, DestructuringElement, EvalError, Expr, JSObjectDataPtr, Statement, StatementKind, create_error, env_get,
        env_set, env_set_recursive, is_error, new_js_object_data, obj_get_key_value, obj_set_key_value, value_to_string,
    },
    raise_eval_error, raise_reference_error,
    unicode::{utf8_to_utf16, utf16_to_utf8},
};
use gc_arena::Gc;
use gc_arena::Mutation as MutationContext;
use gc_arena::lock::RefLock as GcCell;

#[derive(Clone, Debug)]
pub enum ControlFlow<'gc> {
    Normal(Value<'gc>),
    Return(Value<'gc>),
    Throw(Value<'gc>, Option<usize>, Option<usize>), // value, line, column
    Break(Option<String>),
    Continue(Option<String>),
}

pub fn evaluate_statements<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &mut [Statement],
) -> Result<Value<'gc>, EvalError<'gc>> {
    match evaluate_statements_with_context(mc, env, statements)? {
        ControlFlow::Normal(val) => Ok(val),
        ControlFlow::Return(val) => Ok(val),
        ControlFlow::Throw(val, line, column) => Err(EvalError::Throw(val, line, column)),
        ControlFlow::Break(_) => Ok(Value::Undefined),
        ControlFlow::Continue(_) => Ok(Value::Undefined),
    }
}

pub fn evaluate_statements_with_context<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &mut [Statement],
) -> Result<ControlFlow<'gc>, EvalError<'gc>> {
    let mut last_value = Value::Undefined;
    for stmt in statements {
        if let Some(cf) = eval_res(mc, stmt, &mut last_value, env)? {
            return Ok(cf);
        }
    }
    Ok(ControlFlow::Normal(last_value))
}

fn eval_res<'gc>(
    mc: &MutationContext<'gc>,
    stmt: &Statement,
    last_value: &mut Value<'gc>,
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<ControlFlow<'gc>>, EvalError<'gc>> {
    match &stmt.kind {
        StatementKind::Expr(expr) => match evaluate_expr(mc, env, expr) {
            Ok(val) => {
                *last_value = val;
                Ok(None)
            }
            Err(e) => Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
        },
        StatementKind::Let(decls) => {
            for (name, expr_opt) in decls {
                let val = if let Some(expr) = expr_opt {
                    match evaluate_expr(mc, env, expr) {
                        Ok(v) => v,
                        Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                    }
                } else {
                    Value::Undefined
                };
                env_set(mc, env, name, val)?;
            }
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Var(decls) => {
            for (name, expr_opt) in decls {
                let val = if let Some(expr) = expr_opt {
                    match evaluate_expr(mc, env, expr) {
                        Ok(v) => v,
                        Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                    }
                } else {
                    Value::Undefined
                };

                let mut target_env = *env;
                while !target_env.borrow().is_function_scope {
                    if let Some(proto) = target_env.borrow().prototype {
                        target_env = proto;
                    } else {
                        break;
                    }
                }
                env_set(mc, &target_env, name, val)?;
            }
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Const(decls) => {
            for (name, expr) in decls {
                let val = match evaluate_expr(mc, env, expr) {
                    Ok(v) => v,
                    Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                };
                env_set(mc, env, name, val)?;
            }
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Return(expr_opt) => {
            let val = if let Some(expr) = expr_opt {
                match evaluate_expr(mc, env, expr) {
                    Ok(v) => v,
                    Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
                }
            } else {
                Value::Undefined
            };
            Ok(Some(ControlFlow::Return(val)))
        }
        StatementKind::FunctionDeclaration(name, params, body, _) => {
            let mut body_clone = body.clone();
            let func = evaluate_function_expression(mc, env, Some(name.clone()), params, &mut body_clone)?;
            env_set(mc, env, name, func)?;
            Ok(None)
        }
        StatementKind::Throw(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            if let Value::Object(obj) = val {
                if is_error(&val) {
                    let mut filename = String::new();
                    if let Ok(Some(val_ptr)) = obj_get_key_value(mc, env, &"__filename".into()) {
                        if let Value::String(s) = &*val_ptr.borrow() {
                            filename = utf16_to_utf8(s);
                        }
                    }
                    let frame = format!("at <anonymous> ({}:{}:{})", filename, stmt.line, stmt.column);
                    let current_stack = obj.borrow().get_property(mc, "stack").unwrap_or_default();
                    let new_stack = format!("{}\n    {}", current_stack, frame);
                    obj.borrow_mut(mc)
                        .set_property(mc, "stack", Value::String(utf8_to_utf16(&new_stack)));

                    obj.borrow_mut(mc).set_line(stmt.line, mc)?;
                    obj.borrow_mut(mc).set_column(stmt.column, mc)?;
                }
            }
            Ok(Some(ControlFlow::Throw(val, Some(stmt.line), Some(stmt.column))))
        }
        StatementKind::Block(stmts) => {
            let mut stmts_clone = stmts.clone();
            let block_env = new_js_object_data(mc);
            block_env.borrow_mut(mc).prototype = Some(*env);
            let res = evaluate_statements_with_context(mc, &block_env, &mut stmts_clone)?;
            match res {
                ControlFlow::Normal(val) => {
                    *last_value = val;
                    Ok(None)
                }
                other => Ok(Some(other)),
            }
        }
        StatementKind::If(cond, then_block, else_block) => {
            let cond_val = evaluate_expr(mc, env, cond)?;
            let is_true = match cond_val {
                Value::Boolean(b) => b,
                Value::Number(n) => n != 0.0 && !n.is_nan(),
                Value::String(s) => !s.is_empty(),
                Value::Null | Value::Undefined => false,
                Value::Object(_) => true,
                _ => false,
            };

            if is_true {
                let mut stmts = then_block.clone();
                let block_env = new_js_object_data(mc);
                block_env.borrow_mut(mc).prototype = Some(*env);
                let res = evaluate_statements_with_context(mc, &block_env, &mut stmts)?;
                match res {
                    ControlFlow::Normal(val) => {
                        *last_value = val;
                        Ok(None)
                    }
                    other => Ok(Some(other)),
                }
            } else if let Some(else_stmts) = else_block {
                let mut stmts = else_stmts.clone();
                let block_env = new_js_object_data(mc);
                block_env.borrow_mut(mc).prototype = Some(*env);
                let res = evaluate_statements_with_context(mc, &block_env, &mut stmts)?;
                match res {
                    ControlFlow::Normal(val) => {
                        *last_value = val;
                        Ok(None)
                    }
                    other => Ok(Some(other)),
                }
            } else {
                Ok(None)
            }
        }
        StatementKind::TryCatch(try_body, catch_param, catch_body, finally_body) => {
            let mut try_stmts = try_body.clone();
            let try_res = evaluate_statements_with_context(mc, env, &mut try_stmts);

            let mut result = match try_res {
                Ok(cf) => cf,
                Err(e) => match e {
                    EvalError::Js(js_err) => {
                        let val = js_error_to_value(mc, env, &js_err);
                        ControlFlow::Throw(val, js_err.inner.js_line, js_err.inner.js_column)
                    }
                    EvalError::Throw(val, line, column) => ControlFlow::Throw(val, line, column),
                },
            };

            if let ControlFlow::Throw(val, ..) = &result {
                if let Some(catch_stmts) = catch_body {
                    // Create new scope for catch
                    let catch_env = crate::core::new_js_object_data(mc);
                    catch_env.borrow_mut(mc).prototype = Some(*env);

                    if let Some(param_name) = catch_param {
                        env_set(mc, &catch_env, param_name, val.clone())?;
                    }

                    let mut catch_stmts_clone = catch_stmts.clone();
                    let catch_res = evaluate_statements_with_context(mc, &catch_env, &mut catch_stmts_clone);
                    match catch_res {
                        Ok(cf) => result = cf,
                        Err(e) => match e {
                            EvalError::Js(js_err) => {
                                let val = js_error_to_value(mc, env, &js_err);
                                result = ControlFlow::Throw(val, js_err.inner.js_line, js_err.inner.js_column);
                            }
                            EvalError::Throw(val, line, column) => result = ControlFlow::Throw(val, line, column),
                        },
                    }
                }
            }

            if let Some(finally_stmts) = finally_body {
                let mut finally_stmts_clone = finally_stmts.clone();
                let finally_res = evaluate_statements_with_context(mc, env, &mut finally_stmts_clone);
                match finally_res {
                    Ok(ControlFlow::Normal(_)) => {
                        // If finally completes normally, return the previous result (try or catch)
                    }
                    Ok(other) => {
                        // If finally is abrupt (return, throw, break, continue), it overrides.
                        result = other;
                    }
                    Err(e) => match e {
                        EvalError::Js(js_err) => {
                            let val = js_error_to_value(mc, env, &js_err);
                            result = ControlFlow::Throw(val, js_err.inner.js_line, js_err.inner.js_column);
                        }
                        EvalError::Throw(val, line, column) => result = ControlFlow::Throw(val, line, column),
                    },
                }
            }

            match result {
                ControlFlow::Normal(val) => {
                    *last_value = val;
                    Ok(None)
                }
                other => Ok(Some(other)),
            }
        }
        _ => Ok(None),
    }
}

fn refresh_error_by_additional_stack_frame<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    line: usize,
    column: usize,
    mut e: EvalError<'gc>,
) -> EvalError<'gc> {
    let mut filename = String::new();
    if let Ok(Some(val_ptr)) = obj_get_key_value(mc, env, &"__filename".into()) {
        if let Value::String(s) = &*val_ptr.borrow() {
            filename = utf16_to_utf8(s);
        }
    }
    let frame = format!("at <anonymous> ({}:{}:{})", filename, line, column);
    if let EvalError::Js(js_err) = &mut e {
        js_err.inner.stack.push(frame.clone());
    }
    if let EvalError::Throw(val, ..) = &mut e
        && is_error(val)
        && let Value::Object(obj) = val
    {
        let current_stack = obj.borrow().get_property(mc, "stack").unwrap_or_default();
        let new_stack = format!("{}\n    {}", current_stack, frame);
        obj.borrow_mut(mc).set_property(mc, "stack", new_stack.into());
    }
    e
}

pub fn evaluate_expr<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, expr: &Expr) -> Result<Value<'gc>, EvalError<'gc>> {
    match expr {
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::StringLit(s) => Ok(Value::String(s.clone())),
        Expr::Boolean(b) => Ok(Value::Boolean(*b)),
        Expr::Var(name, _, _) => Ok(evaluate_var(mc, env, name)?),
        Expr::Assign(target, value_expr) => {
            let val = evaluate_expr(mc, env, value_expr)?;
            if let Expr::Var(name, _, _) = &**target {
                env_set_recursive(mc, env, name, val.clone())?;
                Ok(val)
            } else {
                Err(EvalError::Js(raise_eval_error!("Only simple assignment implemented")))
            }
        }
        Expr::Binary(left, op, right) => {
            let l_val = evaluate_expr(mc, env, left)?;
            let r_val = evaluate_expr(mc, env, right)?;
            match op {
                BinaryOp::Add => match (l_val, r_val) {
                    (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln + rn)),
                    (Value::String(ls), Value::String(rs)) => {
                        let mut res = ls.clone();
                        res.extend(rs);
                        Ok(Value::String(res))
                    }
                    (Value::String(ls), other) => {
                        let mut res = ls.clone();
                        res.extend(utf8_to_utf16(&value_to_string(&other)));
                        Ok(Value::String(res))
                    }
                    (other, Value::String(rs)) => {
                        let mut res = utf8_to_utf16(&value_to_string(&other));
                        res.extend(rs);
                        Ok(Value::String(res))
                    }
                    _ => Err(EvalError::Js(raise_eval_error!("Binary Add only for numbers or strings"))),
                },
                BinaryOp::Sub => {
                    if let (Value::Number(ln), Value::Number(rn)) = (l_val, r_val) {
                        Ok(Value::Number(ln - rn))
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Binary Sub only for numbers")))
                    }
                }
                BinaryOp::Mul => {
                    if let (Value::Number(ln), Value::Number(rn)) = (l_val, r_val) {
                        Ok(Value::Number(ln * rn))
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Binary Mul only for numbers")))
                    }
                }
                BinaryOp::Div => {
                    if let (Value::Number(ln), Value::Number(rn)) = (l_val, r_val) {
                        Ok(Value::Number(ln / rn))
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Binary Div only for numbers")))
                    }
                }
                BinaryOp::StrictEqual => {
                    let eq = match (l_val, r_val) {
                        (Value::Number(l), Value::Number(r)) => l == r,
                        (Value::String(l), Value::String(r)) => l == r,
                        (Value::Boolean(l), Value::Boolean(r)) => l == r,
                        (Value::Null, Value::Null) => true,
                        (Value::Undefined, Value::Undefined) => true,
                        _ => false,
                    };
                    Ok(Value::Boolean(eq))
                }
                BinaryOp::StrictNotEqual => {
                    let eq = match (l_val, r_val) {
                        (Value::Number(l), Value::Number(r)) => l == r,
                        (Value::String(l), Value::String(r)) => l == r,
                        (Value::Boolean(l), Value::Boolean(r)) => l == r,
                        (Value::Null, Value::Null) => true,
                        (Value::Undefined, Value::Undefined) => true,
                        _ => false,
                    };
                    Ok(Value::Boolean(!eq))
                }
                BinaryOp::GreaterThan => {
                    if let (Value::Number(ln), Value::Number(rn)) = (l_val, r_val) {
                        Ok(Value::Boolean(ln > rn))
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Binary GreaterThan only for numbers")))
                    }
                }
                BinaryOp::LessThan => {
                    if let (Value::Number(ln), Value::Number(rn)) = (l_val, r_val) {
                        Ok(Value::Boolean(ln < rn))
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Binary LessThan only for numbers")))
                    }
                }
                BinaryOp::GreaterEqual => {
                    if let (Value::Number(ln), Value::Number(rn)) = (l_val, r_val) {
                        Ok(Value::Boolean(ln >= rn))
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Binary GreaterEqual only for numbers")))
                    }
                }
                BinaryOp::LessEqual => {
                    if let (Value::Number(ln), Value::Number(rn)) = (l_val, r_val) {
                        Ok(Value::Boolean(ln <= rn))
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Binary LessEqual only for numbers")))
                    }
                }
                _ => todo!(),
            }
        }
        Expr::LogicalNot(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            let b = match val {
                Value::Boolean(b) => b,
                Value::Number(n) => n != 0.0 && !n.is_nan(),
                Value::String(s) => !s.is_empty(),
                Value::Null | Value::Undefined => false,
                Value::Object(_) => true,
                _ => false,
            };
            Ok(Value::Boolean(!b))
        }
        Expr::Function(name, params, body) => {
            let mut body_clone = body.clone();
            Ok(evaluate_function_expression(mc, env, name.clone(), params, &mut body_clone)?)
        }
        Expr::Call(func_expr, args) => {
            let func_val = evaluate_expr(mc, env, func_expr)?;
            let mut eval_args = Vec::new();
            for arg in args {
                eval_args.push(evaluate_expr(mc, env, arg)?);
            }

            match func_val {
                Value::Function(name) => {
                    if name == "console.log" {
                        let output = eval_args
                            .iter()
                            .map(|v| {
                                if is_error(v)
                                    && let Value::Object(obj) = v
                                {
                                    // If it has a stack property, use it
                                    if let Some(stack) = obj.borrow().get_property(mc, "stack") {
                                        return stack;
                                    }
                                }

                                if let Value::String(s) = v {
                                    utf16_to_utf8(s)
                                } else {
                                    value_to_string(v)
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        println!("{}", output);
                        Ok(Value::Undefined)
                    } else if name == "console.error" {
                        let output = eval_args
                            .iter()
                            .map(|v| {
                                if is_error(v)
                                    && let Value::Object(obj) = v
                                {
                                    // If it has a stack property, use it
                                    if let Some(stack) = obj.borrow().get_property(mc, "stack") {
                                        return stack;
                                    }
                                }
                                if let Value::String(s) = v {
                                    utf16_to_utf8(s)
                                } else {
                                    value_to_string(v)
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        println!("{}", output);
                        Ok(Value::Undefined)
                    } else {
                        Err(EvalError::Js(raise_eval_error!(format!("Unknown native function: {}", name))))
                    }
                }
                Value::Object(obj) => {
                    if let Some(cl_ptr) = obj_get_key_value(mc, &obj, &"__closure__".into())? {
                        match &*cl_ptr.borrow() {
                            Value::Closure(cl) => {
                                let call_env = crate::core::new_js_object_data(mc);
                                call_env.borrow_mut(mc).prototype = Some(cl.env);
                                call_env.borrow_mut(mc).is_function_scope = true;

                                for (i, param) in cl.params.iter().enumerate() {
                                    if let DestructuringElement::Variable(name, _) = param {
                                        let arg_val = eval_args.get(i).cloned().unwrap_or(Value::Undefined);
                                        env_set(mc, &call_env, name, arg_val)?;
                                    }
                                }
                                let mut body_clone = cl.body.clone();
                                match evaluate_statements(mc, &call_env, &mut body_clone) {
                                    Ok(v) => Ok(v),
                                    Err(mut e) => {
                                        // Avoid borrowing obj while modifying err_obj if they might be related or if obj is already borrowed?
                                        // obj is the function object.
                                        // We need its name.
                                        let name_opt = obj.borrow().get_name(mc);

                                        if let Some(name_str) = name_opt {
                                            if let EvalError::Js(js_err) = &mut e {
                                                if let Some(last_frame) = js_err.inner.stack.last_mut() {
                                                    if last_frame.contains("<anonymous>") {
                                                        *last_frame = last_frame.replace("<anonymous>", &name_str);
                                                    }
                                                }
                                            }
                                            if let EvalError::Throw(val, ..) = &mut e {
                                                if let Value::Object(err_obj) = val {
                                                    // If err_obj is the same as obj (unlikely for function call), we might have issues.
                                                    // But err_obj is the thrown error. obj is the function being called.

                                                    // The panic "RefCell already borrowed" likely comes from obj_set_key_value borrowing err_obj mutably,
                                                    // while err_obj.borrow().get_property borrowed it immutably.
                                                    // We need to drop the immutable borrow before mutable borrow.

                                                    let stack_str_opt = err_obj.borrow().get_property(mc, "stack");
                                                    if let Some(stack_str) = stack_str_opt {
                                                        let mut lines: Vec<String> = stack_str.lines().map(|s| s.to_string()).collect();
                                                        if let Some(last_line) = lines.last_mut() {
                                                            if last_line.contains("<anonymous>") {
                                                                *last_line = last_line.replace("<anonymous>", &name_str);
                                                            }
                                                        }
                                                        let new_stack = lines.join("\n");
                                                        let _ = obj_set_key_value(
                                                            mc,
                                                            err_obj,
                                                            &"stack".into(),
                                                            Value::String(utf8_to_utf16(&new_stack)),
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                        Err(e)
                                    }
                                }
                            }
                            _ => Err(EvalError::Js(raise_eval_error!("Not a function"))),
                        }
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Not a function")))
                    }
                }
                _ => Err(EvalError::Js(raise_eval_error!("Not a function"))),
            }
        }
        Expr::New(ctor, args) => {
            let func_val = evaluate_expr(mc, env, ctor)?;
            let mut eval_args = Vec::new();
            for arg in args {
                eval_args.push(evaluate_expr(mc, env, arg)?);
            }

            match func_val {
                Value::Object(obj) => {
                    if let Some(cl_ptr) = obj_get_key_value(mc, &obj, &"__closure__".into())? {
                        match &*cl_ptr.borrow() {
                            Value::Closure(cl) => {
                                let call_env = crate::core::new_js_object_data(mc);
                                call_env.borrow_mut(mc).prototype = Some(cl.env);
                                call_env.borrow_mut(mc).is_function_scope = true;

                                for (i, param) in cl.params.iter().enumerate() {
                                    if let DestructuringElement::Variable(name, _) = param {
                                        let arg_val = eval_args.get(i).cloned().unwrap_or(Value::Undefined);
                                        env_set(mc, &call_env, name, arg_val)?;
                                    }
                                }
                                let mut body_clone = cl.body.clone();
                                evaluate_statements(mc, &call_env, &mut body_clone)
                            }
                            _ => Err(EvalError::Js(raise_eval_error!("Not a constructor"))),
                        }
                    } else {
                        if let Some(native_name) = obj_get_key_value(mc, &obj, &"__native_ctor".into())? {
                            if let Value::String(name) = &*native_name.borrow() {
                                if name == &crate::unicode::utf8_to_utf16("Error") {
                                    let msg = eval_args.first().cloned().unwrap_or(Value::Undefined);
                                    let prototype = if let Some(proto_val) = obj_get_key_value(mc, &obj, &"prototype".into())?
                                        && let Value::Object(proto_obj) = &*proto_val.borrow()
                                    {
                                        Some(*proto_obj)
                                    } else {
                                        None
                                    };

                                    return Ok(crate::core::js_error::create_error(mc, prototype, msg)?);
                                }
                            }
                        }
                        let new_obj = crate::core::new_js_object_data(mc);
                        Ok(Value::Object(new_obj))
                    }
                }
                _ => Err(EvalError::Js(raise_eval_error!("Not a constructor"))),
            }
        }
        Expr::Property(obj_expr, key) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            if let Value::Object(obj) = obj_val {
                if let Some(val) = obj_get_key_value(mc, &obj, &key.as_str().into())? {
                    Ok(val.borrow().clone())
                } else {
                    Ok(Value::Undefined)
                }
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                Err(EvalError::Js(raise_eval_error!("Cannot read properties of null or undefined")))
            } else {
                Ok(Value::Undefined)
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;

            if let Value::Object(obj) = obj_val {
                let key = match key_val {
                    Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                    Value::Number(n) => PropertyKey::String(n.to_string()),
                    _ => PropertyKey::String(value_to_string(&key_val)),
                };

                if let Some(val) = obj_get_key_value(mc, &obj, &key)? {
                    Ok(val.borrow().clone())
                } else {
                    Ok(Value::Undefined)
                }
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                Err(EvalError::Js(raise_eval_error!("Cannot read properties of null or undefined")))
            } else {
                Ok(Value::Undefined)
            }
        }
        Expr::TemplateString(parts) => {
            let mut result = Vec::new();
            for part in parts {
                match part {
                    crate::core::TemplatePart::String(s) => result.extend(s),
                    crate::core::TemplatePart::Expr(tokens) => {
                        let (expr, _) = crate::core::parse_simple_expression(tokens, 0)?;
                        let val = evaluate_expr(mc, env, &expr)?;
                        // For template interpolation we must not include surrounding
                        // quotes for string values. Convert different Value variants
                        // to their string content (no extra quotes for strings).
                        match val {
                            Value::String(s) => result.extend(s),
                            Value::Number(n) => result.extend(crate::unicode::utf8_to_utf16(&n.to_string())),
                            Value::BigInt(b) => result.extend(crate::unicode::utf8_to_utf16(&format!("{}n", b))),
                            Value::Boolean(b) => result.extend(crate::unicode::utf8_to_utf16(&b.to_string())),
                            Value::Undefined => result.extend(crate::unicode::utf8_to_utf16("undefined")),
                            Value::Null => result.extend(crate::unicode::utf8_to_utf16("null")),
                            _ => {
                                // Fallback to the generic representation (may include quotes)
                                let s = value_to_string(&val);
                                result.extend(crate::unicode::utf8_to_utf16(&s));
                            }
                        }
                    }
                }
            }
            Ok(Value::String(result))
        }
        _ => Ok(Value::Undefined),
    }
}

fn evaluate_var<'gc>(_mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str) -> Result<Value<'gc>, JSError> {
    let mut current_opt = Some(*env);
    while let Some(current_env) = current_opt {
        if let Some(val_ptr) = env_get(&current_env, name) {
            return Ok(val_ptr.borrow().clone());
        }
        current_opt = current_env.borrow().prototype;
    }
    Err(raise_reference_error!(format!("{} is not defined", name)))
}

fn evaluate_function_expression<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    name: Option<String>,
    params: &[DestructuringElement],
    body: &mut [Statement],
) -> Result<Value<'gc>, JSError> {
    let func_obj = crate::core::new_js_object_data(mc);
    let closure_data = ClosureData {
        params: params.to_vec(),
        body: body.to_vec(),
        env: *env,
        home_object: GcCell::new(None),
        captured_envs: Vec::new(),
        bound_this: None,
    };
    let closure_val = Value::Closure(Gc::new(mc, closure_data));
    obj_set_key_value(mc, &func_obj, &"__closure__".into(), closure_val)?;
    if let Some(n) = name {
        obj_set_key_value(mc, &func_obj, &"name".into(), Value::String(utf8_to_utf16(&n)))?;
    }
    Ok(Value::Object(func_obj))
}

fn js_error_to_value<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, js_err: &JSError) -> Value<'gc> {
    let full_msg = js_err.message();

    let error_proto = if let Some(err_ctor_val) = env_get(env, "Error")
        && let Value::Object(err_ctor) = &*err_ctor_val.borrow()
        && let Ok(Some(proto_val)) = obj_get_key_value(mc, err_ctor, &"prototype".into())
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let err_val = create_error(mc, error_proto, (&full_msg).into()).unwrap_or(Value::Undefined);

    let (name, raw_msg) = match js_err.kind() {
        JSErrorKind::ReferenceError { message } => ("ReferenceError", message.clone()),
        JSErrorKind::SyntaxError { message } => ("SyntaxError", message.clone()),
        JSErrorKind::TypeError { message } => ("TypeError", message.clone()),
        JSErrorKind::RangeError { message } => ("RangeError", message.clone()),
        JSErrorKind::VariableNotFound { name } => ("ReferenceError", format!("{} is not defined", name)),
        JSErrorKind::TokenizationError { message } => ("SyntaxError", message.clone()),
        JSErrorKind::ParseError { message } => ("SyntaxError", message.clone()),
        _ => ("Error", full_msg.clone()),
    };

    if let Value::Object(obj) = &err_val {
        obj.borrow_mut(mc).set_property(mc, "name", name.into());
        obj.borrow_mut(mc).set_property(mc, "message", (&raw_msg).into());

        let stack = js_err.stack();
        let stack_str = if stack.is_empty() {
            format!("{name}: {raw_msg}")
        } else {
            format!("{name}: {raw_msg}\n    {}", stack.join("\n    "))
        };
        obj.borrow_mut(mc).set_property(mc, "stack", stack_str.into());
    }
    err_val
}

/*
use crate::{
    JSError, JSErrorKind, PropertyKey, Value,
    core::{
        BinaryOp, ClosureData, DestructuringElement, Expr, JSObjectDataPtr, ObjectDestructuringElement, Statement, StatementKind,
        SwitchCase, SymbolData, TypedArrayKind, WELL_KNOWN_SYMBOLS, env_get, env_set, env_set_const, env_set_recursive, env_set_var,
        extract_closure_from_value, get_own_property, is_truthy, new_js_object_data, obj_delete, obj_set_key_value, parse_bigint_string,
        prepare_function_call_env, to_primitive, value_to_property_key, value_to_sort_string, value_to_string, values_equal,
    },
    js_array::{create_array, get_array_length, is_array, set_array_length},
    js_class::{
        ClassMember, call_class_method, call_static_method, create_class_object, evaluate_new, evaluate_super, evaluate_super_call,
        evaluate_super_method, evaluate_super_property, evaluate_this, is_class_instance, is_instance_of, is_private_member_declared,
    },
    js_console::{handle_console_method, make_console_object},
    js_date::is_date_object,
    js_math::{handle_math_method, make_math_object},
    js_number::make_number_object,
    js_promise::{JSPromise, PromiseState, handle_promise_method, run_event_loop},
    js_reflect::make_reflect_object,
    js_regexp::is_regex_object,
    js_testintl::make_testintl_object,
    obj_get_key_value, raise_eval_error, raise_reference_error, raise_syntax_error, raise_throw_error, raise_type_error,
    raise_variable_not_found_error,
    sprintf::handle_sprintf_call,
    tmpfile::{create_tmpfile, handle_file_method},
    unicode::{utf8_to_utf16, utf16_char_at, utf16_len, utf16_slice, utf16_to_utf8},
};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::{cell::RefCell, collections::HashMap, rc::Rc, str::FromStr};

// Thread-local storage for last captured stack frames when an error occurs.
thread_local! {
    static LAST_STACK: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn set_last_stack(frames: Vec<String>) {
    LAST_STACK.with(|s| *s.borrow_mut() = frames);
}

fn take_last_stack() -> Vec<String> {
    LAST_STACK.with(|s| s.borrow_mut().drain(..).collect())
}

// Build a human-friendly frame name including approximate source location
fn build_frame_name(caller_env: &JSObjectDataPtr, base: &str) -> String {
    // Attempt to find a script name by walking the env chain for `__script_name`
    let mut script_name = "<script>".to_string();
    let mut line: Option<usize> = None;
    let mut column: Option<usize> = None;
    let mut env_opt = Some(caller_env.clone());
    while let Some(env_ptr) = env_opt {
        if let Ok(Some(sn_rc)) = obj_get_key_value(&env_ptr, &"__script_name".into()) {
            if let Value::String(s_utf16) = &*sn_rc.borrow() {
                script_name = utf16_to_utf8(s_utf16);
            }
        }
        if line.is_none() {
            if let Ok(Some(line_rc)) = obj_get_key_value(&env_ptr, &"__line".into()) {
                if let Value::Number(n) = &*line_rc.borrow() {
                    line = Some(*n as usize);
                }
            }
        }
        if column.is_none() {
            if let Ok(Some(col_rc)) = obj_get_key_value(&env_ptr, &"__column".into()) {
                if let Value::Number(n) = &*col_rc.borrow() {
                    column = Some(*n as usize);
                }
            }
        }
        // follow prototype/caller chain to find root script name if needed (upgrade Weak)
        env_opt = env_ptr.borrow().prototype.clone().and_then(|w| w.upgrade());
    }
    if let Some(ln) = line {
        let col = column.unwrap_or(0);
        format!("{base} ({script_name}:{ln}:{col})")
    } else {
        format!("{base} ({script_name})")
    }
}

// Derive a succinct frame base (function name) and the script name for an
// environment. This collapses the repeated logic used throughout the file
// to prefer an explicit `__frame` value and to read `__script_name`.
fn derive_frame_base_and_script(env: &JSObjectDataPtr) -> (String, String) {
    let frame_name = if let Ok(Some(frame_val_rc)) = obj_get_key_value(env, &"__frame".into()) {
        if let Value::String(s_utf16) = &*frame_val_rc.borrow() {
            utf16_to_utf8(s_utf16)
        } else {
            build_frame_name(env, "<anonymous>")
        }
    } else {
        build_frame_name(env, "<anonymous>")
    };
    let base = match frame_name.find(" (") {
        Some(idx) => frame_name[..idx].to_string(),
        None => frame_name,
    };
    let mut script_name = "<script>".to_string();
    if let Ok(Some(sn_rc)) = obj_get_key_value(env, &"__script_name".into()) {
        if let Value::String(s) = &*sn_rc.borrow() {
            script_name = utf16_to_utf8(s);
        }
    }
    (base, script_name)
}

thread_local! {
    static SYMBOL_REGISTRY: RefCell<HashMap<String, Rc<RefCell<Value>>>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Debug)]
pub enum ControlFlow {
    Normal(Value),
    Break(Option<String>),
    Continue(Option<String>),
    Return(Value),
}

fn validate_declarations(statements: &[Statement]) -> Result<(), JSError> {
    let mut lexical_names = std::collections::HashSet::new();

    for stmt in statements {
        match &stmt.kind {
            StatementKind::Let(decls) => {
                for (name, _) in decls {
                    if lexical_names.contains(name) {
                        let mut err = raise_syntax_error!(format!("Identifier '{name}' has already been declared"));
                        err.set_js_location(stmt.line, stmt.column);
                        return Err(err);
                    }
                    lexical_names.insert(name.clone());
                }
            }
            StatementKind::Const(decls) => {
                for (name, _) in decls {
                    if lexical_names.contains(name) {
                        let mut err = raise_syntax_error!(format!("Identifier '{name}' has already been declared"));
                        err.set_js_location(stmt.line, stmt.column);
                        return Err(err);
                    }
                    lexical_names.insert(name.clone());
                }
            }
            StatementKind::Class(name, _, _) => {
                if lexical_names.contains(name) {
                    let mut err = raise_syntax_error!(format!("Identifier '{name}' has already been declared"));
                    err.set_js_location(stmt.line, stmt.column);
                    return Err(err);
                }
                lexical_names.insert(name.clone());
            }
            StatementKind::FunctionDeclaration(name, _, body, _) => {
                if lexical_names.contains(name) {
                    let mut err = raise_syntax_error!(format!("Identifier '{name}' has already been declared"));
                    err.set_js_location(stmt.line, stmt.column);
                    return Err(err);
                }
                lexical_names.insert(name.clone());
                // Recursively validate function body
                validate_declarations(body)?;
            }
            StatementKind::LetDestructuringArray(pattern, _) | StatementKind::ConstDestructuringArray(pattern, _) => {
                collect_lexical_names_from_array(pattern, &mut lexical_names, stmt.line, stmt.column)?;
            }
            StatementKind::LetDestructuringObject(pattern, _) | StatementKind::ConstDestructuringObject(pattern, _) => {
                collect_lexical_names_from_object(pattern, &mut lexical_names, stmt.line, stmt.column)?;
            }
            StatementKind::Block(stmts) => {
                validate_declarations(stmts)?;
            }
            StatementKind::If(_, then_body, else_body) => {
                validate_declarations(then_body)?;
                if let Some(else_stmts) = else_body {
                    validate_declarations(else_stmts)?;
                }
            }
            StatementKind::For(_, _, _, body) => {
                validate_declarations(body)?;
            }
            StatementKind::ForIn(_, _, body) => {
                validate_declarations(body)?;
            }
            StatementKind::ForOf(_, _, body) => {
                validate_declarations(body)?;
            }
            StatementKind::ForOfDestructuringArray(_, _, body) => {
                validate_declarations(body)?;
            }
            StatementKind::ForOfDestructuringObject(_, _, body) => {
                validate_declarations(body)?;
            }
            StatementKind::While(_, body) => {
                validate_declarations(body)?;
            }
            StatementKind::DoWhile(body, _) => {
                validate_declarations(body)?;
            }
            StatementKind::Switch(_, cases) => {
                for case in cases {
                    match case {
                        SwitchCase::Case(_, stmts) | SwitchCase::Default(stmts) => {
                            validate_declarations(stmts)?;
                        }
                    }
                }
            }
            StatementKind::TryCatch(try_block, _, catch_block, finally_block) => {
                validate_declarations(try_block)?;
                validate_declarations(catch_block)?;
                if let Some(finally_stmts) = finally_block {
                    validate_declarations(finally_stmts)?;
                }
            }
            _ => {}
        }
    }

    let mut var_names = std::collections::HashSet::new();
    collect_var_names(statements, &mut var_names);

    for name in lexical_names {
        if var_names.contains(&name) {
            // We have a conflict between a lexical declaration and a var declaration.
            // We should report the error at the location of the declaration that appears later in the source.
            let lexical_stmt = statements.iter().find(|s| declares_lexical_name(s, &name));
            let var_loc = find_first_var_location(statements, &name);

            if let (Some(l_stmt), Some(v_loc)) = (lexical_stmt, var_loc) {
                let l_loc = (l_stmt.line, l_stmt.column);
                let (err_line, err_col) = if l_loc > v_loc { l_loc } else { v_loc };

                let mut err = raise_syntax_error!(format!("Identifier '{}' has already been declared", name));
                err.set_js_location(err_line, err_col);
                return Err(err);
            }
            return Err(raise_syntax_error!(format!("Identifier '{}' has already been declared", name)));
        }
    }
    Ok(())
}

fn declares_lexical_name(stmt: &Statement, name: &str) -> bool {
    match &stmt.kind {
        StatementKind::Let(decls) => decls.iter().any(|(n, _)| n == name),
        StatementKind::Const(decls) => decls.iter().any(|(n, _)| n == name),
        StatementKind::Class(n, _, _) => n == name,
        StatementKind::FunctionDeclaration(n, _, _, _) => n == name,
        StatementKind::LetDestructuringArray(pattern, _) | StatementKind::ConstDestructuringArray(pattern, _) => {
            pattern_contains_name(pattern, name)
        }
        StatementKind::LetDestructuringObject(pattern, _) | StatementKind::ConstDestructuringObject(pattern, _) => {
            object_pattern_contains_name(pattern, name)
        }
        _ => false,
    }
}

fn find_first_var_location(statements: &[Statement], name: &str) -> Option<(usize, usize)> {
    for stmt in statements {
        match &stmt.kind {
            StatementKind::Var(decls) => {
                for (n, _) in decls {
                    if n == name {
                        return Some((stmt.line, stmt.column));
                    }
                }
            }
            StatementKind::If(_, then_body, else_body) => {
                if let Some(loc) = find_first_var_location(then_body, name) {
                    return Some(loc);
                }
                if let Some(else_stmts) = else_body {
                    if let Some(loc) = find_first_var_location(else_stmts, name) {
                        return Some(loc);
                    }
                }
            }
            StatementKind::For(_, _, _, body) => {
                if let Some(loc) = find_first_var_location(body, name) {
                    return Some(loc);
                }
            }
            StatementKind::ForOf(_, _, body) => {
                if let Some(loc) = find_first_var_location(body, name) {
                    return Some(loc);
                }
            }
            StatementKind::ForIn(var, _, body) => {
                if var == name {
                    return Some((stmt.line, stmt.column));
                }
                if let Some(loc) = find_first_var_location(body, name) {
                    return Some(loc);
                }
            }
            StatementKind::ForOfDestructuringObject(pattern, _, body) => {
                if object_pattern_contains_name(pattern, name) {
                    return Some((stmt.line, stmt.column));
                }
                if let Some(loc) = find_first_var_location(body, name) {
                    return Some(loc);
                }
            }
            StatementKind::ForOfDestructuringArray(pattern, _, body) => {
                if pattern_contains_name(pattern, name) {
                    return Some((stmt.line, stmt.column));
                }
                if let Some(loc) = find_first_var_location(body, name) {
                    return Some(loc);
                }
            }
            StatementKind::While(_, body) => {
                if let Some(loc) = find_first_var_location(body, name) {
                    return Some(loc);
                }
            }
            StatementKind::DoWhile(body, _) => {
                if let Some(loc) = find_first_var_location(body, name) {
                    return Some(loc);
                }
            }
            StatementKind::Switch(_, cases) => {
                for case in cases {
                    match case {
                        SwitchCase::Case(_, stmts) | SwitchCase::Default(stmts) => {
                            if let Some(loc) = find_first_var_location(stmts, name) {
                                return Some(loc);
                            }
                        }
                    }
                }
            }
            StatementKind::TryCatch(try_body, _, catch_body, finally_body) => {
                if let Some(loc) = find_first_var_location(try_body, name) {
                    return Some(loc);
                }
                if let Some(loc) = find_first_var_location(catch_body, name) {
                    return Some(loc);
                }
                if let Some(finally_stmts) = finally_body {
                    if let Some(loc) = find_first_var_location(finally_stmts, name) {
                        return Some(loc);
                    }
                }
            }
            StatementKind::Block(stmts) => {
                if let Some(loc) = find_first_var_location(stmts, name) {
                    return Some(loc);
                }
            }
            StatementKind::Label(_, stmt) => {
                if let Some(loc) = find_first_var_location(std::slice::from_ref(stmt), name) {
                    return Some(loc);
                }
            }
            _ => {}
        }
    }
    None
}

fn pattern_contains_name(pattern: &[DestructuringElement], name: &str) -> bool {
    for element in pattern {
        match element {
            DestructuringElement::Variable(var, _) => {
                if var == name {
                    return true;
                }
            }
            DestructuringElement::NestedArray(nested) => {
                if pattern_contains_name(nested, name) {
                    return true;
                }
            }
            DestructuringElement::NestedObject(nested) => {
                if object_pattern_contains_name(nested, name) {
                    return true;
                }
            }
            DestructuringElement::Rest(var) => {
                if var == name {
                    return true;
                }
            }
            DestructuringElement::Empty => {}
        }
    }
    false
}

fn object_pattern_contains_name(pattern: &[ObjectDestructuringElement], name: &str) -> bool {
    for element in pattern {
        match element {
            ObjectDestructuringElement::Property { value, .. } => match value {
                DestructuringElement::Variable(var, _) => {
                    if var == name {
                        return true;
                    }
                }
                DestructuringElement::NestedArray(nested) => {
                    if pattern_contains_name(nested, name) {
                        return true;
                    }
                }
                DestructuringElement::NestedObject(nested) => {
                    if object_pattern_contains_name(nested, name) {
                        return true;
                    }
                }
                DestructuringElement::Rest(var) => {
                    if var == name {
                        return true;
                    }
                }
                DestructuringElement::Empty => {}
            },
            ObjectDestructuringElement::Rest(var) => {
                if var == name {
                    return true;
                }
            }
        }
    }
    false
}

fn collect_lexical_names_from_array(
    pattern: &[DestructuringElement],
    names: &mut std::collections::HashSet<String>,
    line: usize,
    column: usize,
) -> Result<(), JSError> {
    for element in pattern {
        match element {
            DestructuringElement::Variable(var, _) => {
                if names.contains(var) {
                    let mut err = raise_syntax_error!(format!("Identifier '{var}' has already been declared"));
                    err.set_js_location(line, column);
                    return Err(err);
                }
                names.insert(var.clone());
            }
            DestructuringElement::NestedArray(nested) => collect_lexical_names_from_array(nested, names, line, column)?,
            DestructuringElement::NestedObject(nested) => collect_lexical_names_from_object(nested, names, line, column)?,
            DestructuringElement::Rest(var) => {
                if names.contains(var) {
                    let mut err = raise_syntax_error!(format!("Identifier '{var}' has already been declared"));
                    err.set_js_location(line, column);
                    return Err(err);
                }
                names.insert(var.clone());
            }
            DestructuringElement::Empty => {}
        }
    }
    Ok(())
}

fn collect_lexical_names_from_object(
    pattern: &[ObjectDestructuringElement],
    names: &mut std::collections::HashSet<String>,
    line: usize,
    column: usize,
) -> Result<(), JSError> {
    for element in pattern {
        match element {
            ObjectDestructuringElement::Property { value, .. } => match value {
                DestructuringElement::Variable(var, _) => {
                    if names.contains(var) {
                        let mut err = raise_syntax_error!(format!("Identifier '{var}' has already been declared"));
                        err.set_js_location(line, column);
                        return Err(err);
                    }
                    names.insert(var.clone());
                }
                DestructuringElement::NestedArray(nested) => collect_lexical_names_from_array(nested, names, line, column)?,
                DestructuringElement::NestedObject(nested) => collect_lexical_names_from_object(nested, names, line, column)?,
                DestructuringElement::Rest(var) => {
                    if names.contains(var) {
                        let mut err = raise_syntax_error!(format!("Identifier '{var}' has already been declared"));
                        err.set_js_location(line, column);
                        return Err(err);
                    }
                    names.insert(var.clone());
                }
                DestructuringElement::Empty => {}
            },
            ObjectDestructuringElement::Rest(var) => {
                if names.contains(var) {
                    let mut err = raise_syntax_error!(format!("Identifier '{var}' has already been declared"));
                    err.set_js_location(line, column);
                    return Err(err);
                }
                names.insert(var.clone());
            }
        }
    }
    Ok(())
}

pub fn evaluate_statements(env: &JSObjectDataPtr, statements: &[Statement]) -> Result<Value, JSError> {
    match evaluate_statements_with_context(env, statements)? {
        ControlFlow::Normal(val) => Ok(val),
        ControlFlow::Break(_) => Err(raise_eval_error!("break statement not in loop or switch")),
        ControlFlow::Continue(_) => Err(raise_eval_error!("continue statement not in loop")),
        ControlFlow::Return(val) => Ok(val),
    }
}

fn set_function_name_if_needed(val: &Value, name: &str) -> Result<(), JSError> {
    if let Value::Object(object) = val {
        if let Some(_cl) = obj_get_key_value(object, &"__closure__".into())? {
            let existing = obj_get_key_value(object, &"name".into())?;
            if existing.is_none() {
                let name_val = Value::String(utf8_to_utf16(name));
                obj_set_key_value(object, &"name".into(), name_val)?;
            }
        }
    }
    Ok(())
}

fn ensure_object_destructuring_target(val: &Value, pattern: &[ObjectDestructuringElement], expr: &Expr) -> Result<(), JSError> {
    if !matches!(val, Value::Object(_)) {
        let first_key = pattern.iter().find_map(|el| {
            if let ObjectDestructuringElement::Property { key, .. } = el {
                Some(key.clone())
            } else {
                None
            }
        });

        let message = if let Some(first) = first_key {
            if let Expr::Var(name, _, _) = expr {
                let value_desc = match val {
                    Value::Undefined => "undefined",
                    Value::Object(_) => "object",
                    _ => "non-object value",
                };
                format!("Cannot destructure property '{first}' of '{name}' as it is {value_desc}")
            } else {
                format!("Cannot destructure property '{first}' from non-object value")
            }
        } else {
            "Cannot destructure non-object value".to_string()
        };

        return Err(raise_eval_error!(message));
    }
    Ok(())
}

fn hoist_declarations(env: &JSObjectDataPtr, statements: &[Statement]) -> Result<(), JSError> {
    // Hoist var declarations if this is a function scope
    if env.borrow().is_function_scope {
        let mut var_names = std::collections::HashSet::new();
        collect_var_names(statements, &mut var_names);
        for name in var_names {
            env_set(env, &name, Value::Undefined)?;
            env.borrow_mut().set_non_configurable(PropertyKey::String(name));
        }
    }

    // Hoist function declarations
    for stmt in statements {
        if let StatementKind::FunctionDeclaration(name, params, body, is_generator) = &stmt.kind {
            let func_val = if *is_generator {
                // For generator functions, create a function object wrapper
                let func_obj = new_js_object_data();
                let prototype_obj = new_js_object_data();
                // Link new function prototype to Object.prototype so instances inherit Object.prototype methods
                crate::core::set_internal_prototype_from_constructor(&prototype_obj, env, "Object")?;
                let generator_val = Value::GeneratorFunction(None, Rc::new(ClosureData::new(params, body, env, None)));
                obj_set_key_value(&func_obj, &"__closure__".into(), generator_val)?;
                obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
                obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
                Value::Object(func_obj)
            } else {
                // For regular functions, create a function object wrapper
                let func_obj = new_js_object_data();
                let prototype_obj = new_js_object_data();
                // Link new function prototype to Object.prototype so instances inherit Object.prototype methods
                crate::core::set_internal_prototype_from_constructor(&prototype_obj, env, "Object")?;
                let closure_val = Value::Closure(Rc::new(ClosureData::new(params, body, env, None)));
                obj_set_key_value(&func_obj, &"__closure__".into(), closure_val)?;
                obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
                obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
                // Ensure wrapper function objects inherit from Function.prototype so
                // `.call`/`.apply` are available via the prototype chain.
                if let Some(func_proto) = crate::core::get_constructor_prototype(env, "Function")? {
                    func_obj.borrow_mut().prototype = Some(Rc::downgrade(&func_proto));
                    let _ = crate::core::obj_set_key_value(&func_obj, &"__proto__".into(), Value::Object(func_proto.clone()));
                }
                Value::Object(func_obj)
            };
            env_set(env, name, func_val.clone())?;
            env.borrow_mut().set_non_configurable(PropertyKey::String(name.clone()));
            // In non-strict mode (assumed), function declarations in blocks are hoisted
            // to the nearest function/global scope (Annex B.3.3).
            if !env.borrow().is_function_scope {
                env_set_var(env, name, func_val)?;
            }
        } else if let StatementKind::Class(name, _, _) = &stmt.kind {
            // Hoist class declarations as uninitialized (TDZ)
            env_set(env, name, Value::Uninitialized)?;
            env.borrow_mut().set_non_configurable(PropertyKey::String(name.clone()));
        }
    }
    Ok(())
}

fn evaluate_stmt_let(env: &JSObjectDataPtr, name: &str, expr_opt: &Option<Expr>) -> Result<Value, JSError> {
    if get_own_property(env, &name.into()).is_some() {
        return Err(raise_syntax_error!(format!("Identifier '{name}' has already been declared")));
    }
    let val = expr_opt.clone().map_or(Ok(Value::Undefined), |expr| evaluate_expr(env, &expr))?;
    set_function_name_if_needed(&val, name)?;
    if let Value::Object(object) = &val {
        log::debug!("DBG Let - binding '{name}' into env -> func_obj ptr={:p}", Rc::as_ptr(object));
    } else {
        log::debug!("DBG Let - binding '{name}' into env -> value={val:?}");
    }
    env_set(env, name, val.clone())?;
    env.borrow_mut().set_non_configurable(PropertyKey::String(name.to_string()));
    Ok(val)
}

fn evaluate_stmt_var(env: &JSObjectDataPtr, name: &str, expr_opt: &Option<Expr>) -> Result<Value, JSError> {
    let val = expr_opt.clone().map_or(Ok(Value::Undefined), |expr| evaluate_expr(env, &expr))?;
    set_function_name_if_needed(&val, name)?;
    env_set_var(env, name, val.clone())?;
    Ok(val)
}

fn evaluate_stmt_const(env: &JSObjectDataPtr, name: &str, expr: &Expr) -> Result<Value, JSError> {
    if get_own_property(env, &name.into()).is_some() {
        return Err(raise_syntax_error!(format!("Identifier '{name}' has already been declared")));
    }
    let val = evaluate_expr(env, expr)?;
    set_function_name_if_needed(&val, name)?;
    env_set_const(env, name, val.clone());
    Ok(val)
}

fn evaluate_stmt_class(
    env: &JSObjectDataPtr,
    name: &str,
    extends: &Option<Expr>,
    members: &[crate::js_class::ClassMember],
) -> Result<(), JSError> {
    // Note: Duplicate declaration checks are handled by validate_declarations.
    // For class declarations we need the class name binding to be available during class evaluation
    // (so static blocks can reference the class), so request that the name be bound early.

    let class_obj = create_class_object(name, extends, members, env, true)?;
    // Ensure the binding is set to the final class object (overwrite if necessary)
    env_set(env, name, class_obj)?;
    Ok(())
}

fn evaluate_stmt_block(env: &JSObjectDataPtr, stmts: &[Statement], last_value: &mut Value) -> Result<Option<ControlFlow>, JSError> {
    let block_env = new_js_object_data();
    block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
    block_env.borrow_mut().is_function_scope = false;
    match evaluate_statements_with_context(&block_env, stmts)? {
        ControlFlow::Normal(val) => *last_value = val,
        cf => return Ok(Some(cf)),
    }
    Ok(None)
}

fn evaluate_stmt_assign(env: &JSObjectDataPtr, name: &str, expr: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, expr)?;
    env_set_recursive(env, name, val.clone())?;
    log::trace!("Assigned value to '{name}': {val:?}");
    Ok(val)
}

fn evaluate_stmt_import(
    env: &JSObjectDataPtr,
    specifiers: &[crate::core::statement::ImportSpecifier],
    module_name: &str,
) -> Result<(), JSError> {
    let module_value = crate::js_module::load_module(module_name, None)?;
    for specifier in specifiers {
        match specifier {
            crate::core::statement::ImportSpecifier::Default(name) => {
                match crate::js_module::import_from_module(&module_value, "default") {
                    Ok(default_value) => env_set(env, name, default_value)?,
                    Err(_) => env_set(env, name, module_value.clone())?,
                }
            }
            crate::core::statement::ImportSpecifier::Named(name, alias) => {
                let imported_value = crate::js_module::import_from_module(&module_value, name)?;
                let import_name = alias.as_ref().unwrap_or(name);
                env_set(env, import_name, imported_value)?;
            }
            crate::core::statement::ImportSpecifier::Namespace(name) => {
                env_set(env, name, module_value.clone())?;
            }
        }
    }
    Ok(())
}

fn evaluate_stmt_export(
    env: &JSObjectDataPtr,
    specifiers: &[crate::core::statement::ExportSpecifier],
    maybe_decl: &Option<Box<Statement>>,
) -> Result<(), JSError> {
    if let Some(decl_stmt) = maybe_decl {
        match &decl_stmt.kind {
            StatementKind::Const(decls) => {
                for (name, expr) in decls {
                    evaluate_stmt_const(env, name, expr)?;
                }
            }
            StatementKind::Let(decls) => {
                for (name, expr_opt) in decls {
                    evaluate_stmt_let(env, name, expr_opt)?;
                }
            }
            StatementKind::Var(decls) => {
                for (name, expr_opt) in decls {
                    evaluate_stmt_var(env, name, expr_opt)?;
                }
            }
            StatementKind::Class(name, extends, members) => evaluate_stmt_class(env, name, extends, members)?,
            StatementKind::FunctionDeclaration(name, params, body, is_generator) => {
                let func_val = if *is_generator {
                    let func_obj = new_js_object_data();
                    let prototype_obj = new_js_object_data();
                    let generator_val = Value::GeneratorFunction(None, Rc::new(ClosureData::new(params, body, env, None)));
                    obj_set_key_value(&func_obj, &"__closure__".into(), generator_val)?;
                    obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
                    obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
                    Value::Object(func_obj)
                } else {
                    let func_obj = new_js_object_data();
                    let prototype_obj = new_js_object_data();
                    let closure_val = Value::Closure(Rc::new(ClosureData::new(params, body, env, None)));
                    obj_set_key_value(&func_obj, &"__closure__".into(), closure_val)?;
                    obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
                    obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
                    // Ensure wrapper function objects inherit from Function.prototype so
                    // `.call`/`.apply` are available via the prototype chain.
                    if let Some(func_proto) = crate::core::get_constructor_prototype(env, "Function")? {
                        func_obj.borrow_mut().prototype = Some(Rc::downgrade(&func_proto));
                        let _ = crate::core::obj_set_key_value(&func_obj, &"__proto__".into(), Value::Object(func_proto.clone()));
                    }
                    Value::Object(func_obj)
                };
                env_set(env, name, func_val)?;
            }
            _ => {
                return Err(raise_eval_error!("Invalid export declaration"));
            }
        }
    }

    // Handle exports in module context
    let exports_opt = get_own_property(env, &crate::core::PropertyKey::String("exports".to_string()));
    if let Some(exports_val) = exports_opt {
        if let Value::Object(exports_obj) = &*exports_val.borrow() {
            for specifier in specifiers {
                match specifier {
                    crate::core::statement::ExportSpecifier::Named(name, alias) => {
                        let var_opt = get_own_property(env, &crate::core::PropertyKey::String(name.clone()));
                        if let Some(var_val) = var_opt {
                            let export_name = alias.as_ref().unwrap_or(name).clone();
                            exports_obj.borrow_mut().insert(
                                crate::core::PropertyKey::String(export_name),
                                Rc::new(RefCell::new(var_val.borrow().clone())),
                            );
                        } else {
                            return Err(raise_eval_error!(format!("Export '{}' not found in scope", name)));
                        }
                    }
                    crate::core::statement::ExportSpecifier::Default(expr) => {
                        let val = evaluate_expr(env, expr)?;
                        exports_obj
                            .borrow_mut()
                            .insert(crate::core::PropertyKey::String("default".to_string()), Rc::new(RefCell::new(val)));
                    }
                }
            }
        }
    }
    log::debug!("Export statement: specifiers={:?}", specifiers);
    Ok(())
}

fn evaluate_stmt_expr(env: &JSObjectDataPtr, expr: &Expr, last_value: &mut Value) -> Result<Option<ControlFlow>, JSError> {
    perform_statement_expression(env, expr, last_value)
}

fn evaluate_stmt_return(env: &JSObjectDataPtr, expr_opt: &Option<Expr>) -> Result<Option<ControlFlow>, JSError> {
    let return_val = match expr_opt {
        Some(expr) => evaluate_expr(env, expr)?,
        None => Value::Undefined,
    };
    log::trace!("StatementKind::Return evaluated value = {return_val:?}");
    Ok(Some(ControlFlow::Return(return_val)))
}

fn evaluate_stmt_throw(env: &JSObjectDataPtr, expr: &Expr) -> Result<Option<ControlFlow>, JSError> {
    let throw_val = evaluate_expr(env, expr)?;

    // If the thrown value is an Error-like object, update its `stack` string
    // here at the throw site so it reflects the actual statement location
    // (rather than an earlier construction site or a surrounding callback).
    if let Value::Object(object) = &throw_val {
        // Determine header (Error: message) similar to JSError::message handling
        let mut header = None;
        if let Ok(Some(ctor_rc)) = obj_get_key_value(object, &"constructor".into()) {
            if let Value::Object(ctor_obj) = &*ctor_rc.borrow() {
                if let Ok(Some(name_rc)) = obj_get_key_value(ctor_obj, &"name".into()) {
                    if let Value::String(name_utf16) = &*name_rc.borrow() {
                        let ctor_name = utf16_to_utf8(name_utf16);
                        // prefer message property if present
                        if let Ok(Some(msg_rc)) = obj_get_key_value(object, &"message".into())
                            && let Value::String(msg_utf16) = &*msg_rc.borrow()
                        {
                            let msg = utf16_to_utf8(msg_utf16);
                            header = Some(format!("{ctor_name}: {msg}"));
                        } else {
                            header = ctor_name.into();
                        }
                    }
                }
            }
        }
        if header.is_none() {
            if let Ok(Some(msg_rc)) = obj_get_key_value(object, &"message".into()) {
                if let Value::String(msg_utf16) = &*msg_rc.borrow() {
                    header = Some(format!("Uncaught {}", utf16_to_utf8(msg_utf16)));
                }
            }
        }
        let header = header.unwrap_or_else(|| "Uncaught thrown value".to_string());

        // Fetch the current statement location from env if present
        let mut line = 0usize;
        let mut column = 0usize;
        if let Ok(Some(line_rc)) = obj_get_key_value(env, &"__line".into()) {
            if let Value::Number(n) = &*line_rc.borrow() {
                line = *n as usize;
            }
        }
        if let Ok(Some(col_rc)) = obj_get_key_value(env, &"__column".into()) {
            if let Value::Number(n) = &*col_rc.borrow() {
                column = *n as usize;
            }
        }

        // (debugging removed)

        // Prefer a more-precise location from the thrown expression itself
        // when available (e.g. `new Error(...)` -> use position of `Error`).
        fn expr_position(expr: &crate::core::Expr) -> Option<(usize, usize)> {
            use crate::core::Expr::*;
            match expr {
                Var(_, Some(l), Some(c)) => Some((*l, *c)),
                New(boxed, _) => match &**boxed {
                    Var(_, Some(l), Some(c)) => Some((*l, *c)),
                    _ => None,
                },
                Call(boxed, _) => match &**boxed {
                    Var(_, Some(l), Some(c)) => Some((*l, *c)),
                    _ => None,
                },
                Property(boxed, _) => match &**boxed {
                    Var(_, Some(l), Some(c)) => Some((*l, *c)),
                    _ => None,
                },
                _ => None,
            }
        }

        if let Some((el, ec)) = expr_position(expr) {
            // override column/line with the expression's position for better parity
            // with Node.js which points at the constructor/identifier inside
            // `throw new Error(...)` rather than the `throw` keyword.
            line = el;
            column = ec;
        }

        // Record these precise thrown-site coordinates on the thrown object
        let _ = obj_set_key_value(object, &"__thrown_line".into(), Value::Number(line as f64));
        let _ = obj_set_key_value(object, &"__thrown_column".into(), Value::Number(column as f64));

        // Derive base and script name for the current environment
        let (base, script_name) = derive_frame_base_and_script(env);

        // Build full frame list by walking __frame / __caller links so we
        // include the full call path (innermost first). Replace the innermost
        // frame with the precise thrown-site location (line/column) so the
        // top of the stack points to the actual throw site.
        // Build frames using the same heuristics as `capture_frames_from_env`
        // so we prefer recorded call-site (`__call_*`) info over a
        // function's own declaration/body location.
        let mut frames: Vec<String> = Vec::new();
        let mut env_opt = Some(env.clone());
        while let Some(env_ptr) = env_opt {
            // derive base name
            let (base, _) = derive_frame_base_and_script(&env_ptr);

            // prefer __call_* on the function env, then caller env's __line/__column,
            // then fall back to env's own __line/__column
            let mut line = 0usize;
            let mut col = 0usize;
            let mut script_name = "<script>".to_string();
            if let Ok(Some(call_line_rc)) = obj_get_key_value(&env_ptr, &"__call_line".into()) {
                if let Value::Number(n) = &*call_line_rc.borrow() {
                    line = *n as usize;
                }
            }
            if let Ok(Some(call_col_rc)) = obj_get_key_value(&env_ptr, &"__call_column".into()) {
                if let Value::Number(n) = &*call_col_rc.borrow() {
                    col = *n as usize;
                }
            }
            if let Ok(Some(call_sn_rc)) = obj_get_key_value(&env_ptr, &"__call_script_name".into()) {
                if let Value::String(s) = &*call_sn_rc.borrow() {
                    script_name = utf16_to_utf8(s);
                }
            }

            if line == 0 {
                if let Ok(Some(caller_rc)) = obj_get_key_value(&env_ptr, &"__caller".into()) {
                    if let Value::Object(caller_env) = &*caller_rc.borrow() {
                        if let Ok(Some(line_rc)) = obj_get_key_value(caller_env, &"__line".into()) {
                            if let Value::Number(n) = &*line_rc.borrow() {
                                line = *n as usize;
                            }
                        }
                        if let Ok(Some(col_rc)) = obj_get_key_value(caller_env, &"__column".into()) {
                            if let Value::Number(n) = &*col_rc.borrow() {
                                col = *n as usize;
                            }
                        }
                        if let Ok(Some(sn_rc)) = obj_get_key_value(caller_env, &"__script_name".into()) {
                            if let Value::String(s) = &*sn_rc.borrow() {
                                script_name = utf16_to_utf8(s);
                            }
                        }
                    }
                }
            }

            if line == 0 {
                if let Ok(Some(line_rc)) = obj_get_key_value(&env_ptr, &"__line".into()) {
                    if let Value::Number(n) = &*line_rc.borrow() {
                        line = *n as usize;
                    }
                }
            }
            if col == 0 {
                if let Ok(Some(col_rc)) = obj_get_key_value(&env_ptr, &"__column".into()) {
                    if let Value::Number(n) = &*col_rc.borrow() {
                        col = *n as usize;
                    }
                }
            }
            if script_name == "<script>" {
                if let Ok(Some(sn_rc)) = obj_get_key_value(&env_ptr, &"__script_name".into()) {
                    if let Value::String(s) = &*sn_rc.borrow() {
                        script_name = utf16_to_utf8(s);
                    }
                }
            }

            frames.push(format!("{} ({}:{}:{})", base, script_name, line, col));

            // follow caller link if present
            if let Ok(Some(caller_rc)) = obj_get_key_value(&env_ptr, &"__caller".into()) {
                if let Value::Object(caller_env) = &*caller_rc.borrow() {
                    env_opt = Some(caller_env.clone());
                    continue;
                }
            }
            env_opt = env_ptr.borrow().prototype.clone().and_then(|w| w.upgrade());
        }

        // Create thrown-site frame and place it at the front
        let thrown_frame = format!("{} ({}:{}:{})", base, script_name, line, column);
        if frames.is_empty() {
            frames.push(thrown_frame);
        } else {
            frames[0] = thrown_frame;
        }

        // Build stack string: header + each frame on its own line
        let mut stack_lines = vec![header];
        for f in frames {
            stack_lines.push(format!("    at {}", f));
        }
        let stack_str = stack_lines.join("\n");
        let _ = obj_set_key_value(object, &"stack".into(), Value::String(utf8_to_utf16(&stack_str)));
    }

    Err(raise_throw_error!(throw_val))
}

fn evaluate_stmt_if(
    env: &JSObjectDataPtr,
    condition: &Expr,
    then_body: &[Statement],
    else_body: &Option<Vec<Statement>>,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    perform_statement_if_then_else(env, condition, then_body, else_body, last_value)
}

fn evaluate_stmt_label(
    env: &JSObjectDataPtr,
    label_name: &str,
    inner_stmt: &Statement,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    perform_statement_label(env, label_name, inner_stmt, last_value)
}

fn evaluate_stmt_try_catch(
    env: &JSObjectDataPtr,
    try_body: &[Statement],
    catch_param: &str,
    catch_body: &[Statement],
    finally_body_opt: &Option<Vec<Statement>>,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    statement_try_catch(env, try_body, catch_param, catch_body, finally_body_opt, last_value)
}

fn evaluate_stmt_for(
    env: &JSObjectDataPtr,
    init: &Option<Box<Statement>>,
    condition: &Option<Expr>,
    increment: &Option<Box<Statement>>,
    body: &[Statement],
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    statement_for_init_condition_increment(env, init, condition, increment, body, last_value, None)
}

fn evaluate_stmt_for_of(
    env: &JSObjectDataPtr,
    var: &str,
    iterable: &Expr,
    body: &[Statement],
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    statement_for_of_var_iter(env, var, iterable, body, last_value)
}

fn evaluate_stmt_for_in(
    env: &JSObjectDataPtr,
    var: &str,
    object: &Expr,
    body: &[Statement],
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    statement_for_in_var_object(env, var, object, body, last_value)
}

fn evaluate_stmt_while(
    env: &JSObjectDataPtr,
    condition: &Expr,
    body: &[Statement],
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    statement_while_condition_body(env, condition, body, last_value)
}

fn evaluate_stmt_do_while(
    env: &JSObjectDataPtr,
    body: &[Statement],
    condition: &Expr,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    statement_do_body_while_condition(env, body, condition, last_value)
}

fn evaluate_stmt_switch(
    env: &JSObjectDataPtr,
    expr: &Expr,
    cases: &[crate::core::statement::SwitchCase],
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    eval_switch_statement(env, expr, cases, last_value, None)
}

fn evaluate_stmt_break(opt: &Option<String>) -> Result<Option<ControlFlow>, JSError> {
    Ok(Some(ControlFlow::Break(opt.clone())))
}

fn evaluate_stmt_continue(opt: &Option<String>) -> Result<Option<ControlFlow>, JSError> {
    Ok(Some(ControlFlow::Continue(opt.clone())))
}

fn evaluate_stmt_let_destructuring_array(
    env: &JSObjectDataPtr,
    pattern: &[crate::core::DestructuringElement],
    expr: &Expr,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let val = evaluate_expr(env, expr)?;
    perform_array_destructuring(env, pattern, &val, false)?;
    *last_value = val;
    Ok(None)
}

fn evaluate_stmt_const_destructuring_array(
    env: &JSObjectDataPtr,
    pattern: &[crate::core::DestructuringElement],
    expr: &Expr,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let val = evaluate_expr(env, expr)?;
    perform_array_destructuring(env, pattern, &val, true)?;
    *last_value = val;
    Ok(None)
}

fn evaluate_stmt_let_destructuring_object(
    env: &JSObjectDataPtr,
    pattern: &[crate::core::ObjectDestructuringElement],
    expr: &Expr,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let val = evaluate_expr(env, expr)?;
    ensure_object_destructuring_target(&val, pattern, expr)?;
    perform_object_destructuring(env, pattern, &val, false)?;
    *last_value = val;
    Ok(None)
}

fn evaluate_stmt_const_destructuring_object(
    env: &JSObjectDataPtr,
    pattern: &[crate::core::ObjectDestructuringElement],
    expr: &Expr,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let val = evaluate_expr(env, expr)?;
    ensure_object_destructuring_target(&val, pattern, expr)?;
    perform_object_destructuring(env, pattern, &val, true)?;
    *last_value = val;
    Ok(None)
}

fn evaluate_stmt_var_destructuring_array(
    env: &JSObjectDataPtr,
    pattern: &[crate::core::DestructuringElement],
    expr: &Expr,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let val = evaluate_expr(env, expr)?;
    perform_array_destructuring_var(env, pattern, &val)?;
    *last_value = val;
    Ok(None)
}

fn evaluate_stmt_var_destructuring_object(
    env: &JSObjectDataPtr,
    pattern: &[crate::core::ObjectDestructuringElement],
    expr: &Expr,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let val = evaluate_expr(env, expr)?;
    ensure_object_destructuring_target(&val, pattern, expr)?;
    perform_object_destructuring_var(env, pattern, &val)?;
    *last_value = val;
    Ok(None)
}

fn evaluate_stmt_for_of_destructuring_object(
    env: &JSObjectDataPtr,
    pattern: &[crate::core::ObjectDestructuringElement],
    iterable: &Expr,
    body: &[Statement],
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let iterable_val = evaluate_expr(env, iterable)?;
    if let Some(cf) = for_of_destructuring_object_iter(env, pattern, &iterable_val, body, last_value, None)? {
        return Ok(Some(cf));
    }
    Ok(None)
}

fn evaluate_stmt_for_of_destructuring_array(
    env: &JSObjectDataPtr,
    pattern: &[crate::core::DestructuringElement],
    iterable: &Expr,
    body: &[Statement],
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let iterable_val = evaluate_expr(env, iterable)?;
    if let Some(cf) = for_of_destructuring_array_iter(env, pattern, &iterable_val, body, last_value, None)? {
        return Ok(Some(cf));
    }
    Ok(None)
}

// Evaluate the statement inside a closure so we can log the
// statement index and AST if an error occurs while preserving
// control-flow returns. The closure returns
// Result<Option<ControlFlow>, JSError> where `Ok(None)` means
// continue, `Ok(Some(cf))` means propagate control flow, and
// `Err(e)` means an error that we log and then return.
fn eval_res(stmt: &Statement, last_value: &mut Value, env: &JSObjectDataPtr) -> Result<Option<ControlFlow>, JSError> {
    match &stmt.kind {
        StatementKind::Let(decls) => {
            for (name, expr_opt) in decls {
                *last_value = evaluate_stmt_let(env, name, expr_opt)?;
            }
            Ok(None)
        }
        StatementKind::Var(decls) => {
            for (name, expr_opt) in decls {
                *last_value = evaluate_stmt_var(env, name, expr_opt)?;
            }
            Ok(None)
        }
        StatementKind::Const(decls) => {
            for (name, expr) in decls {
                *last_value = evaluate_stmt_const(env, name, expr)?;
            }
            Ok(None)
        }
        StatementKind::FunctionDeclaration(..) => {
            // Skip function declarations as they are already hoisted
            Ok(None)
        }
        StatementKind::Class(name, extends, members) => {
            evaluate_stmt_class(env, name, extends, members)?;
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Block(stmts) => evaluate_stmt_block(env, stmts, last_value),
        StatementKind::Assign(name, expr) => {
            *last_value = evaluate_stmt_assign(env, name, expr)?;
            Ok(None)
        }
        StatementKind::Expr(expr) => evaluate_stmt_expr(env, expr, last_value),
        StatementKind::Return(expr_opt) => evaluate_stmt_return(env, expr_opt),
        StatementKind::Throw(expr) => evaluate_stmt_throw(env, expr),
        StatementKind::If(condition, then_body, else_body) => evaluate_stmt_if(env, condition, then_body, else_body, last_value),
        StatementKind::ForOfDestructuringObject(pattern, iterable, body) => {
            evaluate_stmt_for_of_destructuring_object(env, pattern, iterable, body, last_value)
        }
        StatementKind::ForOfDestructuringArray(pattern, iterable, body) => {
            evaluate_stmt_for_of_destructuring_array(env, pattern, iterable, body, last_value)
        }
        StatementKind::Label(label_name, inner_stmt) => evaluate_stmt_label(env, label_name, inner_stmt, last_value),
        StatementKind::TryCatch(try_body, catch_param, catch_body, finally_body) => {
            evaluate_stmt_try_catch(env, try_body, catch_param, catch_body, finally_body, last_value)
        }
        StatementKind::For(init, condition, increment, body) => evaluate_stmt_for(env, init, condition, increment, body, last_value),
        StatementKind::ForOf(var, iterable, body) => evaluate_stmt_for_of(env, var, iterable, body, last_value),
        StatementKind::ForIn(var, object, body) => evaluate_stmt_for_in(env, var, object, body, last_value),
        StatementKind::While(condition, body) => evaluate_stmt_while(env, condition, body, last_value),
        StatementKind::DoWhile(body, condition) => evaluate_stmt_do_while(env, body, condition, last_value),
        StatementKind::Switch(expr, cases) => evaluate_stmt_switch(env, expr, cases, last_value),
        StatementKind::Break(opt) => evaluate_stmt_break(opt),
        StatementKind::Continue(opt) => evaluate_stmt_continue(opt),
        StatementKind::LetDestructuringArray(pattern, expr) => evaluate_stmt_let_destructuring_array(env, pattern, expr, last_value),
        StatementKind::VarDestructuringArray(pattern, expr) => evaluate_stmt_var_destructuring_array(env, pattern, expr, last_value),
        StatementKind::ConstDestructuringArray(pattern, expr) => evaluate_stmt_const_destructuring_array(env, pattern, expr, last_value),
        StatementKind::LetDestructuringObject(pattern, expr) => evaluate_stmt_let_destructuring_object(env, pattern, expr, last_value),
        StatementKind::VarDestructuringObject(pattern, expr) => evaluate_stmt_var_destructuring_object(env, pattern, expr, last_value),
        StatementKind::ConstDestructuringObject(pattern, expr) => evaluate_stmt_const_destructuring_object(env, pattern, expr, last_value),
        StatementKind::Import(specifiers, module_name) => {
            evaluate_stmt_import(env, specifiers, module_name)?;
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Export(specifiers, maybe_decl) => {
            evaluate_stmt_export(env, specifiers, maybe_decl)?;
            *last_value = Value::Undefined;
            Ok(None)
        }
    }
}

pub fn evaluate_statements_with_context(env: &JSObjectDataPtr, statements: &[Statement]) -> Result<ControlFlow, JSError> {
    validate_declarations(statements)?;
    hoist_declarations(env, statements)?;

    let mut last_value = Value::Number(0.0);
    for (i, stmt) in statements.iter().enumerate() {
        log::trace!("Evaluating statement {i}: {stmt:?}");
        // Attach statement location to the current env
        let _ = obj_set_key_value(env, &"__line".into(), Value::Number(stmt.line as f64));
        let _ = obj_set_key_value(env, &"__column".into(), Value::Number(stmt.column as f64));

        // Skip function declarations as they are already hoisted
        if let StatementKind::FunctionDeclaration(..) = &stmt.kind {
            continue;
        }

        match eval_res(stmt, &mut last_value, env) {
            Ok(Some(cf)) => return Ok(cf),
            Ok(None) => {}
            Err(mut e) => {
                if e.inner.js_line.is_none() {
                    e.set_js_location(stmt.line, stmt.column);
                }
                // Thrown values (user code `throw`) are expected control flow and
                // we want to preserve the thrown JS `Value` contents for
                // diagnostics rather than letting them be masked by generic
                // EvaluationError messages. Log thrown values at debug level
                // with a readable rendering; keep other engine/internal errors
                // at error level.
                match &e.kind() {
                    JSErrorKind::Throw { value } => {
                        // Provide a helpful representation depending on value type
                        match value {
                            Value::String(s_utf16) => {
                                let s = utf16_to_utf8(s_utf16);
                                log::debug!(
                                    "evaluate_statements_with_context thrown JS value (String) at statement {i}: '{s}' stmt={stmt:?}"
                                );
                            }
                            Value::Object(obj_ptr) => {
                                log::debug!(
                                    "evaluate_statements_with_context thrown JS value (Object) at statement {i}: ptr={:p} stmt={stmt:?}",
                                    Rc::as_ptr(obj_ptr)
                                );
                            }
                            Value::Number(n) => {
                                log::debug!(
                                    "evaluate_statements_with_context thrown JS value (Number) at statement {i}: {n} stmt={stmt:?}"
                                );
                            }
                            Value::Boolean(b) => {
                                log::debug!(
                                    "evaluate_statements_with_context thrown JS value (Boolean) at statement {i}: {b} stmt={stmt:?}"
                                );
                            }
                            Value::Undefined => {
                                log::debug!("evaluate_statements_with_context thrown JS value (Undefined) at statement {i} stmt={stmt:?}");
                            }
                            other => {
                                // Fallback: print Debug and a stringified form
                                log::debug!(
                                    "evaluate_statements_with_context thrown JS value at statement {i}: {other:?} (toString='{}') stmt={stmt:?}",
                                    crate::core::value_to_string(other)
                                );
                            }
                        }
                    }
                    _ => {
                        log::warn!("evaluate_statements_with_context error at statement {i}: {e}, stmt={stmt:?}");
                    }
                }
                // If the thrown JS value recorded a thrown-site on the object
                // (via `__thrown_line` / `__thrown_column`), prefer that
                // location for the error so the message points to the actual
                // throw site instead of the statement's recorded position.
                if let JSErrorKind::Throw { value } = e.kind() {
                    if let Value::Object(obj_ptr) = value {
                        if let Ok(Some(tl_rc)) = obj_get_key_value(obj_ptr, &"__thrown_line".into()) {
                            if let Value::Number(n) = &*tl_rc.borrow() {
                                let col = if let Ok(Some(tc_rc)) = obj_get_key_value(obj_ptr, &"__thrown_column".into()) {
                                    if let Value::Number(nc) = &*tc_rc.borrow() {
                                        *nc as usize
                                    } else {
                                        0
                                    }
                                } else {
                                    0
                                };
                                e.set_js_location(*n as usize, col);
                            }
                        }
                    }
                }

                // Capture a minimal JS-style call stack by walking `__frame`/`__caller`
                // links from the environment where the error occurred. This produces
                // a vector of frame descriptions (innermost first).
                fn capture_frames_from_env(mut env_opt: Option<JSObjectDataPtr>) -> Vec<String> {
                    let mut frames = Vec::new();
                    while let Some(env_ptr) = env_opt {
                        // Derive base name (function name) from __frame if present
                        let frame_name = if let Ok(Some(frame_val_rc)) = obj_get_key_value(&env_ptr, &"__frame".into()) {
                            if let Value::String(s_utf16) = &*frame_val_rc.borrow() {
                                utf16_to_utf8(s_utf16)
                            } else {
                                build_frame_name(&env_ptr, "<anonymous>")
                            }
                        } else {
                            build_frame_name(&env_ptr, "<anonymous>")
                        };
                        let base = match frame_name.find(" (") {
                            Some(idx) => frame_name[..idx].to_string(),
                            None => frame_name,
                        };

                        // Prefer explicit `__call_*` info (set when the call frame
                        // was created) so stack traces show the call-site rather
                        // than the function declaration/body location. Fall back
                        // to the caller env's `__line`/`__column` if present, and
                        // finally to the env's own recorded location.
                        let mut line = 0usize;
                        let mut col = 0usize;
                        let mut script_name = "<script>".to_string();
                        // First, prefer an explicit `__call_line` recorded on the
                        // function env at call-creation time.
                        if let Ok(Some(call_line_rc)) = obj_get_key_value(&env_ptr, &"__call_line".into()) {
                            if let Value::Number(n) = &*call_line_rc.borrow() {
                                line = *n as usize;
                            }
                        }
                        if let Ok(Some(call_col_rc)) = obj_get_key_value(&env_ptr, &"__call_column".into()) {
                            if let Value::Number(n) = &*call_col_rc.borrow() {
                                col = *n as usize;
                            }
                        }
                        if let Ok(Some(call_sn_rc)) = obj_get_key_value(&env_ptr, &"__call_script_name".into()) {
                            if let Value::String(s) = &*call_sn_rc.borrow() {
                                script_name = utf16_to_utf8(s);
                            }
                        }

                        // If no explicit call-site recorded, fall back to the
                        // caller env's recorded `__line`/`__column`/`__script_name`.
                        if line == 0 {
                            if let Ok(Some(caller_rc)) = obj_get_key_value(&env_ptr, &"__caller".into()) {
                                if let Value::Object(caller_env) = &*caller_rc.borrow() {
                                    if let Ok(Some(line_rc)) = obj_get_key_value(caller_env, &"__line".into()) {
                                        if let Value::Number(n) = &*line_rc.borrow() {
                                            line = *n as usize;
                                        }
                                    }
                                    if let Ok(Some(col_rc)) = obj_get_key_value(caller_env, &"__column".into()) {
                                        if let Value::Number(n) = &*col_rc.borrow() {
                                            col = *n as usize;
                                        }
                                    }
                                    if let Ok(Some(sn_rc)) = obj_get_key_value(caller_env, &"__script_name".into()) {
                                        if let Value::String(s) = &*sn_rc.borrow() {
                                            script_name = utf16_to_utf8(s);
                                        }
                                    }
                                }
                            }
                        }

                        // Fallback to the env's own recorded location when caller
                        // did not provide position information.
                        if line == 0 {
                            if let Ok(Some(line_rc)) = obj_get_key_value(&env_ptr, &"__line".into()) {
                                if let Value::Number(n) = &*line_rc.borrow() {
                                    line = *n as usize;
                                }
                            }
                        }
                        if col == 0 {
                            if let Ok(Some(col_rc)) = obj_get_key_value(&env_ptr, &"__column".into()) {
                                if let Value::Number(n) = &*col_rc.borrow() {
                                    col = *n as usize;
                                }
                            }
                        }
                        if script_name == "<script>" {
                            if let Ok(Some(sn_rc)) = obj_get_key_value(&env_ptr, &"__script_name".into()) {
                                if let Value::String(s) = &*sn_rc.borrow() {
                                    script_name = utf16_to_utf8(s);
                                }
                            }
                        }

                        // debug: print raw recorded values for troubleshooting
                        // debug printing removed
                        frames.push(format!("    at {} ({}:{}:{})", base, script_name, line, col));

                        // follow caller link if present
                        if let Ok(Some(caller_rc)) = obj_get_key_value(&env_ptr, &"__caller".into()) {
                            if let Value::Object(caller_env) = &*caller_rc.borrow() {
                                env_opt = Some(caller_env.clone());
                                continue;
                            }
                        }
                        env_opt = env_ptr.borrow().prototype.clone().and_then(|w| w.upgrade());
                    }
                    frames
                }

                let mut frames = capture_frames_from_env(Some(env.clone()));

                // If the thrown JS error recorded a precise statement location (js_line/js_column),
                // prefer that location for the innermost frame so stack traces point to the
                // actual throw site instead of the function's first statement.
                if e.inner.js_line.is_some() {
                    let js_line = e.inner.js_line.unwrap();
                    let js_col = e.inner.js_column.unwrap_or(0);

                    // Try to find the environment whose recorded `__line` matches the
                    // thrown statement location. This gives us the most accurate
                    // function frame name to associate with that statement.
                    let mut matched_env_opt: Option<JSObjectDataPtr> = None;
                    let mut probe_env = Some(env.clone());
                    while let Some(pe) = probe_env {
                        if let Ok(Some(line_rc)) = obj_get_key_value(&pe, &"__line".into()) {
                            if let Value::Number(n) = &*line_rc.borrow() {
                                if (*n as usize) == js_line {
                                    matched_env_opt = Some(pe.clone());
                                    break;
                                }
                            }
                        }
                        probe_env = pe.borrow().prototype.clone().and_then(|w| w.upgrade());
                    }

                    let target_env = matched_env_opt.unwrap_or_else(|| env.clone());

                    // Derive a base frame name from the target env's __frame if present
                    // (it may include additional formatting like "name (script:line:col)").
                    let frame_name = if let Ok(Some(frame_val_rc)) = obj_get_key_value(&target_env, &"__frame".into()) {
                        if let Value::String(s) = &*frame_val_rc.borrow() {
                            utf16_to_utf8(s)
                        } else {
                            build_frame_name(&target_env, "<anonymous>")
                        }
                    } else {
                        build_frame_name(&target_env, "<anonymous>")
                    };
                    let base = match frame_name.find(" (") {
                        Some(idx) => frame_name[..idx].to_string(),
                        None => frame_name,
                    };
                    // Determine script name from the target environment if available
                    let mut script_name = "<script>".to_string();
                    if let Ok(Some(sn_rc)) = obj_get_key_value(&target_env, &"__script_name".into()) {
                        if let Value::String(s) = &*sn_rc.borrow() {
                            script_name = utf16_to_utf8(s);
                        }
                    }
                    let thrown_frame = format!("    at {} ({}:{}:{})", base, script_name, js_line, js_col);
                    if frames.is_empty() {
                        frames.push(thrown_frame);
                    } else {
                        frames[0] = thrown_frame;
                    }
                }

                set_last_stack(frames.clone());
                let mut err = e;
                if err.stack().is_empty() {
                    err.set_stack(frames);
                }
                return Err(err);
            }
        }
    }
    Ok(ControlFlow::Normal(last_value))
}

fn statement_while_condition_body(
    env: &JSObjectDataPtr,
    condition: &Expr,
    body: &[Statement],
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    loop {
        // Check condition
        let cond_val = evaluate_expr(env, condition)?;
        if !is_truthy(&cond_val) {
            break Ok(None);
        }

        // Execute body
        let block_env = new_js_object_data();
        block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
        block_env.borrow_mut().is_function_scope = false;
        match evaluate_statements_with_context(&block_env, body)? {
            ControlFlow::Normal(val) => *last_value = val,
            ControlFlow::Break(None) => break Ok(None),
            ControlFlow::Break(Some(lbl)) => return Ok(Some(ControlFlow::Break(Some(lbl)))),
            ControlFlow::Continue(None) => {}
            ControlFlow::Continue(Some(lbl)) => return Ok(Some(ControlFlow::Continue(Some(lbl)))),
            ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
        }
    }
}

fn statement_do_body_while_condition(
    env: &JSObjectDataPtr,
    body: &[Statement],
    condition: &Expr,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    loop {
        // Execute body first
        let block_env = new_js_object_data();
        block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
        block_env.borrow_mut().is_function_scope = false;
        match evaluate_statements_with_context(&block_env, body)? {
            ControlFlow::Normal(val) => *last_value = val,
            ControlFlow::Break(None) => break Ok(None),
            ControlFlow::Break(Some(lbl)) => return Ok(Some(ControlFlow::Break(Some(lbl)))),
            ControlFlow::Continue(None) => {}
            ControlFlow::Continue(Some(lbl)) => return Ok(Some(ControlFlow::Continue(Some(lbl)))),
            ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
        }

        // Check condition
        let cond_val = evaluate_expr(env, condition)?;
        if !is_truthy(&cond_val) {
            break Ok(None);
        }
    }
}

fn statement_for_init_condition_increment(
    env: &JSObjectDataPtr,
    init: &Option<Box<Statement>>,
    condition: &Option<Expr>,
    increment: &Option<Box<Statement>>,
    body: &[Statement],
    last_value: &mut Value,
    label_name: Option<&str>,
) -> Result<Option<ControlFlow>, JSError> {
    let for_env = new_js_object_data();
    for_env.borrow_mut().prototype = Some(Rc::downgrade(env));
    for_env.borrow_mut().is_function_scope = false;
    // Execute initialization in for_env
    if let Some(init_stmt) = init {
        match &init_stmt.kind {
            StatementKind::Let(decls) => {
                for (name, expr_opt) in decls {
                    let val = expr_opt
                        .clone()
                        .map_or(Ok(Value::Undefined), |expr| evaluate_expr(&for_env, &expr))?;
                    env_set(&for_env, name.as_str(), val)?;
                }
            }
            StatementKind::Var(decls) => {
                for (name, expr_opt) in decls {
                    let val = expr_opt
                        .clone()
                        .map_or(Ok(Value::Undefined), |expr| evaluate_expr(&for_env, &expr))?;
                    env_set_var(&for_env, name.as_str(), val)?;
                }
            }
            StatementKind::Expr(expr) => {
                evaluate_expr(&for_env, expr)?;
            }
            _ => {
                return Err(raise_eval_error!("error"));
            } // For now, only support let and expr in init
        }
    }

    loop {
        // Check condition in for_env
        let should_continue = if let Some(cond_expr) = condition {
            let cond_val = evaluate_expr(&for_env, cond_expr)?;
            is_truthy(&cond_val)
        } else {
            true // No condition means infinite loop
        };

        if !should_continue {
            break;
        }

        // Execute body in block_env
        let block_env = new_js_object_data();
        block_env.borrow_mut().prototype = Some(Rc::downgrade(&for_env));
        block_env.borrow_mut().is_function_scope = false;
        match evaluate_statements_with_context(&block_env, body)? {
            ControlFlow::Normal(val) => *last_value = val,
            ControlFlow::Break(None) => break,
            ControlFlow::Break(Some(lbl)) => {
                if let Some(name) = label_name {
                    if lbl == name {
                        break;
                    }
                }
                return Ok(Some(ControlFlow::Break(Some(lbl))));
            }
            ControlFlow::Continue(None) => {}
            ControlFlow::Continue(Some(lbl)) => {
                if let Some(name) = label_name {
                    if lbl == name {
                        // continue loop
                    } else {
                        return Ok(Some(ControlFlow::Continue(Some(lbl))));
                    }
                } else {
                    return Ok(Some(ControlFlow::Continue(Some(lbl))));
                }
            }
            ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
        }

        // Execute increment in for_env
        if let Some(incr_stmt) = increment {
            match &incr_stmt.kind {
                StatementKind::Expr(expr) => match expr {
                    Expr::Assign(target, value) => {
                        if let Expr::Var(name, _, _) = target.as_ref() {
                            let val = evaluate_expr(&for_env, value)?;
                            env_set_recursive(&for_env, name.as_str(), val)?;
                        }
                    }
                    _ => {
                        evaluate_expr(&for_env, expr)?;
                    }
                },
                _ => {
                    return Err(raise_eval_error!("error"));
                } // For now, only support expr in increment
            }
        }
    }
    Ok(None)
}

fn perform_statement_if_then_else(
    env: &JSObjectDataPtr,
    condition: &Expr,
    then_body: &[Statement],
    else_body: &Option<Vec<Statement>>,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let cond_val = evaluate_expr(env, condition)?;
    if is_truthy(&cond_val) {
        // create new block scope
        let block_env = new_js_object_data();
        block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
        block_env.borrow_mut().is_function_scope = false;
        match evaluate_statements_with_context(&block_env, then_body)? {
            ControlFlow::Normal(val) => *last_value = val,
            cf => return Ok(Some(cf)),
        }
    } else if let Some(else_stmts) = else_body {
        let block_env = new_js_object_data();
        block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
        block_env.borrow_mut().is_function_scope = false;
        match evaluate_statements_with_context(&block_env, else_stmts)? {
            ControlFlow::Normal(val) => *last_value = val,
            cf => return Ok(Some(cf)),
        }
    }
    Ok(None)
}

// Helper: construct a JS Error instance from a constructor name and the original JSError
fn create_js_error_instance(env: &JSObjectDataPtr, ctor_name: &str, err: &JSError) -> Result<Value, JSError> {
    // Try to find the constructor in the environment
    if let Ok(Some(ctor_rc)) = obj_get_key_value(env, &ctor_name.into()) {
        if let Value::Object(ctor_obj) = &*ctor_rc.borrow() {
            let instance = new_js_object_data();
            // Link prototype
            if let Ok(Some(proto_val)) = obj_get_key_value(ctor_obj, &"prototype".into()) {
                if let Value::Object(proto_obj) = &*proto_val.borrow() {
                    instance.borrow_mut().prototype = Some(Rc::downgrade(proto_obj));
                    let _ = obj_set_key_value(&instance, &"__proto__".into(), Value::Object(proto_obj.clone()));
                }
            }
            // name/message
            let _ = obj_set_key_value(&instance, &"name".into(), Value::String(utf8_to_utf16(ctor_name)));
            let _ = obj_set_key_value(&instance, &"message".into(), Value::String(utf8_to_utf16(&err.to_string())));
            // Build stack string from last captured frames plus error string
            let mut stack_lines = Vec::new();
            // first line: ErrorName: message
            stack_lines.push(format!("{}: {}", ctor_name, err));
            let frames = take_last_stack();
            for f in frames.iter() {
                stack_lines.push(format!("    at {}", f));
            }
            let stack_combined = stack_lines.join("\n");
            let _ = obj_set_key_value(&instance, &"stack".into(), Value::String(utf8_to_utf16(&stack_combined)));
            let _ = obj_set_key_value(&instance, &"constructor".into(), Value::Object(ctor_obj.clone()));
            // Mark these properties non-enumerable, non-writable, and non-configurable per ECMAScript semantics
            let name_key = PropertyKey::String("name".to_string());
            let msg_key = PropertyKey::String("message".to_string());
            let stack_key = PropertyKey::String("stack".to_string());
            instance.borrow_mut().set_non_enumerable(name_key.clone());
            instance.borrow_mut().set_non_enumerable(msg_key.clone());
            instance.borrow_mut().set_non_enumerable(stack_key.clone());
            instance.borrow_mut().set_non_writable(name_key.clone());
            instance.borrow_mut().set_non_writable(msg_key.clone());
            instance.borrow_mut().set_non_writable(stack_key.clone());
            instance.borrow_mut().set_non_configurable(name_key.clone());
            instance.borrow_mut().set_non_configurable(msg_key.clone());
            instance.borrow_mut().set_non_configurable(stack_key.clone());
            return Ok(Value::Object(instance));
        }
    }
    // Fallback: plain Error-like object
    let error_obj = new_js_object_data();
    obj_set_key_value(&error_obj, &"name".into(), Value::String(utf8_to_utf16("Error")))?;
    obj_set_key_value(&error_obj, &"message".into(), Value::String(utf8_to_utf16(&err.to_string())))?;
    obj_set_key_value(&error_obj, &"stack".into(), Value::String(utf8_to_utf16(&err.to_string())))?;
    let name_key = PropertyKey::String("name".to_string());
    let msg_key = PropertyKey::String("message".to_string());
    let stack_key = PropertyKey::String("stack".to_string());
    error_obj.borrow_mut().set_non_enumerable(name_key.clone());
    error_obj.borrow_mut().set_non_enumerable(msg_key.clone());
    error_obj.borrow_mut().set_non_enumerable(stack_key.clone());
    error_obj.borrow_mut().set_non_writable(name_key.clone());
    error_obj.borrow_mut().set_non_writable(msg_key.clone());
    error_obj.borrow_mut().set_non_writable(stack_key.clone());
    error_obj.borrow_mut().set_non_configurable(name_key.clone());
    error_obj.borrow_mut().set_non_configurable(msg_key.clone());
    error_obj.borrow_mut().set_non_configurable(stack_key.clone());
    Ok(Value::Object(error_obj))
}

fn execute_finally(
    env: &JSObjectDataPtr,
    finally_body: &[Statement],
    previous_cf: Option<ControlFlow>,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let block_env = new_js_object_data();
    block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
    block_env.borrow_mut().is_function_scope = false;
    match evaluate_statements_with_context(&block_env, finally_body)? {
        ControlFlow::Normal(val) => {
            if let Some(cf) = previous_cf {
                Ok(Some(cf))
            } else {
                *last_value = val;
                Ok(None)
            }
        }
        other => Ok(Some(other)),
    }
}

fn create_catch_value(env: &JSObjectDataPtr, err: &JSError) -> Result<Value, JSError> {
    match &err.kind() {
        JSErrorKind::Throw { value } => {
            let cloned = value.clone();
            if let Value::Object(obj_ptr) = &cloned {
                let has_ctor = get_own_property(obj_ptr, &"constructor".into()).is_some();
                if !has_ctor {
                    if let Some(proto_ptr_rc) = obj_ptr.borrow().prototype.clone().and_then(|w| w.upgrade()) {
                        if let Some(proto_ctor_rc) = get_own_property(&proto_ptr_rc, &"constructor".into()) {
                            let ctor_val = proto_ctor_rc.borrow().clone();
                            let _ = obj_set_key_value(obj_ptr, &"constructor".into(), ctor_val);
                        }
                    }
                }
            }
            Ok(cloned)
        }
        JSErrorKind::TypeError { .. } => create_js_error_instance(env, "TypeError", err),
        JSErrorKind::RangeError { .. } => create_js_error_instance(env, "RangeError", err),
        JSErrorKind::SyntaxError { .. } => create_js_error_instance(env, "SyntaxError", err),
        JSErrorKind::ReferenceError { .. } => create_js_error_instance(env, "ReferenceError", err),
        JSErrorKind::VariableNotFound { .. } => create_js_error_instance(env, "ReferenceError", err),
        JSErrorKind::RuntimeError { .. } | JSErrorKind::EvaluationError { .. } => create_js_error_instance(env, "Error", err),
        _ => create_js_error_instance(env, "Error", err),
    }
}

fn statement_try_catch(
    env: &JSObjectDataPtr,
    try_body: &[Statement],
    catch_param: &str,
    catch_body: &[Statement],
    finally_body_opt: &Option<Vec<Statement>>,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    // Execute try block and handle catch/finally semantics
    match evaluate_statements_with_context(env, try_body) {
        Ok(ControlFlow::Normal(v)) => {
            *last_value = v;
            if let Some(finally_body) = finally_body_opt {
                execute_finally(env, finally_body, None, last_value)
            } else {
                Ok(None)
            }
        }
        Ok(cf) => {
            // For any non-normal control flow, execute finally (if present)
            // then propagate the eventual control flow (finally can override).
            if let Some(finally_body) = finally_body_opt {
                execute_finally(env, finally_body, Some(cf), last_value)
            } else {
                Ok(Some(cf))
            }
        }
        Err(err) => {
            if catch_param.is_empty() {
                if let Some(finally_body) = finally_body_opt {
                    let block_env = new_js_object_data();
                    block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                    block_env.borrow_mut().is_function_scope = false;
                    match evaluate_statements_with_context(&block_env, finally_body)? {
                        ControlFlow::Normal(_) => return Err(err),
                        other => return Ok(Some(other)),
                    }
                }
                Err(err)
            } else {
                let catch_env = new_js_object_data();
                catch_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                catch_env.borrow_mut().is_function_scope = false;

                let catch_value = create_catch_value(env, &err)?;
                env_set(&catch_env, catch_param, catch_value)?;
                match evaluate_statements_with_context(&catch_env, catch_body) {
                    Ok(ControlFlow::Normal(val)) => {
                        *last_value = val;
                        if let Some(finally_body) = finally_body_opt {
                            execute_finally(env, finally_body, None, last_value)
                        } else {
                            Ok(None)
                        }
                    }
                    Ok(cf) => {
                        if let Some(finally_body) = finally_body_opt {
                            execute_finally(env, finally_body, Some(cf), last_value)
                        } else {
                            Ok(Some(cf))
                        }
                    }
                    Err(e) => {
                        if let Some(finally_body) = finally_body_opt {
                            let block_env = new_js_object_data();
                            block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                            block_env.borrow_mut().is_function_scope = false;
                            match evaluate_statements_with_context(&block_env, finally_body) {
                                Ok(ControlFlow::Normal(_)) => Err(e),
                                Ok(other) => Ok(Some(other)),
                                Err(finally_e) => Err(finally_e),
                            }
                        } else {
                            Err(e)
                        }
                    }
                }
            }
        }
    }
}

fn perform_statement_label(
    env: &JSObjectDataPtr,
    label_name: &str,
    inner_stmt: &Statement,
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    // Labels commonly attach to loops/switches. Re-implement
    // loop/switch evaluation here with awareness of the label so
    // labeled `break/continue` control flow can be handled.
    match &inner_stmt.kind {
        StatementKind::For(init, condition, increment, body) => {
            statement_for_init_condition_increment(env, init, condition, increment, body, last_value, Some(label_name))
        }
        StatementKind::ForOf(var, iterable, body) => {
            let iterable_val = evaluate_expr(env, iterable)?;
            match iterable_val {
                Value::Object(object) => {
                    if is_array(&object) {
                        let len = get_array_length(&object).unwrap_or(0);
                        for i in 0..len {
                            let key = PropertyKey::String(i.to_string());
                            if let Some(element_rc) = obj_get_key_value(&object, &key)? {
                                let element = element_rc.borrow().clone();
                                env_set_recursive(env, var.as_str(), element)?;
                                let block_env = new_js_object_data();
                                block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                                block_env.borrow_mut().is_function_scope = false;
                                match evaluate_statements_with_context(&block_env, body)? {
                                    ControlFlow::Normal(val) => *last_value = val,
                                    ControlFlow::Break(None) => break,
                                    ControlFlow::Break(Some(lbl)) => {
                                        if lbl == *label_name {
                                            break;
                                        } else {
                                            return Ok(Some(ControlFlow::Break(Some(lbl))));
                                        }
                                    }
                                    ControlFlow::Continue(None) => {}
                                    ControlFlow::Continue(Some(lbl)) => {
                                        if lbl == *label_name { /* continue */
                                        } else {
                                            return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                        }
                                    }
                                    ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                                }
                            }
                        }
                        Ok(None)
                    } else {
                        /* fallback path uses same behavior as unlabeled */
                        // Reuse existing for-of logic by delegating (no special label handling)
                        match evaluate_statements_with_context(env, std::slice::from_ref(inner_stmt))? {
                            ControlFlow::Normal(_) => Ok(None),
                            cf => match cf {
                                ControlFlow::Normal(_) => Ok(None),
                                _ => Ok(Some(cf)),
                            },
                        }
                    }
                }
                _ => Err(raise_eval_error!("for-of loop requires an iterable")),
            }
        }
        StatementKind::ForIn(var, object, body) => {
            let object_val = evaluate_expr(env, object)?;
            match object_val {
                Value::Object(object) => {
                    let obj_borrow = object.borrow();
                    for key in obj_borrow.properties.keys() {
                        if !obj_borrow.non_enumerable.contains(key) {
                            let key_str = match key {
                                PropertyKey::String(s) => s.clone(),
                                PropertyKey::Symbol(_) => continue,
                            };
                            env_set_recursive(env, var.as_str(), Value::String(utf8_to_utf16(&key_str)))?;
                            match evaluate_statements_with_context(env, body)? {
                                ControlFlow::Normal(val) => *last_value = val,
                                ControlFlow::Break(None) => break,
                                ControlFlow::Break(Some(lbl)) => {
                                    if lbl == *label_name {
                                        /* break out of labeled loop */
                                    } else {
                                        return Ok(Some(ControlFlow::Break(Some(lbl))));
                                    }
                                }
                                ControlFlow::Continue(None) => {}
                                ControlFlow::Continue(Some(lbl)) => {
                                    if lbl == *label_name {
                                        /* continue loop */
                                    } else {
                                        return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                    }
                                }
                                ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                            }
                        }
                    }
                    Ok(None)
                }
                _ => Err(raise_eval_error!("for-in loop requires an object")),
            }
        }
        StatementKind::ForOfDestructuringObject(pattern, iterable, body) => {
            let iterable_val = evaluate_expr(env, iterable)?;
            if let Some(cf) = for_of_destructuring_object_iter(env, pattern, &iterable_val, body, last_value, Some(label_name))? {
                return Ok(Some(cf));
            }
            Ok(None)
        }
        StatementKind::ForOfDestructuringArray(pattern, iterable, body) => {
            let iterable_val = evaluate_expr(env, iterable)?;
            if let Some(cf) = for_of_destructuring_array_iter(env, pattern, &iterable_val, body, last_value, Some(label_name))? {
                return Ok(Some(cf));
            }
            Ok(None)
        }
        StatementKind::Switch(expr, cases) => eval_switch_statement(env, expr, cases, last_value, Some(label_name)),
        // If it's some other statement type, just evaluate it here. Important: a
        // Normal control flow result from the inner statement should *not*
        // be propagated out of the label — labels only affect break/continue
        // that target the label itself. Propagate non-normal control-flow
        // (break/continue/return) as before, but swallow Normal so execution
        // continues.
        other => {
            let stmt = Statement::from(other.clone());
            match evaluate_statements_with_context(env, std::slice::from_ref(&stmt))? {
                ControlFlow::Break(Some(lbl)) if lbl == *label_name => Ok(None),
                ControlFlow::Break(opt) => Ok(Some(ControlFlow::Break(opt))),
                ControlFlow::Continue(Some(lbl)) if lbl == *label_name => Ok(Some(ControlFlow::Continue(None))),
                ControlFlow::Continue(opt) => Ok(Some(ControlFlow::Continue(opt))),
                ControlFlow::Normal(_) => Ok(None),
                cf => Ok(Some(cf)),
            }
        }
    }
}

fn assign_to_target(env: &JSObjectDataPtr, target: &Expr, value: Value) -> Result<Value, JSError> {
    match target {
        Expr::Var(name, _, _) => {
            env_set_recursive(env, name.as_str(), value.clone())?;
            Ok(value)
        }
        Expr::Property(obj_expr, prop_name) => {
            set_prop_env(env, obj_expr, prop_name.as_str(), value.clone())?;
            Ok(value)
        }
        Expr::Index(obj_expr, idx_expr) => {
            let obj_val = evaluate_expr(env, obj_expr)?;
            let idx_val = evaluate_expr(env, idx_expr)?;

            if let (Value::Object(object), Value::Number(n)) = (&obj_val, &idx_val)
                && let Some(ta_val) = obj_get_key_value(object, &"__typedarray".into())?
                && let Value::TypedArray(ta) = &*ta_val.borrow()
            {
                let val_num = match &value {
                    Value::Number(num) => *num as i64,
                    Value::BigInt(h) => h
                        .to_i64()
                        .ok_or(raise_eval_error!("TypedArray assignment value must be a number"))?,
                    _ => return Err(raise_eval_error!("TypedArray assignment value must be a number")),
                };
                ta.borrow_mut()
                    .set(*n as usize, val_num)
                    .map_err(|_| raise_eval_error!("TypedArray index out of bounds"))?;
                return Ok(value);
            }

            match idx_val {
                Value::Number(n) => {
                    let key = n.to_string();
                    if let Value::Object(obj) = obj_val {
                        if key == "__proto__" {
                            if let Value::Object(proto_map) = &value {
                                obj.borrow_mut().prototype = Some(Rc::downgrade(proto_map));
                            } else {
                                obj.borrow_mut().prototype = None;
                            }
                        } else {
                            obj_set_key_value(&obj, &key.into(), value.clone())?;
                        }
                        Ok(value)
                    } else {
                        Ok(value)
                    }
                }
                Value::String(s) => {
                    let key = utf16_to_utf8(&s);
                    if let Value::Object(obj) = obj_val {
                        if key == "__proto__" {
                            if let Value::Object(proto_map) = &value {
                                obj.borrow_mut().prototype = Some(Rc::downgrade(proto_map));
                            } else {
                                obj.borrow_mut().prototype = None;
                            }
                        } else {
                            obj_set_key_value(&obj, &key.into(), value.clone())?;
                        }
                        Ok(value)
                    } else {
                        Ok(value)
                    }
                }
                Value::Symbol(sym) => {
                    if let Value::Object(obj) = obj_val {
                        let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                        obj_set_key_value(&obj, &key, value.clone())?;
                        Ok(value)
                    } else {
                        Ok(value)
                    }
                }
                _ => {
                    let key = value_to_sort_string(&idx_val);
                    if let Value::Object(obj) = obj_val {
                        if key == "__proto__" {
                            if let Value::Object(proto_map) = &value {
                                obj.borrow_mut().prototype = Some(Rc::downgrade(proto_map));
                            } else {
                                obj.borrow_mut().prototype = None;
                            }
                        } else {
                            obj_set_key_value(&obj, &key.into(), value.clone())?;
                        }
                        Ok(value)
                    } else {
                        Ok(value)
                    }
                }
            }
        }
        _ => Err(raise_eval_error!("Invalid assignment target")),
    }
}

fn perform_statement_expression(env: &JSObjectDataPtr, expr: &Expr, last_value: &mut Value) -> Result<Option<ControlFlow>, JSError> {
    match expr {
        Expr::Assign(target, value_expr) => {
            let val = evaluate_expr(env, value_expr)?;
            *last_value = assign_to_target(env, target, val)?;
        }
        Expr::LogicalAndAssign(target, value_expr) => {
            let left_val = evaluate_expr(env, target)?;
            if is_truthy(&left_val) {
                let val = evaluate_expr(env, value_expr)?;
                *last_value = assign_to_target(env, target, val)?;
            } else {
                *last_value = left_val;
            }
        }
        Expr::LogicalOrAssign(target, value_expr) => {
            let left_val = evaluate_expr(env, target)?;
            if !is_truthy(&left_val) {
                let val = evaluate_expr(env, value_expr)?;
                *last_value = assign_to_target(env, target, val)?;
            } else {
                *last_value = left_val;
            }
        }
        Expr::NullishAssign(target, value_expr) => {
            let left_val = evaluate_expr(env, target)?;
            if matches!(left_val, Value::Undefined | Value::Null) {
                let val = evaluate_expr(env, value_expr)?;
                *last_value = assign_to_target(env, target, val)?;
            } else {
                *last_value = left_val;
            }
        }
        _ => {
            *last_value = evaluate_expr(env, expr)?;
        }
    }
    Ok(None)
}

fn perform_array_destructuring(
    env: &JSObjectDataPtr,
    pattern: &[DestructuringElement],
    value: &Value,
    is_const: bool,
) -> Result<(), JSError> {
    match value {
        Value::Object(arr) if is_array(arr) => {
            let mut index = 0;
            let mut rest_index = None;
            let mut rest_var = None;

            for element in pattern {
                match element {
                    DestructuringElement::Variable(var, default_opt) => {
                        let key = PropertyKey::String(index.to_string());
                        let val = if let Some(val_rc) = obj_get_key_value(arr, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        // Apply default initializer when the value is undefined
                        let assigned_val = if matches!(val, Value::Undefined) {
                            if let Some(def_expr) = default_opt {
                                evaluate_expr(env, def_expr)?
                            } else {
                                Value::Undefined
                            }
                        } else {
                            val
                        };
                        if is_const {
                            env_set_const(env, var, assigned_val);
                        } else {
                            env_set(env, var, assigned_val)?;
                        }
                        index += 1;
                    }
                    DestructuringElement::NestedArray(nested_pattern) => {
                        let key = PropertyKey::String(index.to_string());
                        let val = if let Some(val_rc) = obj_get_key_value(arr, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        perform_array_destructuring(env, nested_pattern, &val, is_const)?;
                        index += 1;
                    }
                    DestructuringElement::NestedObject(nested_pattern) => {
                        let key = PropertyKey::String(index.to_string());
                        let val = if let Some(val_rc) = obj_get_key_value(arr, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        perform_object_destructuring(env, nested_pattern, &val, is_const)?;
                        index += 1;
                    }
                    DestructuringElement::Rest(var) => {
                        rest_index = Some(index);
                        rest_var = Some(var.clone());
                        break;
                    }
                    DestructuringElement::Empty => {
                        index += 1;
                    }
                }
            }

            // Handle rest element
            if let (Some(rest_start), Some(var)) = (rest_index, rest_var) {
                let mut rest_elements: Vec<Value> = Vec::new();
                let len = get_array_length(arr).unwrap_or(0);
                for i in rest_start..len {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(val_rc) = obj_get_key_value(arr, &key)? {
                        rest_elements.push(val_rc.borrow().clone());
                    }
                }
                let rest_obj = create_array(env)?;
                let mut rest_index = 0;
                for elem in rest_elements {
                    obj_set_key_value(&rest_obj, &rest_index.to_string().into(), elem)?;
                    rest_index += 1;
                }
                set_array_length(&rest_obj, rest_index)?;
                let rest_value = Value::Object(rest_obj);
                if is_const {
                    env_set_const(env, &var, rest_value);
                } else {
                    env_set(env, &var, rest_value)?;
                }
            }
        }
        _ => {
            return Err(raise_eval_error!("Cannot destructure non-array value"));
        }
    }
    Ok(())
}

fn perform_object_destructuring(
    env: &JSObjectDataPtr,
    pattern: &[ObjectDestructuringElement],
    value: &Value,
    is_const: bool,
) -> Result<(), JSError> {
    match value {
        Value::Object(obj) => {
            for element in pattern {
                match element {
                    ObjectDestructuringElement::Property { key, value: dest } => {
                        let key = PropertyKey::String(key.clone());
                        let prop_val = if let Some(val_rc) = obj_get_key_value(obj, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        match dest {
                            DestructuringElement::Variable(var, default_opt) => {
                                if is_const {
                                    // Use default initializer when property value is undefined
                                    let final_val = if matches!(prop_val, Value::Undefined) {
                                        if let Some(def_expr) = default_opt {
                                            evaluate_expr(env, def_expr)?
                                        } else {
                                            Value::Undefined
                                        }
                                    } else {
                                        prop_val
                                    };
                                    env_set_const(env, var, final_val);
                                } else {
                                    let final_val = if matches!(prop_val, Value::Undefined) {
                                        if let Some(def_expr) = default_opt {
                                            evaluate_expr(env, def_expr)?
                                        } else {
                                            Value::Undefined
                                        }
                                    } else {
                                        prop_val
                                    };
                                    env_set(env, var, final_val)?;
                                }
                            }
                            DestructuringElement::NestedArray(nested_pattern) => {
                                perform_array_destructuring(env, nested_pattern, &prop_val, is_const)?;
                            }
                            DestructuringElement::NestedObject(nested_pattern) => {
                                perform_object_destructuring(env, nested_pattern, &prop_val, is_const)?;
                            }
                            _ => {
                                // Rest in property value not supported in object destructuring
                                return Err(raise_eval_error!("Invalid destructuring pattern"));
                            }
                        }
                    }
                    ObjectDestructuringElement::Rest(var) => {
                        // Collect remaining properties
                        let rest_obj = new_js_object_data();
                        let mut assigned_keys = std::collections::HashSet::new();

                        // Collect keys that were already assigned
                        for element in pattern {
                            if let ObjectDestructuringElement::Property { key, .. } = element {
                                assigned_keys.insert(key.clone());
                            }
                        }

                        // Add remaining properties to rest object
                        for (key, val_rc) in obj.borrow().properties.iter() {
                            if let PropertyKey::String(k) = key
                                && !assigned_keys.contains(k)
                            {
                                rest_obj.borrow_mut().insert(key.clone(), val_rc.clone());
                            }
                        }

                        let rest_value = Value::Object(rest_obj);
                        if is_const {
                            env_set_const(env, var, rest_value);
                        } else {
                            env_set(env, var, rest_value)?;
                        }
                    }
                }
            }
        }
        _ => {
            return Err(raise_eval_error!("Cannot destructure non-object value"));
        }
    }
    Ok(())
}

fn perform_array_destructuring_var(env: &JSObjectDataPtr, pattern: &[DestructuringElement], value: &Value) -> Result<(), JSError> {
    match value {
        Value::Object(arr) if is_array(arr) => {
            let mut index = 0;
            let mut rest_index = None;
            let mut rest_var = None;

            for element in pattern {
                match element {
                    DestructuringElement::Variable(var, default_opt) => {
                        let key = PropertyKey::String(index.to_string());
                        let val = if let Some(val_rc) = obj_get_key_value(arr, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        // Apply default initializer when the value is undefined
                        let assigned_val = if matches!(val, Value::Undefined) {
                            if let Some(def_expr) = default_opt {
                                evaluate_expr(env, def_expr)?
                            } else {
                                Value::Undefined
                            }
                        } else {
                            val
                        };
                        env_set_var(env, var, assigned_val)?;
                        index += 1;
                    }
                    DestructuringElement::NestedArray(nested_pattern) => {
                        let key = PropertyKey::String(index.to_string());
                        let val = if let Some(val_rc) = obj_get_key_value(arr, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        perform_array_destructuring_var(env, nested_pattern, &val)?;
                        index += 1;
                    }
                    DestructuringElement::NestedObject(nested_pattern) => {
                        let key = PropertyKey::String(index.to_string());
                        let val = if let Some(val_rc) = obj_get_key_value(arr, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        perform_object_destructuring_var(env, nested_pattern, &val)?;
                        index += 1;
                    }
                    DestructuringElement::Rest(var) => {
                        rest_index = Some(index);
                        rest_var = Some(var.clone());
                        break;
                    }
                    DestructuringElement::Empty => {
                        index += 1;
                    }
                }
            }

            // Handle rest element
            if let (Some(rest_start), Some(var)) = (rest_index, rest_var) {
                let mut rest_elements: Vec<Value> = Vec::new();
                let len = get_array_length(arr).unwrap_or(0);
                for i in rest_start..len {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(val_rc) = obj_get_key_value(arr, &key)? {
                        rest_elements.push(val_rc.borrow().clone());
                    }
                }
                let rest_obj = create_array(env)?;
                let mut rest_index = 0;
                for elem in rest_elements {
                    obj_set_key_value(&rest_obj, &rest_index.to_string().into(), elem)?;
                    rest_index += 1;
                }
                set_array_length(&rest_obj, rest_index)?;
                let rest_value = Value::Object(rest_obj);
                env_set_var(env, &var, rest_value)?;
            }
        }
        _ => {
            return Err(raise_eval_error!("Cannot destructure non-array value"));
        }
    }
    Ok(())
}

fn perform_object_destructuring_var(env: &JSObjectDataPtr, pattern: &[ObjectDestructuringElement], value: &Value) -> Result<(), JSError> {
    match value {
        Value::Object(obj) => {
            for element in pattern {
                match element {
                    ObjectDestructuringElement::Property { key, value: dest } => {
                        let key = PropertyKey::String(key.clone());
                        let prop_val = if let Some(val_rc) = obj_get_key_value(obj, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        match dest {
                            DestructuringElement::Variable(var, default_opt) => {
                                let final_val = if matches!(prop_val, Value::Undefined) {
                                    if let Some(def_expr) = default_opt {
                                        evaluate_expr(env, def_expr)?
                                    } else {
                                        Value::Undefined
                                    }
                                } else {
                                    prop_val
                                };
                                env_set_var(env, var, final_val)?;
                            }
                            DestructuringElement::NestedArray(nested_pattern) => {
                                perform_array_destructuring_var(env, nested_pattern, &prop_val)?;
                            }
                            DestructuringElement::NestedObject(nested_pattern) => {
                                perform_object_destructuring_var(env, nested_pattern, &prop_val)?;
                            }
                            _ => {
                                return Err(raise_eval_error!("Invalid destructuring pattern"));
                            }
                        }
                    }
                    ObjectDestructuringElement::Rest(var) => {
                        let rest_obj = new_js_object_data();
                        let mut assigned_keys = std::collections::HashSet::new();
                        for element in pattern {
                            if let ObjectDestructuringElement::Property { key, .. } = element {
                                assigned_keys.insert(key.clone());
                            }
                        }
                        for (key, val_rc) in obj.borrow().properties.iter() {
                            if let PropertyKey::String(k) = key
                                && !assigned_keys.contains(k)
                            {
                                rest_obj.borrow_mut().insert(key.clone(), val_rc.clone());
                            }
                        }
                        let rest_value = Value::Object(rest_obj);
                        env_set_var(env, var, rest_value)?;
                    }
                }
            }
        }
        _ => {
            return Err(raise_eval_error!("Cannot destructure non-object value"));
        }
    }
    Ok(())
}

/// Helper: iterate over an iterable value (array-like object) and perform
/// object-pattern destructuring per element, executing `body` each iteration.
/// `label_name` controls how labeled break/continue are handled; pass None for
/// unlabeled loops.
fn for_of_destructuring_object_iter(
    env: &JSObjectDataPtr,
    pattern: &[ObjectDestructuringElement],
    iterable_val: &Value,
    body: &[Statement],
    last_value: &mut Value,
    label_name: Option<&str>,
) -> Result<Option<ControlFlow>, JSError> {
    match iterable_val {
        Value::Object(object) => {
            if is_array(object) {
                let len = get_array_length(object).unwrap_or(0);
                for i in 0..len {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(element_rc) = obj_get_key_value(object, &key)? {
                        let element = element_rc.borrow().clone();
                        // perform destructuring into env (var semantics)
                        perform_object_destructuring(env, pattern, &element, false)?;
                        let block_env = new_js_object_data();
                        block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                        block_env.borrow_mut().is_function_scope = false;
                        match evaluate_statements_with_context(&block_env, body)? {
                            ControlFlow::Normal(val) => *last_value = val,
                            ControlFlow::Break(None) => break,
                            ControlFlow::Break(Some(lbl)) => {
                                if let Some(ln) = label_name {
                                    if lbl == ln {
                                        break;
                                    } else {
                                        return Ok(Some(ControlFlow::Break(Some(lbl))));
                                    }
                                } else {
                                    return Ok(Some(ControlFlow::Break(Some(lbl))));
                                }
                            }
                            ControlFlow::Continue(None) => {}
                            ControlFlow::Continue(Some(lbl)) => {
                                if let Some(ln) = label_name {
                                    if lbl == ln {
                                        // continue outer loop
                                        continue;
                                    } else {
                                        return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                    }
                                } else {
                                    return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                }
                            }
                            ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                        }
                    }
                }
                Ok(None)
            } else {
                Err(raise_eval_error!("for-of loop requires an iterable"))
            }
        }
        _ => Err(raise_eval_error!("for-of loop requires an iterable")),
    }
}

/// Helper: iterate over an iterable value (array-like object) and perform
/// array-pattern destructuring per element, executing `body` each iteration.
fn for_of_destructuring_array_iter(
    env: &JSObjectDataPtr,
    pattern: &[DestructuringElement],
    iterable_val: &Value,
    body: &[Statement],
    last_value: &mut Value,
    label_name: Option<&str>,
) -> Result<Option<ControlFlow>, JSError> {
    match iterable_val {
        Value::Object(object) => {
            if is_array(object) {
                let len = get_array_length(object).unwrap_or(0);
                for i in 0..len {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(element_rc) = obj_get_key_value(object, &key)? {
                        let element = element_rc.borrow().clone();
                        // perform array destructuring into env (var semantics)
                        perform_array_destructuring(env, pattern, &element, false)?;
                        let block_env = new_js_object_data();
                        block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                        block_env.borrow_mut().is_function_scope = false;
                        match evaluate_statements_with_context(&block_env, body)? {
                            ControlFlow::Normal(val) => *last_value = val,
                            ControlFlow::Break(None) => break,
                            ControlFlow::Break(Some(lbl)) => {
                                if let Some(ln) = label_name {
                                    if lbl == ln {
                                        break;
                                    } else {
                                        return Ok(Some(ControlFlow::Break(Some(lbl))));
                                    }
                                } else {
                                    return Ok(Some(ControlFlow::Break(Some(lbl))));
                                }
                            }
                            ControlFlow::Continue(None) => {}
                            ControlFlow::Continue(Some(lbl)) => {
                                if let Some(ln) = label_name {
                                    if lbl == ln {
                                        continue;
                                    } else {
                                        return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                    }
                                } else {
                                    return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                }
                            }
                            ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                        }
                    }
                }
                Ok(None)
            } else {
                // Try iterator protocol for non-array objects
                if let Some(sym_rc) = get_well_known_symbol_rc("iterator") {
                    let iterator_key = PropertyKey::Symbol(Rc::new(RefCell::new(sym_rc.borrow().clone())));
                    if let Some(iterator_val) = obj_get_key_value(object, &iterator_key)? {
                        let iterator_factory = iterator_val.borrow().clone();
                        // Call Symbol.iterator to get the iterator object. Accept
                        // either a direct closure or a function-object wrapper.
                        let iterator = if let Some((params, body, closure_env)) = extract_closure_from_value(&iterator_factory) {
                            // Call the closure with `this` bound to the original object
                            let call_env = prepare_function_call_env(
                                Some(&closure_env),
                                Some(Value::Object(object.clone())),
                                Some(&params),
                                &[],
                                None,
                                None,
                            )?;
                            evaluate_statements(&call_env, &body)?
                        } else {
                            return Err(raise_eval_error!("Symbol.iterator is not a function"));
                        };

                        if let Value::Object(iterator_obj) = iterator {
                            if let Some(next_val) = obj_get_key_value(&iterator_obj, &"next".into())? {
                                let next_func = next_val.borrow().clone();
                                loop {
                                    // Call next() — accept direct closures or function-objects
                                    if let Some((nparams, nbody, nclosure_env)) = extract_closure_from_value(&next_func) {
                                        let call_env = prepare_function_call_env(
                                            Some(&nclosure_env),
                                            Some(Value::Object(iterator_obj.clone())),
                                            Some(&nparams),
                                            &[],
                                            None,
                                            None,
                                        )?;
                                        let next_result = evaluate_statements(&call_env, &nbody)?;

                                        if let Value::Object(result_obj) = next_result {
                                            // Check if done
                                            if let Some(done_val) = obj_get_key_value(&result_obj, &"done".into())?
                                                && let Value::Boolean(true) = *done_val.borrow()
                                            {
                                                break; // Iteration complete
                                            }

                                            // Get value
                                            if let Some(value_val) = obj_get_key_value(&result_obj, &"value".into())? {
                                                let element = value_val.borrow().clone();
                                                // perform array destructuring into env (var semantics)
                                                perform_array_destructuring(env, pattern, &element, false)?;
                                                let block_env = new_js_object_data();
                                                block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                                                block_env.borrow_mut().is_function_scope = false;
                                                match evaluate_statements_with_context(&block_env, body)? {
                                                    ControlFlow::Normal(val) => *last_value = val,
                                                    ControlFlow::Break(None) => break,
                                                    ControlFlow::Break(Some(lbl)) => {
                                                        if let Some(ln) = label_name {
                                                            if lbl == ln {
                                                                break;
                                                            } else {
                                                                return Ok(Some(ControlFlow::Break(Some(lbl))));
                                                            }
                                                        } else {
                                                            return Ok(Some(ControlFlow::Break(Some(lbl))));
                                                        }
                                                    }
                                                    ControlFlow::Continue(None) => {}
                                                    ControlFlow::Continue(Some(lbl)) => {
                                                        if let Some(ln) = label_name {
                                                            if lbl == ln {
                                                                continue;
                                                            } else {
                                                                return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                                            }
                                                        } else {
                                                            return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                                        }
                                                    }
                                                    ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                                                }
                                                continue;
                                            }
                                        } else {
                                            return Err(raise_eval_error!("Iterator next() did not return an object"));
                                        }
                                    } else if let Value::Function(func_name) = &next_func {
                                        // Built-in next handling: call the registered global function
                                        // Bind `this` to the iterator object so native helper can access iterator state
                                        let call_env = prepare_function_call_env(
                                            Some(env),
                                            Some(Value::Object(iterator_obj.clone())),
                                            None,
                                            &[],
                                            None,
                                            None,
                                        )?;
                                        let next_result = crate::js_function::handle_global_function(func_name, &[], &call_env)?;
                                        // next_result should be an object with { value, done }
                                        if let Value::Object(result_obj) = next_result {
                                            // Check if done
                                            if let Some(done_val) = obj_get_key_value(&result_obj, &"done".into())?
                                                && let Value::Boolean(true) = *done_val.borrow()
                                            {
                                                break; // Iteration complete
                                            }

                                            // Get value
                                            if let Some(value_val) = obj_get_key_value(&result_obj, &"value".into())? {
                                                let element = value_val.borrow().clone();
                                                // perform array destructuring into env (var semantics)
                                                perform_array_destructuring(env, pattern, &element, false)?;
                                                let block_env = new_js_object_data();
                                                block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                                                block_env.borrow_mut().is_function_scope = false;
                                                match evaluate_statements_with_context(&block_env, body)? {
                                                    ControlFlow::Normal(val) => *last_value = val,
                                                    ControlFlow::Break(None) => break,
                                                    ControlFlow::Break(Some(lbl)) => {
                                                        if let Some(ln) = label_name {
                                                            if lbl == ln {
                                                                break;
                                                            } else {
                                                                return Ok(Some(ControlFlow::Break(Some(lbl))));
                                                            }
                                                        } else {
                                                            return Ok(Some(ControlFlow::Break(Some(lbl))));
                                                        }
                                                    }
                                                    ControlFlow::Continue(None) => {}
                                                    ControlFlow::Continue(Some(lbl)) => {
                                                        if let Some(ln) = label_name {
                                                            if lbl == ln {
                                                                continue;
                                                            } else {
                                                                return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                                            }
                                                        } else {
                                                            return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                                        }
                                                    }
                                                    ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                                                }
                                                continue;
                                            }
                                        } else {
                                            return Err(raise_eval_error!("Iterator next() did not return an object"));
                                        }
                                    } else {
                                        return Err(raise_eval_error!("Iterator next is not a function"));
                                    }
                                }
                                Ok(None)
                            } else {
                                Err(raise_eval_error!("Iterator does not have next method"))
                            }
                        } else {
                            Err(raise_eval_error!("Symbol.iterator did not return an iterator object"))
                        }
                    } else {
                        Err(raise_eval_error!("Object does not have Symbol.iterator"))
                    }
                } else {
                    Err(raise_eval_error!("for-of loop requires an iterable"))
                }
            }
        }
        _ => Err(raise_eval_error!("for-of loop requires an iterable")),
    }
}

/// Helper: iterate over an iterable value (array-like object) and assign each
/// element to `varname` before executing `body`. Handles array fast-path,
/// iterator protocol and string iteration.
fn statement_for_of_var_iter(
    env: &JSObjectDataPtr,
    var: &str,
    iterable: &Expr,
    body: &[Statement],
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let iterable_val = evaluate_expr(env, iterable)?;
    match iterable_val {
        Value::Object(object) => {
            // Attempt iterator protocol via Symbol.iterator
            // Look up well-known Symbol.iterator and call it on the object to obtain an iterator
            if let Some(iter_sym_rc) = get_well_known_symbol_rc("iterator") {
                let key = PropertyKey::Symbol(iter_sym_rc.clone());
                if let Some(method_rc) = obj_get_key_value(&object, &key)? {
                    // method can be a direct closure, an object-wrapped closure
                    // (function-object), a native function, or an iterator object.
                    let iterator_val = {
                        let method_val = &*method_rc.borrow();
                        if let Some((params, body, captured_env)) = extract_closure_from_value(method_val) {
                            // Call closure with 'this' bound to the object
                            let func_env = prepare_function_call_env(
                                Some(&captured_env),
                                Some(Value::Object(object.clone())),
                                Some(&params),
                                &[],
                                Some(&build_frame_name(env, "[Symbol.iterator]")),
                                Some(env),
                            )?;
                            evaluate_statements(&func_env, &body)?
                        } else if let Value::Function(func_name) = method_val {
                            // Call built-in function (no arguments). Bind `this` to the receiver object.
                            let call_env =
                                prepare_function_call_env(Some(env), Some(Value::Object(object.clone())), None, &[], None, None)?;
                            crate::js_function::handle_global_function(func_name, &[], &call_env)?
                        } else if let Value::Object(iter_obj) = method_val {
                            Value::Object(iter_obj.clone())
                        } else {
                            return Err(raise_eval_error!("iterator property is not callable"));
                        }
                    };

                    // Now we have iterator_val, expected to be an object with next() method
                    if let Value::Object(iter_obj) = iterator_val {
                        loop {
                            // call iter_obj.next()
                            if let Some(next_rc) = obj_get_key_value(&iter_obj, &"next".into())? {
                                let next_val = {
                                    let nv = &*next_rc.borrow();
                                    if let Some((nparams, nbody, ncaptured_env)) = extract_closure_from_value(nv) {
                                        let func_env = prepare_function_call_env(
                                            Some(&ncaptured_env),
                                            Some(Value::Object(iter_obj.clone())),
                                            Some(&nparams),
                                            &[],
                                            Some(&build_frame_name(env, "iterator.next")),
                                            Some(env),
                                        )?;
                                        evaluate_statements(&func_env, &nbody)?
                                    } else if let Value::Function(func_name) = nv {
                                        crate::js_function::handle_global_function(func_name, &[], env)?
                                    } else {
                                        return Err(raise_eval_error!("next is not callable"));
                                    }
                                };

                                // next_val should be an object with { value, done }
                                if let Value::Object(res_obj) = next_val {
                                    // Check done
                                    let done_val = obj_get_key_value(&res_obj, &"done".into())?;
                                    let done = match done_val {
                                        Some(d) => is_truthy(&d.borrow().clone()),
                                        None => false,
                                    };
                                    if done {
                                        break;
                                    }

                                    // Extract value
                                    let value_val = obj_get_key_value(&res_obj, &"value".into())?;
                                    let element = match value_val {
                                        Some(v) => v.borrow().clone(),
                                        None => Value::Undefined,
                                    };

                                    env_set_recursive(env, var, element)?;
                                    let block_env = new_js_object_data();
                                    block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                                    block_env.borrow_mut().is_function_scope = false;
                                    match evaluate_statements_with_context(&block_env, body)? {
                                        ControlFlow::Normal(val) => *last_value = val,
                                        ControlFlow::Break(None) => break,
                                        ControlFlow::Break(Some(lbl)) => {
                                            return Ok(Some(ControlFlow::Break(Some(lbl))));
                                        }
                                        ControlFlow::Continue(None) => {}
                                        ControlFlow::Continue(Some(lbl)) => {
                                            return Ok(Some(ControlFlow::Continue(Some(lbl))));
                                        }
                                        ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                                    }
                                } else {
                                    return Err(raise_eval_error!("iterator.next() must return an object"));
                                }
                            } else {
                                return Err(raise_eval_error!("iterator object missing next()"));
                            }
                        }
                        Ok(None)
                    } else {
                        Err(raise_eval_error!("iterator method did not return an object"))
                    }
                } else {
                    Err(raise_eval_error!("for-of loop requires an iterable"))
                }
            } else {
                Err(raise_eval_error!("for-of loop requires an iterable"))
            }
        }
        Value::String(s) => {
            // Iterate over Unicode code points (surrogate-aware)
            let mut i = 0usize;
            while let Some(first) = utf16_char_at(&s, i) {
                // Determine chunk: either a surrogate pair (2 code units) or single code unit
                let chunk: Vec<u16> = if (0xD800..=0xDBFF).contains(&first)
                    && let Some(second) = utf16_char_at(&s, i + 1)
                    && (0xDC00..=0xDFFF).contains(&second)
                {
                    utf16_slice(&s, i, i + 2)
                } else {
                    vec![first]
                };

                env_set_recursive(env, var, Value::String(chunk.clone()))?;
                let block_env = new_js_object_data();
                block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                block_env.borrow_mut().is_function_scope = false;
                match evaluate_statements_with_context(&block_env, body)? {
                    ControlFlow::Normal(val) => *last_value = val,
                    ControlFlow::Break(None) => break,
                    ControlFlow::Break(Some(lbl)) => return Ok(Some(ControlFlow::Break(Some(lbl)))),
                    ControlFlow::Continue(None) => {}
                    ControlFlow::Continue(Some(lbl)) => return Ok(Some(ControlFlow::Continue(Some(lbl)))),
                    ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                }
                i += chunk.len();
            }
            Ok(None)
        }
        _ => Err(raise_eval_error!("for-of loop requires an iterable")),
    }
}

fn statement_for_in_var_object(
    env: &JSObjectDataPtr,
    var: &str,
    object: &Expr,
    body: &[Statement],
    last_value: &mut Value,
) -> Result<Option<ControlFlow>, JSError> {
    let object_val = evaluate_expr(env, object)?;
    match object_val {
        Value::Object(object) => {
            // Iterate over all enumerable properties
            let obj_borrow = object.borrow();
            for key in obj_borrow.properties.keys() {
                if !obj_borrow.non_enumerable.contains(key) {
                    let key_str = match key {
                        PropertyKey::String(s) => s.clone(),
                        PropertyKey::Symbol(_) => continue, // Skip symbols for now
                    };
                    env_set_recursive(env, var, Value::String(utf8_to_utf16(&key_str)))?;
                    let block_env = new_js_object_data();
                    block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                    block_env.borrow_mut().is_function_scope = false;
                    match evaluate_statements_with_context(&block_env, body)? {
                        ControlFlow::Normal(val) => *last_value = val,
                        ControlFlow::Break(None) => break,
                        ControlFlow::Break(Some(lbl)) => return Ok(Some(ControlFlow::Break(Some(lbl)))),
                        ControlFlow::Continue(None) => {}
                        ControlFlow::Continue(Some(lbl)) => return Ok(Some(ControlFlow::Continue(Some(lbl)))),
                        ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                    }
                }
            }
            Ok(None)
        }
        _ => Err(raise_eval_error!("for-in loop requires an object")),
    }
}

/// Evaluate a `switch` statement's cases. This is shared between labeled and
/// unlabeled switch handling. `label_name` controls how labeled break values
/// are handled (pass `Some(label)` for labeled switches, or `None` for the
/// unlabeled variant).
fn eval_switch_statement(
    env: &JSObjectDataPtr,
    expr: &Expr,
    cases: &[SwitchCase],
    last_value: &mut Value,
    label_name: Option<&str>,
) -> Result<Option<ControlFlow>, JSError> {
    let switch_val = evaluate_expr(env, expr)?;
    let mut found_match = false;
    let mut executed_default = false;

    for case in cases {
        match case {
            SwitchCase::Case(case_expr, case_stmts) => {
                if !found_match {
                    let case_val = evaluate_expr(env, case_expr)?;
                    if values_equal(&switch_val, &case_val) {
                        found_match = true;
                    }
                }
                if found_match {
                    let block_env = new_js_object_data();
                    block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                    block_env.borrow_mut().is_function_scope = false;
                    match evaluate_statements_with_context(&block_env, case_stmts)? {
                        ControlFlow::Normal(val) => *last_value = val,
                        ControlFlow::Break(None) => break,
                        ControlFlow::Break(Some(lbl)) => match label_name {
                            None => return Ok(Some(ControlFlow::Break(Some(lbl)))),
                            Some(name) => {
                                if lbl == name {
                                    break;
                                } else {
                                    return Ok(Some(ControlFlow::Break(Some(lbl))));
                                }
                            }
                        },
                        cf => return Ok(Some(cf)),
                    }
                }
            }
            SwitchCase::Default(default_stmts) => {
                if !found_match && !executed_default {
                    executed_default = true;
                    let block_env = new_js_object_data();
                    block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                    block_env.borrow_mut().is_function_scope = false;
                    match evaluate_statements_with_context(&block_env, default_stmts)? {
                        ControlFlow::Normal(val) => *last_value = val,
                        ControlFlow::Break(None) => break,
                        ControlFlow::Break(Some(lbl)) => match label_name {
                            None => return Ok(Some(ControlFlow::Break(Some(lbl)))),
                            Some(name) => {
                                if lbl == name {
                                    break;
                                } else {
                                    return Ok(Some(ControlFlow::Break(Some(lbl))));
                                }
                            }
                        },
                        cf => return Ok(Some(cf)),
                    }
                } else if found_match {
                    let block_env = new_js_object_data();
                    block_env.borrow_mut().prototype = Some(Rc::downgrade(env));
                    block_env.borrow_mut().is_function_scope = false;
                    match evaluate_statements_with_context(&block_env, default_stmts)? {
                        ControlFlow::Normal(val) => *last_value = val,
                        ControlFlow::Break(None) => break,
                        ControlFlow::Break(Some(lbl)) => match label_name {
                            None => return Ok(Some(ControlFlow::Break(Some(lbl)))),
                            Some(name) => {
                                if lbl == name {
                                    break;
                                } else {
                                    return Ok(Some(ControlFlow::Break(Some(lbl))));
                                }
                            }
                        },
                        cf => return Ok(Some(cf)),
                    }
                }
            }
        }
    }
    Ok(None)
}

pub fn evaluate_expr(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    match expr {
        Expr::Number(n) => evaluate_number(*n),
        Expr::BigInt(s) => Ok(Value::BigInt(parse_bigint_string(s)?)),
        Expr::StringLit(s) => evaluate_string_lit(s),
        Expr::Boolean(b) => evaluate_boolean(*b),
        Expr::Var(name, line, column) => evaluate_var(env, name, *line, *column),
        Expr::Assign(target, value) => evaluate_assign(env, target, value),
        Expr::LogicalAndAssign(target, value) => evaluate_logical_and_assign(env, target, value),
        Expr::LogicalOrAssign(target, value) => evaluate_logical_or_assign(env, target, value),
        Expr::NullishAssign(target, value) => evaluate_nullish_assign(env, target, value),
        Expr::AddAssign(target, value) => evaluate_add_assign(env, target, value),
        Expr::SubAssign(target, value) => evaluate_sub_assign(env, target, value),
        Expr::MulAssign(target, value) => evaluate_mul_assign(env, target, value),
        Expr::PowAssign(target, value) => evaluate_pow_assign(env, target, value),
        Expr::DivAssign(target, value) => evaluate_div_assign(env, target, value),
        Expr::ModAssign(target, value) => evaluate_mod_assign(env, target, value),
        Expr::BitXorAssign(target, value) => evaluate_bitxor_assign(env, target, value),
        Expr::BitAndAssign(target, value) => evaluate_bitand_assign(env, target, value),
        Expr::BitOrAssign(target, value) => evaluate_bitor_assign(env, target, value),
        Expr::LeftShiftAssign(target, value) => evaluate_left_shift_assign(env, target, value),
        Expr::RightShiftAssign(target, value) => evaluate_right_shift_assign(env, target, value),
        Expr::UnsignedRightShiftAssign(target, value) => evaluate_unsigned_right_shift_assign(env, target, value),
        Expr::Increment(expr) => evaluate_increment(env, expr),
        Expr::Decrement(expr) => evaluate_decrement(env, expr),
        Expr::PostIncrement(expr) => evaluate_post_increment(env, expr),
        Expr::PostDecrement(expr) => evaluate_post_decrement(env, expr),
        Expr::UnaryNeg(expr) => evaluate_unary_neg(env, expr),
        Expr::UnaryPlus(expr) => evaluate_unary_plus(env, expr),
        Expr::BitNot(expr) => evaluate_bit_not(env, expr),
        Expr::LogicalNot(expr) => {
            let v = evaluate_expr(env, expr)?;
            Ok(Value::Boolean(!is_truthy(&v)))
        }
        Expr::TypeOf(expr) => evaluate_typeof(env, expr),
        Expr::Delete(expr) => evaluate_delete(env, expr),
        Expr::Void(expr) => evaluate_void(env, expr),
        Expr::Binary(left, op, right) => evaluate_binary(env, left, op, right),
        Expr::LogicalAnd(left, right) => {
            let l = evaluate_expr(env, left)?;
            if is_truthy(&l) { evaluate_expr(env, right) } else { Ok(l) }
        }
        Expr::LogicalOr(left, right) => {
            let l = evaluate_expr(env, left)?;
            if is_truthy(&l) { Ok(l) } else { evaluate_expr(env, right) }
        }
        Expr::Comma(left, right) => {
            evaluate_expr(env, left)?;
            evaluate_expr(env, right)
        }
        Expr::TaggedTemplate(tag, strings, exprs) => evaluate_tagged_template(env, tag, strings, exprs),
        Expr::Index(obj, idx) => evaluate_index(env, obj, idx),
        Expr::Property(obj, prop) => evaluate_property(env, obj, prop),
        Expr::Class(class_def) => create_class_object(&class_def.name, &class_def.extends, &class_def.members, env, false),
        Expr::Call(func_expr, args) => match evaluate_call(env, func_expr, args) {
            Ok(v) => Ok(v),
            Err(e) => {
                log::warn!("evaluate_expr: evaluate_call error for func_expr={func_expr:?} args={args:?} error={e}");
                Err(e)
            }
        },
        Expr::Function(name, params, body) => evaluate_function_expression(env, name.clone(), params, body),
        Expr::GeneratorFunction(name, params, body) => {
            // Create a callable function object wrapper for generator expressions
            let func_obj = new_js_object_data();
            let prototype_obj = new_js_object_data();
            // Link new function prototype to Object.prototype so instances inherit Object.prototype methods
            crate::core::set_internal_prototype_from_constructor(&prototype_obj, env, "Object")?;
            let generator_val = Value::GeneratorFunction(name.clone(), Rc::new(ClosureData::new(params, body, env, None)));
            obj_set_key_value(&func_obj, &"__closure__".into(), generator_val)?;
            // If this is a named generator expression, expose the `name` property
            if let Some(n) = name.clone() {
                obj_set_key_value(&func_obj, &"name".into(), Value::String(utf8_to_utf16(&n)))?;
            }
            obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
            obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
            Ok(Value::Object(func_obj))
        }
        Expr::ArrowFunction(params, body) => {
            // Arrow functions use lexical `this` from the surrounding environment
            let mut closure_data = ClosureData::new(params, body, env, None);
            closure_data.bound_this = Some(evaluate_this(env)?);
            Ok(Value::Closure(Rc::new(closure_data)))
        }
        Expr::AsyncArrowFunction(params, body) => {
            let mut closure_data = ClosureData::new(params, body, env, None);
            closure_data.bound_this = Some(evaluate_this(env)?);
            Ok(Value::AsyncClosure(Rc::new(closure_data)))
        }

        Expr::Object(properties) => evaluate_object(env, properties),
        Expr::Array(elements) => evaluate_array(env, elements),
        Expr::Getter(func_expr) => evaluate_expr(env, func_expr),
        Expr::Setter(func_expr) => evaluate_expr(env, func_expr),
        Expr::Spread(_expr) => Err(raise_eval_error!(
            "Spread operator must be used in array, object, or function call context"
        )),
        Expr::OptionalProperty(obj, prop) => evaluate_optional_property(env, obj, prop),
        Expr::OptionalCall(func_expr, args) => evaluate_optional_call(env, func_expr, args),
        Expr::OptionalIndex(obj, idx) => evaluate_optional_index(env, obj, idx),
        Expr::This => evaluate_this(env),
        Expr::New(constructor, args) => {
            log::trace!("DBG Expr::New - constructor_expr={:?} args.len={}", constructor, args.len());
            evaluate_new(env, constructor, args)
        }
        Expr::Super => evaluate_super(env),
        Expr::SuperCall(args) => evaluate_super_call(env, args),
        Expr::SuperProperty(prop) => evaluate_super_property(env, prop),
        Expr::SuperMethod(method, args) => evaluate_super_method(env, method, args),
        Expr::ArrayDestructuring(pattern) => evaluate_array_destructuring(env, pattern),
        Expr::ObjectDestructuring(pattern) => evaluate_object_destructuring(env, pattern),
        Expr::AsyncFunction(name, params, body) => {
            // Create a callable function object wrapper for async function expressions
            let func_obj = new_js_object_data();
            let prototype_obj = new_js_object_data();
            // Link new function prototype to Object.prototype so instances inherit Object.prototype methods
            crate::core::set_internal_prototype_from_constructor(&prototype_obj, env, "Object")?;
            let closure_val = Value::AsyncClosure(Rc::new(ClosureData::new(params, body, env, None)));
            obj_set_key_value(&func_obj, &"__closure__".into(), closure_val)?;
            // If this is a named async function expression, expose the `name` property
            if let Some(n) = name.clone() {
                obj_set_key_value(&func_obj, &"name".into(), Value::String(utf8_to_utf16(&n)))?;
            }
            obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
            obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
            Ok(Value::Object(func_obj))
        }
        Expr::Await(expr) => evaluate_await_expression(env, expr),
        Expr::Yield(_expr) => {
            // Yield expressions are only valid in generator functions
            Err(raise_eval_error!("yield expression is only valid in generator functions"))
        }
        Expr::YieldStar(_expr) => {
            // Yield* expressions are only valid in generator functions
            Err(raise_eval_error!("yield* expression is only valid in generator functions"))
        }
        Expr::Value(value) => Ok(value.clone()),
        Expr::Regex(pattern, flags) => {
            // Build temporary Expr list to reuse the existing RegExp constructor
            // helper which expects one or two expressions for pattern and flags.
            let p = crate::unicode::utf8_to_utf16(pattern);
            let f = crate::unicode::utf8_to_utf16(flags);
            let args = vec![Expr::StringLit(p), Expr::StringLit(f)];
            crate::js_regexp::handle_regexp_constructor(&args, env)
        }
        Expr::Conditional(condition, true_expr, false_expr) => {
            let cond_val = evaluate_expr(env, condition)?;
            if is_truthy(&cond_val) {
                evaluate_expr(env, true_expr)
            } else {
                evaluate_expr(env, false_expr)
            }
        }
    }
}

fn evaluate_await_expression(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    let promise_val = evaluate_expr(env, expr)?;
    match promise_val {
        Value::Promise(promise) => {
            // Wait for the promise to resolve by running the event loop
            loop {
                run_event_loop()?;
                let promise_borrow = promise.borrow();
                match &promise_borrow.state {
                    PromiseState::Fulfilled(val) => return Ok(val.clone()),
                    PromiseState::Rejected(reason) => {
                        return Err(raise_throw_error!(reason.clone()));
                    }
                    PromiseState::Pending => {
                        // Continue running the event loop
                    }
                }
            }
        }
        Value::Object(obj) => {
            // Check if this is a Promise object with __promise property
            if let Some(promise_rc) = obj_get_key_value(&obj, &"__promise".into())?
                && let Value::Promise(promise) = promise_rc.borrow().clone()
            {
                // Wait for the promise to resolve by running the event loop
                loop {
                    run_event_loop()?;
                    let promise_borrow = promise.borrow();
                    match &promise_borrow.state {
                        PromiseState::Fulfilled(val) => return Ok(val.clone()),
                        PromiseState::Rejected(reason) => {
                            return Err(raise_throw_error!(reason.clone()));
                        }
                        PromiseState::Pending => {
                            // Continue running the event loop
                        }
                    }
                }
            }
            Err(raise_eval_error!("await can only be used with promises"))
        }
        _ => Err(raise_eval_error!("await can only be used with promises")),
    }
}

fn evaluate_function_expression(
    env: &JSObjectDataPtr,
    name: Option<String>,
    params: &[DestructuringElement],
    body: &[Statement],
) -> Result<Value, JSError> {
    log::trace!("evaluate_function_expression: name={:?} params={:?}", name, params);
    // Create a callable function *object* that wraps the closure so
    // script-level assignments like `F.prototype = ...` work. Store
    // the executable closure under an internal `__closure__` key and
    // expose a `prototype` object with a `constructor` backpointer.
    let func_obj = new_js_object_data();

    // Create the associated prototype object for instances
    let prototype_obj = new_js_object_data();
    // Link new function prototype to Object.prototype so instances inherit Object.prototype methods
    crate::core::set_internal_prototype_from_constructor(&prototype_obj, env, "Object")?;

    // Determine the environment to capture.
    // If this is a named function expression, we must create a new environment
    // that binds the function name to the function object itself, so that
    // the function can refer to itself recursively.
    let closure_env = if let Some(n) = &name {
        let new_env = new_js_object_data();
        new_env.borrow_mut().prototype = Some(Rc::downgrade(env));
        obj_set_key_value(&new_env, &n.into(), Value::Object(func_obj.clone()))?;
        new_env
    } else {
        env.clone()
    };

    // Store the closure under an internal key
    let closure_val = Value::Closure(Rc::new(ClosureData::new(params, body, &closure_env, None)));
    obj_set_key_value(&func_obj, &"__closure__".into(), closure_val)?;

    // If this is a named function expression, expose the `name` property
    if let Some(n) = name {
        obj_set_key_value(&func_obj, &"name".into(), Value::String(utf8_to_utf16(&n)))?;
    }

    // Diagnostic: record the function object pointer so we can trace
    // whether the same function wrapper instance is used across bindings
    // and `new` invocations.
    log::trace!(
        "DBG Expr::Function - created func_obj ptr={:p} prototype_ptr={:p}",
        Rc::as_ptr(&func_obj),
        Rc::as_ptr(&prototype_obj)
    );

    // Wire up `prototype` and `prototype.constructor`
    obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
    obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;

    // Ensure function wrapper objects inherit from `Function.prototype` so
    // methods like `.call` and `.apply` are available via the prototype chain.
    if let Some(func_ctor_val) = obj_get_key_value(env, &"Function".into())? {
        if let Value::Object(func_ctor) = &*func_ctor_val.borrow() {
            if let Some(func_proto_val) = obj_get_key_value(func_ctor, &"prototype".into())? {
                if let Value::Object(func_proto) = &*func_proto_val.borrow() {
                    func_obj.borrow_mut().prototype = Some(Rc::downgrade(func_proto));
                    let _ = obj_set_key_value(&func_obj, &"__proto__".into(), Value::Object(func_proto.clone()));
                }
            }
        }
    }

    Ok(Value::Object(func_obj))
}

fn evaluate_number(n: f64) -> Result<Value, JSError> {
    Ok(Value::Number(n))
}

fn evaluate_string_lit(s: &[u16]) -> Result<Value, JSError> {
    Ok(Value::String(s.to_vec()))
}

fn evaluate_boolean(b: bool) -> Result<Value, JSError> {
    Ok(Value::Boolean(b))
}

fn evaluate_var(env: &JSObjectDataPtr, name: &str, line: Option<usize>, column: Option<usize>) -> Result<Value, JSError> {
    // First, attempt to resolve the name in the current scope chain.
    // This ensures script-defined bindings shadow engine-provided helpers
    // such as `assert`.
    let mut current_opt = Some(env.clone());
    while let Some(current_env) = current_opt {
        if let Some(val_rc) = obj_get_key_value(&current_env, &name.into())? {
            let resolved = val_rc.borrow().clone();
            if let Value::Uninitialized = resolved {
                let mut err = raise_reference_error!(format!("Cannot access '{name}' before initialization"));
                if let (Some(l), Some(c)) = (line, column) {
                    err.set_js_location(l, c);
                }
                return Err(err);
            }
            log::trace!("evaluate_var - {} (found in env) -> {:?}", name, resolved);
            return Ok(resolved);
        }
        current_opt = current_env.borrow().prototype.clone().and_then(|w| w.upgrade());
    }

    if name == "console" {
        let v = Value::Object(make_console_object()?);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "testIntl" {
        let v = Value::Object(make_testintl_object()?);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "testWithIntlConstructors" {
        let v = Value::Function("testWithIntlConstructors".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Array" {
        let ctor = super::ensure_constructor_object(env, "Array", "__is_array_constructor")?;
        if let Some(proto_val) = obj_get_key_value(&ctor, &"prototype".into())? {
            if let Value::Object(proto) = &*proto_val.borrow() {
                let methods = [
                    "at",
                    "push",
                    "pop",
                    "join",
                    "slice",
                    "forEach",
                    "map",
                    "filter",
                    "reduce",
                    "reduceRight",
                    "find",
                    "findIndex",
                    "some",
                    "every",
                    "concat",
                    "indexOf",
                    "includes",
                    "sort",
                    "reverse",
                    "splice",
                    "shift",
                    "unshift",
                    "fill",
                    "lastIndexOf",
                    "toString",
                    "flat",
                    "flatMap",
                    "copyWithin",
                    "entries",
                    "findLast",
                    "findLastIndex",
                ];
                for m in methods.iter() {
                    if get_own_property(proto, &m.to_string().into()).is_none() {
                        obj_set_key_value(proto, &m.to_string().into(), Value::Function(format!("Array.prototype.{}", m)))?;
                    }
                }
            }
        }
        let v = Value::Object(ctor);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Date" {
        let ctor = super::ensure_constructor_object(env, "Date", "__is_date_constructor")?;
        if let Some(proto_val) = obj_get_key_value(&ctor, &"prototype".into())? {
            if let Value::Object(proto) = &*proto_val.borrow() {
                let methods = [
                    "toString",
                    "valueOf",
                    "getTime",
                    "getFullYear",
                    "getYear",
                    "getMonth",
                    "getDate",
                    "getDay",
                    "getHours",
                    "getMinutes",
                    "getSeconds",
                    "getMilliseconds",
                    "getTimezoneOffset",
                    "setFullYear",
                    "setMonth",
                    "setDate",
                    "setHours",
                    "setMinutes",
                    "setSeconds",
                    "setMilliseconds",
                    "setTime",
                    "toISOString",
                    "toUTCString",
                    "toGMTString",
                    "toDateString",
                    "toTimeString",
                    "toLocaleDateString",
                    "toLocaleTimeString",
                    "toLocaleString",
                    "toJSON",
                ];
                for m in methods.iter() {
                    if get_own_property(proto, &m.to_string().into()).is_none() {
                        obj_set_key_value(proto, &m.to_string().into(), Value::Function(format!("Date.prototype.{}", m)))?;
                    }
                }
            }
        }
        let v = Value::Object(ctor);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "String" {
        // Ensure a singleton String constructor object exists in the global env
        let ctor = super::ensure_constructor_object(env, "String", "__is_string_constructor")?;

        // Populate String.prototype with methods
        if let Some(proto_val) = obj_get_key_value(&ctor, &"prototype".into())? {
            if let Value::Object(proto) = &*proto_val.borrow() {
                let methods = [
                    "toString",
                    "valueOf",
                    "substring",
                    "substr",
                    "slice",
                    "toUpperCase",
                    "toLowerCase",
                    "indexOf",
                    "lastIndexOf",
                    "replace",
                    "split",
                    "match",
                    "charAt",
                    "charCodeAt",
                    "trim",
                    "trimEnd",
                    "trimStart",
                    "startsWith",
                    "endsWith",
                    "includes",
                    "repeat",
                    "concat",
                    "padStart",
                    "padEnd",
                ];
                for m in methods.iter() {
                    if get_own_property(proto, &m.to_string().into()).is_none() {
                        obj_set_key_value(proto, &m.to_string().into(), Value::Function(format!("String.prototype.{}", m)))?;
                    }
                }
            }
        }

        let v = Value::Object(ctor);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Math" {
        let v = Value::Object(make_math_object()?);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Reflect" {
        let v = Value::Object(make_reflect_object()?);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "JSON" {
        let json_obj = new_js_object_data();
        obj_set_key_value(&json_obj, &"parse".into(), Value::Function("JSON.parse".to_string()))?;
        obj_set_key_value(&json_obj, &"stringify".into(), Value::Function("JSON.stringify".to_string()))?;
        let v = Value::Object(json_obj);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Object" {
        // Return the Object constructor (we store it in the global environment as an object)
        if let Some(val_rc) = obj_get_key_value(env, &"Object".into())? {
            let resolved = val_rc.borrow().clone();
            log::trace!("evaluate_var - {} -> {:?}", name, resolved);
            return Ok(resolved);
        }
        let v = Value::Function("Object".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "parseInt" {
        let v = Value::Function("parseInt".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "parseFloat" {
        let v = Value::Function("parseFloat".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "import" {
        // Dynamic import function
        let v = Value::Function("import".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "isNaN" {
        let v = Value::Function("isNaN".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "isFinite" {
        let v = Value::Function("isFinite".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "encodeURIComponent" {
        let v = Value::Function("encodeURIComponent".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "decodeURIComponent" {
        let v = Value::Function("decodeURIComponent".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "eval" {
        let v = Value::Function("eval".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "encodeURI" {
        let v = Value::Function("encodeURI".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "decodeURI" {
        let v = Value::Function("decodeURI".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Number" {
        // If Number constructor is already stored in the environment, return it.
        if let Some(val_rc) = obj_get_key_value(env, &"Number".into())? {
            let resolved = val_rc.borrow().clone();
            log::trace!("evaluate_var - {} (from env) -> {:?}", name, resolved);
            return Ok(resolved);
        }
        // Otherwise, create the Number constructor object, store it in the env, and return it.
        let number_obj = make_number_object()?;
        obj_set_key_value(env, &"Number".into(), Value::Object(number_obj.clone()))?;
        let v = Value::Object(number_obj);
        log::trace!("evaluate_var - {} (created) -> {:?}", name, v);
        Ok(v)
    } else if name == "BigInt" {
        // Ensure a singleton BigInt constructor object exists in the global env
        let ctor = super::ensure_constructor_object(env, "BigInt", "__is_bigint_constructor")?;
        let v = Value::Object(ctor);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Boolean" {
        // Ensure a singleton Boolean constructor object exists in the global env
        let ctor = super::ensure_constructor_object(env, "Boolean", "__is_boolean_constructor")?;
        let v = Value::Object(ctor);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "RegExp" {
        let v = Value::Function("RegExp".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Promise" {
        let v = Value::Function("Promise".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Proxy" {
        // Return the Proxy constructor (we store it in the global environment)
        if let Some(val_rc) = obj_get_key_value(env, &"Proxy".into())? {
            let resolved = val_rc.borrow().clone();
            log::trace!("evaluate_var - {} -> {:?}", name, resolved);
            return Ok(resolved);
        }
        let v = Value::Function("Proxy".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "new" {
        let v = Value::Function("new".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "__internal_resolve_promise" {
        let v = Value::Function("__internal_resolve_promise".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "__internal_reject_promise" {
        let v = Value::Function("__internal_reject_promise".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "__internal_promise_allsettled_resolve" {
        let v = Value::Function("__internal_promise_allsettled_resolve".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "__internal_promise_allsettled_reject" {
        let v = Value::Function("__internal_promise_allsettled_reject".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "NaN" {
        let v = Value::Number(f64::NAN);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Infinity" {
        let v = Value::Number(f64::INFINITY);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else {
        // Walk up the prototype chain (scope chain) to find the variable binding.
        let mut current_opt = Some(env.clone());
        while let Some(current_env) = current_opt {
            if let Some(val_rc) = obj_get_key_value(&current_env, &name.into())? {
                let resolved = val_rc.borrow().clone();
                log::trace!("evaluate_var - {} (found) -> {:?}", name, resolved);
                return Ok(resolved);
            }
            current_opt = current_env.borrow().prototype.clone().and_then(|w| w.upgrade());
        }
        log::trace!("evaluate_var - {name} not found in scope, try global 'this' object");
        // As a fallback, some scripts (e.g. test harnesses) install
        // constructor functions as properties on the global `this` object
        // rather than as lexical bindings. If the variable wasn't found
        // in the scope chain, attempt to resolve it as a property of
        // the global `this` object.
        if let Ok(this_val) = evaluate_this(env) {
            if let Value::Object(this_obj) = this_val {
                if let Some(val_rc) = obj_get_key_value(&this_obj, &name.into())? {
                    let resolved = val_rc.borrow().clone();
                    log::trace!("evaluate_var - {name} found on global 'this' -> {resolved:?}");
                    return Ok(resolved);
                }
            }
        }
        log::trace!("evaluate_var - {name} not found -> ReferenceError");
        let mut err = raise_variable_not_found_error!(name);
        if let (Some(l), Some(c)) = (line, column) {
            err.set_js_location(l, c);
        }
        Err(err)
    }
}

fn evaluate_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // Evaluate an assignment expression: perform the assignment and return the assigned value
    evaluate_assignment_expr(env, target, value)
}

fn evaluate_logical_and_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a &&= b is equivalent to a && (a = b)
    let left_val = evaluate_expr(env, target)?;
    if is_truthy(&left_val) {
        // Evaluate the assignment
        evaluate_assignment_expr(env, target, value)
    } else {
        // Return the left value without assignment
        Ok(left_val)
    }
}

fn evaluate_logical_or_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a ||= b is equivalent to a || (a = b)
    let left_val = evaluate_expr(env, target)?;
    if !is_truthy(&left_val) {
        // Evaluate the assignment
        evaluate_assignment_expr(env, target, value)
    } else {
        // Return the left value without assignment
        Ok(left_val)
    }
}

fn evaluate_nullish_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a ??= b is equivalent to a ?? (a = b)
    let left_val = evaluate_expr(env, target)?;
    match left_val {
        Value::Undefined => {
            // Evaluate the assignment
            evaluate_assignment_expr(env, target, value)
        }
        _ => {
            // Return the left value without assignment
            Ok(left_val)
        }
    }
}

fn evaluate_add_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a += b is equivalent to a = a + b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => Value::Number(ln + rn),
        (Value::BigInt(la), Value::BigInt(rb)) => Value::BigInt(la + rb),
        (Value::String(ls), Value::String(rs)) => {
            let mut result = ls.clone();
            result.extend_from_slice(&rs);
            Value::String(result)
        }
        (Value::Number(ln), Value::String(rs)) => {
            let mut result = utf8_to_utf16(&ln.to_string());
            result.extend_from_slice(&rs);
            Value::String(result)
        }
        (Value::String(ls), Value::Number(rn)) => {
            let mut result = ls.clone();
            result.extend_from_slice(&utf8_to_utf16(&rn.to_string()));
            Value::String(result)
        }
        // Disallow mixing BigInt and Number for arithmetic
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for +="));
        }
    };
    let assignment_expr = match &result {
        Value::Number(n) => Expr::Number(*n),
        Value::String(s) => Expr::StringLit(s.clone()),
        Value::BigInt(s) => Expr::BigInt(s.to_string()),
        _ => unreachable!(),
    };
    evaluate_assignment_expr(env, target, &assignment_expr)?;
    Ok(result)
}

fn evaluate_sub_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a -= b is equivalent to a = a - b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => Value::Number(ln - rn),
        (Value::BigInt(la), Value::BigInt(rb)) => Value::BigInt(la - rb),
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }

        _ => {
            return Err(raise_eval_error!("Invalid operands for -="));
        }
    };
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_mul_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a *= b is equivalent to a = a * b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => Value::Number(ln * rn),
        (Value::BigInt(la), Value::BigInt(rb)) => Value::BigInt(la * rb),
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for *="));
        }
    };
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_pow_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a **= b is equivalent to a = a ** b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => Value::Number(ln.powf(rn)),
        (Value::BigInt(la), Value::BigInt(rb)) => {
            if rb < BigInt::from(0) {
                return Err(raise_eval_error!("negative exponent for bigint"));
            }
            let exp = rb.to_u32().ok_or(raise_eval_error!("exponent too large"))?;
            Value::BigInt(la.pow(exp))
        }
        // Mixing BigInt and Number is disallowed for exponentiation
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for **="));
        }
    };

    // update assignment target (store result back into target)
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_div_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a /= b is equivalent to a = a / b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => {
            if rn == 0.0 {
                return Err(raise_eval_error!("Division by zero"));
            }
            Value::Number(ln / rn)
        }
        (Value::BigInt(la), Value::BigInt(rb)) => {
            if rb == BigInt::from(0) {
                return Err(raise_eval_error!("Division by zero"));
            }
            Value::BigInt(la / rb)
        }
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for /="));
        }
    };
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_mod_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a %= b is equivalent to a = a % b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => {
            if rn == 0.0 {
                return Err(raise_eval_error!("Division by zero"));
            }
            Value::Number(ln % rn)
        }
        (Value::BigInt(la), Value::BigInt(rb)) => {
            if rb == BigInt::from(0) {
                return Err(raise_eval_error!("Division by zero"));
            }
            Value::BigInt(la % rb)
        }
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for %="));
        }
    };
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_bitxor_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a ^= b is equivalent to a = a ^ b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => {
            Value::Number((crate::core::number::to_int32(ln) ^ crate::core::number::to_int32(rn)) as f64)
        }
        (Value::BigInt(la), Value::BigInt(rb)) => {
            use std::ops::BitXor;
            let res = la.bitxor(&rb);
            Value::BigInt(res)
        }
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for ^="));
        }
    };
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_bitand_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a &= b is equivalent to a = a & b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => {
            Value::Number((crate::core::number::to_int32(ln) & crate::core::number::to_int32(rn)) as f64)
        }
        (Value::BigInt(la), Value::BigInt(rb)) => {
            use std::ops::BitAnd;
            let res = la.bitand(&rb);
            Value::BigInt(res)
        }
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for &="));
        }
    };
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_bitor_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a |= b is equivalent to a = a | b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => {
            Value::Number((crate::core::number::to_int32(ln) | crate::core::number::to_int32(rn)) as f64)
        }
        (Value::BigInt(la), Value::BigInt(rb)) => {
            use std::ops::BitOr;
            let res = la.bitor(&rb);
            Value::BigInt(res)
        }
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for |="));
        }
    };
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_left_shift_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a <<= b is equivalent to a = a << b
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => {
            let a = crate::core::number::to_int32(ln);
            let s = crate::core::number::to_uint32(rn) & 0x1f;
            Value::Number(((a << s) as i32) as f64)
        }
        (Value::BigInt(la), Value::BigInt(rb)) => {
            use std::ops::Shl;
            // try to convert shift amount to usize
            let shift = rb.to_usize().ok_or(raise_eval_error!("invalid bigint shift"))?;
            let res = la.shl(shift);
            Value::BigInt(res)
        }
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for <<="));
        }
    };
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_right_shift_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a >>= b is equivalent to a = a >> b (arithmetic right shift)
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => {
            let a = crate::core::number::to_int32(ln);
            let s = crate::core::number::to_uint32(rn) & 0x1f;
            Value::Number((a >> s) as f64)
        }
        (Value::BigInt(la), Value::BigInt(rb)) => {
            use std::ops::Shr;
            let shift = rb.to_usize().ok_or(raise_eval_error!("invalid bigint shift"))?;
            let res = la.shr(shift);
            Value::BigInt(res)
        }
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for >>="));
        }
    };
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_unsigned_right_shift_assign(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    // a >>>= b is equivalent to a = a >>> b (unsigned right shift)
    let left_val = evaluate_expr(env, target)?;
    let right_val = evaluate_expr(env, value)?;
    let result = match (left_val, right_val) {
        (Value::Number(ln), Value::Number(rn)) => {
            let a = crate::core::number::to_uint32(ln);
            let s = crate::core::number::to_uint32(rn) & 0x1f;
            Value::Number((a >> s) as f64)
        }
        // BigInt does not support unsigned right shift
        (Value::BigInt(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Unsigned right shift not supported for BigInt"));
        }
        (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
            return Err(raise_type_error!("Cannot mix BigInt and other types"));
        }
        _ => {
            return Err(raise_eval_error!("Invalid operands for >>>="));
        }
    };
    match &result {
        Value::Number(n) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::Number(*n))?;
        }
        Value::BigInt(s) => {
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.to_string()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_assignment_expr(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, value)?;
    match target {
        Expr::Var(name, _, _) => {
            log::debug!("evaluate_assignment_expr: assigning Var '{}' = {:?}", name, val);
            env_set_recursive(env, name, val.clone())?;
            Ok(val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(object) => {
                    obj_set_key_value(&object, &prop.into(), val.clone())?;
                    Ok(val)
                }
                _ => Ok(val),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(object), Value::String(s)) => {
                    let key = PropertyKey::String(utf16_to_utf8(&s));
                    obj_set_key_value(&object, &key, val.clone())?;
                    Ok(val)
                }
                (Value::Object(object), Value::Number(n)) => {
                    // Check if this is a TypedArray first
                    let ta_val_opt = obj_get_key_value(&object, &"__typedarray".into());
                    if let Ok(Some(ta_val)) = ta_val_opt
                        && let Value::TypedArray(ta) = &*ta_val.borrow()
                    {
                        // This is a TypedArray, use our set method
                        let idx = n as usize;
                        let val_num = match &val {
                            Value::Number(num) => *num as i64,
                            Value::BigInt(s) => s.to_i64().ok_or(raise_eval_error!("TypedArray assignment value out of range"))?,
                            _ => return Err(raise_eval_error!("TypedArray assignment value must be a number")),
                        };
                        ta.borrow_mut()
                            .set(idx, val_num)
                            .map_err(|_| raise_eval_error!("TypedArray index out of bounds"))?;
                        return Ok(val);
                    }
                    let key = PropertyKey::String(n.to_string());
                    obj_set_key_value(&object, &key, val.clone())?;
                    Ok(val)
                }
                (Value::Object(object), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_key_value(&object, &key, val.clone())?;
                    Ok(val)
                }
                _ => Ok(val),
            }
        }
        _ => Err(raise_eval_error!("Invalid assignment target")),
    }
}

fn evaluate_increment(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // Prefix increment: ++expr
    let current_val = evaluate_expr(env, expr)?;
    let new_val = match current_val {
        Value::Number(n) => Value::Number(n + 1.0),
        _ => {
            return Err(raise_eval_error!("Increment operand must be a number"));
        }
    };
    // Assign back
    match expr {
        Expr::Var(name, _, _) => {
            env_set_recursive(env, name, new_val.clone())?;
            Ok(new_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(object) => {
                    obj_set_key_value(&object, &prop.into(), new_val.clone())?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Cannot increment property of non-object")),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(object), Value::String(s)) => {
                    let key = PropertyKey::String(utf16_to_utf8(&s));
                    obj_set_key_value(&object, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(object), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_key_value(&object, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(object), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_key_value(&object, &key, new_val.clone())?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Invalid index increment")),
            }
        }
        _ => Err(raise_eval_error!("Invalid increment target")),
    }
}

fn evaluate_decrement(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // Prefix decrement: --expr
    let current_val = evaluate_expr(env, expr)?;
    let new_val = match current_val {
        Value::Number(n) => Value::Number(n - 1.0),
        _ => {
            return Err(raise_eval_error!("Decrement operand must be a number"));
        }
    };
    // Assign back
    match expr {
        Expr::Var(name, _, _) => {
            env_set_recursive(env, name, new_val.clone())?;
            Ok(new_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(object) => {
                    obj_set_key_value(&object, &prop.into(), new_val.clone())?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Cannot decrement property of non-object")),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(object), Value::String(s)) => {
                    let key = PropertyKey::String(utf16_to_utf8(&s));
                    obj_set_key_value(&object, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(object), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_key_value(&object, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(object), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_key_value(&object, &key, new_val.clone())?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Invalid index decrement")),
            }
        }
        _ => Err(raise_eval_error!("Invalid decrement target")),
    }
}

fn evaluate_post_increment(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // Postfix increment: expr++
    let current_val = evaluate_expr(env, expr)?;
    let old_val = current_val.clone();
    let new_val = match current_val {
        Value::Number(n) => Value::Number(n + 1.0),
        _ => {
            return Err(raise_eval_error!("Increment operand must be a number"));
        }
    };
    // Assign back
    match expr {
        Expr::Var(name, _, _) => {
            env_set_recursive(env, name, new_val)?;
            Ok(old_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(object) => {
                    obj_set_key_value(&object, &prop.into(), new_val)?;
                    Ok(old_val)
                }
                _ => Err(raise_eval_error!("Cannot increment property of non-object")),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(object), Value::String(s)) => {
                    let key = PropertyKey::String(utf16_to_utf8(&s));
                    obj_set_key_value(&object, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(object), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_key_value(&object, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(object), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_key_value(&object, &key, new_val)?;
                    Ok(old_val)
                }
                _ => Err(raise_eval_error!("Invalid index increment")),
            }
        }
        _ => Err(raise_eval_error!("Invalid increment target")),
    }
}

fn evaluate_post_decrement(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // Postfix decrement: expr--
    let current_val = evaluate_expr(env, expr)?;
    let old_val = current_val.clone();
    let new_val = match current_val {
        Value::Number(n) => Value::Number(n - 1.0),
        _ => {
            return Err(raise_eval_error!("Decrement operand must be a number"));
        }
    };
    // Assign back
    match expr {
        Expr::Var(name, _, _) => {
            env_set_recursive(env, name, new_val)?;
            Ok(old_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(object) => {
                    obj_set_key_value(&object, &prop.into(), new_val)?;
                    Ok(old_val)
                }
                _ => Err(raise_eval_error!("Cannot decrement property of non-object")),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(object), Value::String(s)) => {
                    let key = PropertyKey::String(utf16_to_utf8(&s));
                    obj_set_key_value(&object, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(object), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_key_value(&object, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(object), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_key_value(&object, &key, new_val)?;
                    Ok(old_val)
                }
                _ => Err(raise_eval_error!("Invalid index decrement")),
            }
        }
        _ => Err(raise_eval_error!("Invalid decrement target")),
    }
}

fn evaluate_unary_neg(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, expr)?;
    match val {
        Value::Number(n) => Ok(Value::Number(-n)),
        Value::BigInt(s) => Ok(Value::BigInt(-s)),
        _ => Err(raise_eval_error!("error")),
    }
}

fn evaluate_unary_plus(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, expr)?;
    match val {
        Value::Number(n) => Ok(Value::Number(n)),
        Value::BigInt(_) => Err(raise_type_error!("Cannot convert a BigInt value to a number")),
        _ => {
            let num = match val {
                Value::String(s) => utf16_to_utf8(&s).parse::<f64>().unwrap_or(f64::NAN),
                Value::Boolean(b) => {
                    if b {
                        1.0
                    } else {
                        0.0
                    }
                }
                Value::Null => 0.0,
                Value::Undefined => f64::NAN,
                _ => f64::NAN,
            };
            Ok(Value::Number(num))
        }
    }
}

fn evaluate_bit_not(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, expr)?;
    match val {
        Value::BigInt(n) => Ok(Value::BigInt(!n)),
        _ => {
            let num = match val {
                Value::Number(n) => n,
                Value::String(s) => utf16_to_utf8(&s).parse::<f64>().unwrap_or(f64::NAN),
                Value::Boolean(b) => {
                    if b {
                        1.0
                    } else {
                        0.0
                    }
                }
                Value::Null => 0.0,
                Value::Undefined => f64::NAN,
                _ => f64::NAN,
            };
            let int_val = if num.is_nan() || num.is_infinite() { 0 } else { num as i32 };
            Ok(Value::Number((!int_val) as f64))
        }
    }
}

fn evaluate_typeof(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // `typeof` operator must NOT trigger creation or injection of built-ins
    // when the identifier is undeclared. Evaluate `Expr::Var` specially by
    // performing a lexical lookup only (walk the environment chain) and
    // treat missing bindings as `undefined` per JS semantics.
    let val = match expr {
        Expr::Var(name, _, _) => {
            // Walk env chain searching for own properties; do not consult
            // evaluator fallbacks or built-in helpers here — `typeof` must
            // act like an existence check for declared bindings.
            let mut current_opt: Option<JSObjectDataPtr> = Some(env.clone());
            let mut found_val: Option<Rc<RefCell<Value>>> = None;
            while let Some(current_env) = current_opt {
                if let Some(v) = get_own_property(&current_env, &name.as_str().into()) {
                    found_val = Some(v);
                    break;
                }
                current_opt = current_env.borrow().prototype.clone().and_then(|w| w.upgrade());
            }
            if let Some(rc) = found_val {
                rc.borrow().clone()
            } else {
                // undeclared identifier -> undefined (no builtins injected)
                Value::Undefined
            }
        }
        _ => evaluate_expr(env, expr)?,
    };
    let type_str = match &val {
        Value::Undefined => "undefined",
        Value::Null => "object",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::BigInt(_) => "bigint",
        Value::Object(object) => {
            // If this object wraps a closure under the internal `__closure__` key,
            // report `function` for `typeof` so function-objects behave like functions.
            #[allow(clippy::if_same_then_else)]
            if extract_closure_from_value(&val).is_some() {
                "function"
            } else if obj_get_key_value(object, &"__is_constructor".into()).ok().flatten().is_some() {
                "function"
            } else {
                "object"
            }
        }
        Value::Function(_) => "function",
        Value::Closure(..) | Value::AsyncClosure(..) | Value::GeneratorFunction(..) => "function",
        Value::ClassDefinition(_) => "function",
        Value::Getter(..) => "function",
        Value::Setter(..) => "function",
        Value::Property { .. } => "undefined",
        Value::Promise(_) => "object",
        Value::Symbol(_) => "symbol",
        Value::Map(_) => "object",
        Value::Set(_) => "object",
        Value::WeakMap(_) => "object",
        Value::WeakSet(_) => "object",
        Value::Generator(_) => "object",
        Value::Proxy(_) => "object",
        Value::ArrayBuffer(_) => "object",
        Value::DataView(_) => "object",
        Value::TypedArray(_) => "object",
        Value::Uninitialized => "undefined",
    };
    Ok(Value::String(utf8_to_utf16(type_str)))
}

fn evaluate_delete(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    match expr {
        Expr::Var(name, _, _) => {
            // Walk the scope chain to find the variable
            let mut current_opt = Some(env.clone());
            while let Some(current_env) = current_opt {
                if get_own_property(&current_env, &name.into()).is_some() {
                    // Found the environment record containing the binding.
                    // Try to delete it.
                    let deleted = obj_delete(&current_env, &name.into())?;
                    return Ok(Value::Boolean(deleted));
                }
                current_opt = current_env.borrow().prototype.clone().and_then(|w| w.upgrade());
            }
            // If not found, return true
            Ok(Value::Boolean(true))
        }
        Expr::Property(obj, prop) => {
            // Delete property from object
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(object) => {
                    let deleted = obj_delete(&object, &prop.into())?;
                    Ok(Value::Boolean(deleted))
                }
                _ => Ok(Value::Boolean(false)),
            }
        }
        Expr::Index(obj, idx) => {
            // Delete indexed property
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(object), Value::String(s)) => {
                    let key = PropertyKey::String(utf16_to_utf8(&s));
                    let deleted = obj_delete(&object, &key)?;
                    Ok(Value::Boolean(deleted))
                }
                (Value::Object(object), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    let deleted = obj_delete(&object, &key)?;
                    Ok(Value::Boolean(deleted))
                }
                (Value::Object(object), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    let deleted = obj_delete(&object, &key)?;
                    Ok(Value::Boolean(deleted))
                }
                _ => Ok(Value::Boolean(false)),
            }
        }
        _ => {
            // Cannot delete other types of expressions
            Ok(Value::Boolean(false))
        }
    }
}

fn evaluate_void(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    // Evaluate the expression but always return undefined
    evaluate_expr(env, expr)?;
    Ok(Value::Undefined)
}

// Helper to convert a value to f64 for comparison (ToNumber semantics simplified)
fn to_num(v: &Value) -> Result<f64, JSError> {
    match v {
        Value::Number(n) => Ok(*n),
        Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::BigInt(s) => {
            if let Some(f) = s.to_f64() {
                Ok(f)
            } else {
                Ok(f64::NAN)
            }
        }
        Value::String(s) => {
            let sstr = utf16_to_utf8(s);
            let t = sstr.trim();
            if t.is_empty() {
                Ok(0.0)
            } else {
                match t.parse::<f64>() {
                    Ok(v) => Ok(v),
                    Err(_) => Ok(f64::NAN),
                }
            }
        }
        Value::Undefined => Ok(f64::NAN),
        Value::Symbol(_) => Err(raise_type_error!("Cannot convert Symbol to number")),
        _ => Err(raise_eval_error!("error")),
    }
}

fn to_number_f64(val: &Value) -> f64 {
    match val {
        Value::Number(n) => *n,
        Value::Boolean(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => {
            let s_utf8 = utf16_to_utf8(s);
            if s_utf8.trim().is_empty() {
                0.0
            } else {
                s_utf8.trim().parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Value::Null => 0.0,
        Value::Undefined => f64::NAN,
        _ => f64::NAN,
    }
}

fn evaluate_binary(env: &JSObjectDataPtr, left: &Expr, op: &BinaryOp, right: &Expr) -> Result<Value, JSError> {
    let l = evaluate_expr(env, left)?;
    let r = evaluate_expr(env, right)?;
    match op {
        BinaryOp::Add => {
            // If either side is an object, attempt ToPrimitive coercion (default hint) first
            let l_prim = if matches!(l, Value::Object(_)) {
                to_primitive(&l, "default", env)?
            } else {
                l.clone()
            };
            let r_prim = if matches!(r, Value::Object(_)) {
                to_primitive(&r, "default", env)?
            } else {
                r.clone()
            };
            // '+' should throw when a Symbol is encountered during implicit coercion
            if matches!(l_prim, Value::Symbol(_)) || matches!(r_prim, Value::Symbol(_)) {
                return Err(raise_type_error!("Cannot convert Symbol to primitive"));
            }
            match (l_prim, r_prim) {
                (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln + rn)),
                (Value::BigInt(la), Value::BigInt(rb)) => Ok(Value::BigInt(la + rb)),
                (Value::String(ls), Value::String(rs)) => {
                    let mut result = ls.clone();
                    result.extend_from_slice(&rs);
                    Ok(Value::String(result))
                }
                // Concatenate string with undefined by coercing undefined to "undefined"
                (Value::String(ls), Value::Undefined) => {
                    let mut result = ls.clone();
                    result.extend_from_slice(&utf8_to_utf16("undefined"));
                    Ok(Value::String(result))
                }
                (Value::Undefined, Value::String(rs)) => {
                    let mut result = utf8_to_utf16("undefined");
                    result.extend_from_slice(&rs);
                    Ok(Value::String(result))
                }
                (Value::Number(ln), Value::String(rs)) => {
                    let mut result = utf8_to_utf16(&ln.to_string());
                    result.extend_from_slice(&rs);
                    Ok(Value::String(result))
                }
                (Value::String(ls), Value::Number(rn)) => {
                    let mut result = ls.clone();
                    result.extend_from_slice(&utf8_to_utf16(&rn.to_string()));
                    Ok(Value::String(result))
                }
                (Value::Boolean(lb), Value::String(rs)) => {
                    let mut result = utf8_to_utf16(&lb.to_string());
                    result.extend_from_slice(&rs);
                    Ok(Value::String(result))
                }
                (Value::String(ls), Value::Boolean(rb)) => {
                    let mut result = ls.clone();
                    result.extend_from_slice(&utf8_to_utf16(&rb.to_string()));
                    Ok(Value::String(result))
                }
                (Value::String(ls), Value::BigInt(rb)) => {
                    // String + BigInt -> concatenation (use raw string)
                    let mut result = ls.clone();
                    result.extend_from_slice(&utf8_to_utf16(&rb.to_string()));
                    Ok(Value::String(result))
                }
                (Value::BigInt(la), Value::String(rs)) => {
                    // BigInt + String -> concatenation
                    let mut result = utf8_to_utf16(&la.to_string());
                    result.extend_from_slice(&rs);
                    Ok(Value::String(result))
                }
                // Mixing BigInt and Number for `+` should raise a TypeError
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (l_val, r_val) => {
                    let ln = to_num(&l_val)?;
                    let rn = to_num(&r_val)?;
                    Ok(Value::Number(ln + rn))
                }
            }
        }
        BinaryOp::Sub => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(la), Value::BigInt(rb)) => Ok(Value::BigInt(la - rb)),
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    Ok(Value::Number(ln - rn))
                }
            }
        }
        BinaryOp::Mul => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(la), Value::BigInt(rb)) => Ok(Value::BigInt(la * rb)),
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    Ok(Value::Number(ln * rn))
                }
            }
        }
        BinaryOp::Pow => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(la), Value::BigInt(rb)) => {
                    if rb < BigInt::from(0) {
                        return Err(raise_eval_error!("negative exponent for bigint"));
                    }
                    let exp = rb.to_u32().ok_or(raise_eval_error!("exponent too large"))?;
                    Ok(Value::BigInt(la.pow(exp)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    Ok(Value::Number(ln.powf(rn)))
                }
            }
        }
        BinaryOp::Div => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(la), Value::BigInt(rb)) => {
                    if rb == BigInt::from(0) {
                        return Err(raise_eval_error!("Division by zero"));
                    }
                    Ok(Value::BigInt(la / rb))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    Ok(Value::Number(ln / rn))
                }
            }
        }
        BinaryOp::Equal => {
            // Abstract equality comparison with type coercion
            abstract_equality(&l, &r, env)
        }
        BinaryOp::StrictEqual => {
            // Strict equality comparison without type coercion
            strict_equality(&l, &r)
        }
        BinaryOp::NotEqual => {
            // Abstract inequality: invert abstract equality
            match abstract_equality(&l, &r, env)? {
                Value::Boolean(b) => Ok(Value::Boolean(!b)),
                _ => Err(raise_eval_error!("abstract_equality should return boolean")),
            }
        }
        BinaryOp::StrictNotEqual => {
            // Strict inequality: invert strict equality
            match strict_equality(&l, &r)? {
                Value::Boolean(b) => Ok(Value::Boolean(!b)),
                _ => Err(raise_eval_error!("strict_equality should return boolean")),
            }
        }
        BinaryOp::LessThan => {
            // Follow JS abstract relational comparison with ToPrimitive(Number) hint
            let l_prim = if matches!(l, Value::Object(_)) {
                to_primitive(&l, "number", env)?
            } else {
                l.clone()
            };
            let r_prim = if matches!(r, Value::Object(_)) {
                to_primitive(&r, "number", env)?
            } else {
                r.clone()
            };

            // If both are strings, do lexicographic comparison
            if let (Value::String(ls), Value::String(rs)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(ls < rs));
            }
            if let (Value::BigInt(la), Value::BigInt(rb)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(la < rb));
            }
            if let (Value::BigInt(la), Value::Number(rn)) = (&l_prim, &r_prim) {
                let rn = *rn;
                // NaN / infinite are always false for relational comparisons with BigInt
                if rn.is_nan() || !rn.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                // If number is integer, compare as BigInt exactly
                if rn.fract() == 0.0 {
                    let num_str = format!("{:.0}", rn);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        return Ok(Value::Boolean(la < &num_bi));
                    }
                    return Ok(Value::Boolean(false));
                }
                // Non-integer number: compare BigInt <= floor(number)
                let floor = rn.floor();
                let floor_str = format!("{:.0}", floor);
                if let Ok(floor_bi) = BigInt::from_str(&floor_str) {
                    return Ok(Value::Boolean(la <= &floor_bi));
                }
                return Ok(Value::Boolean(false));
            }
            if let (Value::Number(ln), Value::BigInt(rb)) = (&l_prim, &r_prim) {
                let ln = *ln;
                if ln.is_nan() || !ln.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if ln.fract() == 0.0 {
                    let num_str = format!("{:.0}", ln);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        return Ok(Value::Boolean(&num_bi < rb));
                    }
                    return Ok(Value::Boolean(false));
                }
                // Non-integer: ln < bigint <-> floor(ln) < bigint
                let floor = ln.floor();
                let floor_str = format!("{:.0}", floor);
                if let Ok(floor_bi) = BigInt::from_str(&floor_str) {
                    return Ok(Value::Boolean(&floor_bi < rb));
                }
                return Ok(Value::Boolean(false));
            }
            // Fallback: convert values to numbers and compare. Non-coercible symbols/types will error.
            {
                let ln = to_num(&l_prim)?;
                let rn = to_num(&r_prim)?;
                if ln.is_nan() || rn.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                Ok(Value::Boolean(ln < rn))
            }
        }
        BinaryOp::GreaterThan => {
            // Abstract relational comparison with ToPrimitive(Number) hint
            let l_prim = if matches!(l, Value::Object(_)) {
                to_primitive(&l, "number", env)?
            } else {
                l.clone()
            };
            let r_prim = if matches!(r, Value::Object(_)) {
                to_primitive(&r, "number", env)?
            } else {
                r.clone()
            };

            // If both strings, lexicographic compare
            if let (Value::String(ls), Value::String(rs)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(ls > rs));
            }
            if let (Value::BigInt(la), Value::BigInt(rb)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(la > rb));
            }
            if let (Value::BigInt(la), Value::Number(rn)) = (&l_prim, &r_prim) {
                let rn = *rn;
                if rn.is_nan() || !rn.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                // integer -> exact BigInt compare
                if rn.fract() == 0.0 {
                    let num_str = format!("{:.0}", rn);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        return Ok(Value::Boolean(la > &num_bi));
                    }
                    return Ok(Value::Boolean(false));
                }
                // non-integer -> compare against ceil(rn): a > rn <=> a >= ceil(rn)
                let ceil = rn.ceil();
                let ceil_str = format!("{:.0}", ceil);
                if let Ok(ceil_bi) = BigInt::from_str(&ceil_str) {
                    return Ok(Value::Boolean(la >= &ceil_bi));
                }
                return Ok(Value::Boolean(false));
            }
            if let (Value::Number(ln), Value::BigInt(rb)) = (&l_prim, &r_prim) {
                let ln = *ln;
                if ln.is_nan() || !ln.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if ln.fract() == 0.0 {
                    let num_str = format!("{:.0}", ln);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        return Ok(Value::Boolean(&num_bi > rb));
                    }
                    return Ok(Value::Boolean(false));
                }
                // ln > bigint <=> ceil(ln) > bigint
                let ceil = ln.ceil();
                let ceil_str = format!("{:.0}", ceil);
                if let Ok(ceil_bi) = BigInt::from_str(&ceil_str) {
                    return Ok(Value::Boolean(&ceil_bi > rb));
                }
                return Ok(Value::Boolean(false));
            }
            {
                let ln = to_num(&l_prim)?;
                let rn = to_num(&r_prim)?;
                if ln.is_nan() || rn.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                Ok(Value::Boolean(ln > rn))
            }
        }
        BinaryOp::LessEqual => {
            // Use ToPrimitive(Number) hint then compare, strings compare lexicographically
            let l_prim = if matches!(l, Value::Object(_)) {
                to_primitive(&l, "number", env)?
            } else {
                l.clone()
            };
            let r_prim = if matches!(r, Value::Object(_)) {
                to_primitive(&r, "number", env)?
            } else {
                r.clone()
            };

            if let (Value::String(ls), Value::String(rs)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(ls <= rs));
            }
            if let (Value::BigInt(la), Value::BigInt(rb)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(la <= rb));
            }
            if let (Value::BigInt(la), Value::Number(rn)) = (&l_prim, &r_prim) {
                if rn.is_nan() || !rn.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if rn.fract() == 0.0 {
                    let num_str = format!("{:.0}", rn);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        return Ok(Value::Boolean(la <= &num_bi));
                    }
                    return Ok(Value::Boolean(false));
                }
                // non-integer number: compare a <= floor(rn)
                let floor = rn.floor();
                let floor_str = format!("{:.0}", floor);
                if let Ok(floor_bi) = BigInt::from_str(&floor_str) {
                    return Ok(Value::Boolean(la <= &floor_bi));
                }
                return Ok(Value::Boolean(false));
            }
            if let (Value::Number(ln), Value::BigInt(rb)) = (&l_prim, &r_prim) {
                if ln.is_nan() || !ln.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if ln.fract() == 0.0 {
                    let num_str = format!("{:.0}", ln);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        return Ok(Value::Boolean(&num_bi <= rb));
                    }
                    return Ok(Value::Boolean(false));
                }
                // non-integer number: ln <= bigint <=> floor(ln) < bigint
                let floor = ln.floor();
                let floor_str = format!("{:.0}", floor);
                if let Ok(floor_bi) = BigInt::from_str(&floor_str) {
                    return Ok(Value::Boolean(&floor_bi < rb));
                }
                return Ok(Value::Boolean(false));
            }
            {
                let ln = to_num(&l_prim)?;
                let rn = to_num(&r_prim)?;
                if ln.is_nan() || rn.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                Ok(Value::Boolean(ln <= rn))
            }
        }
        BinaryOp::GreaterEqual => {
            // ToPrimitive(Number) hint with fallback to numeric comparison; strings compare lexicographically
            let l_prim = if matches!(l, Value::Object(_)) {
                to_primitive(&l, "number", env)?
            } else {
                l.clone()
            };
            let r_prim = if matches!(r, Value::Object(_)) {
                to_primitive(&r, "number", env)?
            } else {
                r.clone()
            };

            if let (Value::String(ls), Value::String(rs)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(ls >= rs));
            }
            if let (Value::BigInt(la), Value::BigInt(rb)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(la >= rb));
            }
            if let (Value::BigInt(la), Value::Number(rn)) = (&l_prim, &r_prim) {
                let rn = *rn;
                if rn.is_nan() || !rn.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if rn.fract() == 0.0 {
                    let num_str = format!("{:.0}", rn);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        return Ok(Value::Boolean(la >= &num_bi));
                    }
                    return Ok(Value::Boolean(false));
                }
                // non-integer rn: a >= ceil(rn)
                let ceil = rn.ceil();
                let ceil_str = format!("{:.0}", ceil);
                if let Ok(ceil_bi) = BigInt::from_str(&ceil_str) {
                    return Ok(Value::Boolean(la >= &ceil_bi));
                }
                return Ok(Value::Boolean(false));
            }
            if let (Value::Number(ln), Value::BigInt(rb)) = (&l_prim, &r_prim) {
                let ln = *ln;
                if ln.is_nan() || !ln.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if ln.fract() == 0.0 {
                    let num_str = format!("{:.0}", ln);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        return Ok(Value::Boolean(&num_bi >= rb));
                    }
                    return Ok(Value::Boolean(false));
                }
                // non-integer ln: ln >= b <=> ceil(ln) > b
                let ceil = ln.ceil();
                let ceil_str = format!("{:.0}", ceil);
                if let Ok(ceil_bi) = BigInt::from_str(&ceil_str) {
                    return Ok(Value::Boolean(&ceil_bi > rb));
                }
                return Ok(Value::Boolean(false));
            }
            {
                let ln = to_num(&l_prim)?;
                let rn = to_num(&r_prim)?;
                if ln.is_nan() || rn.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                Ok(Value::Boolean(ln >= rn))
            }
        }
        BinaryOp::Mod => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(la), Value::BigInt(rb)) => {
                    if rb == BigInt::from(0) {
                        return Err(raise_eval_error!("Division by zero"));
                    }
                    Ok(Value::BigInt(la % rb))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    if rn == 0.0 {
                        Ok(Value::Number(f64::NAN))
                    } else {
                        Ok(Value::Number(ln % rn))
                    }
                }
            }
        }
        BinaryOp::InstanceOf => {
            // Check if left is an instance of right (constructor)
            log::trace!("Evaluating instanceof with left={:?}, right={:?}", l, r);
            match (l, r) {
                (Value::Object(obj), Value::Object(constructor)) => {
                    // Debug: inspect the object's direct __proto__ read before instanceof
                    match obj_get_key_value(&obj, &"__proto__".into())? {
                        Some(v) => log::trace!("pre-instanceof: obj.__proto__ = {:?}", v),
                        None => log::trace!("pre-instanceof: obj.__proto__ = None"),
                    }
                    Ok(Value::Boolean(is_instance_of(&obj, &constructor)?))
                }
                _ => Ok(Value::Boolean(false)),
            }
        }
        BinaryOp::BitXor => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(la), Value::BigInt(rb)) => {
                    use std::ops::BitXor;
                    Ok(Value::BigInt(la.bitxor(&rb)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    let a = crate::core::number::to_int32(ln);
                    let b = crate::core::number::to_int32(rn);
                    Ok(Value::Number((a ^ b) as f64))
                }
            }
        }
        BinaryOp::In => {
            // Check if property exists in object. Support private-name `#name in obj` checks.
            if let Value::Object(obj) = r {
                let prim = to_primitive(&l, "string", env)?;
                // If left side is a string starting with '#', treat it as a private name check
                if let Value::String(s) = &prim {
                    let key_utf8 = utf16_to_utf8(s);
                    if let Some(private_name) = key_utf8.strip_prefix('#') {
                        // Look for class definition on object/prototype chain
                        if let Some(class_def_val) = obj_get_key_value(&obj, &"__class_def__".into())? {
                            if let Value::ClassDefinition(class_def) = &*class_def_val.borrow() {
                                for member in &class_def.members {
                                    match member {
                                        ClassMember::PrivateProperty(name, _)
                                        | ClassMember::PrivateMethod(name, _, _)
                                        | ClassMember::PrivateStaticProperty(name, _)
                                        | ClassMember::PrivateStaticMethod(name, _, _) => {
                                            if name == private_name {
                                                return Ok(Value::Boolean(true));
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        return Ok(Value::Boolean(false));
                    }
                }

                let key = match prim {
                    Value::Symbol(s) => PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(s)))),
                    Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                    Value::Number(n) => PropertyKey::String(n.to_string()),
                    Value::Boolean(b) => PropertyKey::String(b.to_string()),
                    Value::Undefined => PropertyKey::String("undefined".to_string()),
                    Value::Null => PropertyKey::String("null".to_string()),
                    Value::BigInt(b) => PropertyKey::String(b.to_string()),
                    _ => PropertyKey::String("[object Object]".to_string()),
                };
                Ok(Value::Boolean(obj_get_key_value(&obj, &key)?.is_some()))
            } else {
                Err(raise_type_error!(format!(
                    "Cannot use 'in' operator to search for '{}' in {:?}",
                    value_to_string(&l),
                    r
                )))
            }
        }
        BinaryOp::BitAnd => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(la), Value::BigInt(rb)) => {
                    use std::ops::BitAnd;
                    Ok(Value::BigInt(la.bitand(&rb)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    let a = crate::core::number::to_int32(ln);
                    let b = crate::core::number::to_int32(rn);
                    Ok(Value::Number((a & b) as f64))
                }
            }
        }
        BinaryOp::BitOr => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(la), Value::BigInt(rb)) => {
                    use std::ops::BitOr;
                    Ok(Value::BigInt(la.bitor(&rb)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    let a = crate::core::number::to_int32(ln);
                    let b = crate::core::number::to_int32(rn);
                    Ok(Value::Number((a | b) as f64))
                }
            }
        }
        BinaryOp::LeftShift => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(la), Value::BigInt(rb)) => {
                    if rb < BigInt::from(0) {
                        return Err(raise_eval_error!("negative shift count"));
                    }
                    let shift = rb.to_u32().ok_or(raise_eval_error!("shift count too large"))?;
                    use std::ops::Shl;
                    Ok(Value::BigInt(la.shl(shift)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    let a = crate::core::number::to_int32(ln);
                    let shift = crate::core::number::to_uint32(rn) & 0x1f;
                    let res = a.wrapping_shl(shift);
                    Ok(Value::Number(res as f64))
                }
            }
        }
        BinaryOp::RightShift => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(la), Value::BigInt(rb)) => {
                    if rb < BigInt::from(0) {
                        return Err(raise_eval_error!("negative shift count"));
                    }
                    let shift = rb.to_u32().ok_or(raise_eval_error!("shift count too large"))?;
                    use std::ops::Shr;
                    Ok(Value::BigInt(la.shr(shift)))
                }
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => Err(raise_type_error!("Cannot mix BigInt and other types")),
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    let a = crate::core::number::to_int32(ln);
                    let shift = crate::core::number::to_uint32(rn) & 0x1f;
                    let res = a >> shift;
                    Ok(Value::Number(res as f64))
                }
            }
        }
        BinaryOp::UnsignedRightShift => {
            let l_prim = to_primitive(&l, "number", env)?;
            let r_prim = to_primitive(&r, "number", env)?;
            match (l_prim, r_prim) {
                (Value::BigInt(_), _) | (_, Value::BigInt(_)) => {
                    Err(raise_type_error!("BigInts have no unsigned right shift, use >> instead"))
                }
                (lp, rp) => {
                    let ln = to_number_f64(&lp);
                    let rn = to_number_f64(&rp);
                    let a = crate::core::number::to_uint32(ln);
                    let shift = crate::core::number::to_uint32(rn) & 0x1f;
                    let res = a >> shift;
                    Ok(Value::Number(res as f64))
                }
            }
        }
        BinaryOp::NullishCoalescing => {
            // Nullish coalescing: return right if left is null or undefined, otherwise left
            match l {
                Value::Undefined | Value::Null => Ok(r),
                _ => Ok(l),
            }
        }
    }
}

fn abstract_equality(x: &Value, y: &Value, env: &JSObjectDataPtr) -> Result<Value, JSError> {
    // Abstract Equality Comparison (==) with type coercion
    // Based on ECMAScript 2023 specification

    // 1. If Type(x) is the same as Type(y), then return the result of performing Strict Equality Comparison x === y.
    if std::mem::discriminant(x) == std::mem::discriminant(y) {
        return strict_equality(x, y);
    }

    // 2. If x is null and y is undefined, return true.
    if matches!(x, Value::Null) && matches!(y, Value::Undefined) {
        return Ok(Value::Boolean(true));
    }
    // 3. If x is undefined and y is null, return true.
    if matches!(x, Value::Undefined) && matches!(y, Value::Null) {
        return Ok(Value::Boolean(true));
    }

    // 4. If Type(x) is Number and Type(y) is String, return the result of the comparison x == ToNumber(y).
    if let (Value::Number(xn), Value::String(ys)) = (x, y) {
        let yn = string_to_number(ys)?;
        return Ok(Value::Boolean(*xn == yn));
    }

    // 5. If Type(x) is String and Type(y) is Number, return the result of the comparison ToNumber(x) == y.
    if let (Value::String(xs), Value::Number(yn)) = (x, y) {
        let xn = string_to_number(xs)?;
        return Ok(Value::Boolean(xn == *yn));
    }

    // 6. If Type(x) is Boolean, return the result of the comparison ToNumber(x) == y.
    if let Value::Boolean(xb) = x {
        let xn = if *xb { 1.0 } else { 0.0 };
        return abstract_equality(&Value::Number(xn), y, env);
    }

    // 7. If Type(y) is Boolean, return the result of the comparison x == ToNumber(y).
    if let Value::Boolean(yb) = y {
        let yn = if *yb { 1.0 } else { 0.0 };
        return abstract_equality(x, &Value::Number(yn), env);
    }

    // 8. If Type(x) is either String, Number, or Symbol and Type(y) is Object, then return the result of the comparison x == ToPrimitive(y).
    if (matches!(x, Value::String(_) | Value::Number(_) | Value::Symbol(_))) && matches!(y, Value::Object(_)) {
        let py = to_primitive(y, "default", env)?;
        return abstract_equality(x, &py, env);
    }

    // 9. If Type(x) is Object and Type(y) is either String, Number, or Symbol, then return the result of the comparison ToPrimitive(x) == y.
    if matches!(x, Value::Object(_)) && (matches!(y, Value::String(_) | Value::Number(_) | Value::Symbol(_))) {
        let px = to_primitive(x, "default", env)?;
        return abstract_equality(&px, y, env);
    }

    // 10. If Type(x) is BigInt and Type(y) is String, then
    if let (Value::BigInt(xb), Value::String(ys)) = (x, y) {
        // a. Let n be StringToBigInt(y).
        if let Ok(yb) = string_to_bigint(ys) {
            // b. If n is undefined, return false.
            // c. Return the result of the comparison x == n.
            let xb_clone = xb.clone();
            let xb_parsed = xb_clone;
            return Ok(Value::Boolean(xb_parsed == yb));
        } else {
            return Ok(Value::Boolean(false));
        }
    }

    // 11. If Type(x) is String and Type(y) is BigInt, then
    if let (Value::String(xs), Value::BigInt(yb)) = (x, y) {
        if let Ok(xb) = string_to_bigint(xs) {
            return Ok(Value::Boolean(&xb == yb));
        } else {
            return Ok(Value::Boolean(false));
        }
    }

    // 12. If Type(x) is BigInt and Type(y) is Number, or Type(x) is Number and Type(y) is BigInt, then
    if let (Value::BigInt(xb), Value::Number(yn)) = (x, y) {
        let xb_clone = xb.clone();
        let xb_val = xb_clone;
        let yn_val = *yn;
        // a. If y is NaN, +∞, or -∞, return false.
        if yn_val.is_nan() || !yn_val.is_finite() {
            return Ok(Value::Boolean(false));
        }
        // b. If y has a fractional part, return false.
        if yn_val.fract() != 0.0 {
            return Ok(Value::Boolean(false));
        }
        // c. Return the result of the comparison x == y.
        let yn_bi = BigInt::from(yn_val as i64);
        return Ok(Value::Boolean(xb_val == yn_bi));
    }
    if let (Value::Number(xn), Value::BigInt(yb)) = (x, y) {
        let xn_val = *xn;
        // a. If y is NaN, +∞, or -∞, return false.
        if xn_val.is_nan() || !xn_val.is_finite() {
            return Ok(Value::Boolean(false));
        }
        // b. If y has a fractional part, return false.
        if xn_val.fract() != 0.0 {
            return Ok(Value::Boolean(false));
        }
        // c. Return the result of the comparison x == y.
        let xn_bi = BigInt::from(xn_val as i64);
        return Ok(Value::Boolean(&xn_bi == yb));
    }

    // 13. Return false.
    Ok(Value::Boolean(false))
}

fn strict_equality(x: &Value, y: &Value) -> Result<Value, JSError> {
    // Strict Equality Comparison (===)
    match (x, y) {
        (Value::Number(ln), Value::Number(rn)) => Ok(Value::Boolean(ln == rn)),
        (Value::BigInt(la), Value::BigInt(rb)) => Ok(Value::Boolean(la == rb)),
        (Value::String(ls), Value::String(rs)) => Ok(Value::Boolean(ls == rs)),
        (Value::Boolean(lb), Value::Boolean(rb)) => Ok(Value::Boolean(lb == rb)),
        (Value::Symbol(sa), Value::Symbol(sb)) => Ok(Value::Boolean(Rc::ptr_eq(sa, sb))),
        (Value::Undefined, Value::Undefined) => Ok(Value::Boolean(true)),
        (Value::Null, Value::Null) => Ok(Value::Boolean(true)),
        (Value::Object(a), Value::Object(b)) => Ok(Value::Boolean(Rc::ptr_eq(a, b))),
        (Value::Function(sa), Value::Function(sb)) => Ok(Value::Boolean(sa == sb)),
        (Value::Closure(a), Value::Closure(b)) => Ok(Value::Boolean(Rc::ptr_eq(a, b))),
        (Value::AsyncClosure(a), Value::AsyncClosure(b)) => Ok(Value::Boolean(Rc::ptr_eq(a, b))),
        (Value::GeneratorFunction(_, a), Value::GeneratorFunction(_, b)) => Ok(Value::Boolean(Rc::ptr_eq(a, b))),
        _ => Ok(Value::Boolean(false)),
    }
}

fn string_to_number(s: &[u16]) -> Result<f64, JSError> {
    let sstr = utf16_to_utf8(s);
    let t = sstr.trim();
    if t.is_empty() {
        Ok(0.0)
    } else {
        match t.parse::<f64>() {
            Ok(v) => Ok(v),
            Err(_) => Ok(f64::NAN),
        }
    }
}

fn string_to_bigint(s: &[u16]) -> Result<BigInt, JSError> {
    let sstr = utf16_to_utf8(s);
    let t = sstr.trim();
    if t.is_empty() {
        Ok(BigInt::from(0))
    } else {
        BigInt::from_str(t).map_err(|_| raise_eval_error!("Invalid BigInt string"))
    }
}

fn evaluate_index(env: &JSObjectDataPtr, obj: &Expr, idx: &Expr) -> Result<Value, JSError> {
    let obj_val = evaluate_expr(env, obj)?;
    let idx_val = evaluate_expr(env, idx)?;
    log::trace!("evaluate_index: obj_val={obj_val:?} idx_val={idx_val:?}");
    match (obj_val, idx_val) {
        (Value::String(s), Value::Number(n)) => {
            let idx = n as usize;
            if let Some(ch) = utf16_char_at(&s, idx) {
                Ok(Value::String(vec![ch]))
            } else {
                Ok(Value::String(Vec::new())) // or return undefined, but use empty string here
            }
        }
        (Value::Object(object), Value::Number(n)) => {
            // Check if this is a TypedArray first
            if let Some(ta_val) = obj_get_key_value(&object, &"__typedarray".into())?
                && let Value::TypedArray(ta) = &*ta_val.borrow()
            {
                // This is a TypedArray, use our get method
                let idx = n as usize;
                match ta.borrow().get(idx) {
                    Ok(val) => {
                        // Convert the raw value to appropriate JavaScript Value based on type
                        let js_val = match ta.borrow().kind {
                            TypedArrayKind::Float32 | TypedArrayKind::Float64 => {
                                // For float types, we need to reinterpret the i64 as f64
                                // This is a simplified conversion - in practice we'd need proper float handling
                                Value::Number(val as f64)
                            }
                            TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => Value::BigInt(BigInt::from(val)),
                            _ => {
                                // For integer types
                                Value::Number(val as f64)
                            }
                        };
                        return Ok(js_val);
                    }
                    Err(_) => return Err(raise_eval_error!("TypedArray index out of bounds")),
                }
            }
            // Array-like indexing
            let key = PropertyKey::String(n.to_string());
            if let Some(val) = obj_get_key_value(&object, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(object), Value::String(s)) => {
            // Object property access with string key
            let key = PropertyKey::String(utf16_to_utf8(&s));
            if let Some(val) = obj_get_key_value(&object, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(object), Value::Symbol(sym)) => {
            // Object property access with symbol key
            let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
            if let Some(val) = obj_get_key_value(&object, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        // Support indexing into function (constructor) values like RegExp[property]
        (Value::Function(_func_name), Value::Number(_n)) => {
            // Functions do not have numeric-indexed properties in our simple value model
            Ok(Value::Undefined)
        }
        (Value::Function(func_name), Value::String(s)) => {
            // Special-case some function constructors that expose static properties by name.
            // For Symbol constructor, map well-known symbol names (keeps parity with evaluate_property).
            if func_name == "Symbol" {
                return WELL_KNOWN_SYMBOLS.with(|wk| {
                    let map = wk.borrow();
                    if let Some(sym_rc) = map.get(&utf16_to_utf8(&s))
                        && let Value::Symbol(sd) = &*sym_rc.borrow()
                    {
                        Ok(Value::Symbol(sd.clone()))
                    } else {
                        Ok(Value::Undefined)
                    }
                });
            }

            // Other constructor/function names currently don't carry properties in this model.
            Ok(Value::Undefined)
        }
        (Value::Function(_func_name), Value::Symbol(_sym)) => {
            // No symbol-keyed properties available on Function values in the current model
            Ok(Value::Undefined)
        }
        (Value::Object(object), other_idx) => {
            let key_str = value_to_sort_string(&other_idx);
            let key = PropertyKey::String(key_str);
            if let Some(val) = obj_get_key_value(&object, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        _ => Err(raise_eval_error!("Invalid index type")), // other types of indexing not supported yet
    }
}

fn evaluate_property(env: &JSObjectDataPtr, obj: &Expr, prop: &str) -> Result<Value, JSError> {
    let obj_val = evaluate_expr(env, obj)?;
    log::trace!("Property access prop={prop}");
    match obj_val {
        Value::String(s) if prop == "length" => Ok(Value::Number(utf16_len(&s) as f64)),
        // Accessing other properties on string primitives should return undefined
        Value::String(_) => {
            // Force initialization of String constructor if not present
            let string_ctor_val = evaluate_expr(env, &Expr::Var("String".to_string(), None, None))?;
            if let Value::Object(ctor_obj) = string_ctor_val {
                if let Some(proto) = obj_get_key_value(&ctor_obj, &"prototype".into())? {
                    if let Value::Object(proto_obj) = &*proto.borrow() {
                        if let Some(val) = obj_get_key_value(proto_obj, &prop.into())? {
                            return Ok(val.borrow().clone());
                        }
                    }
                }
            }
            Ok(Value::Undefined)
        }
        // Special cases for wrapped Map and Set objects
        Value::Object(object) if prop == "size" && get_own_property(&object, &"__map__".into()).is_some() => {
            if let Some(map_val) = get_own_property(&object, &"__map__".into()) {
                if let Value::Map(map) = &*map_val.borrow() {
                    Ok(Value::Number(map.borrow().entries.len() as f64))
                } else {
                    Ok(Value::Undefined)
                }
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::Object(object) if prop == "size" && get_own_property(&object, &"__set__".into()).is_some() => {
            if let Some(set_val) = get_own_property(&object, &"__set__".into()) {
                if let Value::Set(set) = &*set_val.borrow() {
                    Ok(Value::Number(set.borrow().values.len() as f64))
                } else {
                    Ok(Value::Undefined)
                }
            } else {
                Ok(Value::Undefined)
            }
        }
        // Special cases for wrapped Generator objects
        Value::Object(object)
            if (prop == "next" || prop == "return" || prop == "throw") && get_own_property(&object, &"__generator__".into()).is_some() =>
        {
            Ok(Value::Function(format!("Generator.prototype.{}", prop)))
        }
        // Special cases for DataView objects
        Value::Object(object)
            if (prop == "buffer" || prop == "byteLength" || prop == "byteOffset")
                && get_own_property(&object, &"__dataview".into()).is_some() =>
        {
            if let Some(dv_val) = get_own_property(&object, &"__dataview".into()) {
                if let Value::DataView(dv) = &*dv_val.borrow() {
                    let data_view = dv.borrow();
                    match prop {
                        "buffer" => Ok(Value::ArrayBuffer(data_view.buffer.clone())),
                        "byteLength" => Ok(Value::Number(data_view.byte_length as f64)),
                        "byteOffset" => Ok(Value::Number(data_view.byte_offset as f64)),
                        _ => Ok(Value::Undefined),
                    }
                } else {
                    Ok(Value::Undefined)
                }
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::Object(object) => {
            // Check for private field access
            if prop.starts_with('#') {
                let mut allowed = false;
                let mut current_env = Some(env.clone());
                while let Some(e) = current_env {
                    // First check if this environment has a __home_object__ pointing to the class prototype
                    if let Some(home_obj_val) = crate::core::get_own_property(&e, &"__home_object__".into()) {
                        if let Value::Object(home_obj) = &*home_obj_val.borrow() {
                            if let Some(class_def_val) = crate::core::get_own_property(home_obj, &"__class_def__".into()) {
                                if let Value::ClassDefinition(class_def) = &*class_def_val.borrow() {
                                    if is_private_member_declared(class_def, prop) {
                                        allowed = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    // In some cases the environment itself may carry the class definition (e.g., constructor scope)
                    if let Some(class_def_val) = crate::core::get_own_property(&e, &"__class_def__".into()) {
                        if let Value::ClassDefinition(class_def) = &*class_def_val.borrow() {
                            if is_private_member_declared(class_def, prop) {
                                allowed = true;
                                break;
                            }
                        }
                    }
                    current_env = e.borrow().prototype.clone().and_then(|w| w.upgrade());
                }
                if !allowed {
                    if let Some(ctor_val) = obj_get_key_value(&object, &"constructor".into())? {
                        if let Value::Object(ctor_obj) = &*ctor_val.borrow() {
                            if let Some(class_def_val) = obj_get_key_value(ctor_obj, &"__class_def__".into())? {
                                if let Value::ClassDefinition(cd) = &*class_def_val.borrow() {
                                    if is_private_member_declared(cd, prop) {
                                        allowed = true;
                                    }
                                }
                            }
                        }
                    }
                }
                if !allowed {
                    return Err(raise_syntax_error!(format!(
                        "Private field '{prop}' must be declared in an enclosing class",
                    )));
                }
            }

            // Special-case the `__proto__` accessor so property reads return the
            // object's current prototype object when present.
            if prop == "__proto__" {
                if let Some(proto) = object.borrow().prototype.clone().and_then(|w| w.upgrade()) {
                    return Ok(Value::Object(proto));
                } else {
                    return Ok(Value::Undefined);
                }
            }
            if let Some(val_rc) = obj_get_key_value(&object, &prop.into())? {
                let val = val_rc.borrow();
                match &*val {
                    // If it's a getter stored directly on the object (class-created getter)
                    Value::Getter(body, getter_env, home_opt) => {
                        // Prepare a fresh function env with `this` bound to the instance
                        let func_env =
                            prepare_function_call_env(Some(getter_env), Some(Value::Object(object.clone())), None, &[], None, Some(env))?;
                        if let Some(home) = home_opt {
                            crate::core::obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home.clone()))?;
                        }
                        let result = crate::core::evaluate_statements_with_context(&func_env, body)?;
                        if let crate::core::ControlFlow::Return(ret_val) = result {
                            Ok(ret_val)
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    // Property descriptor with getter/setter
                    Value::Property {
                        value: _value_opt,
                        getter: Some((body, getter_env, home_opt)),
                        ..
                    } => {
                        let func_env =
                            prepare_function_call_env(Some(getter_env), Some(Value::Object(object.clone())), None, &[], None, Some(env))?;
                        if let Some(home) = home_opt {
                            crate::core::obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home.clone()))?;
                        }
                        let result = crate::core::evaluate_statements_with_context(&func_env, body)?;
                        if let crate::core::ControlFlow::Return(ret_val) = result {
                            Ok(ret_val)
                        } else {
                            Ok(Value::Undefined)
                        }
                    }
                    // Data property or other values
                    _ => Ok(val.clone()),
                }
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::Number(n) => crate::js_number::box_number_and_get_property(n, prop, env),
        Value::Symbol(symbol_data) if prop == "description" => match symbol_data.description.as_ref() {
            Some(d) => Ok(Value::String(utf8_to_utf16(d))),
            None => Ok(Value::Undefined),
        },
        Value::GeneratorFunction(name_opt, _) if prop == "name" => {
            if let Some(n) = name_opt {
                Ok(Value::String(utf8_to_utf16(&n)))
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::GeneratorFunction(_name_opt, data) if prop == "length" => Ok(Value::Number(data.params.len() as f64)),
        Value::Function(func_name) => {
            // Special-case static properties on constructors like Symbol.iterator
            if func_name == "Symbol" {
                // Look for well-known symbol by name
                return WELL_KNOWN_SYMBOLS.with(|wk| {
                    let map = wk.borrow();
                    if let Some(sym_rc) = map.get(prop)
                        && let Value::Symbol(sd) = &*sym_rc.borrow()
                    {
                        return Ok(Value::Symbol(sd.clone()));
                    }
                    Err(raise_eval_error!(format!(
                        "Property not found for Symbol constructor property: {prop}"
                    )))
                });
            } else if func_name == "Proxy" && prop == "revocable" {
                return Ok(Value::Function("Proxy.revocable".to_string()));
            }

            // Expose Function.prototype.call and apply as properties on function values
            if prop == "call" {
                return Ok(Value::Function("Function.prototype.call".to_string()));
            }
            if prop == "apply" {
                return Ok(Value::Function("Function.prototype.apply".to_string()));
            }

            Err(raise_eval_error!(format!(
                "Property not found for prop={prop} on function={func_name}",
            )))
        }
        // For boolean and other primitive types, property access should usually
        // coerce to a primitive wrapper or return undefined if not found. To
        // keep things simple, return undefined for boolean properties.
        Value::Boolean(_) => Ok(Value::Undefined),
        Value::Map(map) if prop == "size" => Ok(Value::Number(map.borrow().entries.len() as f64)),
        Value::Set(set) if prop == "size" => Ok(Value::Number(set.borrow().values.len() as f64)),
        _ => Err(raise_eval_error!(format!("Property not found for prop={prop}"))),
    }
}

fn evaluate_optional_property(env: &JSObjectDataPtr, obj: &Expr, prop: &str) -> Result<Value, JSError> {
    let obj_val = evaluate_expr(env, obj)?;
    log::trace!("Optional property access prop={prop}");
    match obj_val {
        Value::Undefined | Value::Null => Ok(Value::Undefined),
        Value::Object(object) => {
            if let Some(val) = obj_get_key_value(&object, &prop.into())? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::String(s) if prop == "length" => Ok(Value::Number(utf16_len(&s) as f64)),
        Value::Symbol(symbol_data) if prop == "description" => match symbol_data.description.as_ref() {
            Some(d) => Ok(Value::String(utf8_to_utf16(d))),
            None => Ok(Value::Undefined),
        },
        Value::Function(func_name) if func_name == "Symbol" && (prop == "iterator" || prop == "toStringTag") => {
            // Expose Symbol.iterator and Symbol.toStringTag via optional property access too
            WELL_KNOWN_SYMBOLS.with(|wk| {
                let map = wk.borrow();
                if let Some(sym_rc) = map.get(prop)
                    && let Value::Symbol(sd) = &*sym_rc.borrow()
                {
                    return Ok(Value::Symbol(sd.clone()));
                }
                Ok(Value::Undefined)
            })
        }
        _ => Err(raise_eval_error!(format!("Property not found for prop={prop}"))),
    }
}

fn evaluate_optional_index(env: &JSObjectDataPtr, obj: &Expr, idx: &Expr) -> Result<Value, JSError> {
    let obj_val = evaluate_expr(env, obj)?;
    // If the base is undefined or null, optional chaining returns undefined
    if matches!(obj_val, Value::Undefined | Value::Null) {
        return Ok(Value::Undefined);
    }

    let idx_val = evaluate_expr(env, idx)?;
    match (obj_val, idx_val) {
        (Value::String(s), Value::Number(n)) => {
            let idx = n as usize;
            if let Some(ch) = utf16_char_at(&s, idx) {
                Ok(Value::String(vec![ch]))
            } else {
                Ok(Value::String(Vec::new()))
            }
        }
        (Value::Object(object), Value::Number(n)) => {
            let key = PropertyKey::String(n.to_string());
            if let Some(val) = obj_get_key_value(&object, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(object), Value::String(s)) => {
            let key = PropertyKey::String(utf16_to_utf8(&s));
            if let Some(val) = obj_get_key_value(&object, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(object), Value::Symbol(sym)) => {
            let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
            if let Some(val) = obj_get_key_value(&object, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Function(func_name), Value::String(s)) => {
            // follow same rules as evaluate_index for function/index access
            if func_name == "Symbol" {
                return WELL_KNOWN_SYMBOLS.with(|wk| {
                    let map = wk.borrow();
                    if let Some(sym_rc) = map.get(&utf16_to_utf8(&s))
                        && let Value::Symbol(sd) = &*sym_rc.borrow()
                    {
                        Ok(Value::Symbol(sd.clone()))
                    } else {
                        Ok(Value::Undefined)
                    }
                });
            }
            Ok(Value::Undefined)
        }
        (Value::Function(_f), Value::Number(_n)) => Ok(Value::Undefined),
        (Value::Function(_f), Value::Symbol(_sym)) => Ok(Value::Undefined),
        (Value::Object(object), other_idx) => {
            let key_str = value_to_sort_string(&other_idx);
            let key = PropertyKey::String(key_str);
            if let Some(val) = obj_get_key_value(&object, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        // If obj isn't undefined and index types aren't supported, propagate as error
        _ => Err(raise_eval_error!("Invalid index type")),
    }
}

pub(crate) fn bind_function_parameters(env: &JSObjectDataPtr, params: &[DestructuringElement], args: &[Value]) -> Result<(), JSError> {
    for (i, param) in params.iter().enumerate() {
        let arg = if i < args.len() { Some(args[i].clone()) } else { None };
        bind_destructuring_element(env, param, arg, args, i)?;
    }
    Ok(())
}

fn bind_destructuring_element(
    env: &JSObjectDataPtr,
    element: &DestructuringElement,
    value: Option<Value>,
    all_args: &[Value],
    current_index: usize,
) -> Result<(), JSError> {
    match element {
        DestructuringElement::Variable(name, default_expr) => {
            let val = if let Some(v) = value {
                if matches!(v, Value::Undefined) && default_expr.is_some() {
                    evaluate_expr(env, default_expr.as_ref().unwrap())?
                } else {
                    v
                }
            } else if let Some(expr) = default_expr {
                evaluate_expr(env, expr)?
            } else {
                Value::Undefined
            };
            // Debug: print parameter binding info to help diagnose missing bindings
            log::trace!("[bind] env_ptr={:p} bind name='{}' val={:?}", Rc::as_ptr(env), name, val);
            env_set(env, name, val)?;
        }
        DestructuringElement::Rest(name) => {
            let rest_args = if current_index < all_args.len() {
                all_args[current_index..].to_vec()
            } else {
                Vec::new()
            };
            let array_obj = crate::js_array::create_array(env)?;
            crate::js_array::set_array_length(&array_obj, rest_args.len())?;
            for (j, arg) in rest_args.into_iter().enumerate() {
                obj_set_key_value(&array_obj, &j.to_string().into(), arg)?;
            }
            env_set(env, name, Value::Object(array_obj))?;
        }
        DestructuringElement::NestedObject(elements) => {
            let val = value.unwrap_or(Value::Undefined);
            if matches!(val, Value::Undefined | Value::Null) {
                return Err(raise_type_error!("Cannot destructure undefined or null"));
            }
            if let Value::Object(obj) = val {
                for el in elements {
                    match el {
                        ObjectDestructuringElement::Property { key, value: target } => {
                            let prop_val = obj_get_key_value(&obj, &key.clone().into())?.map(|v| v.borrow().clone());
                            bind_destructuring_element(env, target, prop_val, &[], 0)?;
                        }
                        ObjectDestructuringElement::Rest(_name) => {
                            // TODO: Implement object rest
                        }
                    }
                }
            }
        }
        DestructuringElement::NestedArray(_elements) => {
            // TODO: Implement array destructuring
        }
        DestructuringElement::Empty => {}
    }
    Ok(())
}

fn evaluate_tagged_template(env: &JSObjectDataPtr, tag: &Expr, strings: &[Vec<u16>], exprs: &[Expr]) -> Result<Value, JSError> {
    let strings_array = crate::js_array::create_array(env)?;
    crate::js_array::set_array_length(&strings_array, strings.len())?;
    let raw_array = crate::js_array::create_array(env)?;
    crate::js_array::set_array_length(&raw_array, strings.len())?;

    for (i, s) in strings.iter().enumerate() {
        let val = Value::String(s.clone());
        obj_set_key_value(&strings_array, &i.to_string().into(), val.clone())?;
        obj_set_key_value(&raw_array, &i.to_string().into(), val)?;
    }
    obj_set_key_value(&strings_array, &"raw".into(), Value::Object(raw_array))?;

    let mut new_args = Vec::new();
    new_args.push(Expr::Value(Value::Object(strings_array)));
    new_args.extend_from_slice(exprs);

    evaluate_call(env, tag, &new_args)
}

fn evaluate_call(env: &JSObjectDataPtr, func_expr: &Expr, args: &[Expr]) -> Result<Value, JSError> {
    log::trace!("evaluate_call entry: args_len={} func_expr=...", args.len());
    if let Expr::Property(_, method) = func_expr {
        log::trace!("evaluate_call property method={}", method);
    } else {
        log::trace!("evaluate_call non-property call");
    }

    // Special case for dynamic import: import("module")
    if let Expr::Var(func_name, _, _) = func_expr
        && func_name == "import"
        && args.len() == 1
    {
        // Evaluate the module name argument
        let module_name_val = evaluate_expr(env, &args[0])?;
        let module_name = match module_name_val {
            Value::String(s) => String::from_utf16(&s).map_err(|_| raise_eval_error!("Invalid module name"))?,
            _ => return Err(raise_eval_error!("Module name must be a string")),
        };

        // Load the module
        let module_value = crate::js_module::load_module(&module_name, None)?;

        // Create a Promise that resolves to the module
        let promise = Rc::new(RefCell::new(JSPromise {
            state: PromiseState::Fulfilled(module_value.clone()),
            value: Some(module_value),
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        }));

        // Wrap the promise in an object with __promise property
        let promise_obj = new_js_object_data();
        obj_set_key_value(&promise_obj, &"__promise".into(), Value::Promise(promise))?;

        return Ok(Value::Object(promise_obj));
    }
    // Check if it's a method call first
    if let Expr::Property(obj_expr, method_name) = func_expr {
        // Special case for Array static methods
        if let Expr::Var(var_name, _, _) = &**obj_expr
            && var_name == "Array"
        {
            return crate::js_array::handle_array_static_method(method_name, args, env);
        }

        // Special case for Date static methods
        if let Expr::Var(var_name, _, _) = &**obj_expr
            && var_name == "Date"
        {
            return crate::js_date::handle_date_static_method(method_name, args, env);
        }

        // Special case for Symbol static methods
        if let Expr::Var(var_name, _, _) = &**obj_expr
            && var_name == "Symbol"
        {
            return handle_symbol_static_method(method_name, args, env);
        }

        // Special case for Proxy static methods
        if let Expr::Var(var_name, _, _) = &**obj_expr
            && var_name == "Proxy"
            && method_name == "revocable"
        {
            return crate::js_proxy::handle_proxy_revocable(args, env);
        }

        let obj_val = evaluate_expr(env, obj_expr)?;
        log::trace!("evaluate_call - object evaluated");
        match (obj_val, method_name.as_str()) {
            (Value::Object(object), "log") if get_own_property(&object, &"log".into()).is_some() => {
                handle_console_method(method_name, args, env)
            }
            // Handle toString/valueOf for primitive Symbol values here (they
            // don't go through the object-path below). For other cases (objects)
            // normal property lookup is used so user overrides take precedence
            // and Object.prototype functions act as fallbacks.
            (Value::Symbol(sd), "toString") => crate::js_object::handle_to_string_method(&Value::Symbol(sd.clone()), args, env),
            (Value::Symbol(sd), "valueOf") => crate::js_object::handle_value_of_method(&Value::Symbol(sd.clone()), args, env),
            (Value::Object(object), method) if get_own_property(&object, &"__map__".into()).is_some() => {
                if let Some(map_val) = get_own_property(&object, &"__map__".into()) {
                    if let Value::Map(map) = &*map_val.borrow() {
                        crate::js_map::handle_map_instance_method(map, method, args, env)
                    } else {
                        Err(raise_eval_error!("Invalid Map object"))
                    }
                } else {
                    Err(raise_eval_error!("Invalid Map object"))
                }
            }

            (Value::Object(object), method) if get_own_property(&object, &"__set__".into()).is_some() => {
                if let Some(set_val) = get_own_property(&object, &"__set__".into()) {
                    if let Value::Set(set) = &*set_val.borrow() {
                        crate::js_set::handle_set_instance_method(set, method, args, env)
                    } else {
                        Err(raise_eval_error!("Invalid Set object"))
                    }
                } else {
                    Err(raise_eval_error!("Invalid Set object"))
                }
            }
            (Value::Map(map), method) => crate::js_map::handle_map_instance_method(&map, method, args, env),
            (Value::Set(set), method) => crate::js_set::handle_set_instance_method(&set, method, args, env),
            (Value::WeakMap(weakmap), method) => crate::js_weakmap::handle_weakmap_instance_method(&weakmap, method, args, env),
            (Value::WeakSet(weakset), method) => crate::js_weakset::handle_weakset_instance_method(&weakset, method, args, env),
            (Value::Generator(generator), method) => crate::js_generator::handle_generator_instance_method(&generator, method, args, env),
            (Value::Object(object), method) if get_own_property(&object, &"__generator__".into()).is_some() => {
                if let Some(gen_val) = get_own_property(&object, &"__generator__".into()) {
                    if let Value::Generator(generator) = &*gen_val.borrow() {
                        crate::js_generator::handle_generator_instance_method(generator, method, args, env)
                    } else {
                        Err(raise_eval_error!("Invalid Generator object"))
                    }
                } else {
                    Err(raise_eval_error!("Invalid Generator object"))
                }
            }
            (Value::Object(object), method) => {
                // Object prototype methods are supplied on `Object.prototype`.
                // Lookups will find user-defined (own) methods before inherited
                // ones, so no evaluator fallback is required here.
                // If this object looks like the `std` module (we used 'sprintf' as marker)
                if get_own_property(&object, &"sprintf".into()).is_some() {
                    match method {
                        "sprintf" => {
                            log::trace!("js dispatch calling sprintf with {} args", args.len());
                            return handle_sprintf_call(env, args);
                        }
                        "tmpfile" => {
                            return create_tmpfile();
                        }
                        _ => {}
                    }
                }

                // If this object looks like the `os` module (we used 'open' as marker)
                if get_own_property(&object, &"open".into()).is_some() {
                    return crate::js_os::handle_os_method(&object, method, args, env);
                }

                // If this object looks like the `os.path` module
                if get_own_property(&object, &"join".into()).is_some() {
                    return crate::js_os::handle_os_method(&object, method, args, env);
                }

                // If this object is a file-like object (we use '__file_id' as marker)
                if get_own_property(&object, &"__file_id".into()).is_some() {
                    return handle_file_method(&object, method, args, env);
                }
                // Check if this is the Math object
                if get_own_property(&object, &"PI".into()).is_some() && get_own_property(&object, &"E".into()).is_some() {
                    crate::js_math::handle_math_method(method, args, env)
                // Detect Atomics object (basic ops)
                } else if get_own_property(&object, &"load".into()).is_some() && get_own_property(&object, &"store".into()).is_some() {
                    crate::js_typedarray::handle_atomics_method(method, args, env)
                } else if get_own_property(&object, &"apply".into()).is_some() && get_own_property(&object, &"construct".into()).is_some() {
                    crate::js_reflect::handle_reflect_method(method, args, env)
                } else if get_own_property(&object, &"parse".into()).is_some() && get_own_property(&object, &"stringify".into()).is_some() {
                    crate::js_json::handle_json_method(method, args, env)
                } else if get_own_property(&object, &"keys".into()).is_some() && get_own_property(&object, &"values".into()).is_some() {
                    crate::js_object::handle_object_method(method, args, env)
                } else if get_own_property(&object, &"__arraybuffer".into()).is_some() {
                    if get_own_property(&object, &"__sharedarraybuffer".into()).is_some() {
                        crate::js_typedarray::handle_sharedarraybuffer_constructor(args, env)
                    } else {
                        crate::js_typedarray::handle_arraybuffer_constructor(args, env)
                    }
                } else if get_own_property(&object, &"MAX_VALUE".into()).is_some()
                    && get_own_property(&object, &"MIN_VALUE".into()).is_some()
                {
                    crate::js_number::handle_number_method(method, args, env)
                } else if get_own_property(&object, &"__is_bigint_constructor".into()).is_some() {
                    crate::js_bigint::handle_bigint_static_method(method, args, env)
                } else if get_own_property(&object, &"__value__".into()).is_some() {
                    // Dispatch boxed primitive object methods based on the actual __value__ type
                    if let Some(val_rc) = obj_get_key_value(&object, &"__value__".into())? {
                        match &*val_rc.borrow() {
                            Value::Number(_) => crate::js_number::handle_number_object_method(&object, method, args, env),
                            Value::BigInt(_) => crate::js_bigint::handle_bigint_object_method(&object, method, args, env),
                            Value::String(s) => crate::js_string::handle_string_method(s, method, args, env),
                            Value::Boolean(b) => match method {
                                "toString" => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                                "valueOf" => Ok(Value::Boolean(*b)),
                                _ => Err(raise_eval_error!(format!("Boolean.prototype.{method} is not implemented"))),
                            },
                            Value::Symbol(s) => match method {
                                "toString" => Ok(Value::String(utf8_to_utf16(&format!(
                                    "Symbol({})",
                                    s.description.as_deref().unwrap_or("")
                                )))),
                                "valueOf" => Ok(Value::Symbol(s.clone())),
                                _ => Err(raise_eval_error!(format!("Symbol.prototype.{method} is not implemented"))),
                            },
                            _ => Err(raise_eval_error!("Invalid __value__ for boxed object")),
                        }
                    } else {
                        Err(raise_eval_error!("__value__ not found on instance"))
                    }
                } else if is_date_object(&object) {
                    // Date instance methods
                    crate::js_date::handle_date_method(&object, method, args, env)
                } else if is_regex_object(&object) {
                    // RegExp instance methods
                    crate::js_regexp::handle_regexp_method(&object, method, args, env)
                } else if is_array(&object) {
                    // Array instance methods
                    crate::js_array::handle_array_instance_method(&object, method, args, env)
                } else if get_own_property(&object, &"__promise".into()).is_some() {
                    // Promise instance methods
                    handle_promise_method(&object, method, args, env)
                } else if get_own_property(&object, &"__dataview".into()).is_some() {
                    // DataView instance methods
                    crate::js_typedarray::handle_dataview_method(&object, method, args, env)
                } else if get_own_property(&object, &"testWithIntlConstructors".into()).is_some() {
                    crate::js_testintl::handle_testintl_method(method, args, env)
                } else if get_own_property(&object, &"__locale".into()).is_some() && method == "resolvedOptions" {
                    // Handle resolvedOptions method on mock Intl instances
                    crate::js_testintl::handle_resolved_options(&object)
                }
                // If object has a user-defined property that's a callable, delegate to helper
                else if (obj_get_key_value(&object, &method.into())?).is_some() {
                    handle_user_defined_method_on_instance(&object, method, args, env)
                } else if get_own_property(&object, &"__class_def__".into()).is_some() {
                    // Class static methods
                    call_static_method(&object, method, args, env)
                } else if get_own_property(&object, &"sameValue".into()).is_some() {
                    crate::js_assert::handle_assert_method(method, args, env)
                } else if is_array(&object) {
                    // Class static methods
                    call_static_method(&object, method, args, env)
                } else if is_class_instance(&object)? {
                    call_class_method(&object, method, args, env)
                } else {
                    // Check for user-defined method
                    if let Some(prop_val) = obj_get_key_value(&object, &method.into())? {
                        match prop_val.borrow().clone() {
                            Value::Closure(data) | Value::AsyncClosure(data) => {
                                let params = &data.params;
                                let body = &data.body;
                                let captured_env = &data.env;
                                let home_obj_opt = data.home_object.borrow().clone();
                                // Function call
                                // Collect all arguments, expanding spreads
                                let mut evaluated_args = Vec::new();
                                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                                // Create new environment starting with captured environment
                                let this_val = data.bound_this.clone().unwrap_or(Value::Object(object.clone()));
                                let func_env = prepare_function_call_env(
                                    Some(captured_env),
                                    Some(this_val),
                                    Some(params),
                                    &evaluated_args,
                                    Some(&build_frame_name(env, method)),
                                    Some(env),
                                )?;
                                if let Some(home_weak) = home_obj_opt {
                                    if let Some(home_rc) = home_weak.upgrade() {
                                        log::trace!("DEBUG: Setting __home_object__ in evaluate_call (generic method)");
                                        obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home_rc.clone()))?;
                                    } else {
                                        log::trace!("DEBUG: home_obj weak upgrade failed in evaluate_call (generic method)");
                                    }
                                } else {
                                    log::trace!("DEBUG: home_obj is None in evaluate_call (generic method)");
                                }
                                // Execute function body
                                evaluate_statements(&func_env, body)
                            }
                            Value::Function(func_name) => {
                                // Special-case Object.prototype.* built-ins so they can
                                // operate on the receiver (`this`), which is the
                                // object we fetched the method from (object).
                                // Also handle boxed-primitive built-ins that are
                                // represented as `Value::Function("BigInt_toString")`,
                                // etc., so they can access the receiver's `__value__`.
                                if let Some(v) = crate::js_function::handle_receiver_builtin(&func_name, &object, args, env)? {
                                    return Ok(v);
                                }
                                if func_name.starts_with("Object.prototype.") || func_name == "Error.prototype.toString" {
                                    if let Some(v) = crate::js_object::handle_object_prototype_builtin(&func_name, &object, args, env)? {
                                        return Ok(v);
                                    }
                                    if func_name == "Error.prototype.toString" {
                                        return crate::js_object::handle_error_to_string_method(&Value::Object(object.clone()), args);
                                    }
                                    // Fall back to global handler
                                    crate::js_function::handle_global_function(&func_name, args, env)
                                } else if func_name.starts_with("Function.prototype.") {
                                    // Call Function.prototype.* handlers with the receiver bound as `this`.
                                    let call_env = prepare_function_call_env(
                                        Some(env),
                                        Some(Value::Object(object.clone())),
                                        None,
                                        &[],
                                        None,
                                        Some(env),
                                    )?;
                                    crate::js_function::handle_global_function(&func_name, args, &call_env)
                                } else {
                                    crate::js_function::handle_global_function(&func_name, args, env)
                                }
                            }
                            Value::Object(func_obj_map) => {
                                // Support function-objects stored as properties (they
                                // wrap an internal `__closure__`). Invoke the
                                // internal closure with `this` bound to the
                                // receiver object (`object`). This allows
                                // assignments like `MyError.prototype.toString = function() { ... }`
                                // to be callable as methods.
                                if let Some(cl_rc) = obj_get_key_value(&func_obj_map, &"__closure__".into())? {
                                    match &*cl_rc.borrow() {
                                        Value::Closure(data) => {
                                            let params = &data.params;
                                            let body = &data.body;
                                            let captured_env = &data.env;
                                            let home_obj_opt = data.home_object.borrow().clone();
                                            // Collect all arguments, expanding spreads
                                            let mut evaluated_args = Vec::new();
                                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                                            // Create new environment starting with captured environment (fresh frame) and bind `this` directly
                                            let func_env = prepare_function_call_env(
                                                Some(captured_env),
                                                Some(Value::Object(object.clone())),
                                                Some(params),
                                                &evaluated_args,
                                                Some(&build_frame_name(env, method)),
                                                Some(env),
                                            )?;
                                            if let Some(home_weak) = home_obj_opt {
                                                if let Some(home_rc) = home_weak.upgrade() {
                                                    obj_set_key_value(
                                                        &func_env,
                                                        &"__home_object__".into(),
                                                        Value::Object(home_rc.clone()),
                                                    )?;
                                                }
                                            }

                                            // Create arguments object
                                            let arguments_obj = create_array(&func_env)?;
                                            set_array_length(&arguments_obj, evaluated_args.len())?;
                                            for (i, arg) in evaluated_args.iter().enumerate() {
                                                obj_set_key_value(&arguments_obj, &i.to_string().into(), arg.clone())?;
                                            }
                                            obj_set_key_value(&func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                                            // Execute function body
                                            match evaluate_statements_with_context(&func_env, body)? {
                                                ControlFlow::Normal(_) => Ok(Value::Undefined),
                                                ControlFlow::Return(val) => Ok(val),
                                                ControlFlow::Break(_) => Err(raise_eval_error!("break statement not in loop or switch")),
                                                ControlFlow::Continue(_) => Err(raise_eval_error!("continue statement not in loop")),
                                            }
                                        }
                                        Value::GeneratorFunction(_, data) => {
                                            // Generator method-style call - return a generator object
                                            let params = &data.params;
                                            let body = &data.body;
                                            let captured_env = &data.env;
                                            let home_obj = &data.home_object;
                                            let mut evaluated_args = Vec::new();
                                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                                            let func_env = prepare_function_call_env(
                                                Some(captured_env),
                                                Some(Value::Object(object.clone())),
                                                None,
                                                &evaluated_args,
                                                Some(&build_frame_name(env, method)),
                                                Some(env),
                                            )?;
                                            if let Some(home_weak) = &*home_obj.borrow() {
                                                if let Some(home_rc) = home_weak.upgrade() {
                                                    obj_set_key_value(
                                                        &func_env,
                                                        &"__home_object__".into(),
                                                        Value::Object(home_rc.clone()),
                                                    )?;
                                                }
                                            }
                                            // `this` is bound via prepare_function_call_env
                                            crate::js_generator::handle_generator_function_call(params, body, args, &func_env)
                                        }
                                        Value::AsyncClosure(data) => {
                                            let params = &data.params;
                                            let body = &data.body;
                                            let captured_env = &data.env;
                                            let home_obj_opt = data.home_object.borrow().clone();
                                            // Async method-style call: returns a Promise object
                                            let mut evaluated_args = Vec::new();
                                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                                            // Create a Promise object
                                            let promise = Rc::new(RefCell::new(JSPromise::default()));
                                            let promise_obj = Value::Object(new_js_object_data());
                                            if let Value::Object(obj) = &promise_obj {
                                                obj.borrow_mut()
                                                    .insert("__promise".into(), Rc::new(RefCell::new(Value::Promise(promise.clone()))));
                                            }
                                            // Create new environment and bind `this` directly
                                            let func_env = prepare_function_call_env(
                                                Some(captured_env),
                                                Some(Value::Object(object.clone())),
                                                Some(params),
                                                &evaluated_args,
                                                Some(&build_frame_name(env, method)),
                                                Some(env),
                                            )?;
                                            if let Some(home_weak) = home_obj_opt {
                                                if let Some(home_rc) = home_weak.upgrade() {
                                                    obj_set_key_value(
                                                        &func_env,
                                                        &"__home_object__".into(),
                                                        Value::Object(home_rc.clone()),
                                                    )?;
                                                }
                                            }
                                            func_env.borrow_mut().is_function_scope = true;

                                            // Execute function body synchronously (for now)
                                            let result = evaluate_statements(&func_env, body);
                                            match result {
                                                Ok(val) => crate::js_promise::resolve_promise(&promise, val),
                                                Err(e) => {
                                                    // If the error represents a thrown JS value,
                                                    // reject the promise with that original JS
                                                    // value so script-level handlers see the
                                                    // same object/type as intended.
                                                    match e.kind() {
                                                        crate::JSErrorKind::Throw { value } => {
                                                            crate::js_promise::reject_promise(&promise, value.clone());
                                                        }
                                                        _ => {
                                                            crate::js_promise::reject_promise(
                                                                &promise,
                                                                Value::String(utf8_to_utf16(&format!("{}", e))),
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                            Ok(promise_obj)
                                        }
                                        _ => Err(raise_eval_error!(format!("Property '{method}' is not a function"))),
                                    }
                                } else {
                                    Err(raise_eval_error!(format!("Property '{method}' is not a function")))
                                }
                            }
                            _ => Err(raise_eval_error!(format!("Property '{method}' is not a function"))),
                        }
                    } else {
                        Err(raise_eval_error!(format!("Method {method} not found on object")))
                    }
                }
            }
            // Allow function values and closures to support `.call` and `.apply` forwarding
            (Value::Closure(data), "call") => {
                // Delegate to Function.prototype.call with closure as the target
                let call_env = prepare_function_call_env(Some(env), Some(Value::Closure(data.clone())), None, &[], None, Some(env))?;
                crate::js_function::handle_global_function("Function.prototype.call", args, &call_env)
            }
            (Value::Closure(data), "apply") => {
                // Delegate to Function.prototype.apply with closure as the target
                let call_env = prepare_function_call_env(Some(env), Some(Value::Closure(data.clone())), None, &[], None, Some(env))?;
                crate::js_function::handle_global_function("Function.prototype.apply", args, &call_env)
            }
            (Value::Closure(data), "bind") => {
                if args.is_empty() {
                    return Err(raise_eval_error!("bind requires at least one argument"));
                }
                let bound_this = evaluate_expr(env, &args[0])?;
                let new_data = ClosureData {
                    params: data.params.clone(),
                    body: data.body.clone(),
                    env: data.env.clone(),
                    home_object: data.home_object.clone(),
                    captured_envs: data.captured_envs.clone(),
                    bound_this: Some(bound_this),
                };
                Ok(Value::Closure(Rc::new(new_data)))
            }
            (Value::Function(func_name), "call") => {
                // Delegate to Function.prototype.call
                let call_env = prepare_function_call_env(Some(env), Some(Value::Function(func_name.clone())), None, &[], None, Some(env))?;
                crate::js_function::handle_global_function("Function.prototype.call", args, &call_env)
            }
            (Value::Function(func_name), "apply") => {
                // Delegate to Function.prototype.apply
                let call_env = prepare_function_call_env(Some(env), Some(Value::Function(func_name.clone())), None, &[], None, Some(env))?;
                crate::js_function::handle_global_function("Function.prototype.apply", args, &call_env)
            }
            (Value::Function(func_name), "bind") => {
                // Delegate to Function.prototype.bind
                let call_env = prepare_function_call_env(Some(env), Some(Value::Function(func_name.clone())), None, &[], None, Some(env))?;
                crate::js_function::handle_global_function("Function.prototype.bind", args, &call_env)
            }
            (Value::Function(func_name), method) => {
                // Handle constructor static methods
                match func_name.as_str() {
                    "Object" => crate::js_object::handle_object_method(method, args, env),
                    "Array" => crate::js_array::handle_array_static_method(method, args, env),
                    "Promise" => crate::js_promise::handle_promise_static_method(method, args, env),
                    "Date" => crate::js_date::handle_date_static_method(method, args, env),
                    "BigInt" => crate::js_bigint::handle_bigint_static_method(method, args, env),
                    "MockIntlConstructor" => crate::js_testintl::handle_mock_intl_static_method(method, args, env),
                    _ => Err(raise_eval_error!(format!("{func_name} has no static method '{method}'"))),
                }
            }
            (Value::String(s), method) => crate::js_string::handle_string_method(&s, method, args, env),
            (Value::Number(n), method) => crate::js_number::handle_number_instance_method(&n, method, args, env),
            _ => Err(raise_eval_error!("error")),
        }
    } else if let Expr::OptionalProperty(obj_expr, method_name) = func_expr {
        // Optional method call
        let obj_val = evaluate_expr(env, obj_expr)?;
        match obj_val {
            Value::Undefined | Value::Null => Ok(Value::Undefined),
            Value::Object(object) => handle_optional_method_call(&object, method_name, args, env),
            Value::Function(func_name) => {
                // Handle constructor static methods
                match func_name.as_str() {
                    "Object" => crate::js_object::handle_object_method(method_name, args, env),
                    "Array" => crate::js_array::handle_array_static_method(method_name, args, env),
                    "Promise" => crate::js_promise::handle_promise_static_method(method_name, args, env),
                    "BigInt" => crate::js_bigint::handle_bigint_static_method(method_name, args, env),
                    _ => Err(raise_eval_error!(format!("{func_name} has no static method '{method_name}'"))),
                }
            }
            Value::String(s) => crate::js_string::handle_string_method(&s, method_name, args, env),
            Value::Number(n) => crate::js_number::handle_number_instance_method(&n, method_name, args, env),
            _ => Err(raise_eval_error!("error")),
        }
    } else {
        // Regular function call
        let func_val = evaluate_expr(env, func_expr)?;
        match func_val {
            Value::Proxy(proxy) => {
                // Special case: calling a proxy directly (assumed to be revoke function)
                proxy.borrow_mut().revoked = true;
                Ok(Value::Undefined)
            }
            Value::Function(func_name) => crate::js_function::handle_global_function(&func_name, args, env),
            Value::GeneratorFunction(_, data) => {
                // Generator function call - return a generator object
                crate::js_generator::handle_generator_function_call(&data.params, &data.body, args, &data.env)
            }
            Value::Object(object) if get_own_property(&object, &"__closure__".into()).is_some() => {
                // Function object call - extract the closure and call it
                if let Some(cl_rc) = obj_get_key_value(&object, &"__closure__".into())? {
                    match &*cl_rc.borrow() {
                        Value::AsyncClosure(data) => {
                            let params = &data.params;
                            let body = &data.body;
                            let captured_env = &data.env;
                            // Async function call (direct call on a function-object): returns a Promise
                            let mut evaluated_args = Vec::new();
                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                            // Create a Promise object
                            let promise = Rc::new(RefCell::new(JSPromise::default()));
                            let promise_obj = Value::Object(new_js_object_data());
                            if let Value::Object(obj) = &promise_obj {
                                obj.borrow_mut()
                                    .insert("__promise".into(), Rc::new(RefCell::new(Value::Promise(promise.clone()))));
                            }
                            // Create new environment
                            let func_env = prepare_function_call_env(
                                Some(captured_env),
                                Some(Value::Undefined),
                                Some(params),
                                &evaluated_args,
                                None,
                                Some(env),
                            )?;
                            // Execute function body and resolve/reject promise
                            let result = evaluate_statements(&func_env, body);
                            match result {
                                Ok(val) => crate::js_promise::resolve_promise(&promise, val),
                                Err(e) => match e.kind() {
                                    crate::JSErrorKind::Throw { value } => {
                                        crate::js_promise::reject_promise(&promise, value.clone());
                                    }
                                    _ => {
                                        crate::js_promise::reject_promise(&promise, Value::String(utf8_to_utf16(&format!("{}", e))));
                                    }
                                },
                            }
                            Ok(promise_obj)
                        }
                        Value::Closure(data) => {
                            let params = &data.params;
                            let body = &data.body;
                            let captured_env = &data.env;
                            log::trace!(
                                "[call] invoking closure - func_obj_ptr={:p} captured_env_ptr={:p} caller_env_ptr={:p}",
                                Rc::as_ptr(&object),
                                Rc::as_ptr(captured_env),
                                Rc::as_ptr(env)
                            );
                            // Function call
                            // Collect all arguments, expanding spreads
                            let mut evaluated_args = Vec::new();
                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                            // Create new environment starting with captured environment (fresh frame)
                            let frame_name = if let Expr::Var(name, _, _) = func_expr {
                                name.clone()
                            } else if let Ok(Some(name_rc)) = obj_get_key_value(captured_env, &"name".into()) {
                                if let Value::String(s) = &*name_rc.borrow() {
                                    utf16_to_utf8(s)
                                } else {
                                    "<anonymous>".to_string()
                                }
                            } else {
                                "<anonymous>".to_string()
                            };
                            // Attempt to use the function's recorded declaration site (if present) for clearer frames,
                            // falling back to build_frame_name if no useful decl site is available.
                            let mut frame = build_frame_name(env, &frame_name);
                            // The closure data is available via `cl_rc` (we matched it above), so use its decl site if present.
                            if let Value::Closure(data) | Value::AsyncClosure(data) = &*cl_rc.borrow() {
                                if !data.body.is_empty() {
                                    let decl_line = data.body[0].line;
                                    let decl_col = data.body[0].column;
                                    let mut script_name = "<script>".to_string();
                                    if let Ok(Some(sn_rc)) = obj_get_key_value(&data.env, &"__script_name".into()) {
                                        if let Value::String(s) = &*sn_rc.borrow() {
                                            script_name = utf16_to_utf8(s);
                                        }
                                    }
                                    frame = format!("{} ({}:{}:{})", frame_name, script_name, decl_line, decl_col);
                                }
                            }

                            let func_env = prepare_function_call_env(
                                Some(captured_env),
                                Some(Value::Undefined),
                                Some(params),
                                &evaluated_args,
                                Some(&frame),
                                Some(env),
                            )?;

                            // Create arguments object
                            let arguments_obj = create_array(&func_env)?;
                            set_array_length(&arguments_obj, evaluated_args.len())?;
                            for (i, arg) in evaluated_args.iter().enumerate() {
                                obj_set_key_value(&arguments_obj, &i.to_string().into(), arg.clone())?;
                            }
                            obj_set_key_value(&func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                            // Execute function body
                            match evaluate_statements_with_context(&func_env, body)? {
                                ControlFlow::Normal(_) => Ok(Value::Undefined),
                                ControlFlow::Return(val) => Ok(val),
                                ControlFlow::Break(_) => Err(raise_eval_error!("break statement not in loop or switch")),
                                ControlFlow::Continue(_) => Err(raise_eval_error!("continue statement not in loop")),
                            }
                        }

                        Value::GeneratorFunction(_, data) => {
                            // Generator function call - return a generator object
                            crate::js_generator::handle_generator_function_call(&data.params, &data.body, args, &data.env)
                        }
                        _ => Err(raise_eval_error!("Object is not callable")),
                    }
                } else {
                    Err(raise_eval_error!("Object is not callable"))
                }
            }
            Value::Object(object)
                if obj_get_key_value(&object, &"__is_error_constructor".into())
                    .ok()
                    .flatten()
                    .is_some() =>
            {
                crate::js_class::evaluate_new(env, func_expr, args)
            }
            Value::Closure(data) => {
                let params = &data.params;
                let body = &data.body;
                let captured_env = &data.env;
                // Function call
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                // Prepare frame name and environment for a direct closure call (this = undefined)
                let frame_name = if let Ok(Some(name_rc)) = obj_get_key_value(captured_env, &"name".into()) {
                    if let Value::String(s) = &*name_rc.borrow() {
                        utf16_to_utf8(s)
                    } else {
                        "<anonymous>".to_string()
                    }
                } else {
                    "<anonymous>".to_string()
                };
                // Attempt to use the function's recorded declaration site (if present) for clearer frames,
                // falling back to the first statement's location in the body if necessary.
                let mut frame = build_frame_name(env, &frame_name);

                if !data.body.is_empty() {
                    let decl_line = data.body[0].line;
                    let decl_col = data.body[0].column;
                    // Try to fetch script name from the captured environment
                    let mut script_name = "<script>".to_string();
                    if let Ok(Some(sn_rc)) = obj_get_key_value(&data.env, &"__script_name".into()) {
                        if let Value::String(s) = &*sn_rc.borrow() {
                            script_name = utf16_to_utf8(s);
                        }
                    }
                    frame = format!("{} ({}:{}:{})", frame_name, script_name, decl_line, decl_col);
                }
                let this_val = data.bound_this.clone().unwrap_or(Value::Undefined);
                let func_env = prepare_function_call_env(
                    Some(captured_env),
                    Some(this_val),
                    Some(params),
                    &evaluated_args,
                    Some(&frame),
                    Some(env),
                )?;
                // Execute function body
                match evaluate_statements_with_context(&func_env, body)? {
                    ControlFlow::Normal(_) => Ok(Value::Undefined),
                    ControlFlow::Return(val) => Ok(val),
                    ControlFlow::Break(_) => Err(raise_eval_error!("break statement not in loop or switch")),
                    ControlFlow::Continue(_) => Err(raise_eval_error!("continue statement not in loop")),
                }
            }
            Value::AsyncClosure(data) => {
                let params = &data.params;
                let body = &data.body;
                let captured_env = &data.env;
                // Function call
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                // Create a Promise object
                let promise = Rc::new(RefCell::new(JSPromise::default()));
                let promise_obj = Value::Object(new_js_object_data());
                if let Value::Object(obj) = &promise_obj {
                    obj.borrow_mut()
                        .insert("__promise".into(), Rc::new(RefCell::new(Value::Promise(promise.clone()))));
                }
                // Prepare function call environment for async closure invocation (arrow closures inherit `this` lexically)
                let func_env = prepare_function_call_env(Some(captured_env), None, Some(params), &evaluated_args, None, None)?;
                // Execute function body synchronously (for now)
                let result = evaluate_statements(&func_env, body);
                match result {
                    Ok(val) => {
                        crate::js_promise::resolve_promise(&promise, val);
                    }
                    Err(e) => match e.kind() {
                        crate::JSErrorKind::Throw { value } => {
                            crate::js_promise::reject_promise(&promise, value.clone());
                        }
                        _ => {
                            crate::js_promise::reject_promise(&promise, Value::String(utf8_to_utf16(&format!("{}", e))));
                        }
                    },
                }
                Ok(promise_obj)
            }
            Value::Object(object) => {
                // Check if this is a class constructor being called without 'new'
                if get_own_property(&object, &"__class_def__".into()).is_some() {
                    let name = if let Ok(Some(n)) = obj_get_key_value(&object, &"name".into()) {
                        if let Value::String(s) = &*n.borrow() {
                            utf16_to_utf8(s)
                        } else {
                            "Unknown".to_string()
                        }
                    } else {
                        "Unknown".to_string()
                    };
                    return Err(raise_type_error!(format!(
                        "Class constructor {} cannot be invoked without 'new'",
                        name
                    )));
                }

                // If this object wraps a closure under the internal `__closure__` key,
                // call that closure. This lets script-defined functions be stored
                // as objects (so they have assignable `prototype`), while still
                // being callable.
                if let Some(cl_rc) = obj_get_key_value(&object, &"__closure__".into())? {
                    match &*cl_rc.borrow() {
                        Value::Closure(data) | Value::AsyncClosure(data) => {
                            let params = &data.params;
                            let body = &data.body;
                            let captured_env = &data.env;
                            let home_obj_opt = data.home_object.borrow().clone();
                            // Collect all arguments, expanding spreads
                            let mut evaluated_args = Vec::new();
                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                            // Create frame and prepare environment (this = undefined)
                            let frame_name = if let Ok(Some(nrc)) = obj_get_key_value(&object, &"name".into()) {
                                if let Value::String(s) = &*nrc.borrow() {
                                    utf16_to_utf8(s)
                                } else {
                                    "<anonymous>".to_string()
                                }
                            } else {
                                "<anonymous>".to_string()
                            };
                            let frame = build_frame_name(env, &frame_name);
                            let func_env = prepare_function_call_env(
                                Some(captured_env),
                                Some(Value::Undefined),
                                Some(params),
                                &evaluated_args,
                                Some(&frame),
                                Some(env),
                            )?;
                            if let Some(home_weak) = home_obj_opt {
                                if let Some(home_rc) = home_weak.upgrade() {
                                    obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home_rc.clone()))?;
                                }
                            }
                            // Execute function body
                            return evaluate_statements(&func_env, body);
                        }
                        _ => {}
                    }
                }
                // Object constructor handler. This ensures that calling the
                // constructor object (e.g. `Object(123n)`) behaves like a
                // constructor instead of attempting to call the object as a
                // plain callable value.
                let mut root_env_opt = Some(env.clone());
                while let Some(r) = root_env_opt.clone() {
                    if let Some(parent) = r.borrow().prototype.clone().and_then(|w| w.upgrade()) {
                        root_env_opt = Some(parent);
                    } else {
                        break;
                    }
                }
                if let Some(root_env) = root_env_opt
                    && let Some(obj_ctor_rc) = obj_get_key_value(&root_env, &"Object".into())?
                    && let Value::Object(ctor_map) = &*obj_ctor_rc.borrow()
                    && Rc::ptr_eq(ctor_map, &object)
                {
                    return crate::js_class::handle_object_constructor(args, env);
                }

                // Check if this is a built-in constructor object (Number)
                if get_own_property(&object, &"MAX_VALUE".into()).is_some() && get_own_property(&object, &"MIN_VALUE".into()).is_some() {
                    // Number constructor call
                    crate::js_function::handle_global_function("Number", args, env)
                } else if get_own_property(&object, &"__arraybuffer".into()).is_some() {
                    // ArrayBuffer / SharedArrayBuffer constructor call
                    if get_own_property(&object, &"__sharedarraybuffer".into()).is_some() {
                        crate::js_typedarray::handle_sharedarraybuffer_constructor(args, env)
                    } else {
                        crate::js_typedarray::handle_arraybuffer_constructor(args, env)
                    }
                } else if get_own_property(&object, &"__is_string_constructor".into()).is_some() {
                    crate::js_function::handle_global_function("String", args, env)
                } else if get_own_property(&object, &"__is_boolean_constructor".into()).is_some() {
                    crate::js_function::handle_global_function("Boolean", args, env)
                } else if get_own_property(&object, &"__is_bigint_constructor".into()).is_some() {
                    // BigInt constructor-like object: handle conversion via global function
                    crate::js_function::handle_global_function("BigInt", args, env)
                } else if get_own_property(&object, &"__is_function_constructor".into()).is_some() {
                    crate::js_function::handle_global_function("Function", args, env)
                } else if get_own_property(&object, &"__is_array_constructor".into()).is_some() {
                    crate::js_function::handle_global_function("Array", args, env)
                } else {
                    // Log diagnostic context before returning a generic evaluation error
                    log::error!("evaluate_call - unexpected object method dispatch: object={:?}", object);
                    Err(raise_eval_error!("error"))
                }
            }
            _ => Err(raise_eval_error!("error")),
        }
    }
}

fn evaluate_optional_call(env: &JSObjectDataPtr, func_expr: &Expr, args: &[Expr]) -> Result<Value, JSError> {
    log::trace!("evaluate_optional_call entry: args_len={} func_expr=...", args.len());
    // Check if it's a method call first
    if let Expr::Property(obj_expr, method_name) = func_expr {
        // Special case for Array static methods
        if let Expr::Var(var_name, _, _) = &**obj_expr
            && var_name == "Array"
        {
            return crate::js_array::handle_array_static_method(method_name, args, env);
        }

        let obj_val = evaluate_expr(env, obj_expr)?;
        log::trace!("evaluate_optional_call - object eval result: {obj_val:?}");
        match obj_val {
            Value::Undefined | Value::Null => Ok(Value::Undefined),
            Value::Object(object) => {
                // If this object looks like the `std` module (we used 'sprintf' as marker)
                if get_own_property(&object, &"sprintf".into()).is_some() {
                    match method_name.as_str() {
                        "sprintf" => {
                            log::trace!("js dispatch calling sprintf with {} args", args.len());
                            return handle_sprintf_call(env, args);
                        }
                        "tmpfile" => {
                            return create_tmpfile();
                        }
                        _ => {}
                    }
                }

                // If this object looks like the `os` module (we used 'open' as marker)
                if get_own_property(&object, &"open".into()).is_some() {
                    return crate::js_os::handle_os_method(&object, method_name, args, env);
                }

                // If this object looks like the `os.path` module
                if get_own_property(&object, &"join".into()).is_some() {
                    return crate::js_os::handle_os_method(&object, method_name, args, env);
                }

                // If this object is a file-like object (we use '__file_id' as marker)
                if get_own_property(&object, &"__file_id".into()).is_some() {
                    return handle_file_method(&object, method_name, args, env);
                }
                // Check if this is the Math object
                if get_own_property(&object, &"PI".into()).is_some() && get_own_property(&object, &"E".into()).is_some() {
                    crate::js_math::handle_math_method(method_name, args, env)
                // Detect Atomics object
                } else if get_own_property(&object, &"load".into()).is_some() && get_own_property(&object, &"store".into()).is_some() {
                    crate::js_typedarray::handle_atomics_method(method_name, args, env)
                } else if get_own_property(&object, &"apply".into()).is_some() && get_own_property(&object, &"construct".into()).is_some() {
                    crate::js_reflect::handle_reflect_method(method_name, args, env)
                } else if get_own_property(&object, &"parse".into()).is_some() && get_own_property(&object, &"stringify".into()).is_some() {
                    crate::js_json::handle_json_method(method_name, args, env)
                } else if get_own_property(&object, &"keys".into()).is_some() && get_own_property(&object, &"values".into()).is_some() {
                    crate::js_object::handle_object_method(method_name, args, env)
                } else if get_own_property(&object, &"MAX_VALUE".into()).is_some()
                    && get_own_property(&object, &"MIN_VALUE".into()).is_some()
                {
                    crate::js_number::handle_number_method(method_name, args, env)
                } else if get_own_property(&object, &"__is_bigint_constructor".into()).is_some() {
                    crate::js_bigint::handle_bigint_static_method(method_name, args, env)
                } else if get_own_property(&object, &"__value__".into()).is_some() {
                    if let Some(val_rc) = obj_get_key_value(&object, &"__value__".into())? {
                        match &*val_rc.borrow() {
                            Value::Number(_) => crate::js_number::handle_number_object_method(&object, method_name, args, env),
                            Value::BigInt(_) => crate::js_bigint::handle_bigint_object_method(&object, method_name, args, env),
                            Value::String(s) => crate::js_string::handle_string_method(s, method_name, args, env),
                            Value::Boolean(b) => match method_name.as_str() {
                                "toString" => Ok(Value::String(utf8_to_utf16(&b.to_string()))),
                                "valueOf" => Ok(Value::Boolean(*b)),
                                _ => Err(raise_eval_error!(format!("Boolean.prototype.{method_name} is not implemented"))),
                            },
                            Value::Symbol(s) => match method_name.as_str() {
                                "toString" => Ok(Value::String(utf8_to_utf16(&format!(
                                    "Symbol({})",
                                    s.description.as_deref().unwrap_or("")
                                )))),
                                "valueOf" => Ok(Value::Symbol(s.clone())),
                                _ => Err(raise_eval_error!(format!("Symbol.prototype.{method_name} is not implemented"))),
                            },
                            _ => Err(raise_eval_error!("Invalid __value__ for boxed object")),
                        }
                    } else {
                        Err(raise_eval_error!("__value__ not found on instance"))
                    }
                } else if is_date_object(&object) {
                    // Date instance methods
                    crate::js_date::handle_date_method(&object, method_name, args, env)
                } else if is_regex_object(&object) {
                    // RegExp instance methods
                    crate::js_regexp::handle_regexp_method(&object, method_name, args, env)
                } else if is_array(&object) {
                    // Array instance methods
                    crate::js_array::handle_array_instance_method(&object, method_name, args, env)
                } else if get_own_property(&object, &"__promise".into()).is_some() {
                    // Promise instance methods
                    handle_promise_method(&object, method_name, args, env)
                } else if get_own_property(&object, &"__dataview".into()).is_some() {
                    // Class static methods
                    call_static_method(&object, method_name, args, env)
                } else if is_class_instance(&object)? {
                    call_class_method(&object, method_name, args, env)
                } else {
                    Err(raise_eval_error!(format!("Method {method_name} not found on object")))
                }
            }
            Value::Function(func_name) => {
                // Handle constructor static methods
                match func_name.as_str() {
                    "Object" => crate::js_object::handle_object_method(method_name, args, env),
                    "Array" => crate::js_array::handle_array_static_method(method_name, args, env),
                    "Date" => crate::js_date::handle_date_static_method(method_name, args, env),
                    _ => Err(raise_eval_error!(format!("{func_name} has no static method '{method_name}'"))),
                }
            }
            Value::String(s) => crate::js_string::handle_string_method(&s, method_name, args, env),
            Value::Number(n) => crate::js_number::handle_number_instance_method(&n, method_name, args, env),
            _ => Err(raise_eval_error!("error")),
        }
    } else {
        // Regular function call - check if base is null/undefined
        let func_val = evaluate_expr(env, func_expr)?;
        match func_val {
            Value::Undefined => Ok(Value::Undefined),
            Value::Function(func_name) => crate::js_function::handle_global_function(&func_name, args, env),
            Value::Closure(data) | Value::AsyncClosure(data) => {
                let params = &data.params;
                let body = &data.body;
                let captured_env = &data.env;
                // Function call
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                // Prepare function env for closure call (this = undefined)
                let func_env = prepare_function_call_env(
                    Some(captured_env),
                    Some(Value::Undefined),
                    Some(params),
                    &evaluated_args,
                    None,
                    None,
                )?;
                // Execute function body
                evaluate_statements(&func_env, body)
            }
            _ => Err(raise_eval_error!("error")),
        }
    }
}

fn evaluate_object(env: &JSObjectDataPtr, properties: &Vec<(Expr, Expr, bool)>) -> Result<Value, JSError> {
    let obj = new_js_object_data();
    // Attempt to set the default prototype for object literals to Object.prototype
    // by finding the global 'Object' constructor and using its 'prototype' property.
    // Walk to the top-level environment
    let mut root_env_opt = Some(env.clone());
    while let Some(r) = root_env_opt.clone() {
        if let Some(parent) = r.borrow().prototype.clone().and_then(|w| w.upgrade()) {
            root_env_opt = Some(parent);
        } else {
            break;
        }
    }
    if let Some(root_env) = root_env_opt {
        // Use centralized helper to set default prototype from global Object constructor
        crate::core::set_internal_prototype_from_constructor(&obj, &root_env, "Object")?;
    }

    for (key_expr, value_expr, is_method) in properties {
        if matches!(value_expr, Expr::Spread(_)) {
            // Spread operator: evaluate the expression and spread its properties
            if let Expr::Spread(expr) = value_expr {
                let spread_val = evaluate_expr(env, expr)?;
                if let Value::Object(spread_obj) = spread_val {
                    // Copy enumerable own properties from spread_obj to obj
                    for (prop_key, prop_val) in spread_obj.borrow().properties.iter() {
                        if spread_obj.borrow().is_enumerable(prop_key) {
                            obj.borrow_mut().insert(prop_key.clone(), prop_val.clone());
                        }
                    }
                } else {
                    return Err(raise_eval_error!("Spread operator can only be applied to objects"));
                }
            }
        } else {
            // Evaluate key expression
            let key_val = evaluate_expr(env, key_expr)?;
            let pk = value_to_property_key(&key_val);

            match value_expr {
                Expr::Getter(func_expr) => {
                    if let Expr::Function(_name, _params, body) = func_expr.as_ref() {
                        // Check if property already exists
                        let existing_opt = get_own_property(&obj, &pk);
                        if let Some(existing) = existing_opt {
                            let mut val = existing.borrow().clone();
                            if let Value::Property {
                                value: _,
                                getter,
                                setter: _,
                            } = &mut val
                            {
                                // Update getter
                                getter.replace((body.clone(), env.clone(), None));
                                obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(val)));
                            } else {
                                // Create new property descriptor
                                let prop = Value::Property {
                                    value: Some(existing.clone()),
                                    getter: Some((body.clone(), env.clone(), None)),
                                    setter: None,
                                };
                                obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(prop)));
                            }
                        } else {
                            // Create new property descriptor with getter
                            let prop = Value::Property {
                                value: None,
                                getter: Some((body.clone(), env.clone(), None)),
                                setter: None,
                            };
                            obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(prop)));
                        }
                    } else {
                        return Err(raise_eval_error!("Getter must be a function"));
                    }
                }
                Expr::Setter(func_expr) => {
                    if let Expr::Function(_name, params, body) = func_expr.as_ref() {
                        // Check if property already exists
                        let existing_opt = get_own_property(&obj, &pk);
                        if let Some(existing) = existing_opt {
                            let mut val = existing.borrow().clone();
                            if let Value::Property {
                                value: _,
                                getter: _,
                                setter,
                            } = &mut val
                            {
                                // Update setter
                                setter.replace((params.clone(), body.clone(), env.clone(), None));
                                obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(val)));
                            } else {
                                // Create new property descriptor
                                let prop = Value::Property {
                                    value: Some(existing.clone()),
                                    getter: None,
                                    setter: Some((params.clone(), body.clone(), env.clone(), None)),
                                };
                                obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(prop)));
                            }
                        } else {
                            // Create new property descriptor with setter
                            let prop = Value::Property {
                                value: None,
                                getter: None,
                                setter: Some((params.clone(), body.clone(), env.clone(), None)),
                            };
                            obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(prop)));
                        }
                    } else {
                        return Err(raise_eval_error!("Setter must be a function"));
                    }
                }
                _ => {
                    let mut value = evaluate_expr(env, value_expr)?;
                    if *is_method {
                        match &mut value {
                            Value::Closure(data) => *data.home_object.borrow_mut() = Some(Rc::downgrade(&obj)),
                            Value::AsyncClosure(data) => *data.home_object.borrow_mut() = Some(Rc::downgrade(&obj)),
                            Value::GeneratorFunction(_, data) => *data.home_object.borrow_mut() = Some(Rc::downgrade(&obj)),
                            Value::Object(func_obj) => {
                                if let Some(closure_rc) = obj_get_key_value(func_obj, &"__closure__".into())? {
                                    let mut closure_val = closure_rc.borrow_mut();
                                    match &mut *closure_val {
                                        Value::Closure(data) => *data.home_object.borrow_mut() = Some(Rc::downgrade(&obj)),
                                        Value::AsyncClosure(data) => *data.home_object.borrow_mut() = Some(Rc::downgrade(&obj)),
                                        Value::GeneratorFunction(_, data) => *data.home_object.borrow_mut() = Some(Rc::downgrade(&obj)),
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    // Check if property already exists
                    let existing_rc = get_own_property(&obj, &pk);
                    if let Some(existing) = existing_rc {
                        let mut existing_val = existing.borrow().clone();
                        if let Value::Property {
                            value: prop_value,
                            getter: _,
                            setter: _,
                        } = &mut existing_val
                        {
                            // Update value
                            prop_value.replace(Rc::new(RefCell::new(value)));
                            obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(existing_val)));
                        } else {
                            // Create new property descriptor
                            let prop = Value::Property {
                                value: Some(Rc::new(RefCell::new(value))),
                                getter: None,
                                setter: None,
                            };
                            obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(prop)));
                        }
                    } else {
                        obj_set_key_value(&obj, &pk, value)?;
                    }
                }
            }
        }
    }
    Ok(Value::Object(obj))
}

fn evaluate_array(env: &JSObjectDataPtr, elements: &Vec<Option<Expr>>) -> Result<Value, JSError> {
    let arr = crate::js_array::create_array(env)?;
    let mut index = 0;
    for elem_opt in elements {
        if let Some(elem_expr) = elem_opt {
            if let Expr::Spread(spread_expr) = elem_expr {
                let spread_val = evaluate_expr(env, spread_expr)?;
                match spread_val {
                    Value::Object(spread_obj) => {
                        if let Some(iter_sym_rc) = get_well_known_symbol_rc("iterator") {
                            let key = PropertyKey::Symbol(iter_sym_rc.clone());
                            if let Some(method_rc) = obj_get_key_value(&spread_obj, &key)? {
                                let iterator_val = {
                                    let method_val = &*method_rc.borrow();
                                    if let Some((params, body, captured_env)) = extract_closure_from_value(method_val) {
                                        let func_env = prepare_function_call_env(
                                            Some(&captured_env),
                                            Some(Value::Object(spread_obj.clone())),
                                            Some(&params),
                                            &[],
                                            Some(&build_frame_name(env, "[Symbol.iterator]")),
                                            Some(env),
                                        )?;
                                        evaluate_statements(&func_env, &body)?
                                    } else if let Value::Function(func_name) = method_val {
                                        let call_env = prepare_function_call_env(
                                            Some(env),
                                            Some(Value::Object(spread_obj.clone())),
                                            None,
                                            &[],
                                            Some(&build_frame_name(env, "[Symbol.iterator]")),
                                            Some(env),
                                        )?;
                                        crate::js_function::handle_global_function(func_name, &[], &call_env)?
                                    } else if let Value::Object(iter_obj) = method_val {
                                        Value::Object(iter_obj.clone())
                                    } else {
                                        return Err(raise_type_error!("iterator property is not callable"));
                                    }
                                };

                                if let Value::Object(iter_obj) = iterator_val {
                                    loop {
                                        if let Some(next_rc) = obj_get_key_value(&iter_obj, &"next".into())? {
                                            let next_val = {
                                                let nv = &*next_rc.borrow();
                                                if let Some((nparams, nbody, ncaptured_env)) = extract_closure_from_value(nv) {
                                                    let func_env = prepare_function_call_env(
                                                        Some(&ncaptured_env),
                                                        Some(Value::Object(iter_obj.clone())),
                                                        Some(&nparams),
                                                        &[],
                                                        None,
                                                        None,
                                                    )?;
                                                    evaluate_statements(&func_env, &nbody)?
                                                } else if let Value::Function(func_name) = nv {
                                                    crate::js_function::handle_global_function(func_name, &[], env)?
                                                } else {
                                                    return Err(raise_type_error!("next is not callable"));
                                                }
                                            };

                                            if let Value::Object(res_obj) = next_val {
                                                let done_val = obj_get_key_value(&res_obj, &"done".into())?;
                                                let done = match done_val {
                                                    Some(d) => is_truthy(&d.borrow().clone()),
                                                    None => false,
                                                };
                                                if done {
                                                    break;
                                                }
                                                let value_val = obj_get_key_value(&res_obj, &"value".into())?;
                                                let element = match value_val {
                                                    Some(v) => v.borrow().clone(),
                                                    None => Value::Undefined,
                                                };
                                                obj_set_key_value(&arr, &index.to_string().into(), element)?;
                                                index += 1;
                                            } else {
                                                return Err(raise_type_error!("iterator.next() must return an object"));
                                            }
                                        } else {
                                            return Err(raise_type_error!("iterator object missing next()"));
                                        }
                                    }
                                } else {
                                    return Err(raise_type_error!("iterator method did not return an object"));
                                }
                            } else {
                                return Err(raise_type_error!("Spread syntax requires ...iterable"));
                            }
                        } else {
                            return Err(raise_type_error!("Symbol.iterator not found"));
                        }
                    }
                    Value::String(s) => {
                        let mut i = 0usize;
                        while let Some(first) = utf16_char_at(&s, i) {
                            let chunk: Vec<u16> = if (0xD800..=0xDBFF).contains(&first)
                                && let Some(second) = utf16_char_at(&s, i + 1)
                                && (0xDC00..=0xDFFF).contains(&second)
                            {
                                utf16_slice(&s, i, i + 2)
                            } else {
                                vec![first]
                            };
                            obj_set_key_value(&arr, &index.to_string().into(), Value::String(chunk.clone()))?;
                            index += 1;
                            i += chunk.len();
                        }
                    }
                    _ => return Err(raise_type_error!("Spread syntax requires ...iterable")),
                }
            } else {
                let value = evaluate_expr(env, elem_expr)?;
                obj_set_key_value(&arr, &index.to_string().into(), value)?;
                index += 1;
            }
        } else {
            // Hole (elision) - just increment index, do not set property
            index += 1;
        }
    }
    // Set length property
    set_array_length(&arr, index)?;
    Ok(Value::Object(arr))
}

fn evaluate_array_destructuring(_env: &JSObjectDataPtr, _pattern: &Vec<DestructuringElement>) -> Result<Value, JSError> {
    // Array destructuring is handled at the statement level, not as an expression
    Err(raise_eval_error!("Array destructuring should not be evaluated as an expression"))
}

fn evaluate_object_destructuring(_env: &JSObjectDataPtr, _pattern: &Vec<ObjectDestructuringElement>) -> Result<Value, JSError> {
    // Object destructuring is handled at the statement level, not as an expression
    Err(raise_eval_error!("Object destructuring should not be evaluated as an expression"))
}

fn collect_var_names(statements: &[Statement], names: &mut std::collections::HashSet<String>) {
    for stmt in statements {
        match &stmt.kind {
            StatementKind::Var(decls) => {
                for (name, _) in decls {
                    names.insert(name.clone());
                }
            }
            StatementKind::VarDestructuringArray(pattern, _) => {
                collect_names_from_array_pattern(pattern, names);
            }
            StatementKind::VarDestructuringObject(pattern, _) => {
                collect_names_from_object_pattern(pattern, names);
            }
            StatementKind::If(_, then_body, else_body) => {
                collect_var_names(then_body, names);
                if let Some(else_stmts) = else_body {
                    collect_var_names(else_stmts, names);
                }
            }
            StatementKind::For(_, _, _, body) => {
                collect_var_names(body, names);
            }
            StatementKind::ForOf(_, _, body) => {
                collect_var_names(body, names);
            }
            StatementKind::ForIn(var, _, body) => {
                names.insert(var.clone());
                collect_var_names(body, names);
            }
            StatementKind::ForOfDestructuringObject(pattern, _, body) => {
                // extract variable names from object destructuring pattern
                for element in pattern {
                    match element {
                        ObjectDestructuringElement::Property { key: _, value } => match value {
                            DestructuringElement::Variable(var, _) => {
                                names.insert(var.clone());
                            }
                            DestructuringElement::NestedArray(nested) => collect_names_from_array_pattern(nested, names),
                            DestructuringElement::NestedObject(nested) => collect_names_from_object_pattern(nested, names),
                            DestructuringElement::Rest(var) => {
                                names.insert(var.clone());
                            }
                            DestructuringElement::Empty => {}
                        },
                        ObjectDestructuringElement::Rest(var) => {
                            names.insert(var.clone());
                        }
                    }
                }
                collect_var_names(body, names);
            }
            StatementKind::ForOfDestructuringArray(pattern, _, body) => {
                collect_names_from_array_pattern(pattern, names);
                collect_var_names(body, names);
            }
            StatementKind::While(_, body) => {
                collect_var_names(body, names);
            }
            StatementKind::DoWhile(body, _) => {
                collect_var_names(body, names);
            }
            StatementKind::Switch(_, cases) => {
                for case in cases {
                    match case {
                        SwitchCase::Case(_, stmts) => collect_var_names(stmts, names),
                        SwitchCase::Default(stmts) => collect_var_names(stmts, names),
                    }
                }
            }
            StatementKind::TryCatch(try_body, _, catch_body, finally_body) => {
                collect_var_names(try_body, names);
                collect_var_names(catch_body, names);
                if let Some(finally_stmts) = finally_body {
                    collect_var_names(finally_stmts, names);
                }
            }
            StatementKind::Block(stmts) => {
                collect_var_names(stmts, names);
            }
            StatementKind::Label(_, stmt) => {
                collect_var_names(std::slice::from_ref(stmt), names);
            }
            _ => {}
        }
    }
}

fn collect_names_from_array_pattern(pattern: &Vec<DestructuringElement>, names: &mut std::collections::HashSet<String>) {
    for element in pattern {
        match element {
            DestructuringElement::Variable(var, _) => {
                names.insert(var.clone());
            }
            DestructuringElement::NestedArray(nested) => collect_names_from_array_pattern(nested, names),
            DestructuringElement::NestedObject(nested) => collect_names_from_object_pattern(nested, names),
            DestructuringElement::Rest(var) => {
                names.insert(var.clone());
            }
            DestructuringElement::Empty => {}
        }
    }
}

fn collect_names_from_object_pattern(pattern: &Vec<ObjectDestructuringElement>, names: &mut std::collections::HashSet<String>) {
    for element in pattern {
        match element {
            ObjectDestructuringElement::Property { key: _, value } => match value {
                DestructuringElement::Variable(var, _) => {
                    names.insert(var.clone());
                }
                DestructuringElement::NestedArray(nested) => collect_names_from_array_pattern(nested, names),
                DestructuringElement::NestedObject(nested) => collect_names_from_object_pattern(nested, names),
                DestructuringElement::Rest(var) => {
                    names.insert(var.clone());
                }
                DestructuringElement::Empty => {}
            },
            ObjectDestructuringElement::Rest(var) => {
                names.insert(var.clone());
            }
        }
    }
}

/// Handle optional method call on an object, Similar logic to regular method call but for optional
fn handle_optional_method_call(object: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "log" if get_own_property(object, &"log".into()).is_some() => handle_console_method(method, args, env),
        "toString" => crate::js_object::handle_to_string_method(&Value::Object(object.clone()), args, env),
        "valueOf" => crate::js_object::handle_value_of_method(&Value::Object(object.clone()), args, env),
        method => {
            // If this object looks like the `std` module (we used 'sprintf' as marker)
            if get_own_property(object, &"sprintf".into()).is_some() {
                match method {
                    "sprintf" => {
                        log::trace!("js dispatch calling sprintf with {} args", args.len());
                        handle_sprintf_call(env, args)
                    }
                    "tmpfile" => create_tmpfile(),
                    _ => Ok(Value::Undefined),
                }
            } else if get_own_property(object, &"open".into()).is_some() {
                // If this object looks like the `os` module (we used 'open' as marker)
                crate::js_os::handle_os_method(object, method, args, env)
            } else if get_own_property(object, &"join".into()).is_some() {
                // If this object looks like the `os.path` module
                crate::js_os::handle_os_method(object, method, args, env)
            } else if get_own_property(object, &"__file_id".into()).is_some() {
                // If this object is a file-like object (we use '__file_id' as marker)
                handle_file_method(object, method, args, env)
            } else if get_own_property(object, &"PI".into()).is_some() && get_own_property(object, &"E".into()).is_some() {
                // Check if this is the Math object
                handle_math_method(method, args, env)
            } else if get_own_property(object, &"apply".into()).is_some() && get_own_property(object, &"construct".into()).is_some() {
                // Check if this is the Reflect object
                crate::js_reflect::handle_reflect_method(method, args, env)
            } else if get_own_property(object, &"parse".into()).is_some() && get_own_property(object, &"stringify".into()).is_some() {
                crate::js_json::handle_json_method(method, args, env)
            } else if get_own_property(object, &"keys".into()).is_some() && get_own_property(object, &"values".into()).is_some() {
                crate::js_object::handle_object_method(method, args, env)
            } else if is_date_object(object) {
                // Date instance methods
                crate::js_date::handle_date_method(object, method, args, env)
            } else if is_regex_object(object) {
                // RegExp instance methods
                crate::js_regexp::handle_regexp_method(object, method, args, env)
            } else if is_array(object) {
                // Array instance methods
                crate::js_array::handle_array_instance_method(object, method, args, env)
            } else if get_own_property(object, &"__class_def__".into()).is_some() {
                // Class static methods
                call_static_method(object, method, args, env)
            } else if is_class_instance(object)? {
                call_class_method(object, method, args, env)
            } else {
                // Check for user-defined method
                if let Some(prop_val) = obj_get_key_value(object, &method.into())? {
                    let prop = prop_val.borrow().clone();
                    if let Some((params, body, captured_env)) = extract_closure_from_value(&prop) {
                        // Function call
                        // Collect all arguments, expanding spreads
                        let mut evaluated_args = Vec::new();
                        expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                        // Prepare function env and attach frame/caller for stack traces
                        let func_env = prepare_function_call_env(
                            Some(&captured_env),
                            None,
                            Some(&params),
                            &evaluated_args,
                            Some(&build_frame_name(env, method)),
                            Some(env),
                        )?;
                        // Create arguments object
                        let arguments_obj = create_array(&func_env)?;
                        set_array_length(&arguments_obj, evaluated_args.len())?;
                        for (i, arg) in evaluated_args.iter().enumerate() {
                            obj_set_key_value(&arguments_obj, &i.to_string().into(), arg.clone())?;
                        }
                        obj_set_key_value(&func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                        // Execute function body
                        evaluate_statements(&func_env, &body)
                    } else if let Value::Function(func_name) = prop {
                        crate::js_function::handle_global_function(&func_name, args, env)
                    } else {
                        Err(raise_eval_error!(format!("Property '{method}' is not a function")))
                    }
                } else {
                    Ok(Value::Undefined)
                }
            }
        }
    }
}

fn handle_symbol_static_method(method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "for" => {
            // Symbol.for(key) - returns a symbol from the global registry
            if args.len() != 1 {
                return Err(raise_type_error!("Symbol.for requires exactly one argument"));
            }
            let key_expr = &args[0];
            let key_val = evaluate_expr(env, key_expr)?;
            let key = match key_val {
                Value::String(s) => utf16_to_utf8(&s),
                _ => value_to_string(&key_val),
            };

            SYMBOL_REGISTRY.with(|registry| {
                let mut reg = registry.borrow_mut();
                if let Some(symbol) = reg.get(&key) {
                    Ok(symbol.borrow().clone())
                } else {
                    // Create a new symbol and register it
                    let symbol_data = Rc::new(SymbolData {
                        description: Some(key.clone()),
                    });
                    let symbol = Rc::new(RefCell::new(Value::Symbol(symbol_data)));
                    reg.insert(key, symbol.clone());
                    Ok(symbol.borrow().clone())
                }
            })
        }
        "keyFor" => {
            // Symbol.keyFor(symbol) - returns the key for a symbol in the global registry
            if args.len() != 1 {
                return Err(raise_type_error!("Symbol.keyFor requires exactly one argument"));
            }
            let symbol_expr = &args[0];
            let symbol_val = evaluate_expr(env, symbol_expr)?;

            if let Value::Symbol(symbol_data) = symbol_val {
                SYMBOL_REGISTRY.with(|registry| {
                    let reg = registry.borrow();
                    for (key, sym) in reg.iter() {
                        if let Value::Symbol(stored_data) = &*sym.borrow()
                            && Rc::ptr_eq(&symbol_data, stored_data)
                        {
                            return Ok(Value::String(utf8_to_utf16(key)));
                        }
                    }
                    Ok(Value::Undefined)
                })
            } else {
                Err(raise_type_error!("Symbol.keyFor requires a symbol as argument"))
            }
        }
        _ => Err(raise_type_error!(format!("Symbol has no static method '{method}'"))),
    }
}

/// Expand spread operator in function call arguments
pub(crate) fn expand_spread_in_call_args(env: &JSObjectDataPtr, args: &[Expr], evaluated_args: &mut Vec<Value>) -> Result<(), JSError> {
    for arg_expr in args {
        if let Expr::Spread(spread_expr) = arg_expr {
            let spread_val = evaluate_expr(env, spread_expr)?;
            if let Value::Object(spread_obj) = spread_val {
                // Assume it's an array-like object
                let mut i = 0;
                loop {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(val) = obj_get_key_value(&spread_obj, &key)? {
                        evaluated_args.push(val.borrow().clone());
                        i += 1;
                    } else {
                        break;
                    }
                }
            } else {
                return Err(raise_eval_error!("Spread operator can only be applied to arrays in function calls"));
            }
        } else {
            let arg_val = evaluate_expr(env, arg_expr)?;
            evaluated_args.push(arg_val);
        }
    }
    Ok(())
}

pub fn get_prop_env(env: &JSObjectDataPtr, obj_expr: &Expr, prop: &str) -> Result<Option<Rc<RefCell<Value>>>, JSError> {
    let obj_val = evaluate_expr(env, obj_expr)?;
    match obj_val {
        Value::Object(map) => obj_get_key_value(&map, &prop.into()),
        _ => Ok(None),
    }
}

// Helper to access well-known symbols as Rc<RefCell<Value>> or as Value
pub fn get_well_known_symbol_rc(name: &str) -> Option<Rc<RefCell<Value>>> {
    WELL_KNOWN_SYMBOLS.with(|wk| wk.borrow().get(name).cloned())
}

#[allow(dead_code)]
pub fn get_well_known_symbol(name: &str) -> Option<Value> {
    WELL_KNOWN_SYMBOLS.with(|wk| {
        wk.borrow().get(name).and_then(|v| match &*v.borrow() {
            Value::Symbol(sd) => Some(Value::Symbol(sd.clone())),
            _ => None,
        })
    })
}

// `set_prop_env` attempts to set a property on the object referenced by `obj_expr`.
// Behavior:
// - If `obj_expr` is a variable name (Expr::Var) and that variable exists in `env`
//   and is an object, it mutates the stored object in-place and returns `Ok(None)`.
// - Otherwise it evaluates `obj_expr`, and if it yields an object, it inserts the
//   property into that object's map and returns `Ok(Some(Value::Object(map)))` so
//   the caller can decide what to do with the updated object value.
pub fn set_prop_env(env: &JSObjectDataPtr, obj_expr: &Expr, prop: &str, val: Value) -> Result<Option<Value>, JSError> {
    // Fast path: obj_expr is a variable that we can mutate in-place in env
    if let Expr::Var(varname, _, _) = obj_expr
        && let Some(rc_val) = env_get(env, varname)
    {
        let mut borrowed = rc_val.borrow_mut();
        if let Value::Object(ref mut map) = *borrowed {
            // Special-case `__proto__` assignment: set the prototype
            if prop == "__proto__" {
                if let Value::Object(proto_map) = val {
                    map.borrow_mut().prototype = Some(Rc::downgrade(&proto_map));
                    return Ok(None);
                } else {
                    // Non-object assigned to __proto__: ignore or set to None
                    map.borrow_mut().prototype = None;
                    return Ok(None);
                }
            }

            obj_set_key_value(map, &prop.into(), val)?;
            return Ok(None);
        }
    }

    // Fall back: evaluate the object expression and return an updated object value
    let obj_val = evaluate_expr(env, obj_expr)?;
    match obj_val {
        Value::Object(obj) => {
            // Special-case `__proto__` assignment: set the object's prototype
            if prop == "__proto__" {
                if let Value::Object(proto_map) = val {
                    obj.borrow_mut().prototype = Some(Rc::downgrade(&proto_map));
                    return Ok(Some(Value::Object(obj)));
                } else {
                    obj.borrow_mut().prototype = None;
                    return Ok(Some(Value::Object(obj)));
                }
            }

            obj_set_key_value(&obj, &prop.into(), val)?;
            Ok(Some(Value::Object(obj)))
        }
        _ => {
            // In non-strict mode, assigning to a primitive is ignored.
            // We don't support strict mode yet, so we just ignore it.
            Ok(None)
        }
    }
}

#[allow(dead_code)]
pub fn initialize_global_constructors(env: &JSObjectDataPtr) -> Result<(), JSError> {
    // Initialize ArrayBuffer constructor
    let arraybuffer_constructor = crate::js_typedarray::make_arraybuffer_constructor()?;
    obj_set_key_value(env, &"ArrayBuffer".into(), Value::Object(arraybuffer_constructor))?;

    // Initialize DataView constructor
    let dataview_constructor = crate::js_typedarray::make_dataview_constructor()?;
    obj_set_key_value(env, &"DataView".into(), Value::Object(dataview_constructor))?;

    // Initialize TypedArray constructors
    let typedarray_constructors = crate::js_typedarray::make_typedarray_constructors()?;
    for (name, constructor) in typedarray_constructors {
        obj_set_key_value(env, &name.into(), Value::Object(constructor))?;
    }

    Ok(())
}

pub(crate) fn handle_user_defined_method_on_instance(
    object: &JSObjectDataPtr,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    // Fetch the property value (own or inherited)
    if let Some(prop_val) = obj_get_key_value(object, &method.into())? {
        match prop_val.borrow().clone() {
            Value::Closure(data) | Value::AsyncClosure(data) => {
                let params = &data.params;
                let body = &data.body;
                let captured_env = &data.env;
                let home_obj_opt = data.home_object.borrow().clone();
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                // Use bound `this` if present, otherwise the receiver instance
                let this_val = data.bound_this.clone().unwrap_or(Value::Object(object.clone()));
                let func_env = prepare_function_call_env(
                    Some(captured_env),
                    Some(this_val),
                    Some(params),
                    &evaluated_args,
                    Some(&build_frame_name(env, method)),
                    Some(env),
                )?;
                if let Some(home_weak) = home_obj_opt {
                    if let Some(home_rc) = home_weak.upgrade() {
                        log::trace!("DEBUG: Setting __home_object__ in evaluate_call (generic method)");
                        obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home_rc.clone()))?;
                    }
                }
                evaluate_statements(&func_env, body)
            }
            Value::Function(func_name) => {
                if let Some(v) = crate::js_function::handle_receiver_builtin(&func_name, object, args, env)? {
                    return Ok(v);
                }
                if func_name.starts_with("Object.prototype.") || func_name == "Error.prototype.toString" {
                    if let Some(v) = crate::js_object::handle_object_prototype_builtin(&func_name, object, args, env)? {
                        return Ok(v);
                    }
                    if func_name == "Error.prototype.toString" {
                        return crate::js_object::handle_error_to_string_method(&Value::Object(object.clone()), args);
                    }
                    crate::js_function::handle_global_function(&func_name, args, env)
                } else if func_name.starts_with("Function.prototype.") {
                    let call_env = prepare_function_call_env(Some(env), Some(Value::Object(object.clone())), None, &[], None, Some(env))?;
                    crate::js_function::handle_global_function(&func_name, args, &call_env)
                } else {
                    crate::js_function::handle_global_function(&func_name, args, env)
                }
            }
            Value::Object(func_obj_map) => {
                if let Some(cl_rc) = obj_get_key_value(&func_obj_map, &"__closure__".into())? {
                    match &*cl_rc.borrow() {
                        Value::Closure(data) => {
                            let params = &data.params;
                            let body = &data.body;
                            let captured_env = &data.env;
                            let home_obj_opt = data.home_object.borrow().clone();
                            let mut evaluated_args = Vec::new();
                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                            let func_env = prepare_function_call_env(
                                Some(captured_env),
                                Some(Value::Object(object.clone())),
                                Some(params),
                                &evaluated_args,
                                Some(&build_frame_name(env, method)),
                                Some(env),
                            )?;
                            if let Some(home_weak) = home_obj_opt {
                                if let Some(home_rc) = home_weak.upgrade() {
                                    obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home_rc.clone()))?;
                                }
                            }

                            // Create arguments object
                            let arguments_obj = create_array(&func_env)?;
                            set_array_length(&arguments_obj, evaluated_args.len())?;
                            for (i, arg) in evaluated_args.iter().enumerate() {
                                obj_set_key_value(&arguments_obj, &i.to_string().into(), arg.clone())?;
                            }
                            obj_set_key_value(&func_env, &"arguments".into(), Value::Object(arguments_obj))?;

                            match evaluate_statements_with_context(&func_env, body)? {
                                ControlFlow::Normal(_) => Ok(Value::Undefined),
                                ControlFlow::Return(val) => Ok(val),
                                ControlFlow::Break(_) => Err(raise_eval_error!("break statement not in loop or switch")),
                                ControlFlow::Continue(_) => Err(raise_eval_error!("continue statement not in loop")),
                            }
                        }
                        Value::GeneratorFunction(_, data) => {
                            let params = &data.params;
                            let body = &data.body;
                            let captured_env = &data.env;
                            let home_obj = &data.home_object;
                            let mut evaluated_args = Vec::new();
                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                            let func_env = prepare_function_call_env(
                                Some(captured_env),
                                Some(Value::Object(object.clone())),
                                None,
                                &evaluated_args,
                                Some(&build_frame_name(env, method)),
                                Some(env),
                            )?;
                            if let Some(home_weak) = &*home_obj.borrow() {
                                if let Some(home_rc) = home_weak.upgrade() {
                                    obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home_rc.clone()))?;
                                }
                            }
                            crate::js_generator::handle_generator_function_call(params, body, args, &func_env)
                        }
                        Value::AsyncClosure(data) => {
                            let params = &data.params;
                            let body = &data.body;
                            let captured_env = &data.env;
                            let home_obj_opt = data.home_object.borrow().clone();
                            let mut evaluated_args = Vec::new();
                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                            let promise = Rc::new(RefCell::new(JSPromise::default()));
                            let promise_obj = Value::Object(new_js_object_data());
                            if let Value::Object(obj) = &promise_obj {
                                obj.borrow_mut()
                                    .insert("__promise".into(), Rc::new(RefCell::new(Value::Promise(promise.clone()))));
                            }
                            let func_env = prepare_function_call_env(
                                Some(captured_env),
                                Some(Value::Object(object.clone())),
                                Some(params),
                                &evaluated_args,
                                Some(&build_frame_name(env, method)),
                                Some(env),
                            )?;
                            if let Some(home_weak) = home_obj_opt {
                                if let Some(home_rc) = home_weak.upgrade() {
                                    obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home_rc.clone()))?;
                                }
                            }
                            func_env.borrow_mut().is_function_scope = true;

                            let result = evaluate_statements(&func_env, body);
                            match result {
                                Ok(val) => crate::js_promise::resolve_promise(&promise, val),
                                Err(e) => match e.kind() {
                                    crate::JSErrorKind::Throw { value } => {
                                        crate::js_promise::reject_promise(&promise, value.clone());
                                    }
                                    _ => {
                                        crate::js_promise::reject_promise(&promise, Value::String(utf8_to_utf16(&format!("{}", e))));
                                    }
                                },
                            }
                            Ok(promise_obj)
                        }
                        _ => Err(raise_eval_error!(format!("Property '{method}' is not a function"))),
                    }
                } else {
                    Err(raise_eval_error!(format!("Property '{method}' is not a function")))
                }
            }
            _ => Err(raise_eval_error!(format!("Property '{method}' is not a function"))),
        }
    } else {
        Err(raise_eval_error!(format!("Property '{method}' is not a function")))
    }
}
// */
