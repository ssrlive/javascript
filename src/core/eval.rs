#![allow(warnings)]

use crate::core::{Gc, GcCell, MutationContext};
use crate::js_array::{create_array, handle_array_static_method, is_array, set_array_length};
use crate::js_bigint::bigint_constructor;
use crate::js_date::{handle_date_method, handle_date_static_method, is_date_object};
use crate::js_json::handle_json_method;
use crate::js_number::{handle_number_instance_method, handle_number_prototype_method, handle_number_static_method, number_constructor};
use crate::js_string::{string_from_char_code, string_from_code_point, string_raw};
use crate::{
    JSError, JSErrorKind, PropertyKey, Value,
    core::{
        BinaryOp, ClosureData, DestructuringElement, EvalError, ExportSpecifier, Expr, ImportSpecifier, JSObjectDataPtr,
        ObjectDestructuringElement, Statement, StatementKind, create_error, env_get, env_set, env_set_recursive, is_error,
        new_js_object_data, obj_get_key_value, obj_set_key_value, value_to_string,
    },
    js_math::handle_math_call,
    raise_eval_error, raise_reference_error,
    unicode::{utf8_to_utf16, utf16_to_utf8},
};
use crate::{Token, parse_statements, raise_type_error, tokenize};

#[derive(Clone, Debug)]
pub enum ControlFlow<'gc> {
    Normal(Value<'gc>),
    Return(Value<'gc>),
    Throw(Value<'gc>, Option<usize>, Option<usize>), // value, line, column
    Break(Option<String>),
    Continue(Option<String>),
}

fn collect_names_from_destructuring(pattern: &[DestructuringElement], names: &mut Vec<String>) {
    for element in pattern {
        collect_names_from_destructuring_element(element, names);
    }
}

fn collect_names_from_destructuring_element(element: &DestructuringElement, names: &mut Vec<String>) {
    match element {
        DestructuringElement::Variable(name, _) => names.push(name.clone()),
        DestructuringElement::Property(_, inner) => collect_names_from_destructuring_element(inner, names),
        DestructuringElement::Rest(name) => names.push(name.clone()),
        DestructuringElement::NestedArray(inner) => collect_names_from_destructuring(inner, names),
        DestructuringElement::NestedObject(inner) => collect_names_from_destructuring(inner, names),
        DestructuringElement::Empty => {}
    }
}

fn collect_names_from_object_destructuring(pattern: &[ObjectDestructuringElement], names: &mut Vec<String>) {
    for element in pattern {
        match element {
            ObjectDestructuringElement::Property { key: _, value } => collect_names_from_destructuring_element(value, names),
            ObjectDestructuringElement::Rest(name) => names.push(name.clone()),
        }
    }
}

fn hoist_name<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str) -> Result<(), EvalError<'gc>> {
    let mut target_env = *env;
    while !target_env.borrow().is_function_scope {
        if let Some(proto) = target_env.borrow().prototype {
            target_env = proto;
        } else {
            break;
        }
    }
    if env_get(&target_env, name).is_none() {
        env_set(mc, &target_env, name, Value::Undefined)?;
    }
    Ok(())
}

