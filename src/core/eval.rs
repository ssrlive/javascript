use crate::{
    JSError, JSErrorKind, PropertyKey, Value,
    core::get_own_property,
    core::{
        BigIntHolder, BinaryOp, DestructuringElement, Expr, JSObjectData, JSObjectDataPtr, ObjectDestructuringElement, Statement,
        SwitchCase, SymbolData, TypedArrayKind, WELL_KNOWN_SYMBOLS, env_get, env_set, env_set_const, env_set_recursive, env_set_var,
        is_truthy, obj_delete, obj_set_value, to_primitive, value_to_string, values_equal,
    },
    js_array::{get_array_length, is_array, set_array_length},
    js_assert::make_assert_object,
    js_class::{
        call_class_method, call_static_method, create_class_object, evaluate_new, evaluate_super, evaluate_super_call,
        evaluate_super_method, evaluate_super_property, evaluate_this, is_class_instance, is_instance_of,
    },
    js_console::{handle_console_method, make_console_object},
    js_math::{handle_math_method, make_math_object},
    js_number::make_number_object,
    js_promise::{JSPromise, PromiseState, handle_promise_method, run_event_loop},
    js_reflect::make_reflect_object,
    js_testintl::make_testintl_object,
    obj_get_value, raise_eval_error, raise_throw_error, raise_type_error,
    sprintf::handle_sprintf_call,
    tmpfile::{create_tmpfile, handle_file_method},
    unicode::{utf8_to_utf16, utf16_char_at, utf16_len, utf16_to_utf8},
};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::{cell::RefCell, collections::HashMap, rc::Rc, str::FromStr};

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

pub fn evaluate_statements(env: &JSObjectDataPtr, statements: &[Statement]) -> Result<Value, JSError> {
    match evaluate_statements_with_context(env, statements)? {
        ControlFlow::Normal(val) => Ok(val),
        ControlFlow::Break(_) => Err(raise_eval_error!("break statement not in loop or switch")),
        ControlFlow::Continue(_) => Err(raise_eval_error!("continue statement not in loop")),
        ControlFlow::Return(val) => Ok(val),
    }
}

