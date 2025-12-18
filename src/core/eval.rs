use crate::{
    JSError, JSErrorKind, PropertyKey, Value,
    core::{
        BinaryOp, DestructuringElement, Expr, JSObjectDataPtr, ObjectDestructuringElement, Statement, StatementKind, SwitchCase,
        SymbolData, TypedArrayKind, WELL_KNOWN_SYMBOLS, env_get, env_set, env_set_const, env_set_recursive, env_set_var,
        extract_closure_from_value, get_own_property, is_truthy, new_js_object_data, obj_delete, obj_set_key_value, parse_bigint_string,
        to_primitive, value_to_string, values_equal,
    },
    js_array::{get_array_length, is_array, set_array_length},
    js_assert::make_assert_object,
    js_class::{
        call_class_method, call_static_method, create_class_object, evaluate_new, evaluate_super, evaluate_super_call,
        evaluate_super_method, evaluate_super_property, evaluate_this, is_class_instance, is_instance_of,
    },
    js_console::{handle_console_method, make_console_object},
    js_date::is_date_object,
    js_math::{handle_math_method, make_math_object},
    js_number::make_number_object,
    js_promise::{JSPromise, PromiseState, handle_promise_method, run_event_loop},
    js_reflect::make_reflect_object,
    js_regexp::is_regex_object,
    js_testintl::make_testintl_object,
    obj_get_key_value, raise_eval_error, raise_syntax_error, raise_throw_error, raise_type_error, raise_variable_not_found_error,
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
                script_name = String::from_utf16_lossy(s_utf16);
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
        // follow prototype/caller chain to find root script name if needed
        env_opt = env_ptr.borrow().prototype.clone();
    }
    if let Some(ln) = line {
        let col = column.unwrap_or(0);
        format!("{} ({}:{}:{})", base, script_name, ln, col)
    } else {
        format!("{} ({})", base, script_name)
    }
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
            StatementKind::Let(name, _) | StatementKind::Const(name, _) | StatementKind::Class(name, _, _) => {
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
        StatementKind::Let(n, _) | StatementKind::Const(n, _) | StatementKind::Class(n, _, _) => n == name,
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
            StatementKind::Var(n, _) if n == name => return Some((stmt.line, stmt.column)),
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
    if let Value::Object(obj_map) = val {
        if let Some(_cl) = obj_get_key_value(obj_map, &"__closure__".into())? {
            let existing = obj_get_key_value(obj_map, &"name".into())?;
            if existing.is_none() {
                let name_val = Value::String(utf8_to_utf16(name));
                obj_set_key_value(obj_map, &"name".into(), name_val)?;
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
        }
    }

    // Hoist function declarations
    for stmt in statements {
        if let StatementKind::FunctionDeclaration(name, params, body, is_generator) = &stmt.kind {
            let func_val = if *is_generator {
                // For generator functions, create a function object wrapper
                let func_obj = new_js_object_data();
                let prototype_obj = new_js_object_data();
                let generator_val = Value::GeneratorFunction(None, params.clone(), body.clone(), env.clone(), None);
                obj_set_key_value(&func_obj, &"__closure__".into(), generator_val)?;
                obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
                obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
                Value::Object(func_obj)
            } else {
                // For regular functions, create a function object wrapper
                let func_obj = new_js_object_data();
                let prototype_obj = new_js_object_data();
                let closure_val = Value::Closure(params.clone(), body.clone(), env.clone(), None);
                obj_set_key_value(&func_obj, &"__closure__".into(), closure_val)?;
                obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
                obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
                Value::Object(func_obj)
            };
            env_set(env, name, func_val.clone())?;
            // In non-strict mode (assumed), function declarations in blocks are hoisted
            // to the nearest function/global scope (Annex B.3.3).
            if !env.borrow().is_function_scope {
                env_set_var(env, name, func_val)?;
            }
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
    if let Value::Object(obj_map) = &val {
        log::debug!("DBG Let - binding '{name}' into env -> func_obj ptr={:p}", Rc::as_ptr(obj_map));
    } else {
        log::debug!("DBG Let - binding '{name}' into env -> value={val:?}");
    }
    env_set(env, name, val.clone())?;
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
    if get_own_property(env, &name.into()).is_some() {
        return Err(raise_syntax_error!(format!("Identifier '{name}' has already been declared")));
    }
    let class_obj = create_class_object(name, extends, members, env)?;
    env_set(env, name, class_obj)?;
    Ok(())
}

fn evaluate_stmt_block(env: &JSObjectDataPtr, stmts: &[Statement], last_value: &mut Value) -> Result<Option<ControlFlow>, JSError> {
    let block_env = new_js_object_data();
    block_env.borrow_mut().prototype = Some(env.clone());
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
            StatementKind::Const(name, expr) => {
                evaluate_stmt_const(env, name, expr)?;
            }
            StatementKind::Let(name, expr_opt) => {
                evaluate_stmt_let(env, name, expr_opt)?;
            }
            StatementKind::Var(name, expr_opt) => {
                evaluate_stmt_var(env, name, expr_opt)?;
            }
            StatementKind::Class(name, extends, members) => evaluate_stmt_class(env, name, extends, members)?,
            StatementKind::FunctionDeclaration(name, params, body, is_generator) => {
                let func_val = if *is_generator {
                    let func_obj = new_js_object_data();
                    let prototype_obj = new_js_object_data();
                    let generator_val = Value::GeneratorFunction(None, params.clone(), body.clone(), env.clone(), None);
                    obj_set_key_value(&func_obj, &"__closure__".into(), generator_val)?;
                    obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
                    obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
                    Value::Object(func_obj)
                } else {
                    let func_obj = new_js_object_data();
                    let prototype_obj = new_js_object_data();
                    let closure_val = Value::Closure(params.clone(), body.clone(), env.clone(), None);
                    obj_set_key_value(&func_obj, &"__closure__".into(), closure_val)?;
                    obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
                    obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
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
    log::trace!("StatementKind::Return evaluated value = {:?}", return_val);
    Ok(Some(ControlFlow::Return(return_val)))
}

fn evaluate_stmt_throw(env: &JSObjectDataPtr, expr: &Expr) -> Result<Option<ControlFlow>, JSError> {
    let throw_val = evaluate_expr(env, expr)?;
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

fn evaluate_statements_with_context(env: &JSObjectDataPtr, statements: &[Statement]) -> Result<ControlFlow, JSError> {
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
        // Evaluate the statement inside a closure so we can log the
        // statement index and AST if an error occurs while preserving
        // control-flow returns. The closure returns
        // Result<Option<ControlFlow>, JSError> where `Ok(None)` means
        // continue, `Ok(Some(cf))` means propagate control flow, and
        // `Err(e)` means an error that we log and then return.
        let eval_res: Result<Option<ControlFlow>, JSError> = (|| -> Result<Option<ControlFlow>, JSError> {
            match &stmt.kind {
                StatementKind::Let(name, expr_opt) => {
                    last_value = evaluate_stmt_let(env, name, expr_opt)?;
                    Ok(None)
                }
                StatementKind::Var(name, expr_opt) => {
                    last_value = evaluate_stmt_var(env, name, expr_opt)?;
                    Ok(None)
                }
                StatementKind::Const(name, expr) => {
                    last_value = evaluate_stmt_const(env, name, expr)?;
                    Ok(None)
                }
                StatementKind::FunctionDeclaration(..) => {
                    // Skip function declarations as they are already hoisted
                    Ok(None)
                }
                StatementKind::Class(name, extends, members) => {
                    evaluate_stmt_class(env, name, extends, members)?;
                    last_value = Value::Undefined;
                    Ok(None)
                }
                StatementKind::Block(stmts) => evaluate_stmt_block(env, stmts, &mut last_value),
                StatementKind::Assign(name, expr) => {
                    last_value = evaluate_stmt_assign(env, name, expr)?;
                    Ok(None)
                }
                StatementKind::Expr(expr) => evaluate_stmt_expr(env, expr, &mut last_value),
                StatementKind::Return(expr_opt) => evaluate_stmt_return(env, expr_opt),
                StatementKind::Throw(expr) => evaluate_stmt_throw(env, expr),
                StatementKind::If(condition, then_body, else_body) => {
                    evaluate_stmt_if(env, condition, then_body, else_body, &mut last_value)
                }
                StatementKind::ForOfDestructuringObject(pattern, iterable, body) => {
                    evaluate_stmt_for_of_destructuring_object(env, pattern, iterable, body, &mut last_value)
                }
                StatementKind::ForOfDestructuringArray(pattern, iterable, body) => {
                    evaluate_stmt_for_of_destructuring_array(env, pattern, iterable, body, &mut last_value)
                }
                StatementKind::Label(label_name, inner_stmt) => evaluate_stmt_label(env, label_name, inner_stmt, &mut last_value),
                StatementKind::TryCatch(try_body, catch_param, catch_body, finally_body_opt) => {
                    evaluate_stmt_try_catch(env, try_body, catch_param, catch_body, finally_body_opt, &mut last_value)
                }
                StatementKind::For(init, condition, increment, body) => {
                    evaluate_stmt_for(env, init, condition, increment, body, &mut last_value)
                }
                StatementKind::ForOf(var, iterable, body) => evaluate_stmt_for_of(env, var, iterable, body, &mut last_value),
                StatementKind::ForIn(var, object, body) => evaluate_stmt_for_in(env, var, object, body, &mut last_value),
                StatementKind::While(condition, body) => evaluate_stmt_while(env, condition, body, &mut last_value),
                StatementKind::DoWhile(body, condition) => evaluate_stmt_do_while(env, body, condition, &mut last_value),
                StatementKind::Switch(expr, cases) => evaluate_stmt_switch(env, expr, cases, &mut last_value),
                StatementKind::Break(opt) => evaluate_stmt_break(opt),
                StatementKind::Continue(opt) => evaluate_stmt_continue(opt),
                StatementKind::LetDestructuringArray(pattern, expr) => {
                    evaluate_stmt_let_destructuring_array(env, pattern, expr, &mut last_value)
                }
                StatementKind::ConstDestructuringArray(pattern, expr) => {
                    evaluate_stmt_const_destructuring_array(env, pattern, expr, &mut last_value)
                }
                StatementKind::LetDestructuringObject(pattern, expr) => {
                    evaluate_stmt_let_destructuring_object(env, pattern, expr, &mut last_value)
                }
                StatementKind::ConstDestructuringObject(pattern, expr) => {
                    evaluate_stmt_const_destructuring_object(env, pattern, expr, &mut last_value)
                }
                StatementKind::Import(specifiers, module_name) => {
                    evaluate_stmt_import(env, specifiers, module_name)?;
                    last_value = Value::Undefined;
                    Ok(None)
                }
                StatementKind::Export(specifiers, maybe_decl) => {
                    evaluate_stmt_export(env, specifiers, maybe_decl)?;
                    last_value = Value::Undefined;
                    Ok(None)
                }
            }
        })();
        match eval_res {
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
                        log::warn!("evaluate_statements_with_context error at statement {i}: {e}, stmt={stmt:?}");
                    }
                }
                // Capture a minimal JS-style call stack by walking `__frame`/`__caller`
                // links from the environment where the error occurred. This produces
                // a vector of frame descriptions (innermost first).
                fn capture_frames_from_env(mut env_opt: Option<JSObjectDataPtr>) -> Vec<String> {
                    let mut frames = Vec::new();
                    while let Some(env_ptr) = env_opt {
                        if let Ok(Some(frame_val_rc)) = obj_get_key_value(&env_ptr, &"__frame".into()) {
                            if let Value::String(s_utf16) = &*frame_val_rc.borrow() {
                                frames.push(String::from_utf16_lossy(s_utf16));
                            }
                        }
                        // follow caller link if present
                        if let Ok(Some(caller_rc)) = obj_get_key_value(&env_ptr, &"__caller".into()) {
                            if let Value::Object(caller_env) = &*caller_rc.borrow() {
                                env_opt = Some(caller_env.clone());
                                continue;
                            }
                        }
                        break;
                    }
                    frames
                }

                let frames = capture_frames_from_env(Some(env.clone()));
                set_last_stack(frames);
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
        let block_env = new_js_object_data();
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
        let block_env = new_js_object_data();
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
    label_name: Option<&str>,
) -> Result<Option<ControlFlow>, JSError> {
    let for_env = new_js_object_data();
    for_env.borrow_mut().prototype = Some(env.clone());
    for_env.borrow_mut().is_function_scope = false;
    // Execute initialization in for_env
    if let Some(init_stmt) = init {
        match &init_stmt.kind {
            StatementKind::Let(name, expr_opt) => {
                let val = expr_opt
                    .clone()
                    .map_or(Ok(Value::Undefined), |expr| evaluate_expr(&for_env, &expr))?;
                env_set(&for_env, name.as_str(), val)?;
            }
            StatementKind::Var(name, expr_opt) => {
                let val = expr_opt
                    .clone()
                    .map_or(Ok(Value::Undefined), |expr| evaluate_expr(&for_env, &expr))?;
                env_set_var(&for_env, name.as_str(), val)?;
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
        block_env.borrow_mut().prototype = Some(for_env.clone());
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
        block_env.borrow_mut().prototype = Some(env.clone());
        block_env.borrow_mut().is_function_scope = false;
        match evaluate_statements_with_context(&block_env, then_body)? {
            ControlFlow::Normal(val) => *last_value = val,
            cf => return Ok(Some(cf)),
        }
    } else if let Some(else_stmts) = else_body {
        let block_env = new_js_object_data();
        block_env.borrow_mut().prototype = Some(env.clone());
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
                    instance.borrow_mut().prototype = Some(proto_obj.clone());
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
    block_env.borrow_mut().prototype = Some(env.clone());
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
                    if let Some(proto_ptr) = &obj_ptr.borrow().prototype {
                        if let Some(proto_ctor_rc) = get_own_property(proto_ptr, &"constructor".into()) {
                            let ctor_val = proto_ctor_rc.borrow().clone();
                            let _ = obj_set_key_value(obj_ptr, &"constructor".into(), ctor_val);
                        }
                    }
                }
            }
            Ok(cloned)
        }
        JSErrorKind::TypeError { .. } => create_js_error_instance(env, "TypeError", err),
        JSErrorKind::SyntaxError { .. } => create_js_error_instance(env, "SyntaxError", err),
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
                    block_env.borrow_mut().prototype = Some(env.clone());
                    block_env.borrow_mut().is_function_scope = false;
                    match evaluate_statements_with_context(&block_env, finally_body)? {
                        ControlFlow::Normal(_) => return Err(err),
                        other => return Ok(Some(other)),
                    }
                }
                Err(err)
            } else {
                let catch_env = new_js_object_data();
                catch_env.borrow_mut().prototype = Some(env.clone());
                catch_env.borrow_mut().is_function_scope = false;

                let catch_value = create_catch_value(env, &err)?;
                env_set(&catch_env, catch_param, catch_value)?;
                match evaluate_statements_with_context(&catch_env, catch_body)? {
                    ControlFlow::Normal(val) => {
                        *last_value = val;
                        if let Some(finally_body) = finally_body_opt {
                            execute_finally(env, finally_body, None, last_value)
                        } else {
                            Ok(None)
                        }
                    }
                    cf => {
                        if let Some(finally_body) = finally_body_opt {
                            execute_finally(env, finally_body, Some(cf), last_value)
                        } else {
                            Ok(Some(cf))
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
                Value::Object(obj_map) => {
                    if is_array(&obj_map) {
                        let len = get_array_length(&obj_map).unwrap_or(0);
                        for i in 0..len {
                            let key = PropertyKey::String(i.to_string());
                            if let Some(element_rc) = obj_get_key_value(&obj_map, &key)? {
                                let element = element_rc.borrow().clone();
                                env_set_recursive(env, var.as_str(), element)?;
                                let block_env = new_js_object_data();
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
        StatementKind::ForIn(var, object, body) => {
            let object_val = evaluate_expr(env, object)?;
            match object_val {
                Value::Object(obj_map) => {
                    let obj_borrow = obj_map.borrow();
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

            if let (Value::Object(obj_map), Value::Number(n)) = (&obj_val, &idx_val)
                && let Some(ta_val) = obj_get_key_value(obj_map, &"__typedarray".into())?
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
                                obj.borrow_mut().prototype = Some(proto_map.clone());
                            } else {
                                obj.borrow_mut().prototype = None;
                            }
                        } else {
                            obj_set_key_value(&obj, &key.into(), value.clone())?;
                        }
                        Ok(value)
                    } else {
                        Err(raise_eval_error!("Cannot assign to property of non-object"))
                    }
                }
                Value::String(s) => {
                    let key = String::from_utf16_lossy(&s);
                    if let Value::Object(obj) = obj_val {
                        if key == "__proto__" {
                            if let Value::Object(proto_map) = &value {
                                obj.borrow_mut().prototype = Some(proto_map.clone());
                            } else {
                                obj.borrow_mut().prototype = None;
                            }
                        } else {
                            obj_set_key_value(&obj, &key.into(), value.clone())?;
                        }
                        Ok(value)
                    } else {
                        Err(raise_eval_error!("Cannot assign to property of non-object"))
                    }
                }
                Value::Symbol(sym) => {
                    if let Value::Object(obj) = obj_val {
                        let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                        obj_set_key_value(&obj, &key, value.clone())?;
                        Ok(value)
                    } else {
                        Err(raise_eval_error!("Cannot assign to property of non-object"))
                    }
                }
                _ => Err(raise_eval_error!("Invalid index type")),
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
                let rest_obj = new_js_object_data();
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
        Value::Object(obj_map) => {
            if is_array(obj_map) {
                let len = get_array_length(obj_map).unwrap_or(0);
                for i in 0..len {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(element_rc) = obj_get_key_value(obj_map, &key)? {
                        let element = element_rc.borrow().clone();
                        // perform destructuring into env (var semantics)
                        perform_object_destructuring(env, pattern, &element, false)?;
                        let block_env = new_js_object_data();
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
    pattern: &[DestructuringElement],
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
                    if let Some(element_rc) = obj_get_key_value(obj_map, &key)? {
                        let element = element_rc.borrow().clone();
                        // perform array destructuring into env (var semantics)
                        perform_array_destructuring(env, pattern, &element, false)?;
                        let block_env = new_js_object_data();
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
                    if let Some(iterator_val) = obj_get_key_value(obj_map, &iterator_key)? {
                        let iterator_factory = iterator_val.borrow().clone();
                        // Call Symbol.iterator to get the iterator object. Accept
                        // either a direct closure or a function-object wrapper.
                        let iterator = if let Some((params, body, closure_env)) = extract_closure_from_value(&iterator_factory) {
                            // Call the closure with `this` bound to the original object
                            let call_env = new_js_object_data();
                            call_env.borrow_mut().prototype = Some(closure_env.clone());
                            // Bind `this` to the receiver
                            obj_set_key_value(&call_env, &"this".into(), Value::Object(obj_map.clone()))?;
                            // Bind any declared params to undefined (no args passed)
                            for param in params.iter() {
                                let (name, _) = param;
                                obj_set_key_value(&call_env, &name.clone().into(), Value::Undefined)?;
                            }
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
                                        let call_env = new_js_object_data();
                                        call_env.borrow_mut().prototype = Some(nclosure_env.clone());
                                        // Bind `this` to iterator object
                                        obj_set_key_value(&call_env, &"this".into(), Value::Object(iterator_obj.clone()))?;
                                        // Bind params to undefined (no args)
                                        for param in nparams.iter() {
                                            let (name, _) = param;
                                            obj_set_key_value(&call_env, &name.clone().into(), Value::Undefined)?;
                                        }
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
                                                continue;
                                            }
                                        } else {
                                            return Err(raise_eval_error!("Iterator next() did not return an object"));
                                        }
                                    } else if let Value::Function(func_name) = &next_func {
                                        // Built-in next handling: call the registered global function
                                        // Bind `this` to the iterator object so native helper can access iterator state
                                        let call_env = new_js_object_data();
                                        call_env.borrow_mut().prototype = Some(env.clone());
                                        obj_set_key_value(&call_env, &"this".into(), Value::Object(iterator_obj.clone()))?;
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
        Value::Object(obj_map) => {
            if is_array(&obj_map) {
                let len = get_array_length(&obj_map).unwrap_or(0);
                for i in 0..len {
                    let key = PropertyKey::String(i.to_string());
                    if let Some(element_rc) = obj_get_key_value(&obj_map, &key)? {
                        let element = element_rc.borrow().clone();
                        env_set_recursive(env, var, element)?;
                        let block_env = new_js_object_data();
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
                    if let Some(method_rc) = obj_get_key_value(&obj_map, &key)? {
                        // method can be a direct closure, an object-wrapped closure
                        // (function-object), a native function, or an iterator object.
                        let iterator_val = {
                            let method_val = &*method_rc.borrow();
                            if let Some((params, body, captured_env)) = extract_closure_from_value(method_val) {
                                // Call closure with 'this' bound to the object
                                let func_env = new_js_object_data();
                                func_env.borrow_mut().prototype = Some(captured_env.clone());
                                // mark this as a function scope so var-hoisting and
                                // env_set_var bind into this frame rather than parent
                                func_env.borrow_mut().is_function_scope = true;
                                obj_set_key_value(&func_env, &"this".into(), Value::Object(obj_map.clone()))?;
                                // Bind params to undefined (no args passed)
                                for param in params.iter() {
                                    let (name, _) = param;
                                    obj_set_key_value(&func_env, &name.clone().into(), Value::Undefined)?;
                                }
                                // Execute body to produce iterator result
                                // Attach minimal frame/caller info for stack traces
                                let frame = build_frame_name(env, "[Symbol.iterator]");
                                let _ = obj_set_key_value(&func_env, &"__frame".into(), Value::String(utf8_to_utf16(&frame)));
                                let _ = obj_set_key_value(&func_env, &"__caller".into(), Value::Object(env.clone()));
                                evaluate_statements(&func_env, &body)?
                            } else if let Value::Function(func_name) = method_val {
                                // Call built-in function (no arguments). Bind `this` to the receiver object.
                                let call_env = new_js_object_data();
                                call_env.borrow_mut().prototype = Some(env.clone());
                                obj_set_key_value(&call_env, &"this".into(), Value::Object(obj_map.clone()))?;
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
                                            let func_env = new_js_object_data();
                                            func_env.borrow_mut().prototype = Some(ncaptured_env.clone());
                                            obj_set_key_value(&func_env, &"this".into(), Value::Object(iter_obj.clone()))?;
                                            // Bind params to undefined (no args)
                                            for param in nparams.iter() {
                                                let (name, _) = param;
                                                obj_set_key_value(&func_env, &name.clone().into(), Value::Undefined)?;
                                            }
                                            // Attach frame/caller for iterator.next
                                            let frame = build_frame_name(env, "iterator.next");
                                            let _ = obj_set_key_value(&func_env, &"__frame".into(), Value::String(utf8_to_utf16(&frame)));
                                            let _ = obj_set_key_value(&func_env, &"__caller".into(), Value::Object(env.clone()));
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
        Value::Object(obj_map) => {
            // Iterate over all enumerable properties
            let obj_borrow = obj_map.borrow();
            for key in obj_borrow.properties.keys() {
                if !obj_borrow.non_enumerable.contains(key) {
                    let key_str = match key {
                        PropertyKey::String(s) => s.clone(),
                        PropertyKey::Symbol(_) => continue, // Skip symbols for now
                    };
                    env_set_recursive(env, var, Value::String(utf8_to_utf16(&key_str)))?;
                    let block_env = new_js_object_data();
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
                    let block_env = new_js_object_data();
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
                    let block_env = new_js_object_data();
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
            let generator_val = Value::GeneratorFunction(name.clone(), params.clone(), body.clone(), env.clone(), None);
            obj_set_key_value(&func_obj, &"__closure__".into(), generator_val)?;
            // If this is a named generator expression, expose the `name` property
            if let Some(n) = name.clone() {
                obj_set_key_value(&func_obj, &"name".into(), Value::String(utf8_to_utf16(&n)))?;
            }
            obj_set_key_value(&func_obj, &"prototype".into(), Value::Object(prototype_obj.clone()))?;
            obj_set_key_value(&prototype_obj, &"constructor".into(), Value::Object(func_obj.clone()))?;
            Ok(Value::Object(func_obj))
        }
        Expr::ArrowFunction(params, body) => Ok(Value::Closure(params.clone(), body.clone(), env.clone(), None)),
        Expr::AsyncArrowFunction(params, body) => Ok(Value::AsyncClosure(params.clone(), body.clone(), env.clone(), None)),
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
            log::debug!("DBG Expr::New - constructor_expr={:?} args.len={}", constructor, args.len());
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
            let closure_val = Value::AsyncClosure(params.clone(), body.clone(), env.clone(), None);
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
    params: &[(String, Option<Box<Expr>>)],
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

    // Store the closure under an internal key
    let closure_val = Value::Closure(params.to_vec(), body.to_vec(), env.clone(), None);
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
            log::trace!("evaluate_var - {} (found in env) -> {:?}", name, resolved);
            return Ok(resolved);
        }
        current_opt = current_env.borrow().prototype.clone();
    }

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
    } else if name == "Array" {
        let v = Value::Function("Array".to_string());
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
            current_opt = current_env.borrow().prototype.clone();
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
                Value::Object(obj_map) => {
                    obj_set_key_value(&obj_map, &prop.into(), val.clone())?;
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
                    obj_set_key_value(&obj_map, &key, val.clone())?;
                    Ok(val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    // Check if this is a TypedArray first
                    let ta_val_opt = obj_get_key_value(&obj_map, &"__typedarray".into());
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
                    obj_set_key_value(&obj_map, &key, val.clone())?;
                    Ok(val)
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_key_value(&obj_map, &key, val.clone())?;
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
        Expr::Var(name, _, _) => {
            env_set_recursive(env, name, new_val.clone())?;
            Ok(new_val)
        }
        Expr::Property(obj, prop) => {
            let obj_val = evaluate_expr(env, obj)?;
            match obj_val {
                Value::Object(obj_map) => {
                    obj_set_key_value(&obj_map, &prop.into(), new_val.clone())?;
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
                    obj_set_key_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_key_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_key_value(&obj_map, &key, new_val.clone())?;
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
                Value::Object(obj_map) => {
                    obj_set_key_value(&obj_map, &prop.into(), new_val.clone())?;
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
                    obj_set_key_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_key_value(&obj_map, &key, new_val.clone())?;
                    Ok(new_val)
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_key_value(&obj_map, &key, new_val.clone())?;
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
                Value::Object(obj_map) => {
                    obj_set_key_value(&obj_map, &prop.into(), new_val)?;
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
                    obj_set_key_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_key_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_key_value(&obj_map, &key, new_val)?;
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
                Value::Object(obj_map) => {
                    obj_set_key_value(&obj_map, &prop.into(), new_val)?;
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
                    obj_set_key_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(obj_map), Value::Number(n)) => {
                    let key = PropertyKey::String(n.to_string());
                    obj_set_key_value(&obj_map, &key, new_val)?;
                    Ok(old_val)
                }
                (Value::Object(obj_map), Value::Symbol(sym)) => {
                    let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
                    obj_set_key_value(&obj_map, &key, new_val)?;
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
                current_opt = current_env.borrow().prototype.clone();
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
        Value::Null => "null",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::BigInt(_) => "bigint",
        Value::Object(obj_map) => {
            // If this object wraps a closure under the internal `__closure__` key,
            // report `function` for `typeof` so function-objects behave like functions.
            #[allow(clippy::if_same_then_else)]
            if extract_closure_from_value(&val).is_some() {
                "function"
            } else if obj_get_key_value(obj_map, &"__is_constructor".into()).ok().flatten().is_some() {
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
    };
    Ok(Value::String(utf8_to_utf16(type_str)))
}

fn evaluate_delete(env: &JSObjectDataPtr, expr: &Expr) -> Result<Value, JSError> {
    match expr {
        Expr::Var(..) => {
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
                (Value::BigInt(_), Value::Number(_)) | (Value::Number(_), Value::BigInt(_)) => {
                    Err(raise_type_error!("Cannot mix BigInt and other types"))
                }
                _ => Err(raise_eval_error!("error")),
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
            // Check if property exists in object
            match (l, r) {
                (Value::String(prop), Value::Object(obj)) => {
                    let prop_str = PropertyKey::String(String::from_utf16_lossy(&prop));
                    Ok(Value::Boolean(obj_get_key_value(&obj, &prop_str)?.is_some()))
                }
                _ => Ok(Value::Boolean(false)),
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
        _ => Ok(Value::Boolean(false)),
    }
}

fn string_to_number(s: &[u16]) -> Result<f64, JSError> {
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

fn string_to_bigint(s: &[u16]) -> Result<BigInt, JSError> {
    let sstr = String::from_utf16_lossy(s);
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
        (Value::Object(obj_map), Value::Number(n)) => {
            // Check if this is a TypedArray first
            if let Some(ta_val) = obj_get_key_value(&obj_map, &"__typedarray".into())?
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
            if let Some(val) = obj_get_key_value(&obj_map, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(obj_map), Value::String(s)) => {
            // Object property access with string key
            let key = PropertyKey::String(String::from_utf16_lossy(&s));
            if let Some(val) = obj_get_key_value(&obj_map, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(obj_map), Value::Symbol(sym)) => {
            // Object property access with symbol key
            let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
            if let Some(val) = obj_get_key_value(&obj_map, &key)? {
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
            if let Some(val) = obj_get_key_value(&obj_map, &prop.into())? {
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
        Value::GeneratorFunction(name_opt, params, _body, _env, _) if prop == "name" => {
            if let Some(n) = name_opt {
                Ok(Value::String(utf8_to_utf16(&n)))
            } else {
                Ok(Value::Undefined)
            }
        }
        Value::GeneratorFunction(_name_opt, params, _body, _env, _) if prop == "length" => Ok(Value::Number(params.len() as f64)),
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
        Value::Undefined | Value::Null => Ok(Value::Undefined),
        Value::Object(obj_map) => {
            if let Some(val) = obj_get_key_value(&obj_map, &prop.into())? {
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
        (Value::Object(obj_map), Value::Number(n)) => {
            let key = PropertyKey::String(n.to_string());
            if let Some(val) = obj_get_key_value(&obj_map, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(obj_map), Value::String(s)) => {
            let key = PropertyKey::String(String::from_utf16_lossy(&s));
            if let Some(val) = obj_get_key_value(&obj_map, &key)? {
                Ok(val.borrow().clone())
            } else {
                Ok(Value::Undefined)
            }
        }
        (Value::Object(obj_map), Value::Symbol(sym)) => {
            let key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sym))));
            if let Some(val) = obj_get_key_value(&obj_map, &key)? {
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

pub(crate) fn bind_function_parameters(
    env: &JSObjectDataPtr,
    params: &[(String, Option<Box<Expr>>)],
    args: &[Value],
) -> Result<(), JSError> {
    for (i, param) in params.iter().enumerate() {
        let (name, default_expr_opt) = param;
        if let Some(rest_param_name) = name.strip_prefix("...") {
            let rest_args = if i < args.len() { args[i..].to_vec() } else { Vec::new() };

            // Create array object
            let array_obj = new_js_object_data();
            obj_set_key_value(&array_obj, &"length".into(), Value::Number(rest_args.len() as f64))?;
            array_obj.borrow_mut().set_non_enumerable("length".into());

            for (j, arg) in rest_args.into_iter().enumerate() {
                obj_set_key_value(&array_obj, &j.to_string().into(), arg)?;
            }

            env_set(env, rest_param_name, Value::Object(array_obj))?;
            break; // Rest parameter must be last
        }

        if i < args.len() {
            env_set(env, name.as_str(), args[i].clone())?;
        } else if let Some(expr) = default_expr_opt {
            let val = evaluate_expr(env, expr)?;
            env_set(env, name.as_str(), val)?;
        } else {
            env_set(env, name.as_str(), Value::Undefined)?;
        }
    }
    Ok(())
}

fn evaluate_tagged_template(env: &JSObjectDataPtr, tag: &Expr, strings: &[Vec<u16>], exprs: &[Expr]) -> Result<Value, JSError> {
    let strings_array = new_js_object_data();
    obj_set_key_value(&strings_array, &"length".into(), Value::Number(strings.len() as f64))?;
    let raw_array = new_js_object_data();
    obj_set_key_value(&raw_array, &"length".into(), Value::Number(strings.len() as f64))?;

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
            (Value::Object(obj_map), "log") if get_own_property(&obj_map, &"log".into()).is_some() => {
                handle_console_method(method_name, args, env)
            }
            // Handle toString/valueOf for primitive Symbol values here (they
            // don't go through the object-path below). For other cases (objects)
            // normal property lookup is used so user overrides take precedence
            // and Object.prototype functions act as fallbacks.
            (Value::Symbol(sd), "toString") => crate::js_object::handle_to_string_method(&Value::Symbol(sd.clone()), args, env),
            (Value::Symbol(sd), "valueOf") => crate::js_object::handle_value_of_method(&Value::Symbol(sd.clone()), args, env),
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
                // Detect Atomics object (basic ops)
                } else if get_own_property(&obj_map, &"load".into()).is_some() && get_own_property(&obj_map, &"store".into()).is_some() {
                    crate::js_typedarray::handle_atomics_method(method, args, env)
                } else if get_own_property(&obj_map, &"apply".into()).is_some() && get_own_property(&obj_map, &"construct".into()).is_some()
                {
                    crate::js_reflect::handle_reflect_method(method, args, env)
                } else if get_own_property(&obj_map, &"parse".into()).is_some() && get_own_property(&obj_map, &"stringify".into()).is_some()
                {
                    crate::js_json::handle_json_method(method, args, env)
                } else if get_own_property(&obj_map, &"keys".into()).is_some() && get_own_property(&obj_map, &"values".into()).is_some() {
                    crate::js_object::handle_object_method(method, args, env)
                } else if get_own_property(&obj_map, &"__arraybuffer".into()).is_some() {
                    if get_own_property(&obj_map, &"__sharedarraybuffer".into()).is_some() {
                        crate::js_typedarray::handle_sharedarraybuffer_constructor(args, env)
                    } else {
                        crate::js_typedarray::handle_arraybuffer_constructor(args, env)
                    }
                } else if get_own_property(&obj_map, &"MAX_VALUE".into()).is_some()
                    && get_own_property(&obj_map, &"MIN_VALUE".into()).is_some()
                {
                    crate::js_number::handle_number_method(method, args, env)
                } else if get_own_property(&obj_map, &"__is_bigint_constructor".into()).is_some() {
                    crate::js_bigint::handle_bigint_static_method(method, args, env)
                } else if get_own_property(&obj_map, &"__value__".into()).is_some() {
                    // Dispatch boxed primitive object methods based on the actual __value__ type
                    if let Some(val_rc) = obj_get_key_value(&obj_map, &"__value__".into())? {
                        match &*val_rc.borrow() {
                            Value::Number(_) => crate::js_number::handle_number_object_method(&obj_map, method, args, env),
                            Value::BigInt(_) => crate::js_bigint::handle_bigint_object_method(&obj_map, method, args, env),
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
                } else if is_date_object(&obj_map) {
                    // Date instance methods
                    crate::js_date::handle_date_method(&obj_map, method, args, env)
                } else if is_regex_object(&obj_map) {
                    // RegExp instance methods
                    crate::js_regexp::handle_regexp_method(&obj_map, method, args, env)
                } else if is_array(&obj_map) {
                    // Array instance methods
                    crate::js_array::handle_array_instance_method(&obj_map, method, args, env)
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
                    if let Some(prop_val) = obj_get_key_value(&obj_map, &method.into())? {
                        match prop_val.borrow().clone() {
                            Value::Closure(params, body, captured_env, home_obj)
                            | Value::AsyncClosure(params, body, captured_env, home_obj) => {
                                // Function call
                                // Collect all arguments, expanding spreads
                                let mut evaluated_args = Vec::new();
                                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                                // Create new environment starting with captured environment
                                // Use a fresh environment frame whose prototype points to the captured environment
                                let func_env = new_js_object_data();
                                func_env.borrow_mut().prototype = Some(captured_env.clone());
                                if let Some(home) = home_obj {
                                    log::trace!("DEBUG: Setting __home_object__ in evaluate_call (generic method)");
                                    obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home.clone()))?;
                                } else {
                                    log::trace!("DEBUG: home_obj is None in evaluate_call (generic method)");
                                }
                                // Bind parameters: assign provided args, set missing params to undefined
                                bind_function_parameters(&func_env, &params, &evaluated_args)?;
                                // Attach frame/caller information for stack traces
                                let frame = build_frame_name(env, method);
                                let _ = obj_set_key_value(&func_env, &"__frame".into(), Value::String(utf8_to_utf16(&frame)));
                                let _ = obj_set_key_value(&func_env, &"__caller".into(), Value::Object(env.clone()));
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
                                if func_name.starts_with("Object.prototype.") || func_name == "Error.prototype.toString" {
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
                                            crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), args, env)
                                        }
                                        "Error.prototype.toString" => {
                                            crate::js_object::handle_error_to_string_method(&Value::Object(obj_map.clone()), args)
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
                                            crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), args, env)
                                        }
                                        "Object.prototype.valueOf" => {
                                            // Delegate to handle_value_of_method
                                            crate::js_object::handle_value_of_method(&Value::Object(obj_map.clone()), args, env)
                                        }
                                        _ => crate::js_function::handle_global_function(&func_name, args, env),
                                    }
                                } else {
                                    crate::js_function::handle_global_function(&func_name, args, env)
                                }
                            }
                            Value::Object(func_obj_map) => {
                                // Support function-objects stored as properties (they
                                // wrap an internal `__closure__`). Invoke the
                                // internal closure with `this` bound to the
                                // receiver object (`obj_map`). This allows
                                // assignments like `MyError.prototype.toString = function() { ... }`
                                // to be callable as methods.
                                if let Some(cl_rc) = obj_get_key_value(&func_obj_map, &"__closure__".into())? {
                                    match &*cl_rc.borrow() {
                                        Value::Closure(params, body, captured_env, home_obj) => {
                                            // Collect all arguments, expanding spreads
                                            let mut evaluated_args = Vec::new();
                                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                                            // Create new environment starting with captured environment (fresh frame)
                                            let func_env = new_js_object_data();
                                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                                            if let Some(home) = home_obj {
                                                obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home.clone()))?;
                                            }
                                            // ensure this env is a proper function scope
                                            func_env.borrow_mut().is_function_scope = true;
                                            // Bind `this` to the receiver object
                                            env_set(&func_env, "this", Value::Object(obj_map.clone()))?;
                                            // Bind parameters: provide provided args, set missing params to undefined
                                            bind_function_parameters(&func_env, params, &evaluated_args)?;
                                            // Attach frame/caller information for stack traces
                                            let frame = build_frame_name(env, method);
                                            let _ = obj_set_key_value(&func_env, &"__frame".into(), Value::String(utf8_to_utf16(&frame)));
                                            let _ = obj_set_key_value(&func_env, &"__caller".into(), Value::Object(env.clone()));
                                            // Execute function body
                                            evaluate_statements(&func_env, body)
                                        }
                                        Value::GeneratorFunction(_, params, body, captured_env, home_obj) => {
                                            // Generator method-style call - return a generator object
                                            let mut evaluated_args = Vec::new();
                                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                                            let func_env = new_js_object_data();
                                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                                            if let Some(home) = home_obj {
                                                obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home.clone()))?;
                                            }
                                            func_env.borrow_mut().is_function_scope = true;
                                            // Bind `this` to the receiver object
                                            env_set(&func_env, "this", Value::Object(obj_map.clone()))?;
                                            // Attach frame/caller information for stack traces
                                            let frame = build_frame_name(env, method);
                                            let _ = obj_set_key_value(&func_env, &"__frame".into(), Value::String(utf8_to_utf16(&frame)));
                                            let _ = obj_set_key_value(&func_env, &"__caller".into(), Value::Object(env.clone()));
                                            crate::js_generator::handle_generator_function_call(params, body, args, &func_env)
                                        }
                                        Value::AsyncClosure(params, body, captured_env, home_obj) => {
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
                                            // Create new environment
                                            let func_env = new_js_object_data();
                                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                                            if let Some(home) = home_obj {
                                                obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home.clone()))?;
                                            }
                                            func_env.borrow_mut().is_function_scope = true;
                                            // Bind `this` to the receiver object
                                            env_set(&func_env, "this", Value::Object(obj_map.clone()))?;
                                            // Bind parameters
                                            bind_function_parameters(&func_env, params, &evaluated_args)?;
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
            // Allow function values to support `.call` and `.apply` forwarding
            (Value::Function(func_name), "call") => {
                // Forward Object.prototype.* builtins when called via .call
                if func_name.starts_with("Object.prototype.") {
                    if args.len() < 2 {
                        return Err(raise_eval_error!("call requires a receiver and at least one arg"));
                    }
                    let method = func_name.trim_start_matches("Object.prototype.").to_string();
                    // Special-case hasOwnProperty: call should invoke the builtin
                    // implementation using the provided receiver (args[0]) and
                    // property argument (args[1]) without requiring the receiver
                    // to have the method as an own property.
                    if method == "hasOwnProperty" {
                        // receiver
                        let receiver_val = evaluate_expr(env, &args[0])?;
                        // property name arg
                        let key_val = evaluate_expr(env, &args[1])?;
                        let exists = match receiver_val {
                            Value::Object(obj_map) => match key_val {
                                Value::String(s) => get_own_property(&obj_map, &String::from_utf16_lossy(&s).into()).is_some(),
                                Value::Number(n) => get_own_property(&obj_map, &n.to_string().into()).is_some(),
                                Value::Boolean(b) => get_own_property(&obj_map, &b.to_string().into()).is_some(),
                                Value::Undefined => get_own_property(&obj_map, &"undefined".into()).is_some(),
                                Value::Symbol(sd) => {
                                    let sym_key = PropertyKey::Symbol(Rc::new(RefCell::new(Value::Symbol(sd))));
                                    get_own_property(&obj_map, &sym_key).is_some()
                                }
                                other => get_own_property(&obj_map, &value_to_string(&other).into()).is_some(),
                            },
                            _ => false,
                        };
                        return Ok(Value::Boolean(exists));
                    }
                    let receiver_expr = args[0].clone();
                    let forwarded = &args[1..];
                    let prop_expr = Expr::Property(Box::new(receiver_expr), method);
                    let call_expr = Expr::Call(Box::new(prop_expr), forwarded.to_vec());
                    return evaluate_expr(env, &call_expr);
                }
                Err(raise_eval_error!(format!("{} has no static method 'call'", func_name)))
            }
            (Value::Function(func_name), "apply") => {
                if func_name.starts_with("Object.prototype.") {
                    if args.is_empty() {
                        return Err(raise_eval_error!("apply requires a receiver"));
                    }
                    // receiver
                    let receiver_val = evaluate_expr(env, &args[0])?;
                    // array arg
                    let mut forwarded_exprs: Vec<Expr> = Vec::new();
                    if args.len() >= 2 {
                        match evaluate_expr(env, &args[1])? {
                            Value::Object(arr_obj) if crate::js_array::is_array(&arr_obj) => {
                                let mut i = 0usize;
                                loop {
                                    let key = i.to_string();
                                    if let Some(val_rc) = crate::core::get_own_property(&arr_obj, &key.into()) {
                                        forwarded_exprs.push(Expr::Value(val_rc.borrow().clone()));
                                    } else {
                                        break;
                                    }
                                    i += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                    let method = func_name.trim_start_matches("Object.prototype.").to_string();
                    let receiver_expr = Expr::Value(receiver_val);
                    let prop_expr = Expr::Property(Box::new(receiver_expr), method);
                    let call_expr = Expr::Call(Box::new(prop_expr), forwarded_exprs);
                    return evaluate_expr(env, &call_expr);
                }
                Err(raise_eval_error!(format!("{} has no static method 'apply'", func_name)))
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
            Value::Object(obj_map) => handle_optional_method_call(&obj_map, method_name, args, env),
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
            Value::GeneratorFunction(_, params, body, captured_env, _) => {
                // Generator function call - return a generator object
                crate::js_generator::handle_generator_function_call(&params, &body, args, &captured_env)
            }
            Value::Object(obj_map) if get_own_property(&obj_map, &"__closure__".into()).is_some() => {
                // Function object call - extract the closure and call it
                if let Some(cl_rc) = obj_get_key_value(&obj_map, &"__closure__".into())? {
                    match &*cl_rc.borrow() {
                        Value::AsyncClosure(params, body, captured_env, _) => {
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
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            func_env.borrow_mut().is_function_scope = true;
                            // For direct calls, `this` is undefined
                            env_set(&func_env, "this", Value::Undefined)?;
                            // Bind parameters
                            bind_function_parameters(&func_env, params, &evaluated_args)?;
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
                        Value::Closure(params, body, captured_env, _) => {
                            // Function call
                            // Collect all arguments, expanding spreads
                            let mut evaluated_args = Vec::new();
                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                            // Create new environment starting with captured environment (fresh frame)
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            // ensure this env is a proper function scope
                            func_env.borrow_mut().is_function_scope = true;
                            // Attach minimal frame info (try to derive a name from captured_env, else anonymous)
                            let frame_name = if let Ok(Some(name_rc)) = obj_get_key_value(captured_env, &"name".into()) {
                                if let Value::String(s) = &*name_rc.borrow() {
                                    String::from_utf16_lossy(s)
                                } else {
                                    "<anonymous>".to_string()
                                }
                            } else {
                                "<anonymous>".to_string()
                            };
                            let frame = build_frame_name(env, &frame_name);
                            let _ = obj_set_key_value(&func_env, &"__frame".into(), Value::String(utf8_to_utf16(&frame)));
                            let _ = obj_set_key_value(&func_env, &"__caller".into(), Value::Object(env.clone()));
                            // Bind parameters: provide provided args, set missing params to undefined
                            bind_function_parameters(&func_env, params, &evaluated_args)?;
                            // Execute function body
                            evaluate_statements(&func_env, body)
                        }

                        Value::GeneratorFunction(_, params, body, captured_env, _) => {
                            // Generator function call - return a generator object
                            crate::js_generator::handle_generator_function_call(params, body, args, captured_env)
                        }
                        _ => Err(raise_eval_error!("Object is not callable")),
                    }
                } else {
                    Err(raise_eval_error!("Object is not callable"))
                }
            }
            Value::Object(obj_map)
                if obj_get_key_value(&obj_map, &"__is_error_constructor".into())
                    .ok()
                    .flatten()
                    .is_some() =>
            {
                crate::js_class::evaluate_new(env, func_expr, args)
            }
            Value::Closure(params, body, captured_env, _) => {
                // Function call
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                // Create new environment starting with captured environment (fresh frame)
                let func_env = new_js_object_data();
                func_env.borrow_mut().prototype = Some(captured_env.clone());
                // ensure this env is a proper function scope
                func_env.borrow_mut().is_function_scope = true;
                // Attach minimal frame info (try to derive a name from captured_env, else anonymous)
                let frame_name = if let Ok(Some(name_rc)) = obj_get_key_value(&captured_env, &"name".into()) {
                    if let Value::String(s) = &*name_rc.borrow() {
                        String::from_utf16_lossy(s)
                    } else {
                        "<anonymous>".to_string()
                    }
                } else {
                    "<anonymous>".to_string()
                };
                let frame = build_frame_name(env, &frame_name);
                let _ = obj_set_key_value(&func_env, &"__frame".into(), Value::String(utf8_to_utf16(&frame)));
                let _ = obj_set_key_value(&func_env, &"__caller".into(), Value::Object(env.clone()));
                // Bind parameters: provide provided args, set missing params to undefined
                bind_function_parameters(&func_env, &params, &evaluated_args)?;
                // Execute function body
                evaluate_statements(&func_env, &body)
            }
            Value::AsyncClosure(params, body, captured_env, _) => {
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
                // Create new environment
                let func_env = new_js_object_data();
                func_env.borrow_mut().prototype = Some(captured_env.clone());
                func_env.borrow_mut().is_function_scope = true;
                // Bind parameters
                bind_function_parameters(&func_env, &params, &evaluated_args)?;
                // Execute function body synchronously (for now)
                let result = evaluate_statements(&func_env, &body);
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
            Value::Object(obj_map) => {
                // If this object wraps a closure under the internal `__closure__` key,
                // call that closure. This lets script-defined functions be stored
                // as objects (so they have assignable `prototype`), while still
                // being callable.
                if let Some(cl_rc) = obj_get_key_value(&obj_map, &"__closure__".into())? {
                    match &*cl_rc.borrow() {
                        Value::Closure(params, body, captured_env, home_obj)
                        | Value::AsyncClosure(params, body, captured_env, home_obj) => {
                            // Collect all arguments, expanding spreads
                            let mut evaluated_args = Vec::new();
                            expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                            // Create new environment starting with captured environment (fresh frame)
                            let func_env = new_js_object_data();
                            func_env.borrow_mut().prototype = Some(captured_env.clone());
                            if let Some(home) = home_obj {
                                obj_set_key_value(&func_env, &"__home_object__".into(), Value::Object(home.clone()))?;
                            }
                            // ensure this env is a proper function scope
                            func_env.borrow_mut().is_function_scope = true;
                            // Attach minimal frame info for this callable (derive name from func object if present)
                            let frame_name = if let Ok(Some(nrc)) = obj_get_key_value(&obj_map, &"name".into()) {
                                if let Value::String(s) = &*nrc.borrow() {
                                    String::from_utf16_lossy(s)
                                } else {
                                    "<anonymous>".to_string()
                                }
                            } else {
                                "<anonymous>".to_string()
                            };
                            let frame = build_frame_name(env, &frame_name);
                            let _ = obj_set_key_value(&func_env, &"__frame".into(), Value::String(utf8_to_utf16(&frame)));
                            let _ = obj_set_key_value(&func_env, &"__caller".into(), Value::Object(env.clone()));
                            // Bind parameters: provide provided args, set missing params to undefined
                            bind_function_parameters(&func_env, params, &evaluated_args)?;
                            // Execute function body
                            return evaluate_statements(&func_env, body);
                        }
                        _ => {}
                    }
                }
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
                            && let Expr::Var(fname, _, _) = &**func_expr
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
                                    Box::new(Expr::Var("canonicalizeLanguageTag".to_string(), None, None)),
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
                                    Box::new(Expr::Var("isStructurallyValidLanguageTag".to_string(), None, None)),
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
                    && let Some(obj_ctor_rc) = obj_get_key_value(&root_env, &"Object".into())?
                    && let Value::Object(ctor_map) = &*obj_ctor_rc.borrow()
                    && Rc::ptr_eq(ctor_map, &obj_map)
                {
                    return crate::js_class::handle_object_constructor(args, env);
                }

                // Check if this is a built-in constructor object (Number)
                if get_own_property(&obj_map, &"MAX_VALUE".into()).is_some() && get_own_property(&obj_map, &"MIN_VALUE".into()).is_some() {
                    // Number constructor call
                    crate::js_function::handle_global_function("Number", args, env)
                } else if get_own_property(&obj_map, &"__arraybuffer".into()).is_some() {
                    // ArrayBuffer / SharedArrayBuffer constructor call
                    if get_own_property(&obj_map, &"__sharedarraybuffer".into()).is_some() {
                        crate::js_typedarray::handle_sharedarraybuffer_constructor(args, env)
                    } else {
                        crate::js_typedarray::handle_arraybuffer_constructor(args, env)
                    }
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
        if let Expr::Var(var_name, _, _) = &**obj_expr
            && var_name == "Array"
        {
            return crate::js_array::handle_array_static_method(method_name, args, env);
        }

        let obj_val = evaluate_expr(env, obj_expr)?;
        log::trace!("evaluate_optional_call - object eval result: {obj_val:?}");
        match obj_val {
            Value::Undefined | Value::Null => Ok(Value::Undefined),
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
                // Detect Atomics object
                } else if get_own_property(&obj_map, &"load".into()).is_some() && get_own_property(&obj_map, &"store".into()).is_some() {
                    crate::js_typedarray::handle_atomics_method(method_name, args, env)
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
                    if let Some(val_rc) = obj_get_key_value(&obj_map, &"__value__".into())? {
                        match &*val_rc.borrow() {
                            Value::Number(_) => crate::js_number::handle_number_object_method(&obj_map, method_name, args, env),
                            Value::BigInt(_) => crate::js_bigint::handle_bigint_object_method(&obj_map, method_name, args, env),
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
                } else if is_date_object(&obj_map) {
                    // Date instance methods
                    crate::js_date::handle_date_method(&obj_map, method_name, args, env)
                } else if is_regex_object(&obj_map) {
                    // RegExp instance methods
                    crate::js_regexp::handle_regexp_method(&obj_map, method_name, args, env)
                } else if is_array(&obj_map) {
                    // Array instance methods
                    crate::js_array::handle_array_instance_method(&obj_map, method_name, args, env)
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
            Value::Closure(params, body, captured_env, _) | Value::AsyncClosure(params, body, captured_env, _) => {
                // Function call
                // Collect all arguments, expanding spreads
                let mut evaluated_args = Vec::new();
                expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                // Create new environment starting with captured environment (fresh frame)
                let func_env = new_js_object_data();
                func_env.borrow_mut().prototype = Some(captured_env.clone());
                // Bind parameters: provide provided args, set missing params to undefined
                bind_function_parameters(&func_env, &params, &evaluated_args)?;
                // Execute function body
                evaluate_statements(&func_env, &body)
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

    for (key_expr, value_expr, is_method) in properties {
        if matches!(value_expr, Expr::Spread(_)) {
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
            // Evaluate key expression
            let key_val = evaluate_expr(env, key_expr)?;
            let pk = if let Value::Symbol(_) = key_val {
                PropertyKey::Symbol(Rc::new(RefCell::new(key_val)))
            } else {
                let key_str = match &key_val {
                    Value::String(s) => String::from_utf16_lossy(s),
                    Value::BigInt(b) => b.to_string(),
                    _ => key_val.to_string(),
                };
                PropertyKey::String(key_str)
            };

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
                            Value::Closure(.., home_obj) => *home_obj = Some(obj.clone()),
                            Value::AsyncClosure(.., home_obj) => *home_obj = Some(obj.clone()),
                            Value::GeneratorFunction(.., home_obj) => *home_obj = Some(obj.clone()),
                            Value::Object(func_obj) => {
                                if let Some(closure_rc) = obj_get_key_value(func_obj, &"__closure__".into())? {
                                    let mut closure_val = closure_rc.borrow_mut();
                                    match &mut *closure_val {
                                        Value::Closure(.., home_obj) => *home_obj = Some(obj.clone()),
                                        Value::AsyncClosure(.., home_obj) => *home_obj = Some(obj.clone()),
                                        Value::GeneratorFunction(.., home_obj) => *home_obj = Some(obj.clone()),
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

fn evaluate_array(env: &JSObjectDataPtr, elements: &Vec<Expr>) -> Result<Value, JSError> {
    let arr = new_js_object_data();
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
                    if let Some(val) = obj_get_key_value(&spread_obj, &key.into())? {
                        obj_set_key_value(&arr, &index.to_string().into(), val.borrow().clone())?;
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
            obj_set_key_value(&arr, &index.to_string().into(), value)?;
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
            StatementKind::Var(name, _) => {
                names.insert(name.clone());
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
fn handle_optional_method_call(obj_map: &JSObjectDataPtr, method: &str, args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    match method {
        "log" if get_own_property(obj_map, &"log".into()).is_some() => handle_console_method(method, args, env),
        "toString" => crate::js_object::handle_to_string_method(&Value::Object(obj_map.clone()), args, env),
        "valueOf" => crate::js_object::handle_value_of_method(&Value::Object(obj_map.clone()), args, env),
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
            } else if is_date_object(obj_map) {
                // Date instance methods
                crate::js_date::handle_date_method(obj_map, method, args, env)
            } else if is_regex_object(obj_map) {
                // RegExp instance methods
                crate::js_regexp::handle_regexp_method(obj_map, method, args, env)
            } else if is_array(obj_map) {
                // Array instance methods
                crate::js_array::handle_array_instance_method(obj_map, method, args, env)
            } else if get_own_property(obj_map, &"__class_def__".into()).is_some() {
                // Class static methods
                call_static_method(obj_map, method, args, env)
            } else if is_class_instance(obj_map)? {
                call_class_method(obj_map, method, args, env)
            } else {
                // Check for user-defined method
                if let Some(prop_val) = obj_get_key_value(obj_map, &method.into())? {
                    let prop = prop_val.borrow().clone();
                    if let Some((params, body, captured_env)) = extract_closure_from_value(&prop) {
                        // Function call
                        // Collect all arguments, expanding spreads
                        let mut evaluated_args = Vec::new();
                        expand_spread_in_call_args(env, args, &mut evaluated_args)?;
                        // Create new environment starting with captured environment (fresh frame)
                        let func_env = new_js_object_data();
                        func_env.borrow_mut().prototype = Some(captured_env.clone());
                        // Bind parameters: provide provided args, set missing params to undefined
                        bind_function_parameters(&func_env, &params, &evaluated_args)?;
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
                    map.borrow_mut().prototype = Some(proto_map);
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
                    obj.borrow_mut().prototype = Some(proto_map);
                    return Ok(Some(Value::Object(obj)));
                } else {
                    obj.borrow_mut().prototype = None;
                    return Ok(Some(Value::Object(obj)));
                }
            }

            obj_set_key_value(&obj, &prop.into(), val)?;
            Ok(Some(Value::Object(obj)))
        }
        _ => Err(raise_eval_error!("not an object")),
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