fn hoist_var_declarations<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
) -> Result<(), EvalError<'gc>> {
    for stmt in statements {
        match &stmt.kind {
            StatementKind::Var(decls) => {
                for (name, _) in decls {
                    hoist_name(mc, env, name)?;
                }
            }
            StatementKind::VarDestructuringArray(pattern, _) => {
                let mut names = Vec::new();
                collect_names_from_destructuring(pattern, &mut names);
                for name in names {
                    hoist_name(mc, env, &name)?;
                }
            }
            StatementKind::VarDestructuringObject(pattern, _) => {
                let mut names = Vec::new();
                collect_names_from_object_destructuring(pattern, &mut names);
                for name in names {
                    hoist_name(mc, env, &name)?;
                }
            }
            StatementKind::Block(stmts) => hoist_var_declarations(mc, env, stmts)?,
            StatementKind::If(_, then_block, else_block) => {
                hoist_var_declarations(mc, env, then_block)?;
                if let Some(else_stmts) = else_block {
                    hoist_var_declarations(mc, env, else_stmts)?;
                }
            }
            StatementKind::For(_, _, _, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::ForIn(_, _, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::ForOf(_, _, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::ForOfDestructuringObject(_, _, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::ForOfDestructuringArray(_, _, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::While(_, body) => hoist_var_declarations(mc, env, body)?,
            StatementKind::DoWhile(body, _) => hoist_var_declarations(mc, env, body)?,
            StatementKind::TryCatch(try_body, _, catch_body, finally_body) => {
                hoist_var_declarations(mc, env, try_body)?;
                if let Some(catch_stmts) = catch_body {
                    hoist_var_declarations(mc, env, catch_stmts)?;
                }
                if let Some(finally_stmts) = finally_body {
                    hoist_var_declarations(mc, env, finally_stmts)?;
                }
            }
            StatementKind::Switch(_, cases) => {
                for case in cases {
                    match case {
                        crate::core::SwitchCase::Case(_, stmts) => hoist_var_declarations(mc, env, stmts)?,
                        crate::core::SwitchCase::Default(stmts) => hoist_var_declarations(mc, env, stmts)?,
                    }
                }
            }
            StatementKind::Label(_, stmt) => {
                // Label contains a single statement, but it might be a block or loop
                // We need to wrap it in a slice to recurse
                hoist_var_declarations(mc, env, std::slice::from_ref(stmt))?;
            }
            StatementKind::Export(_, Some(decl)) => {
                hoist_var_declarations(mc, env, std::slice::from_ref(decl))?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn hoist_declarations<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, statements: &[Statement]) -> Result<(), EvalError<'gc>> {
    // 1. Hoist FunctionDeclarations (only top-level in this list of statements)
    for stmt in statements {
        if let StatementKind::FunctionDeclaration(name, params, body, _) = &stmt.kind {
            let mut body_clone = body.clone();
            let func = evaluate_function_expression(mc, env, Some(name.clone()), params, &mut body_clone)?;
            env_set(mc, env, name, func)?;
        }
    }

    // 2. Hoist Var declarations (recursively)
    hoist_var_declarations(mc, env, statements)?;

    // 3. Hoist Lexical declarations (let, const, class) - top-level only, initialize to Uninitialized (TDZ)
    for stmt in statements {
        match &stmt.kind {
            StatementKind::Let(decls) => {
                for (name, _) in decls {
                    env_set(mc, env, name, Value::Uninitialized)?;
                }
            }
            StatementKind::Const(decls) => {
                for (name, _) in decls {
                    env_set(mc, env, name, Value::Uninitialized)?;
                }
            }
            StatementKind::Class(class_def) => {
                env_set(mc, env, &class_def.name, Value::Uninitialized)?;
            }
            StatementKind::Import(specifiers, _) => {
                for spec in specifiers {
                    match spec {
                        ImportSpecifier::Default(name) => {
                            env_set(mc, env, name, Value::Uninitialized)?;
                        }
                        ImportSpecifier::Named(name, alias) => {
                            let binding_name = alias.as_ref().unwrap_or(name);
                            env_set(mc, env, binding_name, Value::Uninitialized)?;
                        }
                        ImportSpecifier::Namespace(name) => {
                            env_set(mc, env, name, Value::Uninitialized)?;
                        }
                    }
                }
            }
            StatementKind::LetDestructuringArray(pattern, _) | StatementKind::ConstDestructuringArray(pattern, _) => {
                let mut names = Vec::new();
                collect_names_from_destructuring(pattern, &mut names);
                for name in names {
                    env_set(mc, env, &name, Value::Uninitialized)?;
                }
            }
            StatementKind::LetDestructuringObject(pattern, _) | StatementKind::ConstDestructuringObject(pattern, _) => {
                let mut names = Vec::new();
                collect_names_from_object_destructuring(pattern, &mut names);
                for name in names {
                    env_set(mc, env, &name, Value::Uninitialized)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn evaluate_statements<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
) -> Result<Value<'gc>, EvalError<'gc>> {
    match evaluate_statements_with_context(mc, env, statements)? {
        ControlFlow::Normal(val) => Ok(val),
        ControlFlow::Return(val) => Ok(val),
        ControlFlow::Throw(val, line, column) => Err(EvalError::Throw(val, line, column)),
        ControlFlow::Break(_) => Ok(Value::Undefined),
        ControlFlow::Continue(_) => Ok(Value::Undefined),
    }
}

/// belongs to src/js_object.rs module
pub(crate) fn handle_object_prototype_to_string<'gc>(mc: &MutationContext<'gc>, val: &Value<'gc>) -> Value<'gc> {
    let tag = match val {
        Value::Undefined => "Undefined",
        Value::Null => "Null",
        Value::String(_) => "String",
        Value::Number(_) => "Number",
        Value::Boolean(_) => "Boolean",
        Value::BigInt(_) => "BigInt",
        Value::Function(_) | Value::Closure(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..) => "Function",
        Value::Object(obj) => {
            if is_array(mc, obj) {
                "Array"
            } else if is_date_object(obj) {
                "Date"
            } else {
                "Object"
            }
        }
        _ => "Object",
    };
    Value::String(utf8_to_utf16(&format!("[object {}]", tag)))
}

pub fn evaluate_statements_with_context<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    statements: &[Statement],
) -> Result<ControlFlow<'gc>, EvalError<'gc>> {
    hoist_declarations(mc, env, statements)?;
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
                if let Some(expr) = expr_opt {
                    let val = match evaluate_expr(mc, env, expr) {
                        Ok(v) => v,
                        Err(e) => return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e)),
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
        StatementKind::Class(class_def) => {
            // Evaluate class definition and bind to environment
            // This initializes the class binding which was hoisted as Uninitialized
            if let Err(e) = crate::js_class::create_class_object(mc, &class_def.name, &class_def.extends, &class_def.members, env, true) {
                return Err(refresh_error_by_additional_stack_frame(mc, env, stmt.line, stmt.column, e.into()));
            }
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Import(specifiers, source) => {
            // Try to deduce base path from env or use current dir
            let base_path = if let Some(cell) = env_get(env, "__filepath") {
                if let Value::String(s) = cell.borrow().clone() {
                    Some(crate::unicode::utf16_to_utf8(&s))
                } else {
                    None
                }
            } else {
                None
            };

            let exports = crate::js_module::load_module(mc, source, base_path.as_deref())
                .map_err(|e| EvalError::Throw(Value::String(utf8_to_utf16(&e.message())), Some(stmt.line), Some(stmt.column)))?;

            if let Value::Object(exports_obj) = exports {
                for spec in specifiers {
                    match spec {
                        ImportSpecifier::Named(name, alias) => {
                            let binding_name = alias.as_ref().unwrap_or(name);

                            let val_ptr_res = obj_get_key_value(&exports_obj, &name.into());
                            let val = if let Ok(Some(cell)) = val_ptr_res {
                                cell.borrow().clone()
                            } else {
                                Value::Undefined
                            };
                            env_set(mc, env, binding_name, val)?;
                        }
                        ImportSpecifier::Default(name) => {
                            let val_ptr_res = obj_get_key_value(&exports_obj, &"default".into());
                            let val = if let Ok(Some(cell)) = val_ptr_res {
                                cell.borrow().clone()
                            } else {
                                Value::Undefined
                            };
                            env_set(mc, env, name, val)?;
                        }
                        ImportSpecifier::Namespace(name) => {
                            env_set(mc, env, name, Value::Object(exports_obj))?;
                        }
                    }
                }
            }
            *last_value = Value::Undefined;
            Ok(None)
        }
        StatementKind::Export(specifiers, inner_stmt) => {
            // 1. Evaluate inner statement if present, to bind variables in current env
            if let Some(stmt) = inner_stmt {
                // Recursively evaluate inner statement
                // Note: inner_stmt is a Box<Statement>. We need to call eval_res or evaluate_statements on it.
                // Since evaluate_statements expects a slice, we can wrap it.
                let mut stmts = vec![*stmt.clone()];
                match evaluate_statements(mc, env, &mut stmts) {
                    Ok(_) => {} // Declarations are hoisted or executed, binding should be in env
                    Err(e) => return Err(e),
                }

                // If inner stmt was a declaration, we need to export the declared names.
                // For now, we handle named exports via specifiers only for `export { ... }`.
                // For `export var x = 1`, the parser should have produced specifiers?
                // My parser implementation for export var/function didn't produce specifiers, just inner_stmt.
                // So we need to look at inner_stmt kind to determine what to export.

                match &stmt.kind {
                    StatementKind::Var(decls) => {
                        for (name, _) in decls {
                            if let Some(cell) = env_get(env, name) {
                                let val = cell.borrow().clone();
                                crate::core::eval::export_value(mc, env, name, val)?;
                            }
                        }
                    }
                    StatementKind::Let(decls) => {
                        for (name, _) in decls {
                            if let Some(cell) = env_get(env, name) {
                                let val = cell.borrow().clone();
                                crate::core::eval::export_value(mc, env, name, val)?;
                            }
                        }
                    }
                    StatementKind::Const(decls) => {
                        for (name, _) in decls {
                            if let Some(cell) = env_get(env, name) {
                                let val = cell.borrow().clone();
                                crate::core::eval::export_value(mc, env, name, val)?;
                            }
                        }
                    }
                    StatementKind::FunctionDeclaration(name, _, _, _) => {
                        if let Some(cell) = env_get(env, name) {
                            let val = cell.borrow().clone();
                            crate::core::eval::export_value(mc, env, name, val)?;
                        }
                    }
                    _ => {}
                }
            }

            // 2. Handle explicit specifiers
            for spec in specifiers {
                match spec {
                    crate::core::statement::ExportSpecifier::Named(name, alias) => {
                        // export { name as alias }
                        // value should be in env
                        if let Some(cell) = env_get(env, name) {
                            let val = cell.borrow().clone();
                            let export_name = alias.as_ref().unwrap_or(name);
                            crate::core::eval::export_value(mc, env, export_name, val)?;
                        } else {
                            return Err(EvalError::Js(raise_reference_error!(format!("{} is not defined", name))));
                        }
                    }
                    crate::core::statement::ExportSpecifier::Default(expr) => {
                        // export default expr
                        let val = evaluate_expr(mc, env, expr)?;
                        crate::core::eval::export_value(mc, env, "default", val)?;
                    }
                }
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
        StatementKind::FunctionDeclaration(_name, _params, _body, _) => {
            // Function declarations are hoisted, so they are already defined.
            Ok(None)
        }
        StatementKind::Throw(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            if let Value::Object(obj) = val {
                if is_error(&val) {
                    let mut filename = String::new();
                    if let Ok(Some(val_ptr)) = obj_get_key_value(env, &"__filepath".into()) {
                        if let Value::String(s) = &*val_ptr.borrow() {
                            filename = utf16_to_utf8(s);
                        }
                    }
                    let frame = format!("at <anonymous> ({}:{}:{})", filename, stmt.line, stmt.column);
                    let current_stack = obj.borrow().get_property("stack").unwrap_or_default();
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
        StatementKind::Label(label, stmt) => {
            let mut stmts = vec![*stmt.clone()];
            let res = evaluate_statements_with_context(mc, env, &mut stmts)?;
            match res {
                ControlFlow::Break(Some(ref l)) if l == label => Ok(None),
                other => Ok(Some(other)),
            }
        }
        StatementKind::Break(label) => Ok(Some(ControlFlow::Break(label.clone()))),
        StatementKind::Continue(label) => Ok(Some(ControlFlow::Continue(label.clone()))),
        StatementKind::For(init, test, update, body) => {
            let loop_env = new_js_object_data(mc);
            loop_env.borrow_mut(mc).prototype = Some(*env);
            if let Some(init_stmt) = init {
                evaluate_statements_with_context(mc, &loop_env, std::slice::from_ref(init_stmt))?;
            }
            loop {
                if let Some(test_expr) = test {
                    let cond_val = evaluate_expr(mc, &loop_env, test_expr)?;
                    let is_true = match cond_val {
                        Value::Boolean(b) => b,
                        Value::Number(n) => n != 0.0 && !n.is_nan(),
                        Value::String(s) => !s.is_empty(),
                        Value::Null | Value::Undefined => false,
                        Value::Object(_) => true,
                        _ => false,
                    };
                    if !is_true {
                        break;
                    }
                }
                let res = evaluate_statements_with_context(mc, &loop_env, body)?;
                match res {
                    ControlFlow::Normal(v) => *last_value = v,
                    ControlFlow::Break(label) => {
                        if label.is_none() {
                            break;
                        }
                        return Ok(Some(ControlFlow::Break(label)));
                    }
                    ControlFlow::Continue(label) => {
                        if label.is_some() {
                            return Ok(Some(ControlFlow::Continue(label)));
                        }
                    }
                    ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                    ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                }
                if let Some(update_stmt) = update {
                    evaluate_statements_with_context(mc, &loop_env, std::slice::from_ref(update_stmt))?;
                }
            }
            Ok(None)
        }
        StatementKind::While(cond, body) => {
            let loop_env = new_js_object_data(mc);
            loop_env.borrow_mut(mc).prototype = Some(*env);
            loop {
                let cond_val = evaluate_expr(mc, &loop_env, cond)?;
                let is_true = match cond_val {
                    Value::Boolean(b) => b,
                    Value::Number(n) => n != 0.0 && !n.is_nan(),
                    Value::String(s) => !s.is_empty(),
                    Value::Null | Value::Undefined => false,
                    Value::Object(_) => true,
                    _ => false,
                };
                if !is_true {
                    break;
                }
                let res = evaluate_statements_with_context(mc, &loop_env, body)?;
                match res {
                    ControlFlow::Normal(v) => *last_value = v,
                    ControlFlow::Break(label) => {
                        if label.is_none() {
                            break;
                        }
                        return Ok(Some(ControlFlow::Break(label)));
                    }
                    ControlFlow::Continue(label) => {
                        if label.is_some() {
                            return Ok(Some(ControlFlow::Continue(label)));
                        }
                    }
                    ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                    ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                }
            }
            Ok(None)
        }
        StatementKind::DoWhile(body, cond) => {
            let loop_env = new_js_object_data(mc);
            loop_env.borrow_mut(mc).prototype = Some(*env);
            loop {
                let res = evaluate_statements_with_context(mc, &loop_env, body)?;
                match res {
                    ControlFlow::Normal(v) => *last_value = v,
                    ControlFlow::Break(label) => {
                        if label.is_none() {
                            break;
                        }
                        return Ok(Some(ControlFlow::Break(label)));
                    }
                    ControlFlow::Continue(label) => {
                        if label.is_some() {
                            return Ok(Some(ControlFlow::Continue(label)));
                        }
                    }
                    ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                    ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                }
                let cond_val = evaluate_expr(mc, &loop_env, cond)?;
                let is_true = match cond_val {
                    Value::Boolean(b) => b,
                    Value::Number(n) => n != 0.0 && !n.is_nan(),
                    Value::String(s) => !s.is_empty(),
                    Value::Null | Value::Undefined => false,
                    Value::Object(_) => true,
                    _ => false,
                };
                if !is_true {
                    break;
                }
            }
            Ok(None)
        }
        StatementKind::ForOf(var_name, iterable, body) => {
            let iter_val = evaluate_expr(mc, env, iterable)?;
            if let Value::Object(obj) = iter_val {
                if is_array(mc, &obj) {
                    let len_val = obj_get_key_value(&obj, &"length".into())?.unwrap().borrow().clone();
                    let len = match len_val {
                        Value::Number(n) => n as usize,
                        _ => 0,
                    };
                    let loop_env = new_js_object_data(mc);
                    loop_env.borrow_mut(mc).prototype = Some(*env);
                    for i in 0..len {
                        let val = obj_get_key_value(&obj, &i.to_string().into())?.unwrap().borrow().clone();
                        env_set(mc, &loop_env, var_name, val)?;
                        let res = evaluate_statements_with_context(mc, &loop_env, body)?;
                        match res {
                            ControlFlow::Normal(v) => *last_value = v,
                            ControlFlow::Break(label) => {
                                if label.is_none() {
                                    break;
                                }
                                return Ok(Some(ControlFlow::Break(label)));
                            }
                            ControlFlow::Continue(label) => {
                                if label.is_some() {
                                    return Ok(Some(ControlFlow::Continue(label)));
                                }
                            }
                            ControlFlow::Return(v) => return Ok(Some(ControlFlow::Return(v))),
                            ControlFlow::Throw(v, l, c) => return Ok(Some(ControlFlow::Throw(v, l, c))),
                        }
                    }
                    return Ok(None);
                }
            }
            Err(EvalError::Js(raise_type_error!("ForOf only supports Arrays currently")))
        }
        _ => Ok(None),
    }
}

pub fn export_value<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str, val: Value<'gc>) -> Result<(), EvalError<'gc>> {
    if let Some(exports_cell) = env_get(env, "exports") {
        let exports = exports_cell.borrow().clone();
        if let Value::Object(exports_obj) = exports {
            obj_set_key_value(mc, &exports_obj, &name.into(), val).map_err(|e| EvalError::Js(e))?;
            return Ok(());
        }
    }

    if let Some(module_cell) = env_get(env, "module") {
        let module = module_cell.borrow().clone();
        if let Value::Object(module_obj) = module {
            if let Ok(Some(exports_val)) = obj_get_key_value(&module_obj, &"exports".into()) {
                if let Value::Object(exports_obj) = &*exports_val.borrow() {
                    obj_set_key_value(mc, exports_obj, &name.into(), val).map_err(|e| EvalError::Js(e))?;
                }
            }
        }
    }
    Ok(())
}

fn refresh_error_by_additional_stack_frame<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    line: usize,
    column: usize,
    mut e: EvalError<'gc>,
) -> EvalError<'gc> {
    let mut filename = String::new();
    if let Ok(Some(val_ptr)) = obj_get_key_value(env, &"__filepath".into()) {
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
        let current_stack = obj.borrow().get_property("stack").unwrap_or_default();
        let new_stack = format!("{}\n    {}", current_stack, frame);
        obj.borrow_mut(mc).set_property(mc, "stack", new_stack.into());
    }
    e
}

fn get_primitive_prototype_property<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj_val: &Value<'gc>,
    key: &PropertyKey<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let proto_name = match obj_val {
        Value::BigInt(_) => "BigInt",
        Value::Number(_) => "Number",
        Value::String(_) => "String",
        Value::Boolean(_) => "Boolean",
        Value::Closure(_) | Value::Function(_) | Value::AsyncClosure(_) | Value::GeneratorFunction(..) => "Function",
        _ => return Ok(Value::Undefined),
    };

    if let Ok(ctor) = evaluate_var(mc, env, proto_name) {
        if let Value::Object(ctor_obj) = ctor {
            if let Some(proto_ref) = obj_get_key_value(&ctor_obj, &"prototype".into())? {
                if let Value::Object(proto) = &*proto_ref.borrow() {
                    if let Some(val) = obj_get_key_value(proto, key)? {
                        return Ok(val.borrow().clone());
                    }
                }
            }
        }
    }
    Ok(Value::Undefined)
}

pub fn evaluate_expr<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, expr: &Expr) -> Result<Value<'gc>, EvalError<'gc>> {
    match expr {
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::StringLit(s) => Ok(Value::String(s.clone())),
        Expr::Boolean(b) => Ok(Value::Boolean(*b)),
        Expr::Null => Ok(Value::Null),
        Expr::Undefined => Ok(Value::Undefined),
        Expr::Var(name, _, _) => Ok(evaluate_var(mc, env, name)?),
        Expr::Assign(target, value_expr) => {
            let val = evaluate_expr(mc, env, value_expr)?;
            match &**target {
                Expr::Var(name, _, _) => {
                    env_set_recursive(mc, env, name, val.clone())?;
                    Ok(val)
                }
                Expr::Property(obj_expr, key) => {
                    let obj_val = evaluate_expr(mc, env, obj_expr)?;
                    if let Value::Object(obj) = obj_val {
                        let key_val = PropertyKey::from(key.to_string());
                        set_property_with_accessors(mc, env, &obj, &key_val, val.clone())?;
                        Ok(val)
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Cannot assign to property of non-object")))
                    }
                }
                Expr::Index(obj_expr, key_expr) => {
                    let obj_val = evaluate_expr(mc, env, obj_expr)?;
                    let key_val_res = evaluate_expr(mc, env, key_expr)?;
                    let key_str = value_to_string(&key_val_res);
                    if let Value::Object(obj) = obj_val {
                        set_property_with_accessors(mc, env, &obj, &PropertyKey::from(key_str), val.clone())?;
                        Ok(val)
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Cannot assign to property of non-object")))
                    }
                }
                _ => todo!("Assignment target not supported"),
            }
        }
        Expr::AddAssign(target, value_expr) => {
            let val = evaluate_expr(mc, env, value_expr)?;
            if let Expr::Var(name, _, _) = &**target {
                let current = evaluate_var(mc, env, name)?;
                let new_val = match (current, val) {
                    (Value::Number(ln), Value::Number(rn)) => Value::Number(ln + rn),
                    (Value::String(ls), Value::String(rs)) => {
                        let mut res = ls.clone();
                        res.extend(rs);
                        Value::String(res)
                    }
                    (Value::String(ls), other) => {
                        let mut res = ls.clone();
                        res.extend(utf8_to_utf16(&value_to_string(&other)));
                        Value::String(res)
                    }
                    (other, Value::String(rs)) => {
                        let mut res = utf8_to_utf16(&value_to_string(&other));
                        res.extend(rs);
                        Value::String(res)
                    }
                    _ => return Err(EvalError::Js(raise_eval_error!("AddAssign types invalid"))),
                };
                env_set_recursive(mc, env, name, new_val.clone())?;
                Ok(new_val)
            } else {
                Err(EvalError::Js(raise_eval_error!("AddAssign only for variables")))
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
                        (Value::Object(l), Value::Object(r)) => Gc::ptr_eq(l, r),
                        (Value::Closure(l), Value::Closure(r)) => Gc::ptr_eq(l, r),
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
                        (Value::Object(l), Value::Object(r)) => Gc::ptr_eq(l, r),
                        (Value::Closure(l), Value::Closure(r)) => Gc::ptr_eq(l, r),
                        _ => false,
                    };
                    Ok(Value::Boolean(!eq))
                }
                BinaryOp::In => {
                    if let Value::Object(obj) = r_val {
                        let key = match l_val {
                            Value::String(s) => utf16_to_utf8(&s),
                            Value::Number(n) => n.to_string(),
                            _ => value_to_string(&l_val),
                        };
                        let present = obj_get_key_value(&obj, &key.into())?.is_some();
                        Ok(Value::Boolean(present))
                    } else {
                        Err(EvalError::Js(crate::raise_type_error!("Right-hand side of 'in' must be an object")))
                    }
                }
                BinaryOp::Equal => {
                    let eq = match (l_val, r_val) {
                        (Value::Null, Value::Undefined) => true,
                        (Value::Undefined, Value::Null) => true,
                        (Value::Number(l), Value::Number(r)) => l == r,
                        (Value::String(l), Value::String(r)) => l == r,
                        (Value::Boolean(l), Value::Boolean(r)) => l == r,
                        (Value::Null, Value::Null) => true,
                        (Value::Undefined, Value::Undefined) => true,
                        (Value::Object(l), Value::Object(r)) => Gc::ptr_eq(l, r),
                        (Value::Closure(l), Value::Closure(r)) => Gc::ptr_eq(l, r),
                        _ => false,
                    };
                    Ok(Value::Boolean(eq))
                }
                BinaryOp::NotEqual => {
                    let eq = match (l_val, r_val) {
                        (Value::Null, Value::Undefined) => true,
                        (Value::Undefined, Value::Null) => true,
                        (Value::Number(l), Value::Number(r)) => l == r,
                        (Value::String(l), Value::String(r)) => l == r,
                        (Value::Boolean(l), Value::Boolean(r)) => l == r,
                        (Value::Null, Value::Null) => true,
                        (Value::Undefined, Value::Undefined) => true,
                        (Value::Object(l), Value::Object(r)) => Gc::ptr_eq(l, r),
                        (Value::Closure(l), Value::Closure(r)) => Gc::ptr_eq(l, r),
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
                BinaryOp::Mod => {
                    if let (Value::Number(ln), Value::Number(rn)) = (l_val, r_val) {
                        Ok(Value::Number(ln % rn))
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Binary Mod only for numbers")))
                    }
                }
                BinaryOp::InstanceOf => match r_val {
                    Value::Object(ctor) => {
                        if let Value::Object(obj) = l_val {
                            let res = crate::js_class::is_instance_of(&obj, &ctor)?;
                            Ok(Value::Boolean(res))
                        } else {
                            Ok(Value::Boolean(false))
                        }
                    }
                    _ => Err(EvalError::Js(crate::raise_type_error!(
                        "Right-hand side of 'instanceof' is not an object"
                    ))),
                },
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
        Expr::Conditional(cond, then_expr, else_expr) => {
            let val = evaluate_expr(mc, env, cond)?;
            let is_true = match val {
                Value::Boolean(b) => b,
                Value::Number(n) => n != 0.0 && !n.is_nan(),
                Value::String(s) => !s.is_empty(),
                Value::Null | Value::Undefined => false,
                Value::Object(_) => true,
                _ => false,
            };

            if is_true {
                evaluate_expr(mc, env, then_expr)
            } else {
                evaluate_expr(mc, env, else_expr)
            }
        }
        Expr::Object(properties) => {
            let obj = crate::core::new_js_object_data(mc);
            if let Some(obj_val) = env_get(env, "Object") {
                if let Value::Object(obj_ctor) = &*obj_val.borrow() {
                    if let Some(proto_val) = obj_get_key_value(obj_ctor, &"prototype".into())? {
                        if let Value::Object(proto) = &*proto_val.borrow() {
                            obj.borrow_mut(mc).prototype = Some(*proto);
                        }
                    }
                }
            }

            for (key_expr, val_expr, is_computed) in properties {
                let key_val = evaluate_expr(mc, env, key_expr)?;
                let val = evaluate_expr(mc, env, val_expr)?;

                let key_str = match key_val {
                    Value::String(s) => utf16_to_utf8(&s),
                    Value::Number(n) => n.to_string(),
                    Value::Boolean(b) => b.to_string(),
                    Value::BigInt(b) => b.to_string(),
                    Value::Undefined => "undefined".to_string(),
                    Value::Null => "null".to_string(),
                    _ => "object".to_string(),
                };
                obj_set_key_value(mc, &obj, &PropertyKey::from(key_str), val)?;
            }
            Ok(Value::Object(obj))
        }
        Expr::Array(elements) => {
            let arr_obj = create_array(mc, env)?;

            for (i, elem_opt) in elements.iter().enumerate() {
                if let Some(elem) = elem_opt {
                    let val = evaluate_expr(mc, env, elem)?;
                    obj_set_key_value(mc, &arr_obj, &i.to_string().into(), val)?;
                }
            }
            set_array_length(mc, &arr_obj, elements.len())?;
            Ok(Value::Object(arr_obj))
        }
        Expr::Function(name, params, body) => {
            let mut body_clone = body.clone();
            Ok(evaluate_function_expression(mc, env, name.clone(), params, &mut body_clone)?)
        }
        Expr::Call(func_expr, args) => {
            let (func_val, this_val) = match &**func_expr {
                Expr::Property(obj_expr, key) => {
                    let obj_val = evaluate_expr(mc, env, obj_expr)?;
                    let f_val = if let Value::Object(obj) = &obj_val {
                        if let Some(val) = obj_get_key_value(obj, &key.as_str().into())? {
                            val.borrow().clone()
                        } else {
                            Value::Undefined
                        }
                    } else if matches!(obj_val, Value::Undefined | Value::Null) {
                        return Err(EvalError::Js(raise_eval_error!("Cannot read properties of null or undefined")));
                    } else {
                        get_primitive_prototype_property(mc, env, &obj_val, &key.as_str().into())?
                    };
                    (f_val, Some(obj_val))
                }
                _ => (evaluate_expr(mc, env, func_expr)?, None),
            };

            let mut eval_args = Vec::new();
            for arg in args {
                eval_args.push(evaluate_expr(mc, env, arg)?);
            }

            match func_val {
                Value::Function(name) => {
                    if let Some(res) = call_native_function(mc, &name, this_val.clone(), &eval_args, env)? {
                        return Ok(res);
                    }
                    if name == "eval" {
                        let first_arg = eval_args.get(0).cloned().unwrap_or(Value::Undefined);
                        if let Value::String(script_str) = first_arg {
                            let script = utf16_to_utf8(&script_str);
                            let mut tokens = tokenize(&script).map_err(EvalError::Js)?;
                            if tokens.last().map(|td| td.token == Token::EOF).unwrap_or(false) {
                                tokens.pop();
                            }
                            let mut index = 0;
                            let mut statements = parse_statements(&tokens, &mut index).map_err(EvalError::Js)?;
                            // eval executes in the current environment
                            match evaluate_statements(mc, env, &mut statements) {
                                Ok(v) => Ok(v),
                                Err(e) => Err(e),
                            }
                        } else {
                            Ok(first_arg)
                        }
                    } else if let Some(method_name) = name.strip_prefix("console.") {
                        crate::js_console::handle_console_method(mc, method_name, &eval_args, env)
                    } else if let Some(method) = name.strip_prefix("os.") {
                        #[cfg(feature = "os")]
                        {
                            let this_val = this_val.clone().unwrap_or(Value::Object(*env));
                            Ok(crate::js_os::handle_os_method(mc, this_val, method, &eval_args, env).map_err(EvalError::Js)?)
                        }
                        #[cfg(not(feature = "os"))]
                        {
                            Err(EvalError::Js(raise_eval_error!(
                                "os module not enabled. Recompile with --features os"
                            )))
                        }
                    } else if let Some(method) = name.strip_prefix("std.") {
                        #[cfg(feature = "std")]
                        {
                            match method {
                                "sprintf" => Ok(crate::js_std::sprintf::handle_sprintf_call(&eval_args).map_err(EvalError::Js)?),
                                "tmpfile" => Ok(crate::js_std::tmpfile::create_tmpfile(mc).map_err(EvalError::Js)?),
                                _ => Err(EvalError::Js(raise_eval_error!(format!("std method '{}' not implemented", method)))),
                            }
                        }
                        #[cfg(not(feature = "std"))]
                        {
                            Err(EvalError::Js(raise_eval_error!(
                                "std module not enabled. Recompile with --features std"
                            )))
                        }
                    } else if let Some(method) = name.strip_prefix("tmp.") {
                        #[cfg(feature = "std")]
                        {
                            if let Some(Value::Object(this_obj)) = this_val {
                                Ok(crate::js_std::tmpfile::handle_file_method(&this_obj, method, &eval_args).map_err(EvalError::Js)?)
                            } else {
                                Err(EvalError::Js(raise_eval_error!(
                                    "TypeError: tmp method called on incompatible receiver"
                                )))
                            }
                        }
                        #[cfg(not(feature = "std"))]
                        {
                            Err(EvalError::Js(raise_eval_error!(
                                "std module (tmpfile) not enabled. Recompile with --features std"
                            )))
                        }
                    } else if let Some(method) = name.strip_prefix("BigInt.prototype.") {
                        let this_v = this_val.clone().unwrap_or(Value::Undefined);
                        Ok(crate::js_bigint::handle_bigint_object_method(this_v, method, &eval_args).map_err(EvalError::Js)?)
                    } else if name == "Object.prototype.toString" {
                        let this_v = this_val.clone().unwrap_or(Value::Undefined);
                        Ok(handle_object_prototype_to_string(mc, &this_v))
                    } else if let Some(method) = name.strip_prefix("BigInt.") {
                        Ok(crate::js_bigint::handle_bigint_static_method(method, &eval_args, env).map_err(EvalError::Js)?)
                    } else if let Some(method) = name.strip_prefix("Number.prototype.") {
                        Ok(handle_number_prototype_method(this_val.clone(), method, &eval_args).map_err(EvalError::Js)?)
                    } else if let Some(method) = name.strip_prefix("Number.") {
                        Ok(handle_number_static_method(method, &eval_args).map_err(EvalError::Js)?)
                    } else if let Some(method) = name.strip_prefix("Math.") {
                        Ok(handle_math_call(mc, method, &eval_args, env).map_err(EvalError::Js)?)
                    } else if let Some(method) = name.strip_prefix("JSON.") {
                        Ok(handle_json_method(mc, method, &eval_args, env).map_err(EvalError::Js)?)
                    } else if let Some(method) = name.strip_prefix("Date.prototype.") {
                        if let Some(this_obj) = this_val {
                            Ok(handle_date_method(mc, &this_obj, method, &eval_args, env).map_err(EvalError::Js)?)
                        } else {
                            Err(EvalError::Js(raise_eval_error!(
                                "TypeError: Date method called on incompatible receiver"
                            )))
                        }
                    } else if let Some(method) = name.strip_prefix("Date.") {
                        Ok(handle_date_static_method(method, &eval_args)?)
                    } else if name.starts_with("String.") {
                        if name == "String.fromCharCode" {
                            Ok(string_from_char_code(mc, &eval_args, env)?)
                        } else if name == "String.fromCodePoint" {
                            Ok(string_from_code_point(mc, &eval_args, env)?)
                        } else if name == "String.raw" {
                            Ok(string_raw(mc, &eval_args, env)?)
                        } else if name.starts_with("String.prototype.") {
                            let method = &name[17..];
                            // String instance methods need a 'this' value which should be the first argument if called directly?
                            // But here we are calling the function object directly.
                            // Usually instance methods are called via method call syntax (obj.method()), which sets 'this'.
                            // If we are here, it means we called the function object directly, e.g. String.prototype.slice.call(str, ...)
                            // But our current implementation of function calls doesn't handle 'this' binding for native functions well yet
                            // unless it's a method call.
                            // However, if we are calling it as a method of String.prototype, 'this' should be passed.
                            // But here 'name' is just a string identifier we assigned to the function.
                            // We need to know the 'this' value.
                            // For now, let's assume the first argument is 'this' if it's called as a standalone function?
                            // No, that's not how it works.
                            // If we are here, it means we are executing the native function body.
                            // We need to access the 'this' binding from the environment or context.
                            // But our native functions don't have a captured environment with 'this'.
                            // We need to change how we handle native function calls to include 'this'.

                            // Wait, the current architecture seems to rely on the caller to handle 'this' or pass it?
                            // In `evaluate_expr` for `Expr::Call`, we don't seem to pass 'this' explicitly for native functions
                            // unless it was a method call.

                            // Let's look at how `Expr::Call` handles method calls.
                            // It evaluates `func_expr`. If it's a property access, it sets `this`.
                            // But `evaluate_expr` returns a `Value`, not a reference.
                            // So we lose the `this` context unless we handle `Expr::Call` specially for property access.

                            // Actually, `Expr::Call` implementation in `eval.rs` (lines 600+) just evaluates `func_expr`.
                            // It doesn't seem to handle `this` binding for method calls properly yet?
                            // Ah, I see `Expr::Call` logic is split.
                            // Let's check `Expr::Call` implementation again.

                            Err(EvalError::Js(raise_eval_error!(
                                "String prototype methods not fully supported in direct calls yet"
                            )))
                        } else {
                            Err(EvalError::Js(raise_eval_error!(format!("Unknown String function: {}", name))))
                        }
                    } else if name.starts_with("Array.") {
                        if let Some(method) = name.strip_prefix("Array.prototype.") {
                            let this_v = this_val.clone().unwrap_or(Value::Undefined);
                            if let Value::Object(obj) = this_v {
                                Ok(crate::js_array::handle_array_instance_method(mc, &obj, method, &eval_args, env)?)
                            } else {
                                Err(EvalError::Js(raise_eval_error!(
                                    "TypeError: Array method called on non-object receiver"
                                )))
                            }
                        } else {
                            let method = &name[6..];
                            Ok(handle_array_static_method(mc, method, &eval_args, env)?)
                        }
                    } else if name.starts_with("Map.") {
                        if let Some(method) = name.strip_prefix("Map.prototype.") {
                            let this_v = this_val.clone().unwrap_or(Value::Undefined);
                            if let Value::Object(obj) = this_v {
                                if let Some(map_val) = obj_get_key_value(&obj, &"__map__".into())? {
                                    if let Value::Map(map_ptr) = &*map_val.borrow() {
                                        Ok(crate::js_map::handle_map_instance_method(mc, map_ptr, method, &eval_args, env)?)
                                    } else {
                                        Err(EvalError::Js(raise_eval_error!(
                                            "TypeError: Map.prototype method called on incompatible receiver"
                                        )))
                                    }
                                } else {
                                    Err(EvalError::Js(raise_eval_error!(
                                        "TypeError: Map.prototype method called on incompatible receiver"
                                    )))
                                }
                            } else if let Value::Map(map_ptr) = this_v {
                                Ok(crate::js_map::handle_map_instance_method(mc, &map_ptr, method, &eval_args, env)?)
                            } else {
                                Err(EvalError::Js(raise_eval_error!(
                                    "TypeError: Map.prototype method called on non-object receiver"
                                )))
                            }
                        } else {
                            Err(EvalError::Js(raise_eval_error!(format!("Unknown Map function: {}", name))))
                        }
                    } else if name.starts_with("Set.") {
                        if let Some(method) = name.strip_prefix("Set.prototype.") {
                            let this_v = this_val.clone().unwrap_or(Value::Undefined);
                            if let Value::Object(obj) = this_v {
                                if let Some(set_val) = obj_get_key_value(&obj, &"__set__".into())? {
                                    if let Value::Set(set_ptr) = &*set_val.borrow() {
                                        Ok(crate::js_set::handle_set_instance_method(
                                            mc,
                                            set_ptr,
                                            this_v.clone(),
                                            method,
                                            &eval_args,
                                            env,
                                        )?)
                                    } else {
                                        Err(EvalError::Js(raise_eval_error!(
                                            "TypeError: Set.prototype method called on incompatible receiver"
                                        )))
                                    }
                                } else {
                                    Err(EvalError::Js(raise_eval_error!(
                                        "TypeError: Set.prototype method called on incompatible receiver"
                                    )))
                                }
                            } else if let Value::Set(set_ptr) = this_v {
                                Ok(crate::js_set::handle_set_instance_method(
                                    mc,
                                    &set_ptr,
                                    Value::Set(set_ptr.clone()),
                                    method,
                                    &eval_args,
                                    env,
                                )?)
                            } else {
                                Err(EvalError::Js(raise_eval_error!(
                                    "TypeError: Set.prototype method called on non-object receiver"
                                )))
                            }
                        } else {
                            Err(EvalError::Js(raise_eval_error!(format!("Unknown Set function: {}", name))))
                        }
                    } else {
                        Err(EvalError::Js(raise_eval_error!(format!("Unknown native function: {}", name))))
                    }
                }
                Value::Object(obj) => {
                    if let Some(cl_ptr) = obj_get_key_value(&obj, &"__closure__".into())? {
                        match &*cl_ptr.borrow() {
                            Value::Closure(cl) => {
                                let call_env = crate::core::new_js_object_data(mc);
                                call_env.borrow_mut(mc).prototype = Some(cl.env);
                                call_env.borrow_mut(mc).is_function_scope = true;
                                if let Some(tv) = &this_val {
                                    obj_set_key_value(mc, &call_env, &"this".into(), tv.clone())?;
                                }

                                for (i, param) in cl.params.iter().enumerate() {
                                    match param {
                                        DestructuringElement::Variable(name, _) => {
                                            let arg_val = eval_args.get(i).cloned().unwrap_or(Value::Undefined);
                                            env_set(mc, &call_env, name, arg_val)?;
                                        }
                                        DestructuringElement::Rest(name) => {
                                            let rest_args = if i < eval_args.len() { eval_args[i..].to_vec() } else { Vec::new() };
                                            let array_obj = crate::js_array::create_array(mc, env)?;
                                            for (j, val) in rest_args.iter().enumerate() {
                                                obj_set_key_value(mc, &array_obj, &PropertyKey::from(j.to_string()), val.clone())?;
                                            }
                                            obj_set_key_value(mc, &array_obj, &"length".into(), Value::Number(rest_args.len() as f64))?;
                                            env_set(mc, &call_env, name, Value::Object(array_obj))?;
                                        }
                                        _ => {}
                                    }
                                }
                                let mut body_clone = cl.body.clone();
                                match evaluate_statements(mc, &call_env, &mut body_clone) {
                                    Ok(v) => Ok(v),
                                    Err(mut e) => {
                                        // Avoid borrowing obj while modifying err_obj if they might be related or if obj is already borrowed?
                                        // obj is the function object.
                                        // We need its name.
                                        let name_opt = obj.borrow().get_property("name");

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

                                                    let stack_str_opt = err_obj.borrow().get_property("stack");
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
                    } else if let Some(_) = obj_get_key_value(&obj, &"__class_def__".into())? {
                        return Err(EvalError::Js(crate::raise_type_error!(
                            "Class constructor cannot be invoked without 'new'"
                        )));
                    } else if let Some(native_name) = obj_get_key_value(&obj, &"__native_ctor".into())? {
                        match &*native_name.borrow() {
                            Value::String(name) => {
                                if name == &crate::unicode::utf8_to_utf16("String") {
                                    Ok(crate::js_string::string_constructor(mc, &eval_args, env)?)
                                } else if name == &crate::unicode::utf8_to_utf16("Number") {
                                    Ok(number_constructor(&eval_args, env).map_err(EvalError::Js)?)
                                } else if name == &crate::unicode::utf8_to_utf16("BigInt") {
                                    Ok(bigint_constructor(&eval_args, env)?)
                                } else {
                                    Err(EvalError::Js(raise_eval_error!("Not a function")))
                                }
                            }
                            _ => Err(EvalError::Js(raise_eval_error!("Not a function"))),
                        }
                    } else {
                        Err(EvalError::Js(raise_eval_error!("Not a function")))
                    }
                }
                Value::Closure(cl) => {
                    let call_env = crate::core::new_js_object_data(mc);
                    call_env.borrow_mut(mc).prototype = Some(cl.env);
                    call_env.borrow_mut(mc).is_function_scope = true;
                    if let Some(tv) = &this_val {
                        obj_set_key_value(mc, &call_env, &"this".into(), tv.clone())?;
                    }

                    for (i, param) in cl.params.iter().enumerate() {
                        match param {
                            DestructuringElement::Variable(name, _) => {
                                let arg_val = eval_args.get(i).cloned().unwrap_or(Value::Undefined);
                                env_set(mc, &call_env, name, arg_val)?;
                            }
                            DestructuringElement::Rest(name) => {
                                let rest_args = if i < eval_args.len() { eval_args[i..].to_vec() } else { Vec::new() };
                                let array_obj = crate::js_array::create_array(mc, env)?;
                                for (j, val) in rest_args.iter().enumerate() {
                                    obj_set_key_value(mc, &array_obj, &PropertyKey::from(j.to_string()), val.clone())?;
                                }
                                obj_set_key_value(mc, &array_obj, &"length".into(), Value::Number(rest_args.len() as f64))?;
                                env_set(mc, &call_env, name, Value::Object(array_obj))?;
                            }
                            _ => {}
                        }
                    }
                    let mut body_clone = cl.body.clone();
                    match evaluate_statements(mc, &call_env, &mut body_clone) {
                        Ok(v) => Ok(v),
                        Err(e) => Err(e),
                    }
                }
                _ => Err(EvalError::Js(raise_eval_error!(format!("Value {func_val:?} is not callable yet")))),
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
                    if let Some(cl_ptr) = obj_get_key_value(&obj, &"__closure__".into())? {
                        match &*cl_ptr.borrow() {
                            Value::Closure(cl) => {
                                // 1. Create instance
                                let instance = crate::core::new_js_object_data(mc);

                                // 2. Set prototype
                                if let Ok(Some(proto_val)) = obj_get_key_value(&obj, &"prototype".into()) {
                                    if let Value::Object(proto_obj) = &*proto_val.borrow() {
                                        instance.borrow_mut(mc).prototype = Some(*proto_obj);
                                        obj_set_key_value(mc, &instance, &"__proto__".into(), Value::Object(*proto_obj))?;
                                    } else {
                                        // Fallback to Object.prototype
                                        if let Some(obj_val) = env_get(env, "Object") {
                                            if let Value::Object(obj_ctor) = &*obj_val.borrow() {
                                                if let Ok(Some(obj_proto_val)) = obj_get_key_value(obj_ctor, &"prototype".into()) {
                                                    if let Value::Object(obj_proto) = &*obj_proto_val.borrow() {
                                                        instance.borrow_mut(mc).prototype = Some(*obj_proto);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                let call_env = crate::core::new_js_object_data(mc);
                                call_env.borrow_mut(mc).prototype = Some(cl.env);
                                call_env.borrow_mut(mc).is_function_scope = true;
                                obj_set_key_value(mc, &call_env, &"this".into(), Value::Object(instance))?;

                                for (i, param) in cl.params.iter().enumerate() {
                                    match param {
                                        DestructuringElement::Variable(name, _) => {
                                            let arg_val = eval_args.get(i).cloned().unwrap_or(Value::Undefined);
                                            env_set(mc, &call_env, name, arg_val)?;
                                        }
                                        DestructuringElement::Rest(name) => {
                                            let rest_args = if i < eval_args.len() { eval_args[i..].to_vec() } else { Vec::new() };
                                            let array_obj = crate::js_array::create_array(mc, env)?;
                                            for (j, val) in rest_args.iter().enumerate() {
                                                obj_set_key_value(mc, &array_obj, &PropertyKey::from(j.to_string()), val.clone())?;
                                            }
                                            obj_set_key_value(mc, &array_obj, &"length".into(), Value::Number(rest_args.len() as f64))?;
                                            env_set(mc, &call_env, name, Value::Object(array_obj))?;
                                        }
                                        _ => {}
                                    }
                                }
                                let mut body_clone = cl.body.clone();
                                let result = evaluate_statements(mc, &call_env, &mut body_clone)?;
                                if let Value::Object(_) = result {
                                    Ok(result)
                                } else {
                                    Ok(Value::Object(instance))
                                }
                            }
                            _ => Err(EvalError::Js(raise_eval_error!("Not a constructor"))),
                        }
                    } else if let Some(class_def_val) = obj_get_key_value(&obj, &"__class_def__".into())? {
                        // Delegate to js_class::evaluate_new
                        // We need to pass evaluated arguments.
                        // But js_class::evaluate_new takes (mc, env, constructor_val, evaluated_args)
                        // constructor_val is func_val.
                        // evaluated_args is eval_args.

                        // Note: We need to return Value::Object(instance) but evaluate_new returns Result<Value>.
                        // So we return Ok(evaluate_new(...)?).

                        let val = crate::js_class::evaluate_new(mc, env, func_val.clone(), &eval_args).map_err(|e| EvalError::Js(e))?;
                        return Ok(val);
                    } else {
                        if let Some(native_name) = obj_get_key_value(&obj, &"__native_ctor".into())? {
                            if let Value::String(name) = &*native_name.borrow() {
                                let name_str = crate::unicode::utf16_to_utf8(name);
                                if matches!(
                                    name_str.as_str(),
                                    "Error" | "ReferenceError" | "TypeError" | "RangeError" | "SyntaxError"
                                ) {
                                    let msg = eval_args.first().cloned().unwrap_or(Value::Undefined);
                                    let prototype = if let Some(proto_val) = obj_get_key_value(&obj, &"prototype".into())?
                                        && let Value::Object(proto_obj) = &*proto_val.borrow()
                                    {
                                        Some(*proto_obj)
                                    } else {
                                        None
                                    };

                                    let err_val = crate::core::js_error::create_error(mc, prototype, msg)?;
                                    if let Value::Object(err_obj) = &err_val {
                                        obj_set_key_value(mc, err_obj, &"name".into(), Value::String(name.clone()))?;
                                    }
                                    return Ok(err_val);
                                } else if name == &crate::unicode::utf8_to_utf16("Number") {
                                    let val = match number_constructor(&eval_args, env).map_err(EvalError::Js)? {
                                        Value::Number(n) => n,
                                        _ => f64::NAN,
                                    };
                                    let new_obj = crate::core::new_js_object_data(mc);
                                    obj_set_key_value(mc, &new_obj, &"__value__".into(), Value::Number(val))?;

                                    if let Some(proto_val) = obj_get_key_value(&obj, &"prototype".into())?
                                        && let Value::Object(proto_obj) = &*proto_val.borrow()
                                    {
                                        new_obj.borrow_mut(mc).prototype = Some(*proto_obj);
                                    }

                                    return Ok(Value::Object(new_obj));
                                } else if name == &crate::unicode::utf8_to_utf16("Date") {
                                    return Ok(crate::js_date::handle_date_constructor(mc, &eval_args, env)?);
                                } else if name == &crate::unicode::utf8_to_utf16("Map") {
                                    return Ok(crate::js_map::handle_map_constructor(mc, &eval_args, env)?);
                                } else if name == &crate::unicode::utf8_to_utf16("Set") {
                                    return Ok(crate::js_set::handle_set_constructor(mc, &eval_args, env)?);
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

            if let Value::Object(obj) = &obj_val {
                get_property_with_accessors(mc, env, obj, &key.as_str().into())
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                Err(EvalError::Js(raise_eval_error!("Cannot read properties of null or undefined")))
            } else {
                get_primitive_prototype_property(mc, env, &obj_val, &key.as_str().into())
            }
        }
        Expr::Index(obj_expr, key_expr) => {
            let obj_val = evaluate_expr(mc, env, obj_expr)?;
            let key_val = evaluate_expr(mc, env, key_expr)?;

            let key = match key_val {
                Value::String(s) => PropertyKey::String(utf16_to_utf8(&s)),
                Value::Number(n) => PropertyKey::String(n.to_string()),
                _ => PropertyKey::String(value_to_string(&key_val)),
            };

            if let Value::Object(obj) = &obj_val {
                get_property_with_accessors(mc, env, obj, &key)
            } else if matches!(obj_val, Value::Undefined | Value::Null) {
                Err(EvalError::Js(raise_eval_error!("Cannot read properties of null or undefined")))
            } else {
                get_primitive_prototype_property(mc, env, &obj_val, &key)
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
        Expr::Class(class_def) => {
            let class_obj = crate::js_class::create_class_object(mc, &class_def.name, &class_def.extends, &class_def.members, env, true)?;
            Ok(class_obj)
        }
        Expr::UnaryNeg(expr) => {
            let val = evaluate_expr(mc, env, expr)?;
            if let Value::Number(n) = val {
                Ok(Value::Number(-n))
            } else {
                Err(EvalError::Js(raise_eval_error!("Unary Negation only for numbers")))
            }
        }
        Expr::TypeOf(expr) => {
            // typeof handles ReferenceError for undeclared variables
            let val_result = evaluate_expr(mc, env, expr);
            let val = match val_result {
                Ok(v) => v,
                Err(e) => {
                    // Check if it is a ReferenceError (simplistic check for now, assuming EvalError could be it)
                    // Ideally we check if the error kind is ReferenceError.
                    // For now, if evaluation fails, return undefined (as string "undefined")
                    // This covers `typeof nonExistentVar` -> "undefined"
                    Value::Undefined
                }
            };

            let type_str = match val {
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Boolean(_) => "boolean",
                Value::Undefined | Value::Uninitialized => "undefined",
                Value::Null => "object",
                Value::Symbol(_) => "symbol",
                Value::BigInt(_) => "bigint",
                Value::Function(_)
                | Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::GeneratorFunction(..)
                | Value::ClassDefinition(_) => "function",
                Value::Object(obj) => {
                    if obj_get_key_value(&obj, &"__closure__".into()).unwrap_or(None).is_some() {
                        "function"
                    } else if let Some(is_ctor) = obj_get_key_value(&obj, &"__is_constructor".into()).unwrap_or(None) {
                        if matches!(*is_ctor.borrow(), Value::Boolean(true)) {
                            "function"
                        } else {
                            "object"
                        }
                    } else {
                        "object"
                    }
                }
                _ => "undefined",
            };
            Ok(Value::String(utf8_to_utf16(type_str)))
        }
        Expr::LogicalAnd(left, right) => {
            let lhs = evaluate_expr(mc, env, left)?;
            let is_truthy = match &lhs {
                Value::Boolean(b) => *b,
                Value::Number(n) => *n != 0.0 && !n.is_nan(),
                Value::String(s) => !s.is_empty(),
                Value::Null | Value::Undefined => false,
                Value::Object(_)
                | Value::Function(_)
                | Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::GeneratorFunction(..)
                | Value::ClassDefinition(_) => true,
                _ => false,
            };
            if !is_truthy { Ok(lhs) } else { evaluate_expr(mc, env, right) }
        }
        Expr::LogicalOr(left, right) => {
            let lhs = evaluate_expr(mc, env, left)?;
            let is_truthy = match &lhs {
                Value::Boolean(b) => *b,
                Value::Number(n) => *n != 0.0 && !n.is_nan(),
                Value::String(s) => !s.is_empty(),
                Value::Null | Value::Undefined => false,
                Value::Object(_)
                | Value::Function(_)
                | Value::Closure(_)
                | Value::AsyncClosure(_)
                | Value::GeneratorFunction(..)
                | Value::ClassDefinition(_) => true,
                _ => false,
            };
            if is_truthy { Ok(lhs) } else { evaluate_expr(mc, env, right) }
        }
        Expr::This => crate::js_class::evaluate_this(mc, env).map_err(EvalError::Js),
        Expr::SuperCall(args) => {
            let mut eval_args = Vec::new();
            for arg in args {
                eval_args.push(evaluate_expr(mc, env, arg)?);
            }
            Ok(crate::js_class::evaluate_super_call(mc, env, &eval_args).map_err(EvalError::Js)?)
        }
        Expr::SuperProperty(prop) => Ok(crate::js_class::evaluate_super_property(env, prop).map_err(EvalError::Js)?),
        Expr::SuperMethod(prop, args) => {
            let mut eval_args = Vec::new();
            for arg in args {
                eval_args.push(evaluate_expr(mc, env, arg)?);
            }
            Ok(crate::js_class::evaluate_super_method(mc, env, prop, &eval_args).map_err(EvalError::Js)?)
        }
        Expr::Super => Ok(crate::js_class::evaluate_super(mc, env).map_err(EvalError::Js)?),
        _ => Ok(Value::Undefined),
    }
}

fn evaluate_var<'gc>(_mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, name: &str) -> Result<Value<'gc>, JSError> {
    let mut current_opt = Some(*env);
    while let Some(current_env) = current_opt {
        if let Some(val_ptr) = env_get(&current_env, name) {
            let val = val_ptr.borrow().clone();
            if let Value::Uninitialized = val {
                return Err(raise_reference_error!(format!("Cannot access '{}' before initialization", name)));
            }
            return Ok(val);
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

    // Set __proto__ to Function.prototype
    if let Some(func_ctor_val) = env_get(env, "Function") {
        if let Value::Object(func_ctor) = &*func_ctor_val.borrow() {
            if let Ok(Some(proto_val)) = obj_get_key_value(func_ctor, &"prototype".into()) {
                if let Value::Object(proto) = &*proto_val.borrow() {
                    func_obj.borrow_mut(mc).prototype = Some(*proto);
                }
            }
        }
    }

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

    // Create prototype object
    let proto_obj = crate::core::new_js_object_data(mc);
    // Set prototype of prototype object to Object.prototype
    if let Some(obj_val) = env_get(env, "Object") {
        if let Value::Object(obj_ctor) = &*obj_val.borrow() {
            if let Ok(Some(obj_proto_val)) = obj_get_key_value(obj_ctor, &"prototype".into()) {
                if let Value::Object(obj_proto) = &*obj_proto_val.borrow() {
                    proto_obj.borrow_mut(mc).prototype = Some(*obj_proto);
                }
            }
        }
    }

    // Set 'constructor' on prototype
    obj_set_key_value(mc, &proto_obj, &"constructor".into(), Value::Object(func_obj))?;
    // Set 'prototype' on function
    obj_set_key_value(mc, &func_obj, &"prototype".into(), Value::Object(proto_obj))?;

    Ok(Value::Object(func_obj))
}

fn get_property_with_accessors<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: &PropertyKey<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    if let Some(val_ptr) = crate::core::obj_get_key_value(obj, key).map_err(EvalError::Js)? {
        let val = val_ptr.borrow().clone();
        match val {
            Value::Property { getter, value, .. } => {
                if let Some(g) = getter {
                    return call_accessor(mc, env, obj, &*g);
                }
                if let Some(v) = value {
                    return Ok(v.borrow().clone());
                }
                Ok(Value::Undefined)
            }
            Value::Getter(..) => call_accessor(mc, env, obj, &val),
            _ => Ok(val),
        }
    } else {
        Ok(Value::Undefined)
    }
}

fn set_property_with_accessors<'gc>(
    mc: &MutationContext<'gc>,
    _env: &JSObjectDataPtr<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: &PropertyKey<'gc>,
    val: Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    if let Some(prop_ptr) = crate::core::obj_get_key_value(obj, key).map_err(EvalError::Js)? {
        let prop = prop_ptr.borrow().clone();
        match prop {
            Value::Property { setter, getter, .. } => {
                if let Some(s) = setter {
                    return call_setter(mc, obj, &*s, val);
                }
                if getter.is_some() {
                    return Err(EvalError::Js(crate::raise_type_error!(
                        "Cannot set property which has only a getter"
                    )));
                }
                crate::core::obj_set_key_value(mc, obj, key, val).map_err(EvalError::Js)?;
                Ok(())
            }
            Value::Setter(params, body, captured_env, _) => call_setter_raw(mc, obj, &params, &body, &captured_env, val),
            Value::Getter(..) => Err(EvalError::Js(crate::raise_type_error!(
                "Cannot set property which has only a getter"
            ))),
            _ => {
                crate::core::obj_set_key_value(mc, obj, key, val).map_err(EvalError::Js)?;
                Ok(())
            }
        }
    } else {
        crate::core::obj_set_key_value(mc, obj, key, val).map_err(EvalError::Js)?;
        Ok(())
    }
}

fn call_native_function<'gc>(
    mc: &MutationContext<'gc>,
    name: &str,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Option<Value<'gc>>, EvalError<'gc>> {
    if name == "MapIterator.prototype.next" {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_map::handle_map_iterator_next(mc, &obj, env).map_err(EvalError::Js)?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: MapIterator.prototype.next called on non-object"
            )));
        }
    }

    if name == "SetIterator.prototype.next" {
        let this_v = this_val.clone().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = this_v {
            return Ok(Some(crate::js_set::handle_set_iterator_next(mc, &obj, env).map_err(EvalError::Js)?));
        } else {
            return Err(EvalError::Js(raise_eval_error!(
                "TypeError: SetIterator.prototype.next called on non-object"
            )));
        }
    }

    if name.starts_with("Map.") {
        if let Some(method) = name.strip_prefix("Map.prototype.") {
            let this_v = this_val.clone().unwrap_or(Value::Undefined);
            if let Value::Object(obj) = this_v {
                if let Some(map_val) = crate::core::obj_get_key_value(&obj, &"__map__".into()).map_err(EvalError::Js)? {
                    if let Value::Map(map_ptr) = &*map_val.borrow() {
                        return Ok(Some(
                            crate::js_map::handle_map_instance_method(mc, map_ptr, method, args, env).map_err(EvalError::Js)?,
                        ));
                    } else {
                        return Err(EvalError::Js(raise_eval_error!(
                            "TypeError: Map.prototype method called on incompatible receiver"
                        )));
                    }
                } else {
                    return Err(EvalError::Js(raise_eval_error!(
                        "TypeError: Map.prototype method called on incompatible receiver"
                    )));
                }
            } else if let Value::Map(map_ptr) = this_v {
                return Ok(Some(
                    crate::js_map::handle_map_instance_method(mc, &map_ptr, method, args, env).map_err(EvalError::Js)?,
                ));
            } else {
                return Err(EvalError::Js(raise_eval_error!(
                    "TypeError: Map.prototype method called on non-object receiver"
                )));
            }
        }
    }

    if name.starts_with("Set.") {
        if let Some(method) = name.strip_prefix("Set.prototype.") {
            let this_v = this_val.clone().unwrap_or(Value::Undefined);
            if let Value::Object(obj) = this_v {
                if let Some(set_val) = crate::core::obj_get_key_value(&obj, &"__set__".into()).map_err(EvalError::Js)? {
                    if let Value::Set(set_ptr) = &*set_val.borrow() {
                        return Ok(Some(
                            crate::js_set::handle_set_instance_method(mc, set_ptr, this_v.clone(), method, args, env)
                                .map_err(EvalError::Js)?,
                        ));
                    } else {
                        return Err(EvalError::Js(raise_eval_error!(
                            "TypeError: Set.prototype method called on incompatible receiver"
                        )));
                    }
                } else {
                    return Err(EvalError::Js(raise_eval_error!(
                        "TypeError: Set.prototype method called on incompatible receiver"
                    )));
                }
            } else if let Value::Set(set_ptr) = this_v {
                return Ok(Some(
                    crate::js_set::handle_set_instance_method(mc, &set_ptr, Value::Set(set_ptr.clone()), method, args, env)
                        .map_err(EvalError::Js)?,
                ));
            } else {
                return Err(EvalError::Js(raise_eval_error!(
                    "TypeError: Set.prototype method called on non-object receiver"
                )));
            }
        }
    }
    Ok(None)
}

fn call_accessor<'gc>(
    mc: &MutationContext<'gc>,
    env: &JSObjectDataPtr<'gc>,
    receiver: &JSObjectDataPtr<'gc>,
    accessor: &Value<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    match accessor {
        Value::Function(name) => {
            if let Some(res) = call_native_function(mc, name, Some(Value::Object(*receiver)), &[], env)? {
                Ok(res)
            } else {
                Err(EvalError::Js(crate::raise_type_error!(format!(
                    "Accessor function {} not supported",
                    name
                ))))
            }
        }
        Value::Getter(body, captured_env, _) => {
            let call_env = crate::core::new_js_object_data(mc);
            call_env.borrow_mut(mc).prototype = Some(*captured_env);
            call_env.borrow_mut(mc).is_function_scope = true;
            crate::core::obj_set_key_value(mc, &call_env, &"this".into(), Value::Object(*receiver)).map_err(EvalError::Js)?;
            let mut body_clone = body.clone();
            evaluate_statements(mc, &call_env, &mut body_clone)
        }
        Value::Closure(cl) => {
            let cl_data = &*cl;
            let call_env = crate::core::new_js_object_data(mc);
            call_env.borrow_mut(mc).prototype = Some(cl_data.env);
            call_env.borrow_mut(mc).is_function_scope = true;
            crate::core::obj_set_key_value(mc, &call_env, &"this".into(), Value::Object(*receiver)).map_err(EvalError::Js)?;
            let mut body_clone = cl_data.body.clone();
            evaluate_statements(mc, &call_env, &mut body_clone)
        }
        _ => Err(EvalError::Js(crate::raise_type_error!("Accessor is not a function"))),
    }
}

fn call_setter<'gc>(
    mc: &MutationContext<'gc>,
    receiver: &JSObjectDataPtr<'gc>,
    setter: &Value<'gc>,
    val: Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    match setter {
        Value::Setter(params, body, captured_env, _) => call_setter_raw(mc, receiver, params, body, captured_env, val),
        Value::Closure(cl) => {
            let cl_data = &*cl;
            let call_env = crate::core::new_js_object_data(mc);
            call_env.borrow_mut(mc).prototype = Some(cl_data.env);
            call_env.borrow_mut(mc).is_function_scope = true;
            crate::core::obj_set_key_value(mc, &call_env, &"this".into(), Value::Object(*receiver)).map_err(EvalError::Js)?;

            if let Some(first_param) = cl_data.params.first() {
                if let DestructuringElement::Variable(name, _) = first_param {
                    crate::core::env_set(mc, &call_env, &name, val).map_err(EvalError::Js)?;
                }
            }
            let mut body_clone = cl_data.body.clone();
            evaluate_statements(mc, &call_env, &mut body_clone).map(|_| ())
        }
        _ => Err(EvalError::Js(crate::raise_type_error!("Setter is not a function"))),
    }
}

fn call_setter_raw<'gc>(
    mc: &MutationContext<'gc>,
    receiver: &JSObjectDataPtr<'gc>,
    params: &[DestructuringElement],
    body: &[Statement],
    env: &JSObjectDataPtr<'gc>,
    val: Value<'gc>,
) -> Result<(), EvalError<'gc>> {
    let call_env = crate::core::new_js_object_data(mc);
    call_env.borrow_mut(mc).prototype = Some(*env);
    call_env.borrow_mut(mc).is_function_scope = true;
    crate::core::obj_set_key_value(mc, &call_env, &"this".into(), Value::Object(*receiver)).map_err(EvalError::Js)?;

    if let Some(param) = params.first() {
        if let DestructuringElement::Variable(name, _) = param {
            crate::core::env_set(mc, &call_env, &name, val).map_err(EvalError::Js)?;
        }
    }
    let mut body_clone = body.to_vec();
    evaluate_statements(mc, &call_env, &mut body_clone).map(|_| ())
}

fn js_error_to_value<'gc>(mc: &MutationContext<'gc>, env: &JSObjectDataPtr<'gc>, js_err: &JSError) -> Value<'gc> {
    let full_msg = js_err.message();

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

    let error_proto = if let Ok(Some(err_ctor_val)) = obj_get_key_value(env, &name.into())
        && let Value::Object(err_ctor) = &*err_ctor_val.borrow()
        && let Ok(Some(proto_val)) = obj_get_key_value(err_ctor, &"prototype".into())
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else if let Ok(Some(err_ctor_val)) = obj_get_key_value(env, &"Error".into())
        && let Value::Object(err_ctor) = &*err_ctor_val.borrow()
        && let Ok(Some(proto_val)) = obj_get_key_value(err_ctor, &"prototype".into())
        && let Value::Object(proto) = &*proto_val.borrow()
    {
        Some(*proto)
    } else {
        None
    };

    let err_val = create_error(mc, error_proto, (&raw_msg).into()).unwrap_or(Value::Undefined);

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

pub fn call_closure<'gc>(
    mc: &MutationContext<'gc>,
    cl: &crate::core::ClosureData<'gc>,
    this_val: Option<Value<'gc>>,
    args: &[Value<'gc>],
    env: &JSObjectDataPtr<'gc>,
) -> Result<Value<'gc>, EvalError<'gc>> {
    let call_env = crate::core::new_js_object_data(mc);
    call_env.borrow_mut(mc).prototype = Some(cl.env);
    call_env.borrow_mut(mc).is_function_scope = true;
    if let Some(tv) = &this_val {
        crate::core::obj_set_key_value(mc, &call_env, &"this".into(), tv.clone()).map_err(EvalError::Js)?;
    }

    for (i, param) in cl.params.iter().enumerate() {
        match param {
            DestructuringElement::Variable(name, _) => {
                let arg_val = args.get(i).cloned().unwrap_or(Value::Undefined);
                crate::core::env_set(mc, &call_env, name, arg_val).map_err(EvalError::Js)?;
            }
            DestructuringElement::Rest(name) => {
                let rest_args = if i < args.len() { args[i..].to_vec() } else { Vec::new() };
                let array_obj = crate::js_array::create_array(mc, env).map_err(EvalError::Js)?;
                for (j, val) in rest_args.iter().enumerate() {
                    crate::core::obj_set_key_value(mc, &array_obj, &PropertyKey::from(j.to_string()), val.clone())
                        .map_err(EvalError::Js)?;
                }
                crate::js_array::set_array_length(mc, &array_obj, rest_args.len()).map_err(EvalError::Js)?;
                crate::core::env_set(mc, &call_env, name, Value::Object(array_obj)).map_err(EvalError::Js)?;
            }
            _ => {}
        }
    }
    let mut body_clone = cl.body.clone();
    evaluate_statements(mc, &call_env, &mut body_clone)
}