fn evaluate_statements_with_context(env: &JSObjectDataPtr, statements: &[Statement]) -> Result<ControlFlow, JSError> {
    // Hoist var declarations if this is a function scope
    if env.borrow().is_function_scope {
        let mut var_names = std::collections::HashSet::new();
        collect_var_names(statements, &mut var_names);
        for name in var_names {
            env_set(env, &name, Value::Undefined)?;
        }
    }

    let mut last_value = Value::Number(0.0);
    for (i, stmt) in statements.iter().enumerate() {
        log::trace!("Evaluating statement {i}: {stmt:?}");
        // Evaluate the statement inside a closure so we can log the
        // statement index and AST if an error occurs while preserving
        // control-flow returns. The closure returns
        // Result<Option<ControlFlow>, JSError> where `Ok(None)` means
        // continue, `Ok(Some(cf))` means propagate control flow, and
        // `Err(e)` means an error that we log and then return.
        let eval_res: Result<Option<ControlFlow>, JSError> = (|| -> Result<Option<ControlFlow>, JSError> {
            match stmt {
                Statement::Let(name, expr_opt) => {
                    let val = expr_opt.clone().map_or(Ok(Value::Undefined), |expr| evaluate_expr(env, &expr))?;
                    env_set(env, name.as_str(), val.clone())?;
                    last_value = val;
                    Ok(None)
                }
                Statement::Var(name, expr_opt) => {
                    let val = expr_opt.clone().map_or(Ok(Value::Undefined), |expr| evaluate_expr(env, &expr))?;
                    env_set_var(env, name.as_str(), val.clone())?;
                    last_value = val;
                    Ok(None)
                }
                Statement::Const(name, expr) => {
                    let val = evaluate_expr(env, expr)?;
                    env_set_const(env, name.as_str(), val.clone());
                    last_value = val;
                    Ok(None)
                }
                Statement::Class(name, extends, members) => {
                    let class_obj = create_class_object(name, extends, members, env)?;
                    env_set(env, name.as_str(), class_obj)?;
                    last_value = Value::Undefined;
                    Ok(None)
                }
                Statement::Block(stmts) => {
                    let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                    block_env.borrow_mut().prototype = Some(env.clone());
                    block_env.borrow_mut().is_function_scope = false;
                    match evaluate_statements_with_context(&block_env, stmts)? {
                        ControlFlow::Normal(val) => last_value = val,
                        cf => return Ok(Some(cf)),
                    }
                    Ok(None)
                }
                Statement::Assign(name, expr) => {
                    let val = evaluate_expr(env, expr)?;
                    env_set_recursive(env, name.as_str(), val.clone())?;
                    last_value = val;
                    Ok(None)
                }
                Statement::Expr(expr) => perform_statement_expression(env, expr, &mut last_value),
                Statement::Return(expr_opt) => {
                    let return_val = match expr_opt {
                        Some(expr) => evaluate_expr(env, expr)?,
                        None => Value::Undefined,
                    };
                    log::trace!("Statement::Return evaluated value = {:?}", return_val);
                    Ok(Some(ControlFlow::Return(return_val)))
                }
                Statement::Throw(expr) => {
                    let throw_val = evaluate_expr(env, expr)?;
                    Err(raise_throw_error!(throw_val))
                }
                Statement::If(condition, then_body, else_body) => {
                    perform_statement_if_then_else(env, condition, then_body, else_body, &mut last_value)
                }
                Statement::ForOfDestructuringObject(pattern, iterable, body) => {
                    let iterable_val = evaluate_expr(env, iterable)?;
                    if let Some(cf) = for_of_destructuring_object_iter(env, pattern, &iterable_val, body, &mut last_value, None)? {
                        return Ok(Some(cf));
                    }
                    Ok(None)
                }
                Statement::ForOfDestructuringArray(pattern, iterable, body) => {
                    let iterable_val = evaluate_expr(env, iterable)?;
                    if let Some(cf) = for_of_destructuring_array_iter(env, pattern, &iterable_val, body, &mut last_value, None)? {
                        return Ok(Some(cf));
                    }
                    Ok(None)
                }

                Statement::Label(label_name, inner_stmt) => perform_statement_label(env, label_name, inner_stmt, &mut last_value),
                Statement::TryCatch(try_body, catch_param, catch_body, finally_body_opt) => {
                    statement_try_catch(env, try_body, catch_param, catch_body, finally_body_opt, &mut last_value)
                }
                Statement::For(init, condition, increment, body) => {
                    statement_for_init_condition_increment(env, init, condition, increment, body, &mut last_value)
                }
                Statement::ForOf(var, iterable, body) => statement_for_of_var_iter(env, var, iterable, body, &mut last_value),
                Statement::While(condition, body) => statement_while_condition_body(env, condition, body, &mut last_value),
                Statement::DoWhile(body, condition) => statement_do_body_while_condition(env, body, condition, &mut last_value),
                Statement::Switch(expr, cases) => eval_switch_statement(env, expr, cases, &mut last_value, None),
                Statement::Break(opt) => Ok(Some(ControlFlow::Break(opt.clone()))),
                Statement::Continue(opt) => Ok(Some(ControlFlow::Continue(opt.clone()))),
                Statement::LetDestructuringArray(pattern, expr) => {
                    let val = evaluate_expr(env, expr)?;
                    perform_array_destructuring(env, pattern, &val, false)?;
                    last_value = val;
                    Ok(None)
                }
                Statement::ConstDestructuringArray(pattern, expr) => {
                    let val = evaluate_expr(env, expr)?;
                    perform_array_destructuring(env, pattern, &val, true)?;
                    last_value = val;
                    Ok(None)
                }
                Statement::LetDestructuringObject(pattern, expr) => {
                    let val = evaluate_expr(env, expr)?;
                    // Provide a clearer error message when the RHS evaluates to
                    // `undefined` or `null` and destructuring is attempted — this
                    // mirrors node's behaviour where the specific property name
                    // and variable are included in the error when possible.
                    if !matches!(val, Value::Object(_)) {
                        // Try to extract a helpful identifier and a property
                        // name to match typical node error messages. We use the
                        // first property key listed in the pattern, if any.
                        let first_key = pattern.iter().find_map(|el| {
                            if let ObjectDestructuringElement::Property { key, .. } = el {
                                Some(key.clone())
                            } else {
                                None
                            }
                        });

                        let message = if let Some(first) = first_key {
                            if let Expr::Var(name) = expr {
                                // e.g. `Cannot destructure property 'seconds' of 'duration' as it is undefined.`
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

                    perform_object_destructuring(env, pattern, &val, false)?;
                    last_value = val;
                    Ok(None)
                }
                Statement::ConstDestructuringObject(pattern, expr) => {
                    let val = evaluate_expr(env, expr)?;
                    if !matches!(val, Value::Object(_)) {
                        let first_key = pattern.iter().find_map(|el| {
                            if let ObjectDestructuringElement::Property { key, .. } = el {
                                Some(key.clone())
                            } else {
                                None
                            }
                        });

                        let message = if let Some(first) = first_key {
                            if let Expr::Var(name) = expr {
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

                    perform_object_destructuring(env, pattern, &val, true)?;
                    last_value = val;
                    Ok(None)
                }
                Statement::Import(specifiers, module_name) => {
                    // Load the module
                    let module_value = crate::js_module::load_module(module_name, None)?;

                    // Import the specifiers into the current environment
                    for specifier in specifiers {
                        match specifier {
                            crate::core::statement::ImportSpecifier::Default(name) => {
                                // For default import, check if the module has a default export (file modules)
                                // or import the entire module (built-in modules)
                                match crate::js_module::import_from_module(&module_value, "default") {
                                    Ok(default_value) => {
                                        // Module has a default export (file module)
                                        env_set(env, name, default_value)?;
                                    }
                                    Err(_) => {
                                        // Module doesn't have a default export (built-in module)
                                        env_set(env, name, module_value.clone())?;
                                    }
                                }
                            }
                            crate::core::statement::ImportSpecifier::Named(name, alias) => {
                                // Import specific named export
                                let imported_value = crate::js_module::import_from_module(&module_value, name)?;
                                let import_name = alias.as_ref().unwrap_or(name);
                                env_set(env, import_name, imported_value)?;
                            }
                            crate::core::statement::ImportSpecifier::Namespace(name) => {
                                // Import entire module as namespace
                                env_set(env, name, module_value.clone())?;
                            }
                        }
                    }

                    last_value = Value::Undefined;
                    Ok(None)
                }
                Statement::Export(specifiers, maybe_decl) => {
                    // If this export included an inner declaration (e.g. `export const x = 1`),
                    // evaluate that declaration now so the exported name exists in the environment.
                    if let Some(decl_stmt) = maybe_decl {
                        match &**decl_stmt {
                            Statement::Const(name, expr) => {
                                let val = evaluate_expr(env, expr)?;
                                env_set_const(env, name.as_str(), val);
                            }
                            Statement::Let(name, Some(expr)) => {
                                let val = evaluate_expr(env, expr)?;
                                env_set(env, name, val)?;
                            }
                            Statement::Var(name, Some(expr)) => {
                                let val = evaluate_expr(env, expr)?;
                                env_set_var(env, name, val)?;
                            }
                            Statement::Class(name, extends, members) => {
                                let class_obj = create_class_object(name.as_str(), extends, members.as_slice(), env)?;
                                env_set(env, name.as_str(), class_obj)?;
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
                                        // For named exports, we need to find the value in current scope
                                        // For now, assume it's a variable in the environment
                                        let var_opt = get_own_property(env, &crate::core::PropertyKey::String(name.clone()));
                                        if let Some(var_val) = var_opt {
                                            let export_name = alias.as_ref().unwrap_or(name).clone();
                                            exports_obj.borrow_mut().insert(
                                                crate::core::PropertyKey::String(export_name),
                                                Rc::new(RefCell::new(var_val.borrow().clone())),
                                            );
                                        } else {
                                            return Err(crate::raise_eval_error!(format!("Export '{}' not found in scope", name)));
                                        }
                                    }
                                    crate::core::statement::ExportSpecifier::Default(expr) => {
                                        // Evaluate the default export expression
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
                    last_value = Value::Undefined;
                    Ok(None)
                }
            }
        })();
        match eval_res {
            Ok(Some(cf)) => return Ok(cf),
            Ok(None) => {}
            Err(e) => {
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
                                    "evaluate_statements_with_context thrown JS value (String) at statement {i}: '{}' stmt={stmt:?}",
                                    s
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
                                    "evaluate_statements_with_context thrown JS value (Number) at statement {i}: {} stmt={stmt:?}",
                                    n
                                );
                            }
                            Value::Boolean(b) => {
                                log::debug!(
                                    "evaluate_statements_with_context thrown JS value (Boolean) at statement {i}: {} stmt={stmt:?}",
                                    b
                                );
                            }
                            Value::Undefined => {
                                log::debug!("evaluate_statements_with_context thrown JS value (Undefined) at statement {i} stmt={stmt:?}");
                            }
                            other => {
                                // Fallback: print Debug and a stringified form
                                log::debug!(
                                    "evaluate_statements_with_context thrown JS value at statement {i}: {:?} (toString='{}') stmt={stmt:?}",
                                    other,
                                    crate::core::value_to_string(other)
                                );
                            }
                        }
                    }
                    _ => {
                        log::error!("evaluate_statements_with_context error at statement {i}: {e}, stmt={stmt:?}");
                    }
                }
                return Err(e);
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
        let block_env = Rc::new(RefCell::new(JSObjectData::new()));
        block_env.borrow_mut().prototype = Some(env.clone());
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
        let block_env = Rc::new(RefCell::new(JSObjectData::new()));
        block_env.borrow_mut().prototype = Some(env.clone());
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
) -> Result<Option<ControlFlow>, JSError> {
    let for_env = Rc::new(RefCell::new(JSObjectData::new()));
    for_env.borrow_mut().prototype = Some(env.clone());
    for_env.borrow_mut().is_function_scope = false;
    // Execute initialization in for_env
    if let Some(init_stmt) = init {
        match init_stmt.as_ref() {
            Statement::Let(name, expr_opt) => {
                let val = expr_opt
                    .clone()
                    .map_or(Ok(Value::Undefined), |expr| evaluate_expr(&for_env, &expr))?;
                env_set(&for_env, name.as_str(), val)?;
            }
            Statement::Var(name, expr_opt) => {
                let val = expr_opt
                    .clone()
                    .map_or(Ok(Value::Undefined), |expr| evaluate_expr(&for_env, &expr))?;
                env_set_var(&for_env, name.as_str(), val)?;
            }
            Statement::Expr(expr) => {
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
        let block_env = Rc::new(RefCell::new(JSObjectData::new()));
        block_env.borrow_mut().prototype = Some(for_env.clone());
        block_env.borrow_mut().is_function_scope = false;
        match evaluate_statements_with_context(&block_env, body)? {
            ControlFlow::Normal(val) => *last_value = val,
            ControlFlow::Break(None) => break,
            ControlFlow::Break(Some(lbl)) => return Ok(Some(ControlFlow::Break(Some(lbl)))),
            ControlFlow::Continue(None) => {}
            ControlFlow::Continue(Some(lbl)) => return Ok(Some(ControlFlow::Continue(Some(lbl)))),
            ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
        }

        // Execute increment in for_env
        if let Some(incr_stmt) = increment {
            match incr_stmt.as_ref() {
                Statement::Expr(expr) => match expr {
                    Expr::Assign(target, value) => {
                        if let Expr::Var(name) = target.as_ref() {
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
        let block_env = Rc::new(RefCell::new(JSObjectData::new()));
        block_env.borrow_mut().prototype = Some(env.clone());
        block_env.borrow_mut().is_function_scope = false;
        match evaluate_statements_with_context(&block_env, then_body)? {
            ControlFlow::Normal(val) => *last_value = val,
            cf => return Ok(Some(cf)),
        }
    } else if let Some(else_stmts) = else_body {
        let block_env = Rc::new(RefCell::new(JSObjectData::new()));
        block_env.borrow_mut().prototype = Some(env.clone());
        block_env.borrow_mut().is_function_scope = false;
        match evaluate_statements_with_context(&block_env, else_stmts)? {
            ControlFlow::Normal(val) => *last_value = val,
            cf => return Ok(Some(cf)),
        }
    }
    Ok(None)
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
        Ok(ControlFlow::Normal(v)) => *last_value = v,
        Ok(cf) => {
            // For any non-normal control flow, execute finally (if present)
            // then propagate the eventual control flow (finally can override).
            if let Some(finally_body) = finally_body_opt {
                let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                block_env.borrow_mut().prototype = Some(env.clone());
                block_env.borrow_mut().is_function_scope = false;
                match evaluate_statements_with_context(&block_env, finally_body)? {
                    ControlFlow::Normal(_) => return Ok(Some(cf)),
                    other => return Ok(Some(other)),
                }
            } else {
                return Ok(Some(cf));
            }
        }
        Err(err) => {
            if catch_param.is_empty() {
                if let Some(finally_body) = finally_body_opt {
                    evaluate_statements_with_context(env, finally_body)?;
                }
                return Err(err);
            } else {
                let catch_env = Rc::new(RefCell::new(JSObjectData::new()));
                catch_env.borrow_mut().prototype = Some(env.clone());
                catch_env.borrow_mut().is_function_scope = false;
                let catch_value = match &err.kind() {
                    // Thrown values created by `throw <expr>` should be delivered
                    // to the catch clause unmodified (as in ECMA-262).
                    // Only JS engine error variants (TypeError, SyntaxError,
                    // RuntimeError, EvaluationError) are converted into
                    // Error-like objects for the catch. Preserve the
                    // original thrown value here.
                    JSErrorKind::Throw { value } => value.clone(),
                    JSErrorKind::TypeError { .. }
                    | JSErrorKind::SyntaxError { .. }
                    | JSErrorKind::RuntimeError { .. }
                    | JSErrorKind::EvaluationError { .. } => {
                        // For engine-generated errors, expose the textual
                        // representation to the catch clause as a string
                        // (tests expect error text rather than an object).
                        Value::String(utf8_to_utf16(&err.to_string()))
                    }
                    _ => {
                        // For other errors, create a generic Error object
                        let error_obj = Rc::new(RefCell::new(JSObjectData::new()));
                        obj_set_value(&error_obj, &"name".into(), Value::String(utf8_to_utf16("Error")))?;
                        obj_set_value(&error_obj, &"message".into(), Value::String(utf8_to_utf16(&err.to_string())))?;
                        Value::Object(error_obj)
                    }
                };
                env_set(&catch_env, catch_param, catch_value)?;
                match evaluate_statements_with_context(&catch_env, catch_body)? {
                    ControlFlow::Normal(val) => *last_value = val,
                    cf => {
                        if let Some(finally_body) = finally_body_opt {
                            let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                            block_env.borrow_mut().prototype = Some(env.clone());
                            block_env.borrow_mut().is_function_scope = false;
                            match evaluate_statements_with_context(&block_env, finally_body)? {
                                ControlFlow::Normal(_) => return Ok(Some(cf)),
                                other => return Ok(Some(other)),
                            }
                        }
                        return Ok(Some(cf));
                    }
                }
            }
        }
    }
    // Finally block executes after try/catch
    if let Some(finally_body) = finally_body_opt {
        let block_env = Rc::new(RefCell::new(JSObjectData::new()));
        block_env.borrow_mut().prototype = Some(env.clone());
        block_env.borrow_mut().is_function_scope = false;
        match evaluate_statements_with_context(&block_env, finally_body)? {
            ControlFlow::Normal(val) => *last_value = val,
            cf => return Ok(Some(cf)),
        }
    }
    Ok(None)
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
    match inner_stmt {
        Statement::For(init, condition, increment, body) => {
            let for_env = Rc::new(RefCell::new(JSObjectData::new()));
            for_env.borrow_mut().prototype = Some(env.clone());
            for_env.borrow_mut().is_function_scope = false;
            // Execute initialization
            if let Some(init_stmt) = init {
                match init_stmt.as_ref() {
                    Statement::Let(name, expr_opt) => {
                        let val = expr_opt
                            .clone()
                            .map_or(Ok(Value::Undefined), |expr| evaluate_expr(&for_env, &expr))?;
                        env_set(&for_env, name.as_str(), val)?;
                    }
                    Statement::Var(name, expr_opt) => {
                        let val = expr_opt
                            .clone()
                            .map_or(Ok(Value::Undefined), |expr| evaluate_expr(&for_env, &expr))?;
                        env_set_var(&for_env, name.as_str(), val)?;
                    }
                    Statement::Expr(expr) => {
                        evaluate_expr(&for_env, expr)?;
                    }
                    _ => {
                        return Err(raise_eval_error!("error"));
                    }
                }
            }

            loop {
                let should_continue = if let Some(cond_expr) = condition {
                    let cond_val = evaluate_expr(&for_env, cond_expr)?;
                    is_truthy(&cond_val)
                } else {
                    true
                };
                if !should_continue {
                    break;
                }

                let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                block_env.borrow_mut().prototype = Some(for_env.clone());
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
                        if lbl == *label_name { /* continue loop */
                        } else {
                            return Ok(Some(ControlFlow::Continue(Some(lbl))));
                        }
                    }
                    ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                }

                if let Some(incr_stmt) = increment {
                    match incr_stmt.as_ref() {
                        Statement::Expr(expr) => match expr {
                            Expr::Assign(target, value) => {
                                if let Expr::Var(name) = target.as_ref() {
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
                        }
                    }
                }
            }
            Ok(None)
        }
        Statement::ForOf(var, iterable, body) => {
            let iterable_val = evaluate_expr(env, iterable)?;
            match iterable_val {
                Value::Object(obj_map) => {
                    if is_array(&obj_map) {
                        let len = get_array_length(&obj_map).unwrap_or(0);
                        for i in 0..len {
                            let key = PropertyKey::String(i.to_string());
                            if let Some(element_rc) = obj_get_value(&obj_map, &key)? {
                                let element = element_rc.borrow().clone();
                                env_set_recursive(env, var.as_str(), element)?;
                                let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                                block_env.borrow_mut().prototype = Some(env.clone());
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
        Statement::ForOfDestructuringObject(pattern, iterable, body) => {
            let iterable_val = evaluate_expr(env, iterable)?;
            if let Some(cf) = for_of_destructuring_object_iter(env, pattern, &iterable_val, body, last_value, Some(label_name))? {
                return Ok(Some(cf));
            }
            Ok(None)
        }
        Statement::ForOfDestructuringArray(pattern, iterable, body) => {
            let iterable_val = evaluate_expr(env, iterable)?;
            if let Some(cf) = for_of_destructuring_array_iter(env, pattern, &iterable_val, body, last_value, Some(label_name))? {
                return Ok(Some(cf));
            }
            Ok(None)
        }
        Statement::While(condition, body) => {
            loop {
                let cond_val = evaluate_expr(env, condition)?;
                if !is_truthy(&cond_val) {
                    break Ok(None);
                }
                let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                block_env.borrow_mut().prototype = Some(env.clone());
                block_env.borrow_mut().is_function_scope = false;
                match evaluate_statements_with_context(&block_env, body)? {
                    ControlFlow::Normal(val) => *last_value = val,
                    ControlFlow::Break(None) => break Ok(None),
                    ControlFlow::Break(Some(lbl)) => {
                        if lbl == *label_name {
                            break Ok(None);
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
        Statement::DoWhile(body, condition) => {
            loop {
                let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                block_env.borrow_mut().prototype = Some(env.clone());
                block_env.borrow_mut().is_function_scope = false;
                match evaluate_statements_with_context(&block_env, body)? {
                    ControlFlow::Normal(val) => *last_value = val,
                    ControlFlow::Break(None) => break Ok(None),
                    ControlFlow::Break(Some(lbl)) => {
                        if lbl == *label_name {
                            break Ok(None);
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
                let cond_val = evaluate_expr(env, condition)?;
                if !is_truthy(&cond_val) {
                    break Ok(None);
                }
            }
        }
        Statement::Switch(expr, cases) => eval_switch_statement(env, expr, cases, last_value, Some(label_name)),
        // If it's some other statement type, just evaluate it here. Important: a
        // Normal control flow result from the inner statement should *not*
        // be propagated out of the label — labels only affect break/continue
        // that target the label itself. Propagate non-normal control-flow
        // (break/continue/return) as before, but swallow Normal so execution
        // continues.
        other => match evaluate_statements_with_context(env, std::slice::from_ref(other))? {
            ControlFlow::Break(Some(lbl)) if lbl == *label_name => Ok(None),
            ControlFlow::Break(opt) => Ok(Some(ControlFlow::Break(opt))),
            ControlFlow::Continue(Some(lbl)) if lbl == *label_name => Ok(Some(ControlFlow::Continue(None))),
            ControlFlow::Continue(opt) => Ok(Some(ControlFlow::Continue(opt))),
            ControlFlow::Normal(_) => Ok(None),
            cf => Ok(Some(cf)),
        },
    }
}

fn perform_statement_expression(env: &JSObjectDataPtr, expr: &Expr, last_value: &mut Value) -> Result<Option<ControlFlow>, JSError> {
    // Special-case assignment expressions so we can mutate `env` or
    // object properties. `parse_statement` only turns simple
    // variable assignments into `Statement::Assign`, so here we
    // handle expression-level assignments such as `obj.prop = val`
    // and `arr[0] = val`.
    if let Expr::Assign(target, value_expr) = expr {
        match target.as_ref() {
            Expr::Var(name) => {
                let v = evaluate_expr(env, value_expr)?;
                env_set_recursive(env, name.as_str(), v.clone())?;
                *last_value = v;
            }
            Expr::Property(obj_expr, prop_name) => {
                let v = evaluate_expr(env, value_expr)?;
                // set_prop_env will attempt to mutate the env-held
                // object when possible, otherwise it will update
                // the evaluated object and return it.
                match set_prop_env(env, obj_expr, prop_name.as_str(), v.clone())? {
                    Some(updated_obj) => *last_value = updated_obj,
                    None => *last_value = v,
                }
            }
            Expr::Index(obj_expr, idx_expr) => {
                // Check if this is a TypedArray assignment first
                let obj_val = evaluate_expr(env, obj_expr)?;
                let idx_val = evaluate_expr(env, idx_expr)?;
                if let (Value::Object(obj_map), Value::Number(n)) = (&obj_val, &idx_val)
                    && let Some(ta_val) = obj_get_value(obj_map, &"__typedarray".into())?
                    && let Value::TypedArray(ta) = &*ta_val.borrow()
                {
                    // This is a TypedArray, use our set method
                    let mut v = evaluate_expr(env, value_expr)?;
                    let val_num = match &mut v {
                        Value::Number(num) => *num as i64,
                        Value::BigInt(h) => h
                            .refresh_parsed(false)?
                            .to_i64()
                            .ok_or(raise_eval_error!("TypedArray assignment value must be a number"))?,
                        _ => return Err(raise_eval_error!("TypedArray assignment value must be a number")),
                    };
                    ta.borrow_mut()
                        .set(*n as usize, val_num)
                        .map_err(|_| raise_eval_error!("TypedArray index out of bounds"))?;
                    *last_value = v;
                    return Ok(None);
                }
                // Evaluate index — support number, string and symbol keys
                let v = evaluate_expr(env, value_expr)?;
                match idx_val {
                    Value::Number(n) => {
                        let key = n.to_string();
                        match set_prop_env(env, obj_expr, &key, v.clone())? {
                            Some(updated_obj) => *last_value = updated_obj,
                            None => *last_value = v,
                        }
                    }
                    Value::String(s) => {
                        let key = String::from_utf16_lossy(&s);
                        match set_prop_env(env, obj_expr, &key, v.clone())? {
                            Some(updated_obj) => *last_value = updated_obj,
                            None => *last_value = v,
                        }
                    }
                    Value::Symbol(sym) => {
                        // For symbols we must set the property with a Symbol key.
                        // Try fast path (obj_expr is a var that points to an object in env)
                        if let Expr::Var(varname) = obj_expr.as_ref()
                            && let Some(rc_val) = env_get(env, varname)
                        {
                            let mut borrowed = rc_val.borrow_mut();
                            if let Value::Object(ref mut map) = *borrowed {
                                let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                                obj_set_value(map, &key, v.clone())?;
                                *last_value = v;
                            } else {
                                return Err(raise_eval_error!("Cannot assign to property of non-object"));
                            }
                        } else {
                            // Fall back: evaluate object expression and set symbol key
                            let obj_val = evaluate_expr(env, obj_expr)?;
                            match obj_val {
                                Value::Object(obj_map) => {
                                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                                    obj_set_value(&obj_map, &key, v.clone())?;
                                    *last_value = v;
                                }
                                _ => {
                                    return Err(raise_eval_error!("Cannot assign to property of non-object"));
                                }
                            }
                        }
                    }
                    _ => {
                        return Err(raise_eval_error!("Invalid index type"));
                    }
                }
            }
            _ => {
                // Fallback: evaluate the expression normally
                *last_value = evaluate_expr(env, expr)?;
            }
        }
    } else if let Expr::LogicalAndAssign(target, value_expr) = expr {
        // Handle logical AND assignment: a &&= b
        let left_val = evaluate_expr(env, target)?;
        if is_truthy(&left_val) {
            match target.as_ref() {
                Expr::Var(name) => {
                    let v = evaluate_expr(env, value_expr)?;
                    env_set_recursive(env, name.as_str(), v.clone())?;
                    *last_value = v;
                }
                Expr::Property(obj_expr, prop_name) => {
                    let v = evaluate_expr(env, value_expr)?;
                    match set_prop_env(env, obj_expr, prop_name.as_str(), v.clone())? {
                        Some(updated_obj) => *last_value = updated_obj,
                        None => *last_value = v,
                    }
                }
                Expr::Index(obj_expr, idx_expr) => {
                    let idx_val = evaluate_expr(env, idx_expr)?;
                    let v = evaluate_expr(env, value_expr)?;
                    match idx_val {
                        Value::Number(n) => {
                            let key = n.to_string();
                            match set_prop_env(env, obj_expr, &key, v.clone())? {
                                Some(updated_obj) => *last_value = updated_obj,
                                None => *last_value = v,
                            }
                        }
                        Value::String(s) => {
                            let key = String::from_utf16_lossy(&s);
                            match set_prop_env(env, obj_expr, &key, v.clone())? {
                                Some(updated_obj) => *last_value = updated_obj,
                                None => *last_value = v,
                            }
                        }
                        Value::Symbol(sym) => {
                            // symbol index — set symbol-keyed property
                            if let Expr::Var(varname) = obj_expr.as_ref()
                                && let Some(rc_val) = env_get(env, varname)
                            {
                                let mut borrowed = rc_val.borrow_mut();
                                if let Value::Object(ref mut map) = *borrowed {
                                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                                    obj_set_value(map, &key, v.clone())?;
                                    *last_value = v;
                                } else {
                                    return Err(raise_eval_error!("Cannot assign to property of non-object"));
                                }
                            } else {
                                let obj_val = evaluate_expr(env, obj_expr)?;
                                match obj_val {
                                    Value::Object(obj_map) => {
                                        let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                                        obj_set_value(&obj_map, &key, v.clone())?;
                                        *last_value = v;
                                    }
                                    _ => {
                                        return Err(raise_eval_error!("Cannot assign to property of non-object"));
                                    }
                                }
                            }
                        }
                        _ => {
                            return Err(raise_eval_error!("Invalid index type"));
                        }
                    }
                }
                _ => {
                    *last_value = evaluate_expr(env, expr)?;
                }
            }
        } else {
            *last_value = left_val;
        }
    } else if let Expr::LogicalOrAssign(target, value_expr) = expr {
        // Handle logical OR assignment: a ||= b
        let left_val = evaluate_expr(env, target)?;
        if !is_truthy(&left_val) {
            match target.as_ref() {
                Expr::Var(name) => {
                    let v = evaluate_expr(env, value_expr)?;
                    env_set_recursive(env, name.as_str(), v.clone())?;
                    *last_value = v;
                }
                Expr::Property(obj_expr, prop_name) => {
                    let v = evaluate_expr(env, value_expr)?;
                    match set_prop_env(env, obj_expr, prop_name.as_str(), v.clone())? {
                        Some(updated_obj) => *last_value = updated_obj,
                        None => *last_value = v,
                    }
                }
                Expr::Index(obj_expr, idx_expr) => {
                    let idx_val = evaluate_expr(env, idx_expr)?;
                    let v = evaluate_expr(env, value_expr)?;
                    match idx_val {
                        Value::Number(n) => {
                            let key = n.to_string();
                            match set_prop_env(env, obj_expr, &key, v.clone())? {
                                Some(updated_obj) => *last_value = updated_obj,
                                None => *last_value = v,
                            }
                        }
                        Value::String(s) => {
                            let key = String::from_utf16_lossy(&s);
                            match set_prop_env(env, obj_expr, &key, v.clone())? {
                                Some(updated_obj) => *last_value = updated_obj,
                                None => *last_value = v,
                            }
                        }
                        Value::Symbol(sym) => {
                            if let Expr::Var(varname) = obj_expr.as_ref()
                                && let Some(rc_val) = env_get(env, varname)
                            {
                                let mut borrowed = rc_val.borrow_mut();
                                if let Value::Object(ref mut map) = *borrowed {
                                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                                    obj_set_value(map, &key, v.clone())?;
                                    *last_value = v;
                                } else {
                                    return Err(raise_eval_error!("Cannot assign to property of non-object"));
                                }
                            } else {
                                let obj_val = evaluate_expr(env, obj_expr)?;
                                match obj_val {
                                    Value::Object(obj_map) => {
                                        let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                                        obj_set_value(&obj_map, &key, v.clone())?;
                                        *last_value = v;
                                    }
                                    _ => {
                                        return Err(raise_eval_error!("Cannot assign to property of non-object"));
                                    }
                                }
                            }
                        }
                        _ => {
                            return Err(raise_eval_error!("Invalid index type"));
                        }
                    }
                }
                _ => {
                    *last_value = evaluate_expr(env, expr)?;
                }
            }
        } else {
            *last_value = left_val;
        }
    } else if let Expr::NullishAssign(target, value_expr) = expr {
        // Handle nullish coalescing assignment: a ??= b
        let left_val = evaluate_expr(env, target)?;
        match left_val {
            Value::Undefined => match target.as_ref() {
                Expr::Var(name) => {
                    let v = evaluate_expr(env, value_expr)?;
                    env_set_recursive(env, name.as_str(), v.clone())?;
                    *last_value = v;
                }
                Expr::Property(obj_expr, prop_name) => {
                    let v = evaluate_expr(env, value_expr)?;
                    match set_prop_env(env, obj_expr, prop_name.as_str(), v.clone())? {
                        Some(updated_obj) => *last_value = updated_obj,
                        None => *last_value = v,
                    }
                }
                Expr::Index(obj_expr, idx_expr) => {
                    let idx_val = evaluate_expr(env, idx_expr)?;
                    let v = evaluate_expr(env, value_expr)?;
                    match idx_val {
                        Value::Number(n) => {
                            let key = n.to_string();
                            match set_prop_env(env, obj_expr, &key, v.clone())? {
                                Some(updated_obj) => *last_value = updated_obj,
                                None => *last_value = v,
                            }
                        }
                        Value::String(s) => {
                            let key = String::from_utf16_lossy(&s);
                            match set_prop_env(env, obj_expr, &key, v.clone())? {
                                Some(updated_obj) => *last_value = updated_obj,
                                None => *last_value = v,
                            }
                        }
                        Value::Symbol(sym) => {
                            if let Expr::Var(varname) = obj_expr.as_ref()
                                && let Some(rc_val) = env_get(env, varname)
                            {
                                let mut borrowed = rc_val.borrow_mut();
                                if let Value::Object(ref mut map) = *borrowed {
                                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                                    obj_set_value(map, &key, v.clone())?;
                                    *last_value = v;
                                } else {
                                    return Err(raise_eval_error!("Cannot assign to property of non-object"));
                                }
                            } else {
                                let obj_val = evaluate_expr(env, obj_expr)?;
                                match obj_val {
                                    Value::Object(obj_map) => {
                                        let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                                        obj_set_value(&obj_map, &key, v.clone())?;
                                        *last_value = v;
                                    }
                                    _ => {
                                        return Err(raise_eval_error!("Cannot assign to property of non-object"));
                                    }
                                }
                            }
                        }
                        _ => {
                            return Err(raise_eval_error!("Invalid index type"));
                        }
                    }
                }
                _ => {
                    *last_value = evaluate_expr(env, expr)?;
                }
            },
            _ => {
                *last_value = left_val;
            }
        }
    } else {
        *last_value = evaluate_expr(env, expr)?;
    }
    Ok(None)
}

fn perform_array_destructuring(
    env: &JSObjectDataPtr,
    pattern: &Vec<DestructuringElement>,
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
                        let val = if let Some(val_rc) = obj_get_value(arr, &key)? {
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
                        let val = if let Some(val_rc) = obj_get_value(arr, &key)? {
                            val_rc.borrow().clone()
                        } else {
                            Value::Undefined
                        };
                        perform_array_destructuring(env, nested_pattern, &val, is_const)?;
                        index += 1;
                    }
                    DestructuringElement::NestedObject(nested_pattern) => {
                        let key = PropertyKey::String(index.to_string());
                        let val = if let Some(val_rc) = obj_get_value(arr, &key)? {
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
                    if let Some(val_rc) = obj_get_value(arr, &key)? {
                        rest_elements.push(val_rc.borrow().clone());
                    }
                }
                let rest_obj = Rc::new(RefCell::new(JSObjectData::new()));
                let mut rest_index = 0;
                for elem in rest_elements {
                    obj_set_value(&rest_obj, &rest_index.to_string().into(), elem)?;
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
    pattern: &Vec<ObjectDestructuringElement>,
    value: &Value,
    is_const: bool,
) -> Result<(), JSError> {
    match value {
        Value::Object(obj) => {
            for element in pattern {
                match element {
                    ObjectDestructuringElement::Property { key, value: dest } => {
                        let key = PropertyKey::String(key.clone());
                        let prop_val = if let Some(val_rc) = obj_get_value(obj, &key)? {
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
                        let rest_obj = Rc::new(RefCell::new(JSObjectData::new()));
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

/// Helper: iterate over an iterable value (array-like object) and perform
/// object-pattern destructuring per element, executing `body` each iteration.
/// `label_name` controls how labeled break/continue are handled; pass None for
/// unlabeled loops.
fn for_of_destructuring_object_iter(
    env: &JSObjectDataPtr,
    pattern: &Vec<ObjectDestructuringElement>,
    iterable_val: &Value,
    body: &[Statement],
    last_value: &mut Value,
    label_name: Option<&str>,
) -> Result<Option<ControlFlow>, JSError> {
    match iterable_val {
        Value::Object(obj_map) => {
            if is_array(obj_map) {
                let len = get_array_length(obj_map).unwrap_or(0);
                for i in 0..len {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(element_rc) = obj_get_value(obj_map, &key)? {
                        let element = element_rc.borrow().clone();
                        // perform destructuring into env (var semantics)
                        perform_object_destructuring(env, pattern, &element, false)?;
                        let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                        block_env.borrow_mut().prototype = Some(env.clone());
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
    pattern: &Vec<DestructuringElement>,
    iterable_val: &Value,
    body: &[Statement],
    last_value: &mut Value,
    label_name: Option<&str>,
) -> Result<Option<ControlFlow>, JSError> {
    match iterable_val {
        Value::Object(obj_map) => {
            if is_array(obj_map) {
                let len = get_array_length(obj_map).unwrap_or(0);
                for i in 0..len {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(element_rc) = obj_get_value(obj_map, &key)? {
                        let element = element_rc.borrow().clone();
                        // perform array destructuring into env (var semantics)
                        perform_array_destructuring(env, pattern, &element, false)?;
                        let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                        block_env.borrow_mut().prototype = Some(env.clone());
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
                    if let Some(iterator_val) = obj_get_value(obj_map, &iterator_key)? {
                        let iterator_factory = iterator_val.borrow().clone();
                        // Call Symbol.iterator to get the iterator object
                        let iterator = match iterator_factory {
                            Value::Closure(_params, body, closure_env) => evaluate_statements(&closure_env, &body)?,
                            _ => return Err(raise_eval_error!("Symbol.iterator is not a function")),
                        };

                        if let Value::Object(iterator_obj) = iterator {
                            if let Some(next_val) = obj_get_value(&iterator_obj, &"next".into())? {
                                let next_func = next_val.borrow().clone();
                                loop {
                                    // Call next()
                                    let next_result = match &next_func {
                                        Value::Closure(_params, body, closure_env) => {
                                            let call_env = Rc::new(RefCell::new(JSObjectData::new()));
                                            call_env.borrow_mut().prototype = Some(closure_env.clone());
                                            evaluate_statements(&call_env, body)?
                                        }
                                        Value::Function(_func_name) => {
                                            // Handle built-in functions if needed
                                            return Err(raise_eval_error!("Iterator next function not implemented"));
                                        }
                                        _ => return Err(raise_eval_error!("Iterator next is not a function")),
                                    };

                                    if let Value::Object(result_obj) = next_result {
                                        // Check if done
                                        if let Some(done_val) = obj_get_value(&result_obj, &"done".into())?
                                            && let Value::Boolean(true) = *done_val.borrow()
                                        {
                                            break; // Iteration complete
                                        }

                                        // Get value
                                        if let Some(value_val) = obj_get_value(&result_obj, &"value".into())? {
                                            let element = value_val.borrow().clone();
                                            // perform array destructuring into env (var semantics)
                                            perform_array_destructuring(env, pattern, &element, false)?;
                                            let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                                            block_env.borrow_mut().prototype = Some(env.clone());
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
                                    } else {
                                        return Err(raise_eval_error!("Iterator next() did not return an object"));
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
        Value::Object(obj_map) => {
            if is_array(&obj_map) {
                let len = get_array_length(&obj_map).unwrap_or(0);
                for i in 0..len {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(element_rc) = obj_get_value(&obj_map, &key)? {
                        let element = element_rc.borrow().clone();
                        env_set_recursive(env, var, element)?;
                        let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                        block_env.borrow_mut().prototype = Some(env.clone());
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
            } else {
                // Attempt iterator protocol via Symbol.iterator
                // Look up well-known Symbol.iterator and call it on the object to obtain an iterator
                if let Some(iter_sym_rc) = get_well_known_symbol_rc("iterator") {
                    let key = PropertyKey::Symbol(iter_sym_rc.clone());
                    if let Some(method_rc) = obj_get_value(&obj_map, &key)? {
                        // method can be a function/closure or an object
                        let iterator_val = match &*method_rc.borrow() {
                            Value::Closure(_params, body, captured_env) | Value::AsyncClosure(_params, body, captured_env) => {
                                // Call closure with 'this' bound to the object
                                let func_env = Rc::new(RefCell::new(JSObjectData::new()));
                                func_env.borrow_mut().prototype = Some(captured_env.clone());
                                // mark this as a function scope so var-hoisting and
                                // env_set_var bind into this frame rather than parent
                                func_env.borrow_mut().is_function_scope = true;
                                obj_set_value(&func_env, &"this".into(), Value::Object(obj_map.clone()))?;
                                // Execute body to produce iterator result
                                evaluate_statements(&func_env, body)?
                            }
                            Value::Function(func_name) => {
                                // Call built-in function (no arguments)
                                crate::js_function::handle_global_function(func_name, &[], env)?
                            }
                            Value::Object(iter_obj) => Value::Object(iter_obj.clone()),
                            _ => {
                                return Err(raise_eval_error!("iterator property is not callable"));
                            }
                        };

                        // Now we have iterator_val, expected to be an object with next() method
                        if let Value::Object(iter_obj) = iterator_val {
                            loop {
                                // call iter_obj.next()
                                if let Some(next_rc) = obj_get_value(&iter_obj, &"next".into())? {
                                    let next_val = match &*next_rc.borrow() {
                                        Value::Closure(_params, body, captured_env) | Value::AsyncClosure(_params, body, captured_env) => {
                                            let func_env = Rc::new(RefCell::new(JSObjectData::new()));
                                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                                            obj_set_value(&func_env, &"this".into(), Value::Object(iter_obj.clone()))?;
                                            evaluate_statements(&func_env, body)?
                                        }
                                        Value::Function(func_name) => crate::js_function::handle_global_function(func_name, &[], env)?,
                                        _ => {
                                            return Err(raise_eval_error!("next is not callable"));
                                        }
                                    };

                                    // next_val should be an object with { value, done }
                                    if let Value::Object(res_obj) = next_val {
                                        // Check done
                                        let done_val = obj_get_value(&res_obj, &"done".into())?;
                                        let done = match done_val {
                                            Some(d) => is_truthy(&d.borrow().clone()),
                                            None => false,
                                        };
                                        if done {
                                            break;
                                        }

                                        // Extract value
                                        let value_val = obj_get_value(&res_obj, &"value".into())?;
                                        let element = match value_val {
                                            Some(v) => v.borrow().clone(),
                                            None => Value::Undefined,
                                        };

                                        env_set_recursive(env, var, element)?;
                                        let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                                        block_env.borrow_mut().prototype = Some(env.clone());
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
        }
        Value::String(s) => {
            // Iterate over UTF-16 units in the string
            let mut i = 0usize;
            while let Some(ch) = utf16_char_at(&s, i) {
                env_set_recursive(env, var, Value::String(vec![ch]))?;
                let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                block_env.borrow_mut().prototype = Some(env.clone());
                block_env.borrow_mut().is_function_scope = false;
                match evaluate_statements_with_context(&block_env, body)? {
                    ControlFlow::Normal(val) => *last_value = val,
                    ControlFlow::Break(None) => break,
                    ControlFlow::Break(Some(lbl)) => return Ok(Some(ControlFlow::Break(Some(lbl)))),
                    ControlFlow::Continue(None) => {}
                    ControlFlow::Continue(Some(lbl)) => return Ok(Some(ControlFlow::Continue(Some(lbl)))),
                    ControlFlow::Return(val) => return Ok(Some(ControlFlow::Return(val))),
                }
                i += 1;
            }
            Ok(None)
        }
        _ => Err(raise_eval_error!("for-of loop requires an iterable")),
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
                    let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                    block_env.borrow_mut().prototype = Some(env.clone());
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
                    let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                    block_env.borrow_mut().prototype = Some(env.clone());
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
                    let block_env = Rc::new(RefCell::new(JSObjectData::new()));
                    block_env.borrow_mut().prototype = Some(env.clone());
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
        Expr::BigInt(s) => Ok(Value::BigInt(BigIntHolder::try_from(s.as_str())?)),
        Expr::StringLit(s) => evaluate_string_lit(s),
        Expr::Boolean(b) => evaluate_boolean(*b),
        Expr::Var(name) => evaluate_var(env, name),
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
        Expr::Index(obj, idx) => evaluate_index(env, obj, idx),
        Expr::Property(obj, prop) => evaluate_property(env, obj, prop),
        Expr::Call(func_expr, args) => match evaluate_call(env, func_expr, args) {
            Ok(v) => Ok(v),
            Err(e) => {
                log::error!(
                    "evaluate_expr: evaluate_call error for func_expr={:?} args={:?} error={e}",
                    func_expr,
                    args
                );
                Err(e)
            }
        },
        Expr::Function(params, body) => Ok(Value::Closure(params.clone(), body.clone(), env.clone())),
        Expr::GeneratorFunction(params, body) => Ok(Value::GeneratorFunction(params.clone(), body.clone(), env.clone())),
        Expr::ArrowFunction(params, body) => Ok(Value::Closure(params.clone(), body.clone(), env.clone())),
        Expr::AsyncArrowFunction(params, body) => Ok(Value::AsyncClosure(params.clone(), body.clone(), env.clone())),
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
        Expr::New(constructor, args) => evaluate_new(env, constructor, args),
        Expr::Super => evaluate_super(env),
        Expr::SuperCall(args) => evaluate_super_call(env, args),
        Expr::SuperProperty(prop) => evaluate_super_property(env, prop),
        Expr::SuperMethod(method, args) => evaluate_super_method(env, method, args),
        Expr::ArrayDestructuring(pattern) => evaluate_array_destructuring(env, pattern),
        Expr::ObjectDestructuring(pattern) => evaluate_object_destructuring(env, pattern),
        Expr::AsyncFunction(params, body) => Ok(Value::AsyncClosure(params.clone(), body.clone(), env.clone())),
        Expr::Await(expr) => {
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
                                return Err(raise_eval_error!(format!("Promise rejected: {}", value_to_string(reason))));
                            }
                            PromiseState::Pending => {
                                // Continue running the event loop
                            }
                        }
                    }
                }
                Value::Object(obj) => {
                    // Check if this is a Promise object with __promise property
                    if let Some(promise_rc) = obj_get_value(&obj, &"__promise".into())?
                        && let Value::Promise(promise) = promise_rc.borrow().clone()
                    {
                        // Wait for the promise to resolve by running the event loop
                        loop {
                            run_event_loop()?;
                            let promise_borrow = promise.borrow();
                            match &promise_borrow.state {
                                PromiseState::Fulfilled(val) => return Ok(val.clone()),
                                PromiseState::Rejected(reason) => {
                                    return Err(raise_eval_error!(format!("Promise rejected: {}", value_to_string(reason))));
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

fn evaluate_number(n: f64) -> Result<Value, JSError> {
    Ok(Value::Number(n))
}

fn evaluate_string_lit(s: &[u16]) -> Result<Value, JSError> {
    Ok(Value::String(s.to_vec()))
}

fn evaluate_boolean(b: bool) -> Result<Value, JSError> {
    Ok(Value::Boolean(b))
}

fn evaluate_var(env: &JSObjectDataPtr, name: &str) -> Result<Value, JSError> {
    if name == "console" {
        let v = Value::Object(make_console_object()?);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "assert" {
        let v = Value::Object(make_assert_object()?);
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
    } else if name == "String" {
        // Ensure a singleton String constructor object exists in the global env
        let ctor = super::ensure_constructor_object(env, "String", "__is_string_constructor")?;
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
        let json_obj = Rc::new(RefCell::new(JSObjectData::new()));
        obj_set_value(&json_obj, &"parse".into(), Value::Function("JSON.parse".to_string()))?;
        obj_set_value(&json_obj, &"stringify".into(), Value::Function("JSON.stringify".to_string()))?;
        let v = Value::Object(json_obj);
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Object" {
        // Return the Object constructor (we store it in the global environment as an object)
        if let Some(val_rc) = obj_get_value(env, &"Object".into())? {
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
    } else if name == "Array" {
        let v = Value::Function("Array".to_string());
        log::trace!("evaluate_var - {} -> {:?}", name, v);
        Ok(v)
    } else if name == "Number" {
        // If Number constructor is already stored in the environment, return it.
        if let Some(val_rc) = obj_get_value(env, &"Number".into())? {
            let resolved = val_rc.borrow().clone();
            log::trace!("evaluate_var - {} (from env) -> {:?}", name, resolved);
            return Ok(resolved);
        }
        // Otherwise, create the Number constructor object, store it in the env, and return it.
        let number_obj = make_number_object()?;
        obj_set_value(env, &"Number".into(), Value::Object(number_obj.clone()))?;
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
    } else if name == "Date" {
        let v = Value::Function("Date".to_string());
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
        if let Some(val_rc) = obj_get_value(env, &"Proxy".into())? {
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
            if let Some(val_rc) = obj_get_value(&current_env, &name.into())? {
                let resolved = val_rc.borrow().clone();
                log::trace!("evaluate_var - {} (found) -> {:?}", name, resolved);
                return Ok(resolved);
            }
            current_opt = current_env.borrow().prototype.clone();
        }
        log::trace!("evaluate_var - {} not found -> Undefined", name);
        Ok(Value::Undefined)
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            Value::BigInt(BigIntHolder::from(a + b))
        }
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
        Value::BigInt(s) => Expr::BigInt(s.raw.clone()),
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            Value::BigInt(BigIntHolder::from(a - b))
        }
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            Value::BigInt(BigIntHolder::from(a * b))
        }
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            if b < BigInt::from(0) {
                return Err(raise_eval_error!("negative exponent for bigint"));
            }
            let exp = b.to_u32().ok_or(raise_eval_error!("exponent too large"))?;
            Value::BigInt(BigIntHolder::from(a.pow(exp)))
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            if b == BigInt::from(0) {
                return Err(raise_eval_error!("Division by zero"));
            }
            Value::BigInt(BigIntHolder::from(a / b))
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            if b == BigInt::from(0) {
                return Err(raise_eval_error!("Division by zero"));
            }
            Value::BigInt(BigIntHolder::from(a % b))
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            use std::ops::BitXor;
            let res = a.bitxor(&b);
            Value::BigInt(BigIntHolder::from(res))
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            use std::ops::BitAnd;
            let res = a.bitand(&b);
            Value::BigInt(BigIntHolder::from(res))
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            use std::ops::BitOr;
            let res = a.bitor(&b);
            Value::BigInt(BigIntHolder::from(res))
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            use std::ops::Shl;
            // try to convert shift amount to usize
            let shift = b.to_usize().ok_or(raise_eval_error!("invalid bigint shift"))?;
            let res = a.shl(shift);
            Value::BigInt(BigIntHolder::from(res))
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
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
        (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
            let a = la.refresh_parsed(false)?;
            let b = rb.refresh_parsed(false)?;
            use std::ops::Shr;
            let shift = b.to_usize().ok_or(raise_eval_error!("invalid bigint shift"))?;
            let res = a.shr(shift);
            Value::BigInt(BigIntHolder::from(res))
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
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
            let _ = evaluate_assignment_expr(env, target, &Expr::BigInt(s.raw.clone()))?;
        }
        _ => unreachable!(),
    }
    Ok(result)
}

fn evaluate_assignment_expr(env: &JSObjectDataPtr, target: &Expr, value: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, value)?;
    match target {
        Expr::Var(name) => {
            log::debug!("evaluate_assignment_expr: assigning Var '{}' = {:?}", name, val);
            env_set_recursive(env, name, val.clone())?;
            Ok(val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_value(&obj_map, &prop.into(), val.clone())?;
                    Ok(val)
                }
                _ => Err(raise_eval_error!("Cannot assign to property of non-object")),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = PropertyKey::String(String::from_utf16_lossy(&s));
                    obj_set_value(&obj_map, &key, val.clone())?;
                    Ok(val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    // Check if this is a TypedArray first
                    let ta_val_opt = obj_get_value(&obj_map, &"__typedarray".into());
                    if let Ok(Some(ta_val)) = ta_val_opt
                        && let Value::TypedArray(ta) = &*ta_val.borrow()
                    {
                        // This is a TypedArray, use our set method
                        let idx = n as usize;
                        let val_num = match &val {
                            Value::Number(num) => *num as i64,
                            Value::BigInt(s) => s.raw.parse().unwrap_or(0),
                            _ => return Err(raise_eval_error!("TypedArray assignment value must be a number")),
                        };
                        ta.borrow_mut()
                            .set(idx, val_num)
                            .map_err(|_| raise_eval_error!("TypedArray index out of bounds"))?;
                        return Ok(val);
                    }
                    let key = PropertyKey::String(n.to_string());
                    obj_set_value(&obj_map, &key, val.clone())?;
                    Ok(val)
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_value(&obj_map, &key, val.clone())?;
                    Ok(val)
                }
                _ => Err(raise_eval_error!("Invalid index assignment")),
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
        Expr::Var(name) => {
            env_set_recursive(env, name, new_val.clone())?;
            Ok(new_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_value(&obj_map, &prop.into(), new_val.clone())?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Cannot increment property of non-object")),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = PropertyKey::String(String::from_utf16_lossy(&s));
                    obj_set_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_value(&obj_map, &key, new_val.clone())?;
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
        Expr::Var(name) => {
            env_set_recursive(env, name, new_val.clone())?;
            Ok(new_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_value(&obj_map, &prop.into(), new_val.clone())?;
                    Ok(new_val)
                }
                _ => Err(raise_eval_error!("Cannot decrement property of non-object")),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = PropertyKey::String(String::from_utf16_lossy(&s));
                    obj_set_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_value(&obj_map, &key, new_val.clone())?;
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
        Expr::Var(name) => {
            env_set_recursive(env, name, new_val)?;
            Ok(old_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_value(&obj_map, &prop.into(), new_val)?;
                    Ok(old_val)
                }
                _ => Err(raise_eval_error!("Cannot increment property of non-object")),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = PropertyKey::String(String::from_utf16_lossy(&s));
                    obj_set_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_value(&obj_map, &key, new_val)?;
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
        Expr::Var(name) => {
            env_set_recursive(env, name, new_val)?;
            Ok(old_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_value(&obj_map, &prop.into(), new_val)?;
                    Ok(old_val)
                }
                _ => Err(raise_eval_error!("Cannot decrement property of non-object")),
            }
        }
        Expr::Index(obj, idx) => {
            let obj_val = evaluate_expr(env, obj)?;
            let idx_val = evaluate_expr(env, idx)?;
            match (obj_val, idx_val) {
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = PropertyKey::String(String::from_utf16_lossy(&s));
                    obj_set_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_value(&obj_map, &key, new_val)?;
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
        Value::BigInt(mut s) => {
            // Negate BigInt (use cached parse)
            let a = s.refresh_parsed(false)?;
            let neg = -a;
            Ok(Value::BigInt(BigIntHolder::from(neg)))
        }
        _ => Err(raise_eval_error!("error")),
    }
}

fn evaluate_typeof(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    let val = evaluate_expr(env, expr)?;
    let type_str = match val {
        Value::Undefined => "undefined",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::BigInt(_) => "bigint",
        Value::Object(_) => "object",
        Value::Function(_) => "function",
        Value::Closure(_, _, _) | Value::AsyncClosure(_, _, _) | Value::GeneratorFunction(_, _, _) => "function",
        Value::ClassDefinition(_) => "function",
        Value::Getter(_, _) => "function",
        Value::Setter(_, _, _) => "function",
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
    };
    Ok(Value::String(utf8_to_utf16(type_str)))
}

fn evaluate_delete(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    match expr {
        Expr::Var(_) => {
            // Cannot delete local variables
            Ok(Value::Boolean(false))
        }
        Expr::Property(obj, prop) => {
            // Delete property from object
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    let deleted = obj_delete(&obj_map, &prop.into())?;
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
                (Value::Object(obj_map), Value::String(s)) => {
                    let key = PropertyKey::String(String::from_utf16_lossy(&s));
                    let deleted = obj_delete(&obj_map, &key)?;
                    Ok(Value::Boolean(deleted))
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    let deleted = obj_delete(&obj_map, &key)?;
                    Ok(Value::Boolean(deleted))
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    let deleted = obj_delete(&obj_map, &key)?;
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

fn evaluate_binary(env: &JSObjectDataPtr, left: &Expr, op: &BinaryOp, right: &Expr) -> Result<Value, JSError> {
    let l = evaluate_expr(env, left)?;
    let r = evaluate_expr(env, right)?;
    match op {
        BinaryOp::Add => {
            // If either side is an object, attempt ToPrimitive coercion (default hint) first
            let l_prim = if matches!(l, Value::Object(_)) {
                to_primitive(&l, "default")?
            } else {
                l.clone()
            };
            let r_prim = if matches!(r, Value::Object(_)) {
                to_primitive(&r, "default")?
            } else {
                r.clone()
            };
            // '+' should throw when a Symbol is encountered during implicit coercion
            if matches!(l_prim, Value::Symbol(_)) || matches!(r_prim, Value::Symbol(_)) {
                return Err(raise_type_error!("Cannot convert Symbol to primitive"));
            }
            match (l_prim, r_prim) {
                (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln + rn)),
                (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                    // BigInt + BigInt -> BigInt (use cached parse)
                    let a = la.refresh_parsed(false)?;
                    let b = rb.refresh_parsed(false)?;
                    let res = a + b;
                    Ok(Value::BigInt(BigIntHolder::from(res)))
                }
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
                    result.extend_from_slice(&utf8_to_utf16(&rb.raw));
                    Ok(Value::String(result))
                }
                (Value::BigInt(la), Value::String(rs)) => {
                    // BigInt + String -> concatenation
                    let mut result = utf8_to_utf16(&la.raw);
                    result.extend_from_slice(&rs);
                    Ok(Value::String(result))
                }
                _ => Err(raise_eval_error!("error")),
            }
        }
        BinaryOp::Sub => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln - rn)),
            (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                let res = a - b;
                Ok(Value::BigInt(BigIntHolder::from(res)))
            }
            // Mixing BigInt and Number is not allowed for arithmetic
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("error")),
        },
        BinaryOp::Mul => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln * rn)),
            (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                let res = a * b;
                Ok(Value::BigInt(BigIntHolder::from(res)))
            }
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("error")),
        },
        BinaryOp::Pow => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Number(ln.powf(rn))),
            (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                // exponent must be non-negative and fit into u32 for pow
                if b < BigInt::from(0) {
                    return Err(raise_eval_error!("negative exponent for bigint"));
                }
                let exp = b.to_u32().ok_or(raise_eval_error!("exponent too large"))?;
                let res = a.pow(exp);
                Ok(Value::BigInt(BigIntHolder::from(res)))
            }
            // Mixing BigInt and Number is disallowed for exponentiation
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("error")),
        },
        BinaryOp::Div => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => {
                if rn == 0.0 {
                    Err(raise_eval_error!("error"))
                } else {
                    Ok(Value::Number(ln / rn))
                }
            }
            (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                if b == BigInt::from(0) {
                    Err(raise_eval_error!("error"))
                } else {
                    let res = a / b;
                    Ok(Value::BigInt(BigIntHolder::from(res)))
                }
            }
            // Mixing BigInt and Number is not allowed
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("error")),
        },
        BinaryOp::Equal => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Boolean(ln == rn)),
            (Value::BigInt(la), Value::BigInt(rb)) => Ok(Value::Boolean(la.raw == rb.raw)),
            (Value::BigInt(mut la), Value::Number(rn)) => {
                // If Number is NaN or infinite, it's never equal to a BigInt
                if rn.is_nan() || !rn.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                // Only integral numbers can equal a BigInt. If the number has a fractional
                // part it cannot equal a BigInt.
                if rn.fract() != 0.0 {
                    return Ok(Value::Boolean(false));
                }

                // Convert the Number's integer value to a decimal string and parse as BigInt
                // so we perform an exact integer comparison without floating-point precision pitfalls.
                let num_str = format!("{:.0}", rn);
                if let Ok(num_bi) = BigInt::from_str(&num_str) {
                    let a = la.refresh_parsed(false)?;
                    return Ok(Value::Boolean(a == num_bi));
                }

                Ok(Value::Boolean(false))
            }
            (Value::Number(ln), Value::BigInt(rb)) => {
                // If Number is NaN or infinite, it's never equal to a BigInt
                if ln.is_nan() || !ln.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if ln.fract() != 0.0 {
                    return Ok(Value::Boolean(false));
                }

                let num_str = format!("{:.0}", ln);
                if let Ok(num_bi) = BigInt::from_str(&num_str) {
                    let mut rb = rb;
                    let b = rb.refresh_parsed(false)?;
                    return Ok(Value::Boolean(b == num_bi));
                }

                Ok(Value::Boolean(false))
            }
            (Value::String(ls), Value::String(rs)) => Ok(Value::Boolean(ls == rs)),
            (Value::Boolean(lb), Value::Boolean(rb)) => Ok(Value::Boolean(lb == rb)),
            (Value::Symbol(sa), Value::Symbol(sb)) => Ok(Value::Boolean(Rc::ptr_eq(&sa, &sb))),
            (Value::Undefined, Value::Undefined) => Ok(Value::Boolean(true)),
            (Value::Object(a), Value::Object(b)) => Ok(Value::Boolean(Rc::ptr_eq(&a, &b))),
            _ => Ok(Value::Boolean(false)), // Different types are not equal
        },
        BinaryOp::StrictEqual => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Boolean(ln == rn)),
            (Value::BigInt(la), Value::BigInt(rb)) => Ok(Value::Boolean(la.raw == rb.raw)),
            (Value::String(ls), Value::String(rs)) => Ok(Value::Boolean(ls == rs)),
            (Value::Boolean(lb), Value::Boolean(rb)) => Ok(Value::Boolean(lb == rb)),
            (Value::Symbol(sa), Value::Symbol(sb)) => Ok(Value::Boolean(Rc::ptr_eq(&sa, &sb))),
            (Value::Undefined, Value::Undefined) => Ok(Value::Boolean(true)),
            (Value::Object(a), Value::Object(b)) => Ok(Value::Boolean(Rc::ptr_eq(&a, &b))),
            _ => Ok(Value::Boolean(false)), // Different types are not equal
        },
        BinaryOp::NotEqual => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Boolean(ln != rn)),
            (Value::BigInt(la), Value::BigInt(rb)) => Ok(Value::Boolean(la.raw != rb.raw)),
            (Value::BigInt(mut la), Value::Number(rn)) => {
                // reuse equality rules: invert them
                if rn.is_nan() || !rn.is_finite() {
                    return Ok(Value::Boolean(true));
                }
                if rn.fract() != 0.0 {
                    return Ok(Value::Boolean(true));
                }
                let num_str = format!("{:.0}", rn);
                if let Ok(num_bi) = BigInt::from_str(&num_str) {
                    let a = la.refresh_parsed(false)?;
                    return Ok(Value::Boolean(a != num_bi));
                }
                Ok(Value::Boolean(true))
            }
            (Value::Number(ln), Value::BigInt(mut rb)) => {
                if ln.is_nan() || !ln.is_finite() {
                    return Ok(Value::Boolean(true));
                }
                if ln.fract() != 0.0 {
                    return Ok(Value::Boolean(true));
                }
                let num_str = format!("{:.0}", ln);
                if let Ok(num_bi) = BigInt::from_str(&num_str) {
                    let b = rb.refresh_parsed(false)?;
                    return Ok(Value::Boolean(b != num_bi));
                }
                Ok(Value::Boolean(true))
            }
            (Value::String(ls), Value::String(rs)) => Ok(Value::Boolean(ls != rs)),
            (Value::Boolean(lb), Value::Boolean(rb)) => Ok(Value::Boolean(lb != rb)),
            (Value::Symbol(sa), Value::Symbol(sb)) => Ok(Value::Boolean(!Rc::ptr_eq(&sa, &sb))),
            (Value::Undefined, Value::Undefined) => Ok(Value::Boolean(false)),
            (Value::Object(a), Value::Object(b)) => Ok(Value::Boolean(!Rc::ptr_eq(&a, &b))),
            _ => Ok(Value::Boolean(true)), // Different types are not equal, so not equal is true
        },
        BinaryOp::StrictNotEqual => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => Ok(Value::Boolean(ln != rn)),
            (Value::BigInt(la), Value::BigInt(rb)) => Ok(Value::Boolean(la.raw != rb.raw)),
            (Value::String(ls), Value::String(rs)) => Ok(Value::Boolean(ls != rs)),
            (Value::Boolean(lb), Value::Boolean(rb)) => Ok(Value::Boolean(lb != rb)),
            (Value::Symbol(sa), Value::Symbol(sb)) => Ok(Value::Boolean(!Rc::ptr_eq(&sa, &sb))),
            (Value::Undefined, Value::Undefined) => Ok(Value::Boolean(false)),
            (Value::Object(a), Value::Object(b)) => Ok(Value::Boolean(!Rc::ptr_eq(&a, &b))),
            _ => Ok(Value::Boolean(true)), // Different types are not equal, so not equal is true
        },
        BinaryOp::LessThan => {
            // Follow JS abstract relational comparison with ToPrimitive(Number) hint
            let mut l_prim = if matches!(l, Value::Object(_)) {
                to_primitive(&l, "number")?
            } else {
                l.clone()
            };
            let mut r_prim = if matches!(r, Value::Object(_)) {
                to_primitive(&r, "number")?
            } else {
                r.clone()
            };

            // If both are strings, do lexicographic comparison
            if let (Value::String(ls), Value::String(rs)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(ls < rs));
            }
            if let (Value::BigInt(la), Value::BigInt(rb)) = (&mut l_prim, &mut r_prim) {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                return Ok(Value::Boolean(a < b));
            }
            if let (Value::BigInt(la), Value::Number(rn)) = (&mut l_prim, &r_prim) {
                let rn = *rn;
                // NaN / infinite are always false for relational comparisons with BigInt
                if rn.is_nan() || !rn.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                // If number is integer, compare as BigInt exactly
                if rn.fract() == 0.0 {
                    let num_str = format!("{:.0}", rn);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        let a = la.refresh_parsed(false)?;
                        return Ok(Value::Boolean(a < num_bi));
                    }
                    return Ok(Value::Boolean(false));
                }
                // Non-integer number: compare BigInt <= floor(number)
                let floor = rn.floor();
                let floor_str = format!("{:.0}", floor);
                if let Ok(floor_bi) = BigInt::from_str(&floor_str) {
                    let a = la.refresh_parsed(false)?;
                    return Ok(Value::Boolean(a <= floor_bi));
                }
                return Ok(Value::Boolean(false));
            }
            if let (Value::Number(ln), Value::BigInt(rb)) = (&l_prim, &mut r_prim) {
                let ln = *ln;
                if ln.is_nan() || !ln.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if ln.fract() == 0.0 {
                    let num_str = format!("{:.0}", ln);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        let b = rb.refresh_parsed(false)?;
                        return Ok(Value::Boolean(num_bi < b));
                    }
                    return Ok(Value::Boolean(false));
                }
                // Non-integer: ln < bigint <-> floor(ln) < bigint
                let floor = ln.floor();
                let floor_str = format!("{:.0}", floor);
                if let Ok(floor_bi) = BigInt::from_str(&floor_str) {
                    let b = rb.refresh_parsed(false)?;
                    return Ok(Value::Boolean(floor_bi < b));
                }
                return Ok(Value::Boolean(false));
            }
            // Fallback: convert values to numbers and compare. Non-coercible symbols/types will error.
            {
                // Helper to convert a value to f64 for comparison (ToNumber semantics simplified)
                let to_num = |v: &mut Value| -> Result<f64, JSError> {
                    match v {
                        Value::Number(n) => Ok(*n),
                        Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
                        Value::BigInt(s) => {
                            if let Some(f) = s.refresh_parsed(true)?.to_f64() {
                                Ok(f)
                            } else {
                                Ok(f64::NAN)
                            }
                        }
                        Value::String(s) => {
                            let sstr = String::from_utf16_lossy(s);
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
                };

                let ln = to_num(&mut l_prim)?;
                let rn = to_num(&mut r_prim)?;
                if ln.is_nan() || rn.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                Ok(Value::Boolean(ln < rn))
            }
        }
        BinaryOp::GreaterThan => {
            // Abstract relational comparison with ToPrimitive(Number) hint
            let mut l_prim = if matches!(l, Value::Object(_)) {
                to_primitive(&l, "number")?
            } else {
                l.clone()
            };
            let mut r_prim = if matches!(r, Value::Object(_)) {
                to_primitive(&r, "number")?
            } else {
                r.clone()
            };

            // If both strings, lexicographic compare
            if let (Value::String(ls), Value::String(rs)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(ls > rs));
            }
            if let (Value::BigInt(la), Value::BigInt(rb)) = (&mut l_prim, &mut r_prim) {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                return Ok(Value::Boolean(a > b));
            }
            if let (Value::BigInt(la), Value::Number(rn)) = (&mut l_prim, &r_prim) {
                let rn = *rn;
                if rn.is_nan() || !rn.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                // integer -> exact BigInt compare
                if rn.fract() == 0.0 {
                    let num_str = format!("{:.0}", rn);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        let a = la.refresh_parsed(false)?;
                        return Ok(Value::Boolean(a > num_bi));
                    }
                    return Ok(Value::Boolean(false));
                }
                // non-integer -> compare against ceil(rn): a > rn <=> a >= ceil(rn)
                let ceil = rn.ceil();
                let ceil_str = format!("{:.0}", ceil);
                if let Ok(ceil_bi) = BigInt::from_str(&ceil_str) {
                    let a = la.refresh_parsed(false)?;
                    return Ok(Value::Boolean(a >= ceil_bi));
                }
                return Ok(Value::Boolean(false));
            }
            if let (Value::Number(ln), Value::BigInt(rb)) = (&l_prim, &mut r_prim) {
                let ln = *ln;
                if ln.is_nan() || !ln.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if ln.fract() == 0.0 {
                    let num_str = format!("{:.0}", ln);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        let b = rb.refresh_parsed(false)?;
                        return Ok(Value::Boolean(num_bi > b));
                    }
                    return Ok(Value::Boolean(false));
                }
                // ln > bigint <=> ceil(ln) > bigint
                let ceil = ln.ceil();
                let ceil_str = format!("{:.0}", ceil);
                if let Ok(ceil_bi) = BigInt::from_str(&ceil_str) {
                    let b = rb.refresh_parsed(false)?;
                    return Ok(Value::Boolean(ceil_bi > b));
                }
                return Ok(Value::Boolean(false));
            }
            {
                let to_num = |v: &mut Value| -> Result<f64, JSError> {
                    match v {
                        Value::Number(n) => Ok(*n),
                        Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
                        Value::BigInt(s) => {
                            if let Some(f) = s.refresh_parsed(true)?.to_f64() {
                                Ok(f)
                            } else {
                                Ok(f64::NAN)
                            }
                        }
                        Value::String(s) => {
                            let sstr = String::from_utf16_lossy(s);
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
                };

                let ln = to_num(&mut l_prim)?;
                let rn = to_num(&mut r_prim)?;
                if ln.is_nan() || rn.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                Ok(Value::Boolean(ln > rn))
            }
        }
        BinaryOp::LessEqual => {
            // Use ToPrimitive(Number) hint then compare, strings compare lexicographically
            let mut l_prim = if matches!(l, Value::Object(_)) {
                to_primitive(&l, "number")?
            } else {
                l.clone()
            };
            let mut r_prim = if matches!(r, Value::Object(_)) {
                to_primitive(&r, "number")?
            } else {
                r.clone()
            };

            if let (Value::String(ls), Value::String(rs)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(ls <= rs));
            }
            if let (Value::BigInt(la), Value::BigInt(rb)) = (&mut l_prim, &mut r_prim) {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                return Ok(Value::Boolean(a <= b));
            }
            if let (Value::BigInt(la), Value::Number(rn)) = (&mut l_prim, &r_prim) {
                if rn.is_nan() || !rn.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if rn.fract() == 0.0 {
                    let num_str = format!("{:.0}", rn);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        let a = la.refresh_parsed(false)?;
                        return Ok(Value::Boolean(a <= num_bi));
                    }
                    return Ok(Value::Boolean(false));
                }
                // non-integer number: compare a <= floor(rn)
                let floor = rn.floor();
                let floor_str = format!("{:.0}", floor);
                if let Ok(floor_bi) = BigInt::from_str(&floor_str) {
                    let a = la.refresh_parsed(false)?;
                    return Ok(Value::Boolean(a <= floor_bi));
                }
                return Ok(Value::Boolean(false));
            }
            if let (Value::Number(ln), Value::BigInt(rb)) = (&l_prim, &mut r_prim) {
                if ln.is_nan() || !ln.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if ln.fract() == 0.0 {
                    let num_str = format!("{:.0}", ln);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        let b = rb.refresh_parsed(false)?;
                        return Ok(Value::Boolean(num_bi <= b));
                    }
                    return Ok(Value::Boolean(false));
                }
                // non-integer number: ln <= bigint <=> floor(ln) < bigint
                let floor = ln.floor();
                let floor_str = format!("{:.0}", floor);
                if let Ok(floor_bi) = BigInt::from_str(&floor_str) {
                    let b = rb.refresh_parsed(false)?;
                    return Ok(Value::Boolean(floor_bi < b));
                }
                return Ok(Value::Boolean(false));
            }
            {
                let to_num = |v: &mut Value| -> Result<f64, JSError> {
                    match v {
                        Value::Number(n) => Ok(*n),
                        Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
                        Value::BigInt(s) => {
                            if let Some(f) = s.refresh_parsed(false)?.to_f64() {
                                Ok(f)
                            } else {
                                Ok(f64::NAN)
                            }
                        }
                        Value::String(s) => {
                            let sstr = String::from_utf16_lossy(s);
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
                };

                let ln = to_num(&mut l_prim)?;
                let rn = to_num(&mut r_prim)?;
                if ln.is_nan() || rn.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                Ok(Value::Boolean(ln <= rn))
            }
        }
        BinaryOp::GreaterEqual => {
            // ToPrimitive(Number) hint with fallback to numeric comparison; strings compare lexicographically
            let mut l_prim = if matches!(l, Value::Object(_)) {
                to_primitive(&l, "number")?
            } else {
                l.clone()
            };
            let mut r_prim = if matches!(r, Value::Object(_)) {
                to_primitive(&r, "number")?
            } else {
                r.clone()
            };

            if let (Value::String(ls), Value::String(rs)) = (&l_prim, &r_prim) {
                return Ok(Value::Boolean(ls >= rs));
            }
            if let (Value::BigInt(la), Value::BigInt(rb)) = (&mut l_prim, &mut r_prim) {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                return Ok(Value::Boolean(a >= b));
            }
            if let (Value::BigInt(la), Value::Number(rn)) = (&mut l_prim, &r_prim) {
                let rn = *rn;
                if rn.is_nan() || !rn.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if rn.fract() == 0.0 {
                    let num_str = format!("{:.0}", rn);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        let a = la.refresh_parsed(false)?;
                        return Ok(Value::Boolean(a >= num_bi));
                    }
                    return Ok(Value::Boolean(false));
                }
                // non-integer rn: a >= ceil(rn)
                let ceil = rn.ceil();
                let ceil_str = format!("{:.0}", ceil);
                if let Ok(ceil_bi) = BigInt::from_str(&ceil_str) {
                    let a = la.refresh_parsed(false)?;
                    return Ok(Value::Boolean(a >= ceil_bi));
                }
                return Ok(Value::Boolean(false));
            }
            if let (Value::Number(ln), Value::BigInt(rb)) = (&l_prim, &mut r_prim) {
                let ln = *ln;
                if ln.is_nan() || !ln.is_finite() {
                    return Ok(Value::Boolean(false));
                }
                if ln.fract() == 0.0 {
                    let num_str = format!("{:.0}", ln);
                    if let Ok(num_bi) = BigInt::from_str(&num_str) {
                        let b = rb.refresh_parsed(false)?;
                        return Ok(Value::Boolean(num_bi >= b));
                    }
                    return Ok(Value::Boolean(false));
                }
                // non-integer ln: ln >= b <=> ceil(ln) > b
                let ceil = ln.ceil();
                let ceil_str = format!("{:.0}", ceil);
                if let Ok(ceil_bi) = BigInt::from_str(&ceil_str) {
                    let b = rb.refresh_parsed(false)?;
                    return Ok(Value::Boolean(ceil_bi > b));
                }
                return Ok(Value::Boolean(false));
            }
            {
                let to_num = |v: &mut Value| -> Result<f64, JSError> {
                    match v {
                        Value::Number(n) => Ok(*n),
                        Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
                        Value::BigInt(s) => {
                            if let Some(f) = s.refresh_parsed(false)?.to_f64() {
                                Ok(f)
                            } else {
                                Ok(f64::NAN)
                            }
                        }
                        Value::String(s) => {
                            let sstr = String::from_utf16_lossy(s);
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
                };

                let ln = to_num(&mut l_prim)?;
                let rn = to_num(&mut r_prim)?;
                if ln.is_nan() || rn.is_nan() {
                    return Ok(Value::Boolean(false));
                }
                Ok(Value::Boolean(ln >= rn))
            }
        }
        BinaryOp::Mod => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => {
                if rn == 0.0 {
                    Err(raise_eval_error!("Division by zero"))
                } else {
                    Ok(Value::Number(ln % rn))
                }
            }
            (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                if b == BigInt::from(0) {
                    Err(raise_eval_error!("Division by zero"))
                } else {
                    Ok(Value::BigInt(BigIntHolder::from(a % b)))
                }
            }
            // Mixing BigInt and Number is not allowed
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("Modulo operation only supported for numbers")),
        },
        BinaryOp::InstanceOf => {
            // Check if left is an instance of right (constructor)
            log::trace!("Evaluating instanceof with left={:?}, right={:?}", l, r);
            match (l, r) {
                (Value::Object(obj), Value::Object(constructor)) => {
                    // Debug: inspect the object's direct __proto__ read before instanceof
                    match crate::core::obj_get_value(&obj, &"__proto__".into())? {
                        Some(v) => log::trace!("pre-instanceof: obj.__proto__ = {:?}", v),
                        None => log::trace!("pre-instanceof: obj.__proto__ = None"),
                    }
                    Ok(Value::Boolean(is_instance_of(&obj, &constructor)?))
                }
                _ => Ok(Value::Boolean(false)),
            }
        }
        BinaryOp::BitXor => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => {
                let a = crate::core::number::to_int32(ln);
                let b = crate::core::number::to_int32(rn);
                Ok(Value::Number((a ^ b) as f64))
            }
            (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                use std::ops::BitXor;
                let res = a.bitxor(&b);
                Ok(Value::BigInt(BigIntHolder::from(res)))
            }
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("Bitwise XOR only supported for numbers or BigInt")),
        },
        BinaryOp::In => {
            // Check if property exists in object
            match (l, r) {
                (Value::String(prop), Value::Object(obj)) => {
                    let prop_str = PropertyKey::String(String::from_utf16_lossy(&prop));
                    Ok(Value::Boolean(obj_get_value(&obj, &prop_str)?.is_some()))
                }
                _ => Ok(Value::Boolean(false)),
            }
        }
        BinaryOp::BitAnd => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => {
                let a = crate::core::number::to_int32(ln);
                let b = crate::core::number::to_int32(rn);
                Ok(Value::Number((a & b) as f64))
            }
            (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                use std::ops::BitAnd;
                let res = a.bitand(&b);
                Ok(Value::BigInt(BigIntHolder::from(res)))
            }
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("Bitwise AND only supported for numbers or BigInt")),
        },
        BinaryOp::BitOr => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => {
                let a = crate::core::number::to_int32(ln);
                let b = crate::core::number::to_int32(rn);
                Ok(Value::Number((a | b) as f64))
            }
            (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                use std::ops::BitOr;
                let res = a.bitor(&b);
                Ok(Value::BigInt(BigIntHolder::from(res)))
            }
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("Bitwise OR only supported for numbers or BigInt")),
        },
        BinaryOp::LeftShift => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => {
                let a = crate::core::number::to_int32(ln);
                let shift = crate::core::number::to_uint32(rn) & 0x1f;
                let res = a.wrapping_shl(shift);
                Ok(Value::Number(res as f64))
            }
            (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                if b < BigInt::from(0) {
                    return Err(raise_eval_error!("negative shift count"));
                }
                let shift = b.to_u32().ok_or(raise_eval_error!("shift count too large"))?;
                use std::ops::Shl;
                let res = a.shl(shift);
                Ok(Value::BigInt(BigIntHolder::from(res)))
            }
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("Left shift only supported for numbers or BigInt")),
        },
        BinaryOp::RightShift => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => {
                let a = crate::core::number::to_int32(ln);
                let shift = crate::core::number::to_uint32(rn) & 0x1f;
                let res = a >> shift;
                Ok(Value::Number(res as f64))
            }
            (Value::BigInt(mut la), Value::BigInt(mut rb)) => {
                let a = la.refresh_parsed(false)?;
                let b = rb.refresh_parsed(false)?;
                if b < BigInt::from(0) {
                    return Err(raise_eval_error!("negative shift count"));
                }
                let shift = b.to_u32().ok_or(raise_eval_error!("shift count too large"))?;
                use std::ops::Shr;
                let res = a.shr(shift);
                Ok(Value::BigInt(BigIntHolder::from(res)))
            }
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("Right shift only supported for numbers or BigInt")),
        },
        BinaryOp::UnsignedRightShift => match (l, r) {
            (Value::Number(ln), Value::Number(rn)) => {
                let a = crate::core::number::to_uint32(ln);
                let shift = crate::core::number::to_uint32(rn) & 0x1f;
                let res = a >> shift;
                Ok(Value::Number(res as f64))
            }
            (Value::BigInt(_), Value::BigInt(_)) => Err(raise_type_error!("Unsigned right shift is not supported for BigInt")),
            (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                Err(raise_type_error!("Cannot mix BigInt and other types"))
            }
            _ => Err(raise_eval_error!("Unsigned right shift only supported for numbers")),
        },
        BinaryOp::NullishCoalescing => {
            // Nullish coalescing: return right if left is null or undefined, otherwise left
            match l {
                Value::Undefined => Ok(r),
                _ => Ok(l),
            }
        }
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
        (Value::Object(obj_map), Value::Number(n)) => {
            // Check if this is a TypedArray first
            if let Some(ta_val) = obj_get_value(&obj_map, &"__typedarray".into())?
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
                            TypedArrayKind::BigInt64 | TypedArrayKind::BigUint64 => Value::BigInt(BigIntHolder::from(BigInt::from(val))),
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
            if let Some(val) = obj_get_value(&obj_map, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(obj_map), Value::String(s)) => {
            // Object property access with string key
            let key = PropertyKey::String(String::from_utf16_lossy(&s));
            if let Some(val) = obj_get_value(&obj_map, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(obj_map), Value::Symbol(sym)) => {
            // Object property access with symbol key
            let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
            if let Some(val) = obj_get_value(&obj_map, &key)? {
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
                    if let Some(sym_rc) = map.get(&String::from_utf16_lossy(&s))
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
        _ => Err(raise_eval_error!("Invalid index type")), // other types of indexing not supported yet
    }
}

fn evaluate_property(env: &JSObjectDataPtr, obj: &Expr, prop: &str) -> Result<Value, JSError> {
    let obj_val = evaluate_expr(env, obj)?;
    log::trace!("Property access prop={prop}");
    match obj_val {
        Value::String(s) if prop == "length" => Ok(Value::Number(utf16_len(&s) as f64)),
        // Accessing other properties on string primitives should return undefined
        Value::String(_) => Ok(Value::Undefined),
        // Special cases for wrapped Map and Set objects
        Value::Object(obj_map) if prop == "size" && get_own_property(&obj_map, &"__map__".into()).is_some() => {
            if let Some(map_val) = get_own_property(&obj_map, &"__map__".into()) {
                if let Value::Map(map) = &*map_val.borrow() {
                    Ok(Value::Number(map.borrow().entries.len() as f64))
                } else {
                    Ok(Value::Undefined)
                }
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::Object(obj_map) if prop == "size" && get_own_property(&obj_map, &"__set__".into()).is_some() => {
            if let Some(set_val) = get_own_property(&obj_map, &"__set__".into()) {
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
        Value::Object(obj_map)
            if (prop == "next" || prop == "return" || prop == "throw") && get_own_property(&obj_map, &"__generator__".into()).is_some() =>
        {
            Ok(Value::Function(format!("Generator.prototype.{}", prop)))
        }
        // Special cases for DataView objects
        Value::Object(obj_map)
            if (prop == "buffer" || prop == "byteLength" || prop == "byteOffset")
                && get_own_property(&obj_map, &"__dataview".into()).is_some() =>
        {
            if let Some(dv_val) = get_own_property(&obj_map, &"__dataview".into()) {
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
        Value::Object(obj_map) => {
            // Special-case the `__proto__` accessor so property reads return the
            // object's current prototype object when present.
            if prop == "__proto__" {
                if let Some(proto) = obj_map.borrow().prototype.clone() {
                    return Ok(Value::Object(proto));
                } else {
                    return Ok(Value::Undefined);
                }
            }
            if let Some(val) = obj_get_value(&obj_map, &prop.into())? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::Number(n) => crate::js_number::box_number_and_get_property(n, prop, env),
        Value::Symbol(symbol_data) if prop == "description" => match symbol_data.description.as_ref() {
            Some(d) => Ok(Value::String(utf8_to_utf16(d))),
            None => Ok(Value::Undefined),
        },
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

            Err(raise_eval_error!(format!("Property not found for prop={prop}")))
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
        Value::Undefined => Ok(Value::Undefined),
        Value::Object(obj_map) => {
            if let Some(val) = obj_get_value(&obj_map, &prop.into())? {
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
    // If the base is undefined, optional chaining returns undefined
    if let Value::Undefined = obj_val {
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
        (Value::Object(obj_map), Value::Number(n)) => {
            let key = PropertyKey::String(n.to_string());
            if let Some(val) = obj_get_value(&obj_map, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(obj_map), Value::String(s)) => {
            let key = PropertyKey::String(String::from_utf16_lossy(&s));
            if let Some(val) = obj_get_value(&obj_map, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(obj_map), Value::Symbol(sym)) => {
            let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
            if let Some(val) = obj_get_value(&obj_map, &key)? {
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
                    if let Some(sym_rc) = map.get(&String::from_utf16_lossy(&s))
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
        // If obj isn't undefined and index types aren't supported, propagate as error
        _ => Err(raise_eval_error!("Invalid index type")),
    }
}

fn evaluate_call(env: &JSObjectDataPtr, func_expr: &Expr, args: &[Expr]) -> Result<Value, JSError> {
    log::trace!("evaluate_call entry: args_len={} func_expr=...", args.len());

    // Special case for dynamic import: import("module")
    if let Expr::Var(func_name) = func_expr
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
        let promise_obj = Rc::new(RefCell::new(JSObjectData::new()));
        obj_set_value(&promise_obj, &"__promise".into(), Value::Promise(promise))?;

        return Ok(Value::Object(promise_obj));
    }
    // Check if it's a method call first
    if let Expr::Property(obj_expr, method_name) = func_expr {
        // Special case for Array static methods
        if let Expr::Var(var_name) = &**obj_expr
            && var_name == "Array"
        {
            return crate::js_array::handle_array_static_method(method_name, args, env);
        }

        // Special case for Symbol static methods
        if let Expr::Var(var_name) = &**obj_expr
            && var_name == "Symbol"
        {
            return handle_symbol_static_method(method_name, args, env);
        }

        // Special case for Proxy static methods
        if let Expr::Var(var_name) = &**obj_expr
            && var_name == "Proxy"
            && method_name == "revocable"
        {
            return crate::js_proxy::handle_proxy_revocable(args, env);
        }

        let obj_val = evaluate_expr(env, obj_expr)?;
        log::trace!("evaluate_call - object evaluated");
        match (obj_val, method_name.as_str()) {
            (Value::Object(obj_map), "log") if get_own_property(&obj_map, &"log".into()).is_some() => {
                handle_console_method(method_name, args, env)
            }
            // Handle toString/valueOf for primitive Symbol values here (they
            // don't go through the object-path below). For other cases (objects)
            // normal property lookup is used so user overrides take precedence
            // and Object.prototype functions act as fallbacks.
            (Value::Symbol(sd), "toString") => crate::js_object::handle_to_string_method(&Value::Symbol(sd.clone()), args),
            (Value::Symbol(sd), "valueOf") => crate::js_object::handle_value_of_method(&Value::Symbol(sd.clone()), args),
            (Value::Object(obj_map), method) if get_own_property(&obj_map, &"__map__".into()).is_some() => {
                if let Some(map_val) = get_own_property(&obj_map, &"__map__".into()) {
                    if let Value::Map(map) = &*map_val.borrow() {
                        crate::js_map::handle_map_instance_method(map, method, args, env)
                    } else {
                        Err(raise_eval_error!("Invalid Map object"))
                    }
                } else {
                    Err(raise_eval_error!("Invalid Map object"))
                }
            }
            (Value::Object(obj_map), method) if get_own_property(&obj_map, &"__set__".into()).is_some() => {
                if let Some(set_val) = get_own_property(&obj_map, &"__set__".into()) {
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
            (Value::Object(obj_map), method) if get_own_property(&obj_map, &"__generator__".into()).is_some() => {
                if let Some(gen_val) = get_own_property(&obj_map, &"__generator__".into()) {
                    if let Value::Generator(generator) = &*gen_val.borrow() {
                        crate::js_generator::handle_generator_instance_method(generator, method, args, env)
                    } else {
                        Err(raise_eval_error!("Invalid Generator object"))
                    }
                } else {
                    Err(raise_eval_error!("Invalid Generator object"))
                }
            }
            (Value::Object(obj_map), method) => {
                // Object prototype methods are supplied on `Object.prototype`.
                // Lookups will find user-defined (own) methods before inherited
                // ones, so no evaluator fallback is required here.
                // If this object looks like the `std` module (we used 'sprintf' as marker)
                if get_own_property(&obj_map, &"sprintf".into()).is_some() {
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
                if get_own_property(&obj_map, &"open".into()).is_some() {
                    return crate::js_os::handle_os_method(&obj_map, method, args, env);
                }

                // If this object looks like the `os.path` module
                if get_own_property(&obj_map, &"join".into()).is_some() {
                    return crate::js_os::handle_os_method(&obj_map, method, args, env);
                }

                // If this object is a file-like object (we use '__file_id' as marker)
                if get_own_property(&obj_map, &"__file_id".into()).is_some() {
                    return handle_file_method(&obj_map, method, args, env);
                }
                // Check if this is the Math object
                if get_own_property(&obj_map, &"PI".into()).is_some() && get_own_property(&obj_map, &"E".into()).is_some() {
                    crate::js_math::handle_math_method(method, args, env)
                } else if get_own_property(&obj_map, &"apply".into()).is_some() && get_own_property(&obj_map, &"construct".into()).is_some()
                {
                    crate::js_reflect::handle_reflect_method(method, args, env)
                } else if get_own_property(&obj_map, &"parse".into()).is_some() && get_own_property(&obj_map, &"stringify".into()).is_some()
                {
                    crate::js_json::handle_json_method(method, args, env)
                } else if get_own_property(&obj_map, &"keys".into()).is_some() && get_own_property(&obj_map, &"values".into()).is_some() {
                    crate::js_object::handle_object_method(method, args, env)
                } else if get_own_property(&obj_map, &"MAX_VALUE".into()).is_some()
                    && get_own_property(&obj_map, &"MIN_VALUE".into()).is_some()
                {
                    crate::js_number::handle_number_method(method, args, env)
                } else if get_own_property(&obj_map, &"__is_bigint_constructor".into()).is_some() {
                    crate::js_bigint::handle_bigint_static_method(method, args, env)
                } else if get_own_property(&obj_map, &"__value__".into()).is_some() {
                    // Dispatch boxed primitive object methods based on the actual __value__ type
                    if let Some(val_rc) = obj_get_value(&obj_map, &"__value__".into())? {
                        match &*val_rc.borrow() {
                            Value::Number(_) => crate::js_number::handle_number_object_method(&obj_map, method, args, env),
                            Value::BigInt(_) => crate::js_bigint::handle_bigint_object_method(&obj_map, method, args, env),
                            _ => Err(raise_eval_error!("Invalid __value__ for boxed object")),
                        }
                    } else {
                        Err(raise_eval_error!("__value__ not found on instance"))
                    }
                } else if get_own_property(&obj_map, &"__timestamp".into()).is_some() {
                    // Date instance methods
                    crate::js_date::handle_date_method(&obj_map, method, args)
                } else if get_own_property(&obj_map, &"__regex".into()).is_some() {
                    // RegExp instance methods
                    crate::js_regexp::handle_regexp_method(&obj_map, method, args, env)
                } else if is_array(&obj_map) {
                    // Array instance methods
                    crate::js_array::handle_array_instance_method(&obj_map, method, args, env, obj_expr)
                } else if get_own_property(&obj_map, &"__promise".into()).is_some() {
                    // Promise instance methods
                    handle_promise_method(&obj_map, method, args, env)
                } else if get_own_property(&obj_map, &"__dataview".into()).is_some() {
                    // DataView instance methods
                    crate::js_typedarray::handle_dataview_method(&obj_map, method, args, env)
                } else if get_own_property(&obj_map, &"__class_def__".into()).is_some() {
                    // Class static methods
                    call_static_method(&obj_map, method, args, env)
                } else if get_own_property(&obj_map, &"sameValue".into()).is_some() {
                    crate::js_assert::handle_assert_method(method, args, env)
                } else if get_own_property(&obj_map, &"testWithIntlConstructors".into()).is_some() {
                    crate::js_testintl::handle_testintl_method(method, args, env)
                } else if get_own_property(&obj_map, &"__locale".into()).is_some() && method == "resolvedOptions" {
                    // Handle resolvedOptions method on mock Intl instances
                    crate::js_testintl::handle_resolved_options(&obj_map)
                } else if is_array(&obj_map) {
                    // Class static methods
                    call_static_method(&obj_map, method, args, env)
                } else if is_class_instance(&obj_map)? {
                    call_class_method(&obj_map, method, args, env)
                } else {
                    // Check for user-defined method
                    if let Some(prop_val) = obj_get_value(&obj_map, &method.into())? {
                        match prop_val.borrow().clone() {
                            Value::Closure(params, body, captured_env) | Value::AsyncClosure(params, body, captured_env) => {
                                // Function call
                                // Collect all arguments, expanding spreads
                                let mut evaluated_args = Vec::new();
                                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                                // Create new environment starting with captured environment
                                // Use a fresh environment frame whose prototype points to the captured environment
                                let func_env = Rc::new(RefCell::new(JSObjectData::new()));
                                func_env.borrow_mut().prototype = Some(captured_env.clone());
                                // Bind parameters: assign provided args, set missing params to undefined
                                for (i, param) in params.iter().enumerate() {
                                    if i < evaluated_args.len() {
                                        env_set(&func_env, param.as_str(), evaluated_args[i].clone())?;
                                    } else {
                                        env_set(&func_env, param.as_str(), Value::Undefined)?;
                                    }
                                }
                                // Execute function body
                                evaluate_statements(&func_env, &body)
                            }
                            Value::Function(func_name) => {
                                // Special-case Object.prototype.* built-ins so they can
                                // operate on the receiver (`this`), which is the
                                // object we fetched the method from (obj_map).
                                // Also handle boxed-primitive built-ins that are
                                // represented as `Value::Function("BigInt_toString")`,
                                // etc., so they can access the receiver's `__value__`.
                                if func_name == "BigInt_toString" {
                                    return crate::js_bigint::handle_bigint_object_method(&obj_map, "toString", args, env);
                                }
                                if func_name == "BigInt_valueOf" {
                                    return crate::js_bigint::handle_bigint_object_method(&obj_map, "valueOf", args, env);
                                }
                                if func_name.starts_with("Object.prototype.") {
                                    match func_name.as_str() {
                                        "Object.prototype.hasOwnProperty" => {
                                            // hasOwnProperty takes one argument; evaluate it in caller env
                                            if args.len() != 1 {
                                                return Err(raise_eval_error!("hasOwnProperty requires one argument"));
                                            }
                                            let key_val = evaluate_expr(env, &args[0])?;
                                            let exists = match key_val {
                                                Value::String(s) => {
                                                    get_own_property(&obj_map, &String::from_utf16_lossy(&s).into()).is_some()
                                                }
                                                Value::Number(n) => get_own_property(&obj_map, &n.to_string().into()).is_some(),
                                                Value::Boolean(b) => get_own_property(&obj_map, &b.to_string().into()).is_some(),
                                                Value::Undefined => get_own_property(&obj_map, &"undefined".into()).is_some(),
                                                Value::Symbol(sd) => {
                                                    let sym_key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sd))));
                                                    get_own_property(&obj_map, &sym_key).is_some()
                                                }
                                                other => get_own_property(&obj_map, &value_to_string(&other).into()).is_some(),
                                            };
                                            Ok(Value::Boolean(exists))
                                        }
                                        "Object.prototype.isPrototypeOf" => {
                                            if args.len() != 1 {
                                                return Err(raise_eval_error!("isPrototypeOf requires one argument"));
                                            }
                                            let target_val = evaluate_expr(env, &args[0])?;
                                            match target_val {
                                                Value::Object(target_map) => {
                                                    let mut current_opt = target_map.borrow().prototype.clone();
                                                    let mut found = false;
                                                    while let Some(parent) = current_opt {
                                                        if Rc::ptr_eq(&parent, &obj_map) {
                                                            found = true;
                                                            break;
                                                        }
                                                        current_opt = parent.borrow().prototype.clone();
                                                    }
                                                    Ok(Value::Boolean(found))
                                                }
                                                _ => Ok(Value::Boolean(false)),
                                            }
                                        }
                                        "Object.prototype.toLocaleString" => {
                                            // Delegate Object.prototype.toLocaleString to the
                                            // same handler as toString (defaults to toString)
                                            crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), args)
                                        }
                                        "Object.prototype.propertyIsEnumerable" => {
                                            if args.len() != 1 {
                                                return Err(raise_eval_error!("propertyIsEnumerable requires one argument"));
                                            }
                                            let key_val = evaluate_expr(env, &args[0])?;
                                            let exists = match key_val {
                                                Value::String(s) => {
                                                    get_own_property(&obj_map, &String::from_utf16_lossy(&s).into()).is_some()
                                                }
                                                Value::Number(n) => get_own_property(&obj_map, &n.to_string().into()).is_some(),
                                                Value::Boolean(b) => get_own_property(&obj_map, &b.to_string().into()).is_some(),
                                                Value::Undefined => get_own_property(&obj_map, &"undefined".into()).is_some(),
                                                Value::Symbol(sd) => {
                                                    let sym_key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sd))));
                                                    get_own_property(&obj_map, &sym_key).is_some()
                                                }
                                                other => get_own_property(&obj_map, &value_to_string(&other).into()).is_some(),
                                            };
                                            Ok(Value::Boolean(exists))
                                        }
                                        "Object.prototype.toString" => {
                                            // Delegate the built-in toString behavior to js_object::handle_to_string_method
                                            // which handles wrapped primitives, arrays, and Symbol.toStringTag
                                            // The function is invoked with `this` bound to obj_map (receiver)
                                            crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), args)
                                        }
                                        "Object.prototype.valueOf" => {
                                            // Delegate to handle_value_of_method
                                            crate::js_object::handle_value_of_method(&Value::Object(obj_map.clone()), args)
                                        }
                                        _ => crate::js_function::handle_global_function(&func_name, args, env),
                                    }
                                } else {
                                    crate::js_function::handle_global_function(&func_name, args, env)
                                }
                            }
                            _ => Err(raise_eval_error!(format!("Property '{method}' is not a function"))),
                        }
                    } else {
                        Err(raise_eval_error!(format!("Method {method} not found on object")))
                    }
                }
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
            Value::Undefined => Ok(Value::Undefined),
            Value::Object(obj_map) => handle_optional_method_call(&obj_map, method_name, args, env, obj_expr),
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
            Value::GeneratorFunction(params, body, captured_env) => {
                // Generator function call - return a generator object
                crate::js_generator::handle_generator_function_call(&params, &body, args, &captured_env)
            }
            Value::Closure(params, body, captured_env) => {
                // Function call
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                // Create new environment starting with captured environment (fresh frame)
                let func_env = Rc::new(RefCell::new(JSObjectData::new()));
                func_env.borrow_mut().prototype = Some(captured_env.clone());
                // ensure this env is a proper function scope
                func_env.borrow_mut().is_function_scope = true;
                // Bind parameters: provide provided args, set missing params to undefined
                for (i, param) in params.iter().enumerate() {
                    if i < evaluated_args.len() {
                        env_set(&func_env, param.as_str(), evaluated_args[i].clone())?;
                    } else {
                        env_set(&func_env, param.as_str(), Value::Undefined)?;
                    }
                }
                // Execute function body
                evaluate_statements(&func_env, &body)
            }
            Value::AsyncClosure(params, body, captured_env) => {
                // Function call
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                // Create a Promise object
                let promise = Rc::new(RefCell::new(JSPromise::default()));
                let promise_obj = Value::Object(Rc::new(RefCell::new(JSObjectData::new())));
                if let Value::Object(obj) = &promise_obj {
                    obj.borrow_mut()
                        .insert("__promise".into(), Rc::new(RefCell::new(Value::Promise(promise.clone()))));
                }
                // Create new environment
                let func_env = Rc::new(RefCell::new(JSObjectData::new()));
                func_env.borrow_mut().prototype = Some(captured_env.clone());
                func_env.borrow_mut().is_function_scope = true;
                // Bind parameters
                for (i, param) in params.iter().enumerate() {
                    let val = if i < evaluated_args.len() {
                        evaluated_args[i].clone()
                    } else {
                        Value::Undefined
                    };
                    env_set(&func_env, param.as_str(), val)?;
                }
                // Execute function body synchronously (for now)
                let result = evaluate_statements(&func_env, &body);
                match result {
                    Ok(val) => {
                        promise.borrow_mut().state = PromiseState::Fulfilled(val);
                    }
                    Err(e) => {
                        promise.borrow_mut().state = PromiseState::Rejected(Value::String(utf8_to_utf16(&format!("{}", e))));
                    }
                }
                Ok(promise_obj)
            }
            Value::Object(obj_map) => {
                // Support calling the `assert` testing object as a function as well
                // Many tests use `assert(condition, message)` in addition to
                // `assert.sameValue(...)`. If this object appears to be the
                // assert object (it exposes `sameValue`), treat a direct call
                // as an assertion: evaluate the first argument for truthiness
                // and throw an EvaluationError with the given message when
                // the assertion fails.
                if get_own_property(&obj_map, &"sameValue".into()).is_some() {
                    if args.is_empty() {
                        return Err(raise_eval_error!("assert requires at least one argument"));
                    }
                    // Evaluate the condition
                    let cond_val = evaluate_expr(env, &args[0])?;
                    if is_truthy(&cond_val) {
                        return Ok(Value::Undefined);
                    }

                    // Build a message from the optional second argument
                    let message = if args.len() > 1 {
                        let msg_val = evaluate_expr(env, &args[1])?;
                        match msg_val {
                            Value::String(s) => String::from_utf16_lossy(&s),
                            other => value_to_string(&other),
                        }
                    } else {
                        "Assertion failed".to_string()
                    };

                    // Extra diagnostic: when the assertion is of the form
                    // isCanonicalizedStructurallyValidLanguageTag(x) and it
                    // failed, log canonicalize/isStructurallyValid results for x.
                    if let Some(first_arg_expr) = args.first() {
                        use crate::core::Expr;
                        if let Expr::Call(func_expr, call_args) = first_arg_expr
                            && let Expr::Var(fname) = &**func_expr
                            && fname == "isCanonicalizedStructurallyValidLanguageTag"
                            && call_args.len() == 1
                        {
                            // Try to evaluate the inner argument to a string
                            if let Ok(val) = evaluate_expr(env, &call_args[0])
                                && let Value::String(s_utf16) = val
                            {
                                let s = String::from_utf16_lossy(&s_utf16);
                                // Evaluate canonicalizeLanguageTag(s)
                                let canon_call = Expr::Call(
                                    Box::new(Expr::Var("canonicalizeLanguageTag".to_string())),
                                    vec![Expr::StringLit(crate::unicode::utf8_to_utf16(&s))],
                                );
                                match evaluate_expr(env, &canon_call) {
                                    Ok(Value::String(canon_utf16)) => {
                                        let canon = String::from_utf16_lossy(&canon_utf16);
                                        log::error!("Assertion diagnostic: input='{}' canonicalizeLanguageTag='{}'", s, canon);
                                        // Raw UTF-16 buffer dump for deeper diagnostics
                                        log::error!(
                                            "Assertion diagnostic RAW UTF-16: input_vec={:?} canonical_vec={:?}",
                                            s_utf16,
                                            canon_utf16
                                        );
                                        // Also print hex codepoints for easier visual diff
                                        let input_hex: Vec<String> = s_utf16.iter().map(|u| format!("0x{:04x}", u)).collect();
                                        let canon_hex: Vec<String> = canon_utf16.iter().map(|u| format!("0x{:04x}", u)).collect();
                                        log::error!(
                                            "Assertion diagnostic RAW HEX: input_hex={} canonical_hex={}",
                                            input_hex.join(","),
                                            canon_hex.join(",")
                                        );
                                    }
                                    Ok(other) => {
                                        log::error!("Assertion diagnostic: canonicalizeLanguageTag returned non-string: {:?}", other);
                                    }
                                    Err(e) => {
                                        log::error!("Assertion diagnostic: canonicalizeLanguageTag error: {:?}", e);
                                    }
                                }

                                // Evaluate isStructurallyValidLanguageTag(s)
                                let struct_call = Expr::Call(
                                    Box::new(Expr::Var("isStructurallyValidLanguageTag".to_string())),
                                    vec![Expr::StringLit(crate::unicode::utf8_to_utf16(&s))],
                                );
                                match evaluate_expr(env, &struct_call) {
                                    Ok(Value::Boolean(b)) => {
                                        log::error!("Assertion diagnostic: isStructurallyValidLanguageTag('{}') = {}", s, b);
                                    }
                                    Ok(other) => {
                                        log::error!(
                                            "Assertion diagnostic: isStructurallyValidLanguageTag returned non-boolean: {:?}",
                                            other
                                        );
                                    }
                                    Err(e) => {
                                        log::error!("Assertion diagnostic: isStructurallyValidLanguageTag error: {:?}", e);
                                    }
                                }
                            }
                        }
                    }

                    return Err(raise_eval_error!(format!("{message}")));
                }
                // If this object is the global `Object` constructor (stored in the
                // root environment as an object), route the call to the
                // Object constructor handler. This ensures that calling the
                // constructor object (e.g. `Object(123n)`) behaves like a
                // constructor instead of attempting to call the object as a
                // plain callable value.
                let mut root_env_opt = Some(env.clone());
                while let Some(r) = root_env_opt.clone() {
                    if r.borrow().prototype.is_some() {
                        root_env_opt = r.borrow().prototype.clone();
                    } else {
                        break;
                    }
                }
                if let Some(root_env) = root_env_opt
                    && let Some(obj_ctor_rc) = obj_get_value(&root_env, &"Object".into())?
                    && let Value::Object(ctor_map) = &*obj_ctor_rc.borrow()
                    && Rc::ptr_eq(ctor_map, &obj_map)
                {
                    return crate::js_class::handle_object_constructor(args, env);
                }

                // Check if this is a built-in constructor object (Number)
                if get_own_property(&obj_map, &"MAX_VALUE".into()).is_some() && get_own_property(&obj_map, &"MIN_VALUE".into()).is_some() {
                    // Number constructor call
                    crate::js_function::handle_global_function("Number", args, env)
                } else if get_own_property(&obj_map, &"__is_string_constructor".into()).is_some() {
                    crate::js_function::handle_global_function("String", args, env)
                } else if get_own_property(&obj_map, &"__is_boolean_constructor".into()).is_some() {
                    crate::js_function::handle_global_function("Boolean", args, env)
                } else if get_own_property(&obj_map, &"__is_bigint_constructor".into()).is_some() {
                    // BigInt constructor-like object: handle conversion via global function
                    crate::js_function::handle_global_function("BigInt", args, env)
                } else {
                    // Log diagnostic context before returning a generic evaluation error
                    log::error!("evaluate_call - unexpected object method dispatch: obj_map={:?}", obj_map);
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
        if let Expr::Var(var_name) = &**obj_expr
            && var_name == "Array"
        {
            return crate::js_array::handle_array_static_method(method_name, args, env);
        }

        let obj_val = evaluate_expr(env, obj_expr)?;
        log::trace!("evaluate_optional_call - object eval result: {obj_val:?}");
        match obj_val {
            Value::Undefined => Ok(Value::Undefined),
            Value::Object(obj_map) => {
                // If this object looks like the `std` module (we used 'sprintf' as marker)
                if get_own_property(&obj_map, &"sprintf".into()).is_some() {
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
                if get_own_property(&obj_map, &"open".into()).is_some() {
                    return crate::js_os::handle_os_method(&obj_map, method_name, args, env);
                }

                // If this object looks like the `os.path` module
                if get_own_property(&obj_map, &"join".into()).is_some() {
                    return crate::js_os::handle_os_method(&obj_map, method_name, args, env);
                }

                // If this object is a file-like object (we use '__file_id' as marker)
                if get_own_property(&obj_map, &"__file_id".into()).is_some() {
                    return handle_file_method(&obj_map, method_name, args, env);
                }
                // Check if this is the Math object
                if get_own_property(&obj_map, &"PI".into()).is_some() && get_own_property(&obj_map, &"E".into()).is_some() {
                    crate::js_math::handle_math_method(method_name, args, env)
                } else if get_own_property(&obj_map, &"apply".into()).is_some() && get_own_property(&obj_map, &"construct".into()).is_some()
                {
                    crate::js_reflect::handle_reflect_method(method_name, args, env)
                } else if get_own_property(&obj_map, &"parse".into()).is_some() && get_own_property(&obj_map, &"stringify".into()).is_some()
                {
                    crate::js_json::handle_json_method(method_name, args, env)
                } else if get_own_property(&obj_map, &"keys".into()).is_some() && get_own_property(&obj_map, &"values".into()).is_some() {
                    crate::js_object::handle_object_method(method_name, args, env)
                } else if get_own_property(&obj_map, &"MAX_VALUE".into()).is_some()
                    && get_own_property(&obj_map, &"MIN_VALUE".into()).is_some()
                {
                    crate::js_number::handle_number_method(method_name, args, env)
                } else if get_own_property(&obj_map, &"__is_bigint_constructor".into()).is_some() {
                    crate::js_bigint::handle_bigint_static_method(method_name, args, env)
                } else if get_own_property(&obj_map, &"__value__".into()).is_some() {
                    if let Some(val_rc) = obj_get_value(&obj_map, &"__value__".into())? {
                        match &*val_rc.borrow() {
                            Value::Number(_) => crate::js_number::handle_number_object_method(&obj_map, method_name, args, env),
                            Value::BigInt(_) => crate::js_bigint::handle_bigint_object_method(&obj_map, method_name, args, env),
                            _ => Err(raise_eval_error!("Invalid __value__ for boxed object")),
                        }
                    } else {
                        Err(raise_eval_error!("__value__ not found on instance"))
                    }
                } else if get_own_property(&obj_map, &"__timestamp".into()).is_some() {
                    // Date instance methods
                    crate::js_date::handle_date_method(&obj_map, method_name, args)
                } else if get_own_property(&obj_map, &"__regex".into()).is_some() {
                    // RegExp instance methods
                    crate::js_regexp::handle_regexp_method(&obj_map, method_name, args, env)
                } else if is_array(&obj_map) {
                    // Array instance methods
                    crate::js_array::handle_array_instance_method(&obj_map, method_name, args, env, obj_expr)
                } else if get_own_property(&obj_map, &"__promise".into()).is_some() {
                    // Promise instance methods
                    handle_promise_method(&obj_map, method_name, args, env)
                } else if get_own_property(&obj_map, &"__dataview".into()).is_some() {
                    // Class static methods
                    call_static_method(&obj_map, method_name, args, env)
                } else if is_class_instance(&obj_map)? {
                    call_class_method(&obj_map, method_name, args, env)
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
            Value::Closure(params, body, captured_env) | Value::AsyncClosure(params, body, captured_env) => {
                // Function call
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                // Create new environment starting with captured environment (fresh frame)
                let func_env = Rc::new(RefCell::new(JSObjectData::new()));
                func_env.borrow_mut().prototype = Some(captured_env.clone());
                // Bind parameters: provide provided args, set missing params to undefined
                for (i, param) in params.iter().enumerate() {
                    if i < evaluated_args.len() {
                        env_set(&func_env, param.as_str(), evaluated_args[i].clone())?;
                    } else {
                        env_set(&func_env, param.as_str(), Value::Undefined)?;
                    }
                }
                // Execute function body
                evaluate_statements(&func_env, &body)
            }
            _ => Err(raise_eval_error!("error")),
        }
    }
}

fn evaluate_object(env: &JSObjectDataPtr, properties: &Vec<(String, Expr)>) -> Result<Value, JSError> {
    let obj = Rc::new(RefCell::new(JSObjectData::new()));
    // Attempt to set the default prototype for object literals to Object.prototype
    // by finding the global 'Object' constructor and using its 'prototype' property.
    // Walk to the top-level environment
    let mut root_env_opt = Some(env.clone());
    while let Some(r) = root_env_opt.clone() {
        if r.borrow().prototype.is_some() {
            root_env_opt = r.borrow().prototype.clone();
        } else {
            break;
        }
    }
    if let Some(root_env) = root_env_opt {
        // Use centralized helper to set default prototype from global Object constructor
        crate::core::set_internal_prototype_from_constructor(&obj, &root_env, "Object")?;
    }

    for (key, value_expr) in properties {
        // helper: convert parser-produced computed keys like "[Symbol.toPrimitive]"
        fn key_to_property_key(key: &str) -> PropertyKey {
            if key.starts_with('[') && key.ends_with(']') {
                let inner = &key[1..key.len() - 1];
                if let Some(sym_name) = inner.strip_prefix("Symbol.")
                    && let Some(sym_rc) = get_well_known_symbol_rc(sym_name)
                {
                    return PropertyKey::Symbol(sym_rc.clone());
                }
            }
            PropertyKey::String(key.to_string())
        }
        if key.is_empty() && matches!(value_expr, Expr::Spread(_)) {
            // Spread operator: evaluate the expression and spread its properties
            if let Expr::Spread(expr) = value_expr {
                let spread_val = evaluate_expr(env, expr)?;
                if let Value::Object(spread_obj) = spread_val {
                    // Copy all properties from spread_obj to obj
                    for (prop_key, prop_val) in spread_obj.borrow().properties.iter() {
                        obj.borrow_mut().insert(prop_key.clone(), prop_val.clone());
                    }
                } else {
                    return Err(raise_eval_error!("Spread operator can only be applied to objects"));
                }
            }
        } else {
            match value_expr {
                Expr::Getter(func_expr) => {
                    if let Expr::Function(_params, body) = func_expr.as_ref() {
                        // Check if property already exists
                        let pk = key_to_property_key(key);
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
                                getter.replace((body.clone(), env.clone()));
                                obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(val)));
                            } else {
                                // Create new property descriptor
                                let prop = Value::Property {
                                    value: Some(existing.clone()),
                                    getter: Some((body.clone(), env.clone())),
                                    setter: None,
                                };
                                obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(prop)));
                            }
                        } else {
                            // Create new property descriptor with getter
                            let prop = Value::Property {
                                value: None,
                                getter: Some((body.clone(), env.clone())),
                                setter: None,
                            };
                            obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(prop)));
                        }
                    } else {
                        return Err(raise_eval_error!("Getter must be a function"));
                    }
                }
                Expr::Setter(func_expr) => {
                    if let Expr::Function(params, body) = func_expr.as_ref() {
                        // Check if property already exists
                        let pk = key_to_property_key(key);
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
                                setter.replace((params.clone(), body.clone(), env.clone()));
                                obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(val)));
                            } else {
                                // Create new property descriptor
                                let prop = Value::Property {
                                    value: Some(existing.clone()),
                                    getter: None,
                                    setter: Some((params.clone(), body.clone(), env.clone())),
                                };
                                obj.borrow_mut().insert(pk.clone(), Rc::new(RefCell::new(prop)));
                            }
                        } else {
                            // Create new property descriptor with setter
                            let prop = Value::Property {
                                value: None,
                                getter: None,
                                setter: Some((params.clone(), body.clone(), env.clone())),
                            };
                            obj.borrow_mut()
                                .insert(PropertyKey::String(key.to_string()), Rc::new(RefCell::new(prop)));
                        }
                    } else {
                        return Err(raise_eval_error!("Setter must be a function"));
                    }
                }
                _ => {
                    let value = evaluate_expr(env, value_expr)?;
                    // Check if property already exists
                    let pk = key_to_property_key(key);
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
                        let pk = key_to_property_key(key);
                        obj_set_value(&obj, &pk, value)?;
                    }
                }
            }
        }
    }
    Ok(Value::Object(obj))
}

fn evaluate_array(env: &JSObjectDataPtr, elements: &Vec<Expr>) -> Result<Value, JSError> {
    let arr = Rc::new(RefCell::new(JSObjectData::new()));
    // Give arrays a default prototype (Object.prototype) until Array.prototype exists
    let mut root_env_opt = Some(env.clone());
    while let Some(r) = root_env_opt.clone() {
        if r.borrow().prototype.is_some() {
            root_env_opt = r.borrow().prototype.clone();
        } else {
            break;
        }
    }
    if let Some(root_env) = root_env_opt {
        crate::core::set_internal_prototype_from_constructor(&arr, &root_env, "Object")?;
    }
    let mut index = 0;
    for elem_expr in elements {
        if let Expr::Spread(spread_expr) = elem_expr {
            // Spread operator: evaluate the expression and spread its elements
            let spread_val = evaluate_expr(env, spread_expr)?;
            if let Value::Object(spread_obj) = spread_val {
                // Assume it's an array-like object
                let mut i = 0;
                loop {
                    let key = i.to_string();
                    if let Some(val) = obj_get_value(&spread_obj, &key.into())? {
                        obj_set_value(&arr, &index.to_string().into(), val.borrow().clone())?;
                        index += 1;
                        i += 1;
                    } else {
                        break;
                    }
                }
            } else {
                return Err(raise_eval_error!("Spread operator can only be applied to arrays"));
            }
        } else {
            let value = evaluate_expr(env, elem_expr)?;
            obj_set_value(&arr, &index.to_string().into(), value)?;
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
        match stmt {
            Statement::Var(name, _) => {
                names.insert(name.clone());
            }
            Statement::If(_, then_body, else_body) => {
                collect_var_names(then_body, names);
                if let Some(else_stmts) = else_body {
                    collect_var_names(else_stmts, names);
                }
            }
            Statement::For(_, _, _, body) => {
                collect_var_names(body, names);
            }
            Statement::ForOf(_, _, body) => {
                collect_var_names(body, names);
            }
            Statement::ForOfDestructuringObject(pattern, _, body) => {
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
            Statement::ForOfDestructuringArray(pattern, _, body) => {
                collect_names_from_array_pattern(pattern, names);
                collect_var_names(body, names);
            }
            Statement::While(_, body) => {
                collect_var_names(body, names);
            }
            Statement::DoWhile(body, _) => {
                collect_var_names(body, names);
            }
            Statement::Switch(_, cases) => {
                for case in cases {
                    match case {
                        SwitchCase::Case(_, stmts) => collect_var_names(stmts, names),
                        SwitchCase::Default(stmts) => collect_var_names(stmts, names),
                    }
                }
            }
            Statement::TryCatch(try_body, _, catch_body, finally_body) => {
                collect_var_names(try_body, names);
                collect_var_names(catch_body, names);
                if let Some(finally_stmts) = finally_body {
                    collect_var_names(finally_stmts, names);
                }
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
fn handle_optional_method_call(
    obj_map: &JSObjectDataPtr,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
    obj_expr: &Expr,
) -> Result<Value, JSError> {
    match method {
        "log" if get_own_property(obj_map, &"log".into()).is_some() => handle_console_method(method, args, env),
        "toString" => crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), args),
        "valueOf" => crate::js_object::handle_value_of_method(&Value::Object(obj_map.clone()), args),
        method => {
            // If this object looks like the `std` module (we used 'sprintf' as marker)
            if get_own_property(obj_map, &"sprintf".into()).is_some() {
                match method {
                    "sprintf" => {
                        log::trace!("js dispatch calling sprintf with {} args", args.len());
                        handle_sprintf_call(env, args)
                    }
                    "tmpfile" => create_tmpfile(),
                    _ => Ok(Value::Undefined),
                }
            } else if get_own_property(obj_map, &"open".into()).is_some() {
                // If this object looks like the `os` module (we used 'open' as marker)
                crate::js_os::handle_os_method(obj_map, method, args, env)
            } else if get_own_property(obj_map, &"join".into()).is_some() {
                // If this object looks like the `os.path` module
                crate::js_os::handle_os_method(obj_map, method, args, env)
            } else if get_own_property(obj_map, &"__file_id".into()).is_some() {
                // If this object is a file-like object (we use '__file_id' as marker)
                handle_file_method(obj_map, method, args, env)
            } else if get_own_property(obj_map, &"PI".into()).is_some() && get_own_property(obj_map, &"E".into()).is_some() {
                // Check if this is the Math object
                handle_math_method(method, args, env)
            } else if get_own_property(obj_map, &"apply".into()).is_some() && get_own_property(obj_map, &"construct".into()).is_some() {
                // Check if this is the Reflect object
                crate::js_reflect::handle_reflect_method(method, args, env)
            } else if get_own_property(obj_map, &"parse".into()).is_some() && get_own_property(obj_map, &"stringify".into()).is_some() {
                crate::js_json::handle_json_method(method, args, env)
            } else if get_own_property(obj_map, &"keys".into()).is_some() && get_own_property(obj_map, &"values".into()).is_some() {
                crate::js_object::handle_object_method(method, args, env)
            } else if get_own_property(obj_map, &"__timestamp".into()).is_some() {
                // Date instance methods
                crate::js_date::handle_date_method(obj_map, method, args)
            } else if get_own_property(obj_map, &"__regex".into()).is_some() {
                // RegExp instance methods
                crate::js_regexp::handle_regexp_method(obj_map, method, args, env)
            } else if is_array(obj_map) {
                // Array instance methods
                crate::js_array::handle_array_instance_method(obj_map, method, args, env, obj_expr)
            } else if get_own_property(obj_map, &"__class_def__".into()).is_some() {
                // Class static methods
                call_static_method(obj_map, method, args, env)
            } else if is_class_instance(obj_map)? {
                call_class_method(obj_map, method, args, env)
            } else {
                // Check for user-defined method
                if let Some(prop_val) = obj_get_value(obj_map, &method.into())? {
                    match prop_val.borrow().clone() {
                        Value::Closure(params, body, captured_env) | Value::AsyncClosure(params, body, captured_env) => {
                            // Function call
                            // Collect all arguments, expanding spreads
                            let mut evaluated_args = Vec::new();
                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                            // Create new environment starting with captured environment (fresh frame)
                            let func_env = Rc::new(RefCell::new(JSObjectData::new()));
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            // Bind parameters: provide provided args, set missing params to undefined
                            for (i, param) in params.iter().enumerate() {
                                if i < evaluated_args.len() {
                                    env_set(&func_env, param.as_str(), evaluated_args[i].clone())?;
                                } else {
                                    env_set(&func_env, param.as_str(), Value::Undefined)?;
                                }
                            }
                            // Execute function body
                            evaluate_statements(&func_env, &body)
                        }
                        Value::Function(func_name) => crate::js_function::handle_global_function(&func_name, args, env),
                        _ => Err(raise_eval_error!(format!("Property '{method}' is not a function"))),
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
fn expand_spread_in_call_args(env: &JSObjectDataPtr, args: &[Expr], evaluated_args: &mut Vec<Value>) -> Result<(), JSError> {
    for arg_expr in args {
        if let Expr::Spread(spread_expr) = arg_expr {
            let spread_val = evaluate_expr(env, spread_expr)?;
            if let Value::Object(spread_obj) = spread_val {
                // Assume it's an array-like object
                let mut i = 0;
                loop {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(val) = obj_get_value(&spread_obj, &key)? {
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
        Value::Object(map) => obj_get_value(&map, &prop.into()),
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
    if let Expr::Var(varname) = obj_expr
        && let Some(rc_val) = env_get(env, varname)
    {
        let mut borrowed = rc_val.borrow_mut();
        if let Value::Object(ref mut map) = *borrowed {
            // Special-case `__proto__` assignment: set the prototype
            if prop == "__proto__" {
                if let Value::Object(proto_map) = val {
                    map.borrow_mut().prototype = Some(proto_map);
                    return Ok(None);
                } else {
                    // Non-object assigned to __proto__: ignore or set to None
                    map.borrow_mut().prototype = None;
                    return Ok(None);
                }
            }

            obj_set_value(map, &prop.into(), val)?;
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
                    obj.borrow_mut().prototype = Some(proto_map);
                    return Ok(Some(Value::Object(obj)));
                } else {
                    obj.borrow_mut().prototype = None;
                    return Ok(Some(Value::Object(obj)));
                }
            }

            obj_set_value(&obj, &prop.into(), val)?;
            Ok(Some(Value::Object(obj)))
        }
        _ => Err(raise_eval_error!("not an object")),
    }
}

#[allow(dead_code)]
pub fn initialize_global_constructors(env: &JSObjectDataPtr) -> Result<(), JSError> {
    // Initialize ArrayBuffer constructor
    let arraybuffer_constructor = crate::js_typedarray::make_arraybuffer_constructor()?;
    obj_set_value(env, &"ArrayBuffer".into(), Value::Object(arraybuffer_constructor))?;

    // Initialize DataView constructor
    let dataview_constructor = crate::js_typedarray::make_dataview_constructor()?;
    obj_set_value(env, &"DataView".into(), Value::Object(dataview_constructor))?;

    // Initialize TypedArray constructors
    let typedarray_constructors = crate::js_typedarray::make_typedarray_constructors()?;
    for (name, constructor) in typedarray_constructors {
        obj_set_value(env, &name.into(), Value::Object(constructor))?;
    }

    Ok(())
}
